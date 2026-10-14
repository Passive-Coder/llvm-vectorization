//! Project traversal and source-to-LLVM-IR adapters for the CLI.
use super::cache::Cache;
use super::inputs::{self, CompileUnit, IrUnit};
use super::process;
use super::report::{self, Diagnostics, FileReport, RunReport};
use super::{Options, add_target_options, build_command, command_display};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::{Builder, NamedTempFile};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Ir,
    C,
    Cxx,
    ObjC,
    ObjCxx,
    Fortran,
    Rust,
    Swift,
    Custom,
}

#[derive(Debug)]
struct WorkItem {
    input: PathBuf,
    output: PathBuf,
    kind: Kind,
    compile: Option<CompileUnit>,
    source_ir: Option<PathBuf>,
}

type Frontends = BTreeMap<String, Vec<String>>;
const OUTPUT_MARKER: &str = ".rv-vectorize-output";
const OUTPUT_MARKER_CONTENT: &str = "rust-loop-vectorizer project output v1\n";

fn is_marked_output(directory: &Path) -> bool {
    let marker = directory.join(OUTPUT_MARKER);
    fs::symlink_metadata(&marker).is_ok_and(|metadata| {
        metadata.file_type().is_file()
            && metadata.len() == OUTPUT_MARKER_CONTENT.len() as u64
            && fs::read_to_string(&marker).is_ok_and(|content| content == OUTPUT_MARKER_CONTENT)
    })
}

fn load_frontends(path: Option<&Path>) -> Result<Frontends, String> {
    let Some(path) = path else {
        return Ok(Frontends::new());
    };
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot read frontend config {}: {error}", path.display()))?;
    if metadata.len() > 1024 * 1024 {
        return Err("frontend config exceeds 1 MiB".to_owned());
    }
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot read frontend config {}: {error}", path.display()))?;
    let frontends: Frontends = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid frontend config {}: {error}", path.display()))?;
    for (extension, command) in &frontends {
        if extension.is_empty()
            || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || command.is_empty()
            || command[0].is_empty()
            || command[0].contains("{input}")
            || command[0].contains("{output}")
            || command
                .iter()
                .map(|arg| arg.matches("{input}").count())
                .sum::<usize>()
                != 1
            || command
                .iter()
                .map(|arg| arg.matches("{output}").count())
                .sum::<usize>()
                != 1
        {
            return Err(format!(
                "frontend for .{extension} needs a nonempty executable and exactly one {{input}} and {{output}} placeholder"
            ));
        }
    }
    Ok(frontends)
}

fn kind(path: &Path, frontends: &Frontends) -> Option<Kind> {
    let extension = path.extension()?.to_str()?;
    if frontends.contains_key(extension) {
        return Some(Kind::Custom);
    }
    match extension {
        "ll" | "bc" => Some(Kind::Ir),
        "c" => Some(Kind::C),
        "cc" | "cpp" | "cxx" | "C" => Some(Kind::Cxx),
        "m" => Some(Kind::ObjC),
        "mm" => Some(Kind::ObjCxx),
        "f" | "f90" | "f95" | "f03" | "f08" | "F" | "F90" => Some(Kind::Fortran),
        "rs" => Some(Kind::Rust),
        "swift" => Some(Kind::Swift),
        _ => None,
    }
}

fn is_rust_crate_root(path: &Path, root: &Path) -> bool {
    let mut ancestor = path.parent();
    let mut has_manifest = false;
    while let Some(directory) = ancestor {
        if directory.join("Cargo.toml").is_file() {
            has_manifest = true;
            break;
        }
        if directory == root {
            break;
        }
        ancestor = directory.parent();
    }
    if !has_manifest {
        return true;
    }
    let name = path.file_name().and_then(|name| name.to_str());
    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    matches!(name, Some("lib.rs" | "main.rs")) || matches!(parent_name, Some("bin" | "examples"))
}

fn output_path(output_root: &Path, relative: &Path, kind: Kind, options: &Options) -> PathBuf {
    let mut output = output_root.join(relative);
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let target = if options.emit_object {
        "o"
    } else if options.emit_bitcode {
        "bc"
    } else {
        "ll"
    };
    if kind == Kind::Ir && extension == target {
        return output;
    }
    output.set_extension(format!("{extension}.{target}"));
    output
}

struct Selection {
    gitignore: Option<Gitignore>,
    include: Option<GlobSet>,
    exclude: GlobSet,
}

fn glob_set(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder
            .add(Glob::new(pattern).map_err(|error| format!("invalid glob {pattern:?}: {error}"))?);
    }
    builder
        .build()
        .map_err(|error| format!("invalid glob set: {error}"))
}

