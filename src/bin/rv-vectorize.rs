use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[path = "rv-vectorize/cache.rs"]
mod cache;
#[path = "rv-vectorize/compare.rs"]
mod compare;
#[path = "rv-vectorize/doctor.rs"]
mod doctor;
#[path = "rv-vectorize/inputs.rs"]
mod inputs;
#[path = "rv-vectorize/process.rs"]
mod process;
#[path = "rv-vectorize/project.rs"]
mod project;
#[path = "rv-vectorize/report.rs"]
mod report;

const HELP: &str = r"rv-vectorize - run the Rust LLVM loop-vectorization pass

USAGE:
    rv-vectorize [OPTIONS] <INPUT.ll|INPUT.bc|SOURCE_FILE|PROJECT_DIR>

OPTIONS:
    -o, --output <PATH>       Output file, or output directory for project mode
        --frontend-config <PATH>
                            JSON extension-to-argv map for additional LLVM frontends
        --compile-commands <PATH>
                            Clang JSON compilation database (arguments entries)
        --input-manifest <PATH>
                            JSON list of project source identities and emitted LLVM IR
        --include <GLOB>      Select matching project paths (repeatable)
        --exclude <GLOB>      Exclude matching project paths (repeatable)
        --no-gitignore        Ignore .gitignore rules while scanning
        --explain-skips       Print why project paths are skipped
        --jobs <N>            Process up to N modules concurrently
        --cache-dir <PATH>    Reuse verified artifacts when inputs match
        --timeout-seconds <N> Kill a subprocess after N seconds
        --max-memory-mib <N>  Limit subprocess memory usage
        --max-input-bytes <N> Reject larger source or IR files
        --compare-config <PATH>
                            Compare explicit baseline and candidate programs
        --compare-auto        Build and compare a standalone C, C++, or LLVM IR program
        --compare-report-json  Emit bounded program output and timings for the TUI
        --policy <POLICY>     balanced, conservative, or aggressive
        --vf <WIDTH>          auto, 2, 4, 8, or 16
        --engine <ENGINE>     rust, hybrid, or llvm
        --target-triple <TRIPLE>
                            LLVM target triple override
        --cpu <CPU>           LLVM target CPU for planning and object emission
        --features <FEATURES>
                            LLVM target feature list, e.g. +avx2,-avx512f
        --report              Print per-loop decisions to stderr
        --report-json <PATH>  Write per-file and per-loop results as JSON
        --fail-on-missed <N>  Fail when rejected candidate loops exceed N
        --emit-bitcode        Emit LLVM bitcode instead of textual IR
        --emit-object         Emit linkable native object files
        --no-verify           Do not run LLVM's verifier after the pass
        --llvm-prefix <PATH>  LLVM 21 installation prefix
        --opt <PATH>          Path to LLVM 21 opt
        --plugin <PATH>       Path to the vectorizer pass plugin
        --doctor              Check LLVM 21 tools and plugin loading
        --dry-run             Print the resolved opt command without running it
    -h, --help                Print help
    -V, --version             Print version

ENVIRONMENT:
    LLVM_SYS_211_PREFIX       LLVM 21 installation prefix
    LLVM_CONFIG_PATH         Path to llvm-config (its sibling opt is used)
    RV_PLUGIN_PATH           Path to the vectorizer pass plugin

Directories are scanned recursively. LLVM IR/bitcode and recognized source
languages are emitted as separate LLVM modules under the output directory.
Other files are skipped. The compiler frontends must emit LLVM 21-compatible IR.
";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Policy {
    #[default]
    Balanced,
    Conservative,
    Aggressive,
}

impl Policy {
    fn parse(value: &OsStr) -> Result<Self, String> {
        match value.to_str() {
            Some("balanced") => Ok(Self::Balanced),
            Some("conservative") => Ok(Self::Conservative),
            Some("aggressive") => Ok(Self::Aggressive),
            _ => Err(format!(
                "invalid policy {value:?}; expected balanced, conservative, or aggressive"
            )),
        }
    }

