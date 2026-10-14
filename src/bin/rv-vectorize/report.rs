//! Versioned, atomic CLI result records.
use super::Options;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::Builder;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct SourceLocation {
    pub(super) file: String,
    pub(super) line: u32,
    pub(super) column: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct LoopDecision {
    pub(super) function: String,
    #[serde(rename = "loop")]
    pub(super) loop_name: String,
    pub(super) decision: String,
    pub(super) reason: String,
    pub(super) vf: u32,
    pub(super) estimated_vector_coverage: u64,
    pub(super) issued_lane_utilization: u64,
    pub(super) analysis_us: f64,
    pub(super) transform_us: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) source: Option<SourceLocation>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Diagnostics {
    pub(super) loops: Vec<LoopDecision>,
    pub(super) llvm_remarks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct FileReport {
    pub(super) input: PathBuf,
    pub(super) output: PathBuf,
    pub(super) status: &'static str,
    #[serde(flatten)]
    pub(super) diagnostics: Diagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct RunReport {
    pub(super) schema_version: u32,
    pub(super) engine: &'static str,
    pub(super) opt: PathBuf,
    pub(super) plugin: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) toolchain: Option<ToolchainInfo>,
    pub(super) files: Vec<FileReport>,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolchainInfo {
    pub(super) llvm_version: String,
    pub(super) clang_version: Option<String>,
    pub(super) plugin_blake3: String,
    pub(super) target_triple: Option<String>,
    pub(super) cpu: Option<String>,
    pub(super) features: Option<String>,
}

pub(super) fn toolchain_info(
    options: &Options,
    opt: &Path,
    plugin: &Path,
) -> Result<ToolchainInfo, String> {
    let version = Command::new(opt)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot read LLVM version: {error}"))?;
    if !version.status.success() {
        return Err("LLVM opt version probe failed".to_owned());
    }
    let text = String::from_utf8_lossy(&version.stdout);
    let llvm_version = text
        .lines()
        .find(|line| line.contains("LLVM version"))
        .ok_or("LLVM opt did not report a version")?
        .trim()
        .to_owned();
    let clang_version = opt
        .parent()
        .map(|bin| bin.join("clang"))
        .and_then(|clang| Command::new(clang).arg("--version").output().ok())
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::to_owned)
        });
    let plugin_blake3 = blake3::hash(
        &fs::read(plugin)
            .map_err(|error| format!("cannot hash plugin {}: {error}", plugin.display()))?,
    )
    .to_hex()
    .to_string();
    Ok(ToolchainInfo {
        llvm_version,
        clang_version,
        plugin_blake3,
        target_triple: options.target_triple.clone(),
        cpu: options.cpu.clone(),
        features: options.features.clone(),
    })
}

impl RunReport {
    pub(super) fn missed(&self) -> usize {
        self.files
            .iter()
            .flat_map(|file| &file.diagnostics.loops)
            .filter(|loop_| loop_.decision == "rejected")
            .count()
    }

    pub(super) fn write(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            return Err(format!("report parent {} does not exist", parent.display()));
        }
        if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!("refusing report symlink {}", path.display()));
        }
        let mut temporary = Builder::new()
            .prefix(".rv-report-")
            .tempfile_in(parent)
            .map_err(|error| format!("cannot create report temporary file: {error}"))?;
        serde_json::to_writer_pretty(&mut temporary, self)
            .map_err(|error| format!("cannot serialize report: {error}"))?;
        temporary
            .write_all(b"\n")
            .map_err(|error| format!("cannot finish report: {error}"))?;
        temporary
            .persist(path)
            .map_err(|error| format!("cannot save report {}: {}", path.display(), error.error))?;
        Ok(())
    }
}

fn decode_atom(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            if let (Some(high), Some(low)) = (
                (raw[index + 1] as char).to_digit(16),
                (raw[index + 2] as char).to_digit(16),
            ) {
                bytes
                    .push(u8::try_from(high * 16 + low).expect("two hexadecimal digits fit in u8"));
                index += 3;
                continue;
            }
        }
        bytes.push(raw[index]);
        index += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(super) fn parse_remarks(stderr: &str) -> Vec<LoopDecision> {
    stderr
        .lines()
        .filter_map(|line| {
            let fields = line.strip_prefix("rv-vectorize: function=")?;
            let mut map = std::collections::HashMap::new();
            for field in std::iter::once(format!("function={}", fields.split_whitespace().next()?))
                .chain(fields.split_whitespace().skip(1).map(str::to_owned))
            {
                let (key, value) = field.split_once('=')?;
                map.insert(key.to_owned(), value.to_owned());
            }
            let source = map.get("source_file").map(|file| SourceLocation {
                file: decode_atom(file),
                line: map
                    .get("source_line")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
                column: map
                    .get("source_column")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            });
            Some(LoopDecision {
                function: decode_atom(map.get("function")?),
                loop_name: decode_atom(map.get("loop")?),
                decision: map.get("decision")?.clone(),
                reason: map.get("reason")?.clone(),
                vf: map.get("vf")?.parse().ok()?,
                estimated_vector_coverage: map
                    .get("estimated_vector_coverage")?
                    .trim_end_matches('%')
                    .parse()
                    .ok()?,
                issued_lane_utilization: map
                    .get("issued_lane_utilization")?
                    .trim_end_matches('%')
                    .parse()
                    .ok()?,
                analysis_us: map.get("analysis_us")?.parse().ok()?,
                transform_us: map.get("transform_us")?.parse().ok()?,
                source,
            })
        })
        .collect()
}

pub(super) fn parse_diagnostics(stderr: &str) -> Diagnostics {
    Diagnostics {
        loops: parse_remarks(stderr),
        llvm_remarks: stderr
            .lines()
            .filter(|line| line.starts_with("remark: "))
            .map(str::to_owned)
            .collect(),
    }
}