fn load_gitignore(root: &Path, output_root: &Path) -> Result<Gitignore, String> {
    let mut builder = GitignoreBuilder::new(root);
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("cannot scan {}: {error}", directory.display()))?
        {
            let entry =
                entry.map_err(|error| format!("cannot scan {}: {error}", directory.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if path != output_root
                    && !is_marked_output(&path)
                    && !name.starts_with('.')
                    && !matches!(name.as_ref(), "target" | "node_modules" | "vendor")
                {
                    pending.push(path);
                }
            } else if file_type.is_file() && entry.file_name() == ".gitignore" {
                if let Some(error) = builder.add(&path) {
                    return Err(format!("invalid .gitignore {}: {error}", path.display()));
                }
            }
        }
    }
    builder
        .build()
        .map_err(|error| format!("cannot load .gitignore rules: {error}"))
}

impl Selection {
    fn new(root: &Path, output_root: &Path, options: &Options) -> Result<Self, String> {
        Ok(Self {
            gitignore: if options.no_gitignore {
                None
            } else {
                Some(load_gitignore(root, output_root)?)
            },
            include: if options.include.is_empty() {
                None
            } else {
                Some(glob_set(&options.include)?)
            },
            exclude: glob_set(&options.exclude)?,
        })
    }

    fn skip_reason(&self, root: &Path, path: &Path, is_dir: bool) -> Option<&'static str> {
        let relative = path.strip_prefix(root).ok()?;
        if self.exclude.is_match(relative) {
            return Some("exclude");
        }
        if self
            .gitignore
            .as_ref()
            .is_some_and(|ignore| ignore.matched_path_or_any_parents(path, is_dir).is_ignore())
        {
            return Some("gitignore");
        }
        if !is_dir
            && self
                .include
                .as_ref()
                .is_some_and(|include| !include.is_match(relative))
        {
            return Some("include");
        }
        None
    }
}

fn explain_skip(options: &Options, path: &Path, reason: &str) {
    if options.explain_skips {
        eprintln!("rv-vectorize: skip={} reason={reason}", path.display());
    }
}

#[allow(clippy::too_many_lines)]
fn collect(
    root: &Path,
    output_root: &Path,
    frontends: &Frontends,
    options: &Options,
    compile_units: Vec<CompileUnit>,
    ir_units: Vec<IrUnit>,
    has_compile_database: bool,
) -> Result<(Vec<WorkItem>, usize), String> {
    let mut pending = vec![root.to_owned()];
    let mut items = Vec::new();
    let mut skipped = 0;
    let mut destinations = HashSet::new();
    let selection = Selection::new(root, output_root, options)?;
    let database_sources: HashSet<_> = compile_units
        .iter()
        .map(|unit| unit.source.clone())
        .collect();
    let manifest_sources: HashSet<_> = ir_units.iter().map(|unit| unit.source.clone()).collect();
    let manifest_ir: HashSet<_> = ir_units.iter().map(|unit| unit.ir.clone()).collect();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("cannot scan {}: {error}", directory.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("cannot scan {}: {error}", directory.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                skipped += 1;
                explain_skip(options, &path, "symlink");
                continue;
            }
            if file_type.is_dir() {
                if path == output_root || is_marked_output(&path) {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.')
                    || matches!(name.as_ref(), "target" | "node_modules" | "vendor")
                {
                    explain_skip(options, &path, "generated-or-hidden-directory");
                    continue;
                }
                if let Some(reason) = selection.skip_reason(root, &path, true) {
                    explain_skip(options, &path, reason);
                    continue;
                }
                pending.push(path);
                continue;
            }
            if !file_type.is_file() {
                skipped += 1;
                explain_skip(options, &path, "non-file");
                continue;
            }
            if let Some(reason) = selection.skip_reason(root, &path, false) {
                skipped += 1;
                explain_skip(options, &path, reason);
                continue;
            }
            if database_sources.contains(&path)
                || manifest_sources.contains(&path)
                || manifest_ir.contains(&path)
            {
                continue;
            }
            let Some(file_kind) = kind(&path, frontends) else {
                skipped += 1;
                explain_skip(options, &path, "unsupported-file");
                continue;
            };
            if has_compile_database
                && matches!(file_kind, Kind::C | Kind::Cxx | Kind::ObjC | Kind::ObjCxx)
            {
                skipped += 1;
                explain_skip(options, &path, "unlisted-compilation-unit");
                continue;
            }
            if file_kind == Kind::Rust && !is_rust_crate_root(&path, root) {
                skipped += 1;
                explain_skip(options, &path, "rust-module");
                continue;
            }
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            let output = output_path(output_root, relative, file_kind, options);
            if !destinations.insert(output.clone()) {
                return Err(format!(
                    "multiple inputs map to output {}",
                    output.display()
                ));
            }
            items.push(WorkItem {
                input: path,
                output,
                kind: file_kind,
                compile: None,
                source_ir: None,
            });
        }
    }
    for unit in compile_units {
        if let Some(reason) = selection.skip_reason(root, &unit.source, false) {
            skipped += 1;
            explain_skip(options, &unit.source, reason);
            continue;
        }
        let source_kind = kind(&unit.source, frontends).ok_or_else(|| {
            format!(
                "unsupported compilation database source {}",
                unit.source.display()
            )
        })?;
        if !matches!(source_kind, Kind::C | Kind::Cxx | Kind::ObjC | Kind::ObjCxx) {
            return Err(format!(
                "compilation database source {} is not C-family",
                unit.source.display()
            ));
        }
        let relative = unit
            .source
            .strip_prefix(root)
            .map_err(|error| error.to_string())?;
        let relative = if let Some(variant) = &unit.variant {
            let mut path = relative.to_owned();
            let name = path
                .file_name()
                .ok_or("source has no name")?
                .to_string_lossy();
            path.set_file_name(format!("{name}.v{variant}"));
            path
        } else {
            relative.to_owned()
        };
        let output = output_path(output_root, &relative, source_kind, options);
        if !destinations.insert(output.clone()) {
            return Err(format!(
                "multiple inputs map to output {}",
                output.display()
            ));
        }
        items.push(WorkItem {
            input: unit.source.clone(),
            output,
            kind: source_kind,
            compile: Some(unit),
            source_ir: None,
        });
    }
    for unit in ir_units {
        if let Some(reason) = selection.skip_reason(root, &unit.source, false) {
            skipped += 1;
            explain_skip(options, &unit.source, reason);
            continue;
        }
        let relative = unit
            .source
            .strip_prefix(root)
            .map_err(|error| error.to_string())?;
        let output = output_path(output_root, relative, Kind::Ir, options);
        if !destinations.insert(output.clone()) {
            return Err(format!(
                "multiple inputs map to output {}",
                output.display()
            ));
        }
        items.push(WorkItem {
            input: unit.source,
            output,
            kind: Kind::Ir,
            compile: None,
            source_ir: Some(unit.ir),
        });
    }
    items.sort_by(|a, b| a.input.cmp(&b.input));
    Ok((items, skipped))
}