    const fn pass_stem(self) -> &'static str {
        match self {
            Self::Balanced => "rust-loop-vectorize",
            Self::Conservative => "rust-loop-vectorize-conservative",
            Self::Aggressive => "rust-loop-vectorize-aggressive",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum VectorWidth {
    #[default]
    Auto,
    Fixed(u8),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Engine {
    #[default]
    Rust,
    Hybrid,
    Llvm,
}

impl Engine {
    fn parse(value: &OsStr) -> Result<Self, String> {
        match value.to_str() {
            Some("rust") => Ok(Self::Rust),
            Some("hybrid") => Ok(Self::Hybrid),
            Some("llvm") => Ok(Self::Llvm),
            _ => Err(format!(
                "invalid engine {value:?}; expected rust, hybrid, or llvm"
            )),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Hybrid => "hybrid",
            Self::Llvm => "llvm",
        }
    }
}

impl VectorWidth {
    fn parse(value: &OsStr) -> Result<Self, String> {
        match value.to_str() {
            Some("auto") => Ok(Self::Auto),
            Some("2") => Ok(Self::Fixed(2)),
            Some("4") => Ok(Self::Fixed(4)),
            Some("8") => Ok(Self::Fixed(8)),
            Some("16") => Ok(Self::Fixed(16)),
            _ => Err(format!(
                "invalid vector width {value:?}; expected auto, 2, 4, 8, or 16"
            )),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
struct Options {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    policy: Policy,
    vector_width: VectorWidth,
    engine: Engine,
    target_triple: Option<String>,
    cpu: Option<String>,
    features: Option<String>,
    report: bool,
    report_json: Option<PathBuf>,
    fail_on_missed: Option<usize>,
    emit_bitcode: bool,
    emit_object: bool,
    verify: bool,
    llvm_prefix: Option<PathBuf>,
    opt: Option<PathBuf>,
    plugin: Option<PathBuf>,
    frontend_config: Option<PathBuf>,
    compile_commands: Option<PathBuf>,
    input_manifest: Option<PathBuf>,
    include: Vec<String>,
    exclude: Vec<String>,
    no_gitignore: bool,
    explain_skips: bool,
    doctor: bool,
    jobs: usize,
    cache_dir: Option<PathBuf>,
    timeout_seconds: Option<u64>,
    max_memory_mib: Option<u64>,
    max_input_bytes: Option<u64>,
    compare_config: Option<PathBuf>,
    compare_auto: bool,
    compare_report_json: bool,
    dry_run: bool,
}

enum Parsed {
    Run(Box<Options>),
    Help,
    Version,
}

fn take_value(arguments: &[OsString], index: &mut usize, option: &str) -> Result<OsString, String> {
    *index += 1;
    arguments
        .get(*index)
        .cloned()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn set_input(input: &mut Option<PathBuf>, value: &OsStr) -> Result<(), String> {
    if input.is_some() {
        return Err(format!("unexpected extra input {value:?}"));
    }
    *input = Some(PathBuf::from(value));
    Ok(())
}

fn parse_positive(value: &OsStr, option: &str) -> Result<u64, String> {
    let number: u64 = value
        .to_str()
        .ok_or_else(|| format!("{option} needs a positive integer"))?
        .parse()
        .map_err(|_| format!("{option} needs a positive integer"))?;
    if number == 0 {
        return Err(format!("{option} needs a positive integer"));
    }
    Ok(number)
}

fn parse_jobs(value: &OsStr) -> Result<usize, String> {
    let value = parse_positive(value, "--jobs")?;
    usize::try_from(value).map_err(|_| "--jobs is too large".to_owned())
}

#[allow(clippy::too_many_lines)]
fn parse_args(arguments: &[OsString]) -> Result<Parsed, String> {
    let mut options = Options {
        verify: true,
        jobs: 1,
        ..Options::default()
    };
    let mut index = 0;
    let mut positional_only = false;

    while index < arguments.len() {
        let argument = &arguments[index];
        let text = argument.to_str();

        if positional_only {
            set_input(&mut options.input, argument)?;
        } else if text == Some("--") {
            positional_only = true;
        } else if matches!(text, Some("-h" | "--help")) {
            return Ok(Parsed::Help);
        } else if matches!(text, Some("-V" | "--version")) {
            return Ok(Parsed::Version);
        } else if matches!(text, Some("-o" | "--output")) {
            options.output = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                text.unwrap_or("--output"),
            )?));
        } else if text == Some("--policy") {
            options.policy = Policy::parse(&take_value(arguments, &mut index, "--policy")?)?;
        } else if text == Some("--vf") {
            options.vector_width = VectorWidth::parse(&take_value(arguments, &mut index, "--vf")?)?;
        } else if text == Some("--engine") {
            options.engine = Engine::parse(&take_value(arguments, &mut index, "--engine")?)?;
        } else if text == Some("--target-triple") {
            options.target_triple = Some(
                take_value(arguments, &mut index, "--target-triple")?
                    .to_string_lossy()
                    .into_owned(),
            );
        } else if text == Some("--cpu") {
            options.cpu = Some(
                take_value(arguments, &mut index, "--cpu")?
                    .to_string_lossy()
                    .into_owned(),
            );
        } else if text == Some("--features") {
            options.features = Some(
                take_value(arguments, &mut index, "--features")?
                    .to_string_lossy()
                    .into_owned(),
            );
        } else if text == Some("--llvm-prefix") {
            options.llvm_prefix = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--llvm-prefix",
            )?));
        } else if text == Some("--opt") {
            options.opt = Some(PathBuf::from(take_value(arguments, &mut index, "--opt")?));
        } else if text == Some("--plugin") {
            options.plugin = Some(PathBuf::from(take_value(
                arguments, &mut index, "--plugin",
            )?));
        } else if text == Some("--frontend-config") {
            options.frontend_config = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--frontend-config",
            )?));
        } else if text == Some("--compile-commands") {
            options.compile_commands = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--compile-commands",
            )?));
        } else if text == Some("--input-manifest") {
            options.input_manifest = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--input-manifest",
            )?));
        } else if text == Some("--include") {
            options.include.push(
                take_value(arguments, &mut index, "--include")?
                    .to_string_lossy()
                    .into_owned(),
            );
        } else if text == Some("--exclude") {
            options.exclude.push(
                take_value(arguments, &mut index, "--exclude")?
                    .to_string_lossy()
                    .into_owned(),
            );
        } else if text == Some("--no-gitignore") {
            options.no_gitignore = true;
        } else if text == Some("--explain-skips") {
            options.explain_skips = true;
        } else if text == Some("--doctor") {
            options.doctor = true;
        } else if text == Some("--jobs") {
            options.jobs = parse_jobs(&take_value(arguments, &mut index, "--jobs")?)?;
        } else if text == Some("--cache-dir") {
            options.cache_dir = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--cache-dir",
            )?));
        } else if text == Some("--timeout-seconds") {
            options.timeout_seconds = Some(parse_positive(
                &take_value(arguments, &mut index, "--timeout-seconds")?,
                "--timeout-seconds",
            )?);
        } else if text == Some("--max-memory-mib") {
            options.max_memory_mib = Some(parse_positive(
                &take_value(arguments, &mut index, "--max-memory-mib")?,
                "--max-memory-mib",
            )?);
        } else if text == Some("--max-input-bytes") {
            options.max_input_bytes = Some(parse_positive(
                &take_value(arguments, &mut index, "--max-input-bytes")?,
                "--max-input-bytes",
            )?);
        } else if text == Some("--compare-config") {
            options.compare_config = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--compare-config",
            )?));
        } else if text == Some("--compare-report-json") {
            options.compare_report_json = true;
        } else if text == Some("--compare-auto") {
            options.compare_auto = true;
        } else if text == Some("--report") {
            options.report = true;
        } else if text == Some("--report-json") {
            options.report_json = Some(PathBuf::from(take_value(
                arguments,
                &mut index,
                "--report-json",
            )?));
        } else if text == Some("--fail-on-missed") {
            let value = take_value(arguments, &mut index, "--fail-on-missed")?;
            options.fail_on_missed = Some(
                value
                    .to_string_lossy()
                    .parse()
                    .map_err(|_| "--fail-on-missed needs a nonnegative integer".to_owned())?,
            );
        } else if text == Some("--emit-bitcode") {
            options.emit_bitcode = true;
        } else if text == Some("--emit-object") {
            options.emit_object = true;
        } else if text == Some("--no-verify") {
            options.verify = false;
        } else if text == Some("--dry-run") {
            options.dry_run = true;
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--output=")) {
            options.output = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--policy=")) {
            options.policy = Policy::parse(OsStr::new(value))?;
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--vf=")) {
            options.vector_width = VectorWidth::parse(OsStr::new(value))?;
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--engine=")) {
            options.engine = Engine::parse(OsStr::new(value))?;
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--target-triple=")) {
            options.target_triple = Some(value.to_owned());
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--cpu=")) {
            options.cpu = Some(value.to_owned());
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--features=")) {
            options.features = Some(value.to_owned());
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--llvm-prefix=")) {
            options.llvm_prefix = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--opt=")) {
            options.opt = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--plugin=")) {
            options.plugin = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--frontend-config="))
        {
            options.frontend_config = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--compile-commands="))
        {
            options.compile_commands = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--input-manifest=")) {
            options.input_manifest = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--report-json=")) {
            options.report_json = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--fail-on-missed=")) {
            options.fail_on_missed = Some(
                value
                    .parse()
                    .map_err(|_| "--fail-on-missed needs a nonnegative integer".to_owned())?,
            );
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--include=")) {
            options.include.push(value.to_owned());
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--exclude=")) {
            options.exclude.push(value.to_owned());
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--jobs=")) {
            options.jobs = parse_jobs(OsStr::new(value))?;
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--cache-dir=")) {
            options.cache_dir = Some(PathBuf::from(value));
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--timeout-seconds="))
        {
            options.timeout_seconds = Some(parse_positive(OsStr::new(value), "--timeout-seconds")?);
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--max-memory-mib=")) {
            options.max_memory_mib = Some(parse_positive(OsStr::new(value), "--max-memory-mib")?);
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--max-input-bytes="))
        {
            options.max_input_bytes = Some(parse_positive(OsStr::new(value), "--max-input-bytes")?);
        } else if let Some(value) = text.and_then(|value| value.strip_prefix("--compare-config=")) {
            options.compare_config = Some(PathBuf::from(value));
        } else if text.is_some_and(|value| value.starts_with('-')) {
            return Err(format!("unknown option {argument:?}"));
        } else {
            set_input(&mut options.input, argument)?;
        }
        index += 1;
    }

    if options.input.is_none() && !options.doctor {
        return Err("missing input LLVM IR, bitcode, or project directory".to_owned());
    }
    if options.emit_bitcode && options.emit_object {
        return Err("--emit-bitcode and --emit-object cannot be combined".to_owned());
    }
    if options.jobs > 256 {
        return Err("--jobs must be at most 256".to_owned());
    }
    if options.emit_object && options.output.is_none() {
        return Err("--emit-object requires -o/--output".to_owned());
    }
    if options.dry_run && (options.report_json.is_some() || options.fail_on_missed.is_some()) {
        return Err("report and missed-loop gates require a real run".to_owned());
    }
    if options.dry_run && (options.compare_config.is_some() || options.compare_auto) {
        return Err("program comparison requires a real run".to_owned());
    }
    if options.compare_auto && options.compare_config.is_some() {
        return Err("--compare-auto and --compare-config cannot be combined".to_owned());
    }
    if options.compare_auto && options.input.as_ref().is_some_and(|input| input.is_dir()) {
        return Err("--compare-auto requires a standalone file; use --compare-config for a project directory".to_owned());
    }
    if options.compare_report_json && options.compare_config.is_none() && !options.compare_auto {
        return Err("--compare-report-json requires a comparison mode".to_owned());
    }
    if options.input.as_ref().is_some_and(|input| input.is_file())
        && (options.compile_commands.is_some()
            || options.input_manifest.is_some()
            || !options.include.is_empty()
            || !options.exclude.is_empty()
            || options.no_gitignore
            || options.explain_skips
            || options.jobs != 1
            || options.cache_dir.is_some())
    {
        return Err("project selection, build metadata, jobs, and cache options require a project directory".to_owned());
    }
    if options.policy != Policy::Balanced && options.vector_width != VectorWidth::Auto {
        return Err("--policy and a fixed --vf cannot be combined".to_owned());
    }
    if options.engine != Engine::Rust && options.fail_on_missed.is_some() {
        return Err("--fail-on-missed currently measures only the Rust engine".to_owned());
    }
    if options.engine == Engine::Llvm && options.policy != Policy::Balanced {
        return Err("--policy applies only when the Rust pass runs".to_owned());
    }
    if [
        options.target_triple.as_deref(),
        options.cpu.as_deref(),
        options.features.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(str::is_empty)
    {
        return Err("target triple, CPU, and features values must be nonempty".to_owned());
    }
    if let (Some(input), Some(output)) = (&options.input, &options.output) {
        if input == output {
            return Err("input and output paths must differ".to_owned());
        }
    }
    Ok(Parsed::Run(Box::new(options)))
}

fn pass_pipeline(options: &Options) -> String {
    let mut pass = match options.vector_width {
        VectorWidth::Auto => options.policy.pass_stem().to_owned(),
        VectorWidth::Fixed(width) => format!("rust-loop-vectorize-force-vf{width}"),
    };
    if options.report || options.report_json.is_some() || options.fail_on_missed.is_some() {
        pass.push_str("-report");
    }
    let mut passes = Vec::new();
    if options.engine != Engine::Llvm {
        passes.push(pass);
    }
    if options.engine != Engine::Rust {
        passes.push("function(loop-simplify,loop-vectorize)".to_owned());
    }
    if options.verify {
        passes.push("verify".to_owned());
    }
    passes.join(",")
}

fn llvm_prefix_candidates(options: &Options) -> Vec<PathBuf> {
    if let Some(prefix) = &options.llvm_prefix {
        return vec![prefix.clone()];
    }

    let mut candidates = Vec::new();
    if let Some(prefix) = env::var_os("LLVM_SYS_211_PREFIX") {
        candidates.push(PathBuf::from(prefix));
    }
    if let Some(config) = env::var_os("LLVM_CONFIG_PATH") {
        let config = PathBuf::from(config);
        if let Some(prefix) = config.parent().and_then(Path::parent) {
            candidates.push(prefix.to_owned());
        }
    }
    candidates.extend(
        [
            "/opt/homebrew/opt/llvm",
            "/usr/local/opt/llvm",
            "/usr/lib/llvm-21",
        ]
        .map(PathBuf::from),
    );
    candidates
}

fn is_llvm_21_opt(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("LLVM version 21.")
        })
}

fn resolve_opt(options: &Options) -> Result<PathBuf, String> {
    if let Some(path) = &options.opt {
        if is_llvm_21_opt(path) {
            return Ok(path.clone());
        }
        return Err(format!(
            "{} is not an executable LLVM 21 opt",
            path.display()
        ));
    }
    for prefix in llvm_prefix_candidates(options) {
        let candidate = prefix.join("bin/opt");
        if is_llvm_21_opt(&candidate) {
            return Ok(candidate);
        }
    }
    Err("LLVM 21 opt was not found; use --opt, --llvm-prefix, or LLVM_SYS_211_PREFIX".to_owned())
}

fn plugin_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "librust_loop_vectorizer.dylib"
    } else if cfg!(target_os = "windows") {
        "rust_loop_vectorizer.dll"
    } else {
        "librust_loop_vectorizer.so"
    }
}

