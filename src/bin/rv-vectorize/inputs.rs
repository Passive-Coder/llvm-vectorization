//! Build metadata inputs. All commands are argv arrays; no shell parsing occurs.
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(super) struct CompileUnit {
    pub(super) source: PathBuf,
    pub(super) directory: PathBuf,
    pub(super) arguments: Vec<String>,
    pub(super) variant: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct IrUnit {
    pub(super) source: PathBuf,
    pub(super) ir: PathBuf,
}

#[derive(Deserialize)]
struct RawCompileUnit {
    directory: PathBuf,
    file: PathBuf,
    arguments: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawIrUnit {
    file: PathBuf,
    ir: PathBuf,
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if metadata.len() > 32 * 1024 * 1024 {
        return Err(format!("{} exceeds 32 MiB", path.display()));
    }
    let content =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&content)
        .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))
}

fn canonical_project_file(root: &Path, path: &Path, label: &str) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(format!(
            "{label} {} must be a regular file inside {}",
            path.display(),
            root.display()
        ));
    }
    Ok(resolved)
}

pub(super) fn load_compile_commands(path: &Path, root: &Path) -> Result<Vec<CompileUnit>, String> {
    let entries: Vec<RawCompileUnit> = read_json(path)?;
    let database_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut units = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let directory = if entry.directory.is_absolute() {
            entry.directory
        } else {
            database_dir.join(entry.directory)
        };
        let directory = fs::canonicalize(&directory).map_err(|error| {
            format!(
                "compilation database entry {} has invalid directory {}: {error}",
                index,
                directory.display()
            )
        })?;
        let source = if entry.file.is_absolute() {
            entry.file
        } else {
            directory.join(entry.file)
        };
        let source = canonical_project_file(root, &source, "compilation database source")?;
        let arguments = entry.arguments.ok_or_else(|| {
            format!(
                "compilation database entry {index} needs an arguments array; command strings are not executed"
            )
        })?;
        if arguments.is_empty() || arguments[0].is_empty() {
            return Err(format!(
                "compilation database entry {index} has no compiler"
            ));
        }
        units.push(CompileUnit {
            source,
            directory,
            arguments,
            variant: None,
        });
    }
    let mut counts = HashMap::new();
    for unit in &units {
        *counts.entry(unit.source.clone()).or_insert(0_usize) += 1;
    }
    for (ordinal, unit) in units.iter_mut().enumerate() {
        if counts[&unit.source] > 1 {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&ordinal.to_le_bytes());
            hasher.update(unit.directory.as_os_str().as_encoded_bytes());
            for argument in &unit.arguments {
                hasher.update(&(argument.len() as u64).to_le_bytes());
                hasher.update(argument.as_bytes());
            }
            unit.variant = Some(hasher.finalize().to_hex()[..12].to_owned());
        }
    }
    Ok(units)
}

pub(super) fn load_ir_manifest(path: &Path, root: &Path) -> Result<Vec<IrUnit>, String> {
    let entries: Vec<RawIrUnit> = read_json(path)?;
    let manifest_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut units = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.file.is_absolute() {
            return Err("IR manifest file identities must be project-relative".to_owned());
        }
        let source = canonical_project_file(root, &root.join(entry.file), "IR manifest source")?;
        let ir = if entry.ir.is_absolute() {
            entry.ir
        } else {
            manifest_dir.join(entry.ir)
        };
        let ir = fs::canonicalize(&ir)
            .map_err(|error| format!("cannot resolve manifest IR {}: {error}", ir.display()))?;
        if !ir.is_file()
            || !matches!(
                ir.extension().and_then(|ext| ext.to_str()),
                Some("ll" | "bc")
            )
        {
            return Err(format!(
                "manifest IR {} must be a .ll or .bc file",
                ir.display()
            ));
        }
        units.push(IrUnit { source, ir });
    }
    Ok(units)
}