fn run_command(mut command: Command, label: &Path, options: &Options) -> Result<(), String> {
    let output = process::run(&mut command, label, options)?;
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed for {} with status {}",
            command.get_program().to_string_lossy(),
            label.display(),
            output.status
        ))
    }
}

fn run_opt(mut command: Command, label: &Path, options: &Options) -> Result<Diagnostics, String> {
    let output = process::run(&mut command, label, options)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if options.report || !output.status.success() {
        eprint!("{stderr}");
    }
    if !output.status.success() {
        return Err(format!(
            "{} failed for {} with status {}",
            command.get_program().to_string_lossy(),
            label.display(),
            output.status
        ));
    }
    Ok(report::parse_diagnostics(&stderr))
}

fn compile_source(
    item: &WorkItem,
    ir: &Path,
    opt: &Path,
    frontends: &Frontends,
    options: &Options,
) -> Result<(), String> {
    let llvm_bin = opt.parent().ok_or("LLVM opt has no parent directory")?;
    if let Some(unit) = &item.compile {
        return compile_database_source(unit, ir, llvm_bin, options);
    }
    let mut command = match item.kind {
        Kind::C | Kind::Cxx | Kind::ObjC | Kind::ObjCxx => {
            let compiler = if matches!(item.kind, Kind::Cxx | Kind::ObjCxx) {
                "clang++"
            } else {
                "clang"
            };
            let mut command = Command::new(llvm_bin.join(compiler));
            command.args([
                "-O1",
                "-fno-vectorize",
                "-fno-slp-vectorize",
                "-fno-unroll-loops",
                "-ffp-contract=off",
                "-emit-llvm",
                "-S",
            ]);
            command
        }
        Kind::Fortran => {
            let flang = ["flang", "flang-new"]
                .into_iter()
                .map(|name| llvm_bin.join(name))
                .find(|path| path.is_file())
                .unwrap_or_else(|| llvm_bin.join("flang"));
            let mut command = Command::new(flang);
            command.args(["-O1", "-emit-llvm", "-S"]);
            command
        }
        Kind::Rust => {
            let mut command = Command::new("rustc");
            command.args([
                "--crate-type=lib",
                "--crate-name=rv_module",
                "--emit=llvm-ir",
                "-C",
                "opt-level=1",
                "-C",
                "no-vectorize-loops",
                "-C",
                "no-vectorize-slp",
            ]);
            command
        }
        Kind::Swift => {
            let mut command = Command::new("swiftc");
            command.args(["-O", "-emit-ir"]);
            command
        }
        Kind::Custom => {
            let extension = item
                .input
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let template = &frontends[extension];
            let mut command = Command::new(&template[0]);
            for argument in &template[1..] {
                let argument = argument
                    .replace("{input}", &item.input.to_string_lossy())
                    .replace("{output}", &ir.to_string_lossy());
                command.arg(argument);
            }
            return run_command(command, &item.input, options);
        }
        Kind::Ir => return Ok(()),
    };
    command.arg(&item.input).arg("-o").arg(ir);
    run_command(command, &item.input, options)
}