fn resolve_plugin(options: &Options) -> Result<PathBuf, String> {
    if let Some(path) = &options.plugin {
        return path
            .is_file()
            .then(|| path.clone())
            .ok_or_else(|| format!("pass plugin not found at {}", path.display()));
    }
    if let Some(path) = env::var_os("RV_PLUGIN_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "pass plugin from RV_PLUGIN_PATH not found at {}",
            path.display()
        ));
    }

    let mut candidates = Vec::new();
    if let Ok(executable) = env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join(plugin_filename()));
        }
    }
    if let Ok(directory) = env::current_dir() {
        candidates.push(directory.join("target/release").join(plugin_filename()));
        candidates.push(directory.join("target/debug").join(plugin_filename()));
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "pass plugin was not found; run cargo build --release or use --plugin".to_owned()
        })
}

fn shell_quote(value: &OsStr) -> String {
    let text = value.to_string_lossy();
    if !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_+-.,/:=".contains(&byte))
    {
        return text.into_owned();
    }
    let mut quoted = String::from("'");
    for character in text.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            let _ = quoted.write_char(character);
        }
    }
    quoted.push('\'');
    quoted
}

fn command_display(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_command(
    options: &Options,
    opt: &Path,
    plugin: &Path,
    input: &Path,
    output: &Path,
) -> Command {
    let mut command = Command::new(opt);
    add_target_options(&mut command, options);
    if options.engine != Engine::Rust {
        if let VectorWidth::Fixed(width) = options.vector_width {
            command.arg(format!("-force-vector-width={width}"));
        }
        if options.report || options.report_json.is_some() {
            command.args([
                "-pass-remarks=loop-vectorize",
                "-pass-remarks-missed=loop-vectorize",
                "-pass-remarks-analysis=loop-vectorize",
            ]);
        }
    }
    command.arg(format!("-load-pass-plugin={}", plugin.display()));
    command.arg(format!("-passes={}", pass_pipeline(options)));
    if !options.emit_bitcode && !options.emit_object {
        command.arg("-S");
    }
    command.arg(input);
    command.arg("-o");
    command.arg(output);
    command
}

fn add_target_options(command: &mut Command, options: &Options) {
    if let Some(triple) = &options.target_triple {
        command.arg(format!("-mtriple={triple}"));
    }
    if let Some(cpu) = &options.cpu {
        command.arg(format!("-mcpu={cpu}"));
    }
    if let Some(features) = &options.features {
        command.arg(format!("-mattr={features}"));
    }
}

fn run(options: &Options) -> Result<(), String> {
    if options.doctor {
        return doctor::run(options);
    }
    let input = options.input.as_ref().expect("validated input");
    if !input.is_file() && !input.is_dir() {
        return Err(format!(
            "input file or directory not found at {}",
            input.display()
        ));
    }
    let opt = resolve_opt(options)?;
    let plugin = resolve_plugin(options)?;
    if input.is_dir() {
        project::run_directory(options, &opt, &plugin)?;
    } else {
        project::run_single_file(options, &opt, &plugin)?;
    }
    if let Some(config) = &options.compare_config {
        compare::run(
            config,
            input,
            options.output.as_deref().unwrap_or_else(|| Path::new("-")),
            options,
        )?;
    } else if options.compare_auto {
        compare::run_auto(
            input,
            options.output.as_deref().unwrap_or_else(|| Path::new("-")),
            options,
        )?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    match parse_args(&arguments) {
        Ok(Parsed::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Version) => {
            println!("rv-vectorize {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(options)) => match run(&options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("rv-vectorize: error: {error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("rv-vectorize: error: {error}\n\nTry 'rv-vectorize --help' for usage.");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_normal_invocation() {
        let Parsed::Run(options) = parse_args(&args(&[
            "--policy",
            "aggressive",
            "--report",
            "-o",
            "out.ll",
            "in.ll",
        ]))
        .expect("valid arguments") else {
            panic!("expected runnable options");
        };
        assert_eq!(options.input, Some(PathBuf::from("in.ll")));
        assert_eq!(options.output, Some(PathBuf::from("out.ll")));
        assert_eq!(options.policy, Policy::Aggressive);
        assert!(options.report);
    }

    #[test]
    fn builds_balanced_pipeline_with_verifier() {
        let options = Options {
            verify: true,
            report: true,
            ..Options::default()
        };
        assert_eq!(pass_pipeline(&options), "rust-loop-vectorize-report,verify");
    }

    #[test]
    fn builds_forced_width_pipeline() {
        let options = Options {
            vector_width: VectorWidth::Fixed(8),
            verify: false,
            ..Options::default()
        };
        assert_eq!(pass_pipeline(&options), "rust-loop-vectorize-force-vf8");
    }

    #[test]
    fn rejects_policy_with_fixed_width() {
        let result = parse_args(&args(&["--policy=aggressive", "--vf=4", "in.ll"]));
        assert!(matches!(result, Err(message) if message.contains("cannot be combined")));
    }

    #[test]
    fn quotes_shell_metacharacters() {
        assert_eq!(shell_quote(OsStr::new("plain.ll")), "plain.ll");
        assert_eq!(
            shell_quote(OsStr::new("it's here.ll")),
            "'it'\\''s here.ll'"
        );
    }

    #[test]
    fn rejects_project_selection_and_cache_for_single_files() {
        for option in ["--cache-dir", "--include"] {
            let values = if option == "--cache-dir" {
                [option, "cache", "tests/fixtures/vectorizable.ll"]
            } else {
                [option, "*.ll", "tests/fixtures/vectorizable.ll"]
            };
            assert!(
                matches!(parse_args(&args(&values)), Err(message) if message.contains("project directory"))
            );
        }
    }
}