fn compile_database_source(
    unit: &CompileUnit,
    ir: &Path,
    llvm_bin: &Path,
    options: &Options,
) -> Result<(), String> {
    let mut command = database_command(unit, llvm_bin)?;
    command.args([
        "-O1",
        "-fno-vectorize",
        "-fno-slp-vectorize",
        "-fno-unroll-loops",
        "-ffp-contract=off",
        "-emit-llvm",
        "-S",
        "-gline-tables-only",
    ]);
    command.arg(&unit.source).arg("-o").arg(ir);
    run_command(command, &unit.source, options)
}

fn database_command(unit: &CompileUnit, llvm_bin: &Path) -> Result<Command, String> {
    let compiler_name = Path::new(&unit.arguments[0])
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid compilation database compiler")?;
    let compiler = match compiler_name {
        "clang" | "cc" | "gcc" => "clang",
        "clang++" | "c++" | "g++" => "clang++",
        _ => {
            return Err(format!(
                "unsupported compilation database compiler {compiler_name}; expected a direct C-family compiler"
            ));
        }
    };
    let mut command = Command::new(llvm_bin.join(compiler));
    command.current_dir(&unit.directory);
    let mut consumed_source = false;
    let mut index = 1;
    while index < unit.arguments.len() {
        let argument = &unit.arguments[index];
        if matches!(argument.as_str(), "-o" | "-MF" | "-MT" | "-MQ" | "-MJ") {
            index += 2;
            if index > unit.arguments.len() {
                return Err(format!(
                    "missing value after {argument} in compilation database"
                ));
            }
            continue;
        }
        if matches!(
            argument.as_str(),
            "-c" | "-S"
                | "-E"
                | "-emit-llvm"
                | "-M"
                | "-MM"
                | "-MD"
                | "-MMD"
                | "-MP"
                | "-MG"
                | "-fsyntax-only"
                | "-save-temps"
        ) || argument.starts_with("-o") && argument.len() > 2
            || ["-MF", "-MT", "-MQ", "-MJ"]
                .iter()
                .any(|flag| argument.starts_with(flag) && argument.len() > flag.len())
        {
            index += 1;
            continue;
        }
        if !argument.starts_with('-') {
            let candidate = Path::new(argument);
            let candidate = if candidate.is_absolute() {
                candidate.to_owned()
            } else {
                unit.directory.join(candidate)
            };
            if fs::canonicalize(&candidate).ok().as_deref() == Some(unit.source.as_path()) {
                if consumed_source {
                    return Err(format!(
                        "source {} occurs more than once in compilation database arguments",
                        unit.source.display()
                    ));
                }
                consumed_source = true;
                index += 1;
                continue;
            }
        }
        command.arg(argument);
        index += 1;
    }
    if !consumed_source {
        return Err(format!(
            "source {} missing from compilation database arguments",
            unit.source.display()
        ));
    }
    Ok(command)
}

fn parse_make_dependencies(text: &str, directory: &Path) -> Option<Vec<PathBuf>> {
    let text = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let body = text.strip_prefix("rv:")?;
    let mut paths = Vec::new();
    let mut token = String::new();
    let mut escaped = false;
    for character in body.chars() {
        if escaped {
            token.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
            if !token.is_empty() {
                let path = Path::new(&token);
                let path = if path.is_absolute() {
                    path.to_owned()
                } else {
                    directory.join(path)
                };
                paths.push(fs::canonicalize(path).ok()?);
                token.clear();
            }
        } else {
            token.push(character);
        }
    }
    if escaped {
        return None;
    }
    if !token.is_empty() {
        let path = Path::new(&token);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            directory.join(path)
        };
        paths.push(fs::canonicalize(path).ok()?);
    }
    if paths.is_empty() { None } else { Some(paths) }
}

fn database_dependencies(
    unit: &CompileUnit,
    opt: &Path,
    options: &Options,
) -> Result<Option<Vec<PathBuf>>, String> {
    let llvm_bin = opt.parent().ok_or("LLVM opt has no parent directory")?;
    let mut command = database_command(unit, llvm_bin)?;
    command.args([
        "-O1",
        "-fno-vectorize",
        "-fno-slp-vectorize",
        "-fno-unroll-loops",
        "-ffp-contract=off",
        "-gline-tables-only",
    ]);
    command
        .args(["-M", "-MT", "rv", "-MF", "-"])
        .arg(&unit.source);
    let output = process::run(&mut command, &unit.source, options)?;
    if !output.status.success() {
        return Ok(None);
    }
    let Some(paths) =
        parse_make_dependencies(&String::from_utf8_lossy(&output.stdout), &unit.directory)
    else {
        return Ok(None);
    };
    for path in &paths {
        let content = fs::read(path)
            .map_err(|error| format!("cannot read dependency {}: {error}", path.display()))?;
        if [
            b"__TIME__".as_slice(),
            b"__DATE__".as_slice(),
            b"__TIMESTAMP__".as_slice(),
        ]
        .iter()
        .any(|needle| {
            content
                .windows(needle.len())
                .any(|window| window == *needle)
        }) {
            return Ok(None);
        }
    }
    Ok(Some(paths))
}

fn cache_key(
    item: &WorkItem,
    cache: &Cache,
    options: &Options,
    opt: &Path,
) -> Result<Option<String>, String> {
    if let Some(unit) = &item.compile {
        // Clang response files can change independently of `arguments` and
        // may contain nested response files. Build them afresh until the full
        // expansion is available for a cache key.
        if unit.arguments.iter().any(|argument| {
            argument.starts_with('@')
                || ["__TIME__", "__DATE__", "__TIMESTAMP__"]
                    .iter()
                    .any(|macro_name| argument.contains(macro_name))
        }) {
            return Ok(None);
        }
        let Some(dependencies) = database_dependencies(unit, opt, options)? else {
            return Ok(None);
        };
        return cache
            .key(&item.input, &unit.arguments, &dependencies)
            .map(Some);
    }
    if item.kind == Kind::Ir {
        let path = item.source_ir.as_deref().unwrap_or(&item.input);
        return cache.key(&item.input, &[], &[path.to_owned()]).map(Some);
    }
    Ok(None)
}

fn temporary(parent: &Path) -> Result<NamedTempFile, String> {
    Builder::new()
        .prefix(".rv-vectorize-")
        .tempfile_in(parent)
        .map_err(|error| {
            format!(
                "cannot create temporary file in {}: {error}",
                parent.display()
            )
        })
}

fn reject_output_alias(input: &Path, output: &Path) -> Result<(), String> {
    if fs::symlink_metadata(output).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!("refusing output symlink {}", output.display()));
    }
    if output.exists() && fs::canonicalize(output).ok() == fs::canonicalize(input).ok() {
        return Err(format!("input and output alias: {}", output.display()));
    }
    Ok(())
}

fn resolved_destination(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path)
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("cannot resolve {}: {error}", parent.display()))?;
    Ok(parent.join(path.file_name().ok_or("report path has no filename")?))
}

fn reject_report_alias(path: &Path, inputs: &[PathBuf], outputs: &[PathBuf]) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!("report path {} is a symlink", path.display()));
    }
    let report = resolved_destination(path)?;
    for candidate in inputs.iter().chain(outputs) {
        if resolved_destination(candidate).is_ok_and(|resolved| resolved == report) {
            return Err(format!(
                "report path {} aliases an input or output",
                path.display()
            ));
        }
    }
    Ok(())
}

fn check_input_size(path: &Path, options: &Options) -> Result<(), String> {
    if let Some(limit) = options.max_input_bytes {
        let length = fs::metadata(path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
            .len();
        if length > limit {
            return Err(format!(
                "{} exceeds --max-input-bytes ({length} > {limit})",
                path.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn run_single_file(options: &Options, opt: &Path, plugin: &Path) -> Result<(), String> {
    let input = options
        .input
        .as_deref()
        .expect("validated input")
        .to_owned();
    let output = options.output.clone().unwrap_or_else(|| PathBuf::from("-"));
    if let Some(path) = &options.report_json {
        let outputs = options.output.iter().cloned().collect::<Vec<_>>();
        reject_report_alias(path, std::slice::from_ref(&input), &outputs)?;
    }
    let result = run_single_file_inner(options, opt, plugin);
    if let Some(path) = &options.report_json {
        let file = match &result {
            Ok(diagnostics) => FileReport {
                input,
                output,
                status: "processed",
                diagnostics: diagnostics.clone(),
                error: None,
            },
            Err(error) => FileReport {
                input,
                output,
                status: "failed",
                diagnostics: Diagnostics::default(),
                error: Some(error.clone()),
            },
        };
        RunReport {
            schema_version: 1,
            engine: options.engine.name(),
            opt: opt.to_owned(),
            plugin: plugin.to_owned(),
            toolchain: Some(report::toolchain_info(options, opt, plugin)?),
            files: vec![file],
        }
        .write(path)?;
    }
    let diagnostics = result?;
    if let Some(limit) = options.fail_on_missed {
        let missed = diagnostics
            .loops
            .iter()
            .filter(|loop_| loop_.decision == "rejected")
            .count();
        if missed > limit {
            return Err(format!(
                "missed-loop gate failed: {missed} rejected candidate loops exceeds {limit}"
            ));
        }
    }
    Ok(())
}

fn run_single_file_inner(
    options: &Options,
    opt: &Path,
    plugin: &Path,
) -> Result<Diagnostics, String> {
    let requested_input = options.input.as_deref().expect("validated input");
    let resolved_input = fs::canonicalize(requested_input).map_err(|error| {
        format!(
            "cannot resolve input {}: {error}",
            requested_input.display()
        )
    })?;
    let input = resolved_input.as_path();
    check_input_size(input, options)?;
    let frontends = load_frontends(options.frontend_config.as_deref())?;
    let source_kind = kind(input, &frontends).unwrap_or(Kind::Ir);
    let output = options.output.as_deref();
    if let Some(output) = output {
        reject_output_alias(input, output)?;
    }
    if options.dry_run {
        let command_input = if source_kind == Kind::Ir {
            input
        } else {
            Path::new("<compiled-ir>")
        };
        if source_kind != Kind::Ir {
            println!(
                "compile {} as {:?} to temporary LLVM IR",
                input.display(),
                source_kind
            );
        }
        let command = build_command(
            options,
            opt,
            plugin,
            command_input,
            output.unwrap_or_else(|| Path::new("-")),
        );
        println!("{}", command_display(&command));
        return Ok(Diagnostics::default());
    }
    let parent = output
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let source_ir = if source_kind == Kind::Ir {
        None
    } else {
        let temporary_parent = if output.is_some() {
            parent.to_owned()
        } else {
            std::env::temp_dir()
        };
        let ir = temporary(&temporary_parent)?;
        let item = WorkItem {
            input: input.to_owned(),
            output: output.unwrap_or_else(|| Path::new("-")).to_owned(),
            kind: source_kind,
            compile: None,
            source_ir: None,
        };
        compile_source(&item, ir.path(), opt, &frontends, options)?;
        Some(ir)
    };
    let opt_input = source_ir.as_ref().map_or(input, NamedTempFile::path);
    let result = temporary(parent)?;
    let optimized = if options.emit_object {
        Some(temporary(parent)?)
    } else {
        None
    };
    let opt_output = optimized
        .as_ref()
        .map_or(result.path(), NamedTempFile::path);
    let diagnostics = run_opt(
        build_command(options, opt, plugin, opt_input, opt_output),
        input,
        options,
    )?;
    if let Some(optimized) = &optimized {
        emit_object(opt, optimized.path(), result.path(), input, options)?;
    }
    if let Some(output) = output {
        result
            .persist(output)
            .map_err(|error| format!("cannot save {}: {}", output.display(), error.error))?;
    } else {
        let mut file = result
            .reopen()
            .map_err(|error| format!("cannot read output: {error}"))?;
        std::io::copy(&mut file, &mut std::io::stdout())
            .map_err(|error| format!("cannot write stdout: {error}"))?;
    }
    Ok(diagnostics)
}

fn emit_object(
    opt: &Path,
    input: &Path,
    output: &Path,
    label: &Path,
    options: &Options,
) -> Result<(), String> {
    let llc = opt
        .parent()
        .ok_or("LLVM opt has no parent directory")?
        .join("llc");
    let mut command = Command::new(llc);
    add_target_options(&mut command, options);
    command
        .arg("-filetype=obj")
        .arg(input)
        .arg("-o")
        .arg(output);
    run_command(command, label, options)
}

fn create_safe_parent(parent: &Path, output_root: &Path) -> Result<(), String> {
    let relative = parent
        .strip_prefix(output_root)
        .map_err(|_| format!("output path escaped {}", output_root.display()))?;
    let mut cursor = output_root.to_owned();
    for component in relative.components() {
        cursor.push(component);
        if fs::symlink_metadata(&cursor).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!(
                "refusing output directory symlink {}",
                cursor.display()
            ));
        }
    }
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    if !fs::canonicalize(parent).is_ok_and(|resolved| resolved.starts_with(output_root)) {
        return Err(format!("output path escaped {}", output_root.display()));
    }
    Ok(())
}

struct ItemOutcome {
    status: &'static str,
    diagnostics: Diagnostics,
}

fn process_item(
    item: &WorkItem,
    options: &Options,
    opt: &Path,
    plugin: &Path,
    frontends: &Frontends,
    output_root: &Path,
    cache: Option<&Cache>,
) -> Result<ItemOutcome, String> {
    check_input_size(&item.input, options)?;
    if let Some(ir) = &item.source_ir {
        check_input_size(ir, options)?;
    }
    let parent = item.output.parent().ok_or("output path has no parent")?;
    create_safe_parent(parent, output_root)?;
    reject_output_alias(&item.input, &item.output)?;
    let key = cache
        .map(|cache| cache_key(item, cache, options, opt))
        .transpose()?
        .flatten();
    if let (Some(cache), Some(key)) = (cache, key.as_deref()) {
        if let Some(diagnostics) = cache.lookup(key, &item.output)? {
            return Ok(ItemOutcome {
                status: "cached",
                diagnostics,
            });
        }
    }
    let source_ir = if item.kind == Kind::Ir {
        None
    } else {
        let ir = temporary(parent)?;
        compile_source(item, ir.path(), opt, frontends, options)?;
        Some(ir)
    };
    let input = source_ir.as_ref().map_or_else(
        || item.source_ir.as_deref().unwrap_or(&item.input),
        NamedTempFile::path,
    );
    let result = temporary(parent)?;
    let optimized = if options.emit_object {
        Some(temporary(parent)?)
    } else {
        None
    };
    let opt_output = optimized
        .as_ref()
        .map_or(result.path(), NamedTempFile::path);
    let diagnostics = run_opt(
        build_command(options, opt, plugin, input, opt_output),
        &item.input,
        options,
    )?;
    if let Some(optimized) = &optimized {
        emit_object(opt, optimized.path(), result.path(), &item.input, options)?;
    }
    result
        .persist(&item.output)
        .map_err(|error| format!("cannot save {}: {}", item.output.display(), error.error))?;
    if let (Some(cache), Some(key)) = (cache, key.as_deref()) {
        if let Err(error) = cache.store(key, &item.output, &diagnostics) {
            eprintln!("rv-vectorize: cache warning: {error}");
        }
    }
    Ok(ItemOutcome {
        status: "processed",
        diagnostics,
    })
}

fn process_items(
    items: &[WorkItem],
    options: &Options,
    opt: &Path,
    plugin: &Path,
    frontends: &Frontends,
    output_root: &Path,
    cache: Option<&Cache>,
) -> Vec<Result<ItemOutcome, String>> {
    if options.jobs == 1 {
        return items
            .iter()
            .map(|item| process_item(item, options, opt, plugin, frontends, output_root, cache))
            .collect();
    }
    let cursor = AtomicUsize::new(0);
    let slots = Mutex::new((0..items.len()).map(|_| None).collect::<Vec<_>>());
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.min(items.len()) {
            let cursor = &cursor;
            let slots = &slots;
            scope.spawn(move || {
                loop {
                    let index = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result =
                        process_item(item, options, opt, plugin, frontends, output_root, cache);
                    slots.lock().expect("result mutex poisoned")[index] = Some(result);
                }
            });
        }
    });
    slots
        .into_inner()
        .expect("result mutex poisoned")
        .into_iter()
        .map(|result| result.expect("worker omitted a result"))
        .collect()
}

fn prepare_output_root(root: &Path, output: &Path, dry_run: bool) -> Result<PathBuf, String> {
    if fs::symlink_metadata(output).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!(
            "refusing output directory symlink {}",
            output.display()
        ));
    }
    let output_existed = output.exists();
    if !dry_run {
        fs::create_dir_all(output).map_err(|error| {
            format!(
                "cannot create output directory {}: {error}",
                output.display()
            )
        })?;
    }
    let output_root = if output.exists() {
        fs::canonicalize(output)
            .map_err(|error| format!("cannot resolve output {}: {error}", output.display()))?
    } else if output.is_absolute() {
        output.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(output)
    };
    if root == output_root || root.starts_with(&output_root) {
        return Err("output directory must not be the input or its ancestor".to_owned());
    }
    if output_root.starts_with(root) {
        let marker = output_root.join(OUTPUT_MARKER);
        if output_existed {
            if !is_marked_output(&output_root) {
                return Err(format!(
                    "existing project subdirectory {} is not a rv-vectorize output directory",
                    output_root.display()
                ));
            }
        } else if !dry_run {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(|error| {
                    format!(
                        "cannot mark output directory {}: {error}",
                        output_root.display()
                    )
                })?;
            file.write_all(OUTPUT_MARKER_CONTENT.as_bytes())
                .map_err(|error| {
                    format!(
                        "cannot mark output directory {}: {error}",
                        output_root.display()
                    )
                })?;
        }
    }
    Ok(output_root)
}

#[allow(clippy::too_many_lines)]
pub(super) fn run_directory(options: &Options, opt: &Path, plugin: &Path) -> Result<(), String> {
    let input = options.input.as_ref().expect("validated input");
    let output = options
        .output
        .as_ref()
        .ok_or("project input requires -o/--output directory")?;
    let root = fs::canonicalize(input)
        .map_err(|error| format!("cannot resolve input {}: {error}", input.display()))?;
    let output_root = prepare_output_root(&root, output, options.dry_run)?;
    let cache = if options.dry_run {
        None
    } else {
        options
            .cache_dir
            .as_deref()
            .map(|path| Cache::new(path, &root, &output_root, options, opt, plugin))
            .transpose()?
    };
    let frontends = load_frontends(options.frontend_config.as_deref())?;
    let compile_units = options.compile_commands.as_deref().map_or_else(
        || Ok(Vec::new()),
        |path| inputs::load_compile_commands(path, &root),
    )?;
    let ir_units = options.input_manifest.as_deref().map_or_else(
        || Ok(Vec::new()),
        |path| inputs::load_ir_manifest(path, &root),
    )?;
    let (items, skipped) = collect(
        &root,
        &output_root,
        &frontends,
        options,
        compile_units,
        ir_units,
        options.compile_commands.is_some(),
    )?;
    if let Some(path) = &options.report_json {
        if resolved_destination(path)
            .is_ok_and(|report| report.starts_with(&root) && !report.starts_with(&output_root))
        {
            return Err(format!(
                "report path {} aliases the input project tree",
                path.display()
            ));
        }
        reject_report_alias(
            path,
            &items
                .iter()
                .map(|item| item.input.clone())
                .collect::<Vec<_>>(),
            &items
                .iter()
                .map(|item| item.output.clone())
                .collect::<Vec<_>>(),
        )?;
    }
    if items.is_empty() {
        return Err(format!(
            "no supported LLVM IR or source files found in {}",
            root.display()
        ));
    }
    if options.dry_run {
        for item in &items {
            println!(
                "{} -> {} ({:?})",
                item.input.display(),
                item.output.display(),
                item.kind
            );
        }
        println!("planned={} skipped={skipped}", items.len());
        return Ok(());
    }
    let mut processed = 0;
    let mut failed = 0;
    let mut file_reports = Vec::with_capacity(items.len());
    let outcomes = process_items(
        &items,
        options,
        opt,
        plugin,
        &frontends,
        &output_root,
        cache.as_ref(),
    );
    for (item, outcome) in items.iter().zip(outcomes) {
        if options.report {
            eprintln!("rv-vectorize: input={}", item.input.display());
        }
        match outcome {
            Ok(outcome) => {
                processed += 1;
                file_reports.push(FileReport {
                    input: item.input.clone(),
                    output: item.output.clone(),
                    status: outcome.status,
                    diagnostics: outcome.diagnostics,
                    error: None,
                });
            }
            Err(error) => {
                failed += 1;
                eprintln!("rv-vectorize: error: {error}");
                file_reports.push(FileReport {
                    input: item.input.clone(),
                    output: item.output.clone(),
                    status: "failed",
                    diagnostics: Diagnostics::default(),
                    error: Some(error),
                });
            }
        }
    }
    let report = RunReport {
        schema_version: 1,
        engine: options.engine.name(),
        opt: opt.to_owned(),
        plugin: plugin.to_owned(),
        toolchain: options
            .report_json
            .as_ref()
            .map(|_| report::toolchain_info(options, opt, plugin))
            .transpose()?,
        files: file_reports,
    };
    if let Some(path) = &options.report_json {
        report.write(path)?;
    }
    println!(
        "processed={processed} failed={failed} skipped={skipped} output={}",
        output_root.display()
    );
    if let Some(limit) = options.fail_on_missed {
        let missed = report.missed();
        if missed > limit {
            return Err(format!(
                "missed-loop gate failed: {missed} rejected candidate loops exceeds {limit}"
            ));
        }
    }
    if failed > 0 {
        Err(format!("{failed} project file(s) failed"))
    } else {
        Ok(())
    }
}
