//! Content-keyed artifact cache. Callers supply complete dependency lists.
use super::Options;
use super::report::Diagnostics;
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::Builder;

#[derive(Debug)]
pub(super) struct Cache {
    root: PathBuf,
    base: blake3::Hash,
}

#[derive(Deserialize, Serialize)]
struct Record {
    schema_version: u32,
    artifact_hash: String,
    diagnostics: Diagnostics,
}

fn hash_file(hasher: &mut Hasher, path: &Path) -> Result<(), String> {
    let mut file =
        File::open(path).map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(())
}

fn file_digest(path: &Path) -> Result<String, String> {
    let mut hasher = Hasher::new();
    hash_file(&mut hasher, path)?;
    Ok(hasher.finalize().to_hex().to_string())
}

impl Cache {
    pub(super) fn new(
        path: &Path,
        input_root: &Path,
        output_root: &Path,
        options: &Options,
        opt: &Path,
        plugin: &Path,
    ) -> Result<Self, String> {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!("refusing cache symlink {}", path.display()));
        }
        fs::create_dir_all(path)
            .map_err(|error| format!("cannot create cache {}: {error}", path.display()))?;
        let root = fs::canonicalize(path)
            .map_err(|error| format!("cannot resolve cache {}: {error}", path.display()))?;
        if root.starts_with(input_root)
            || input_root.starts_with(&root)
            || root.starts_with(output_root)
            || output_root.starts_with(&root)
        {
            return Err("cache directory must be separate from input and output trees".to_owned());
        }
        let mut hasher = Hasher::new();
        hasher.update(b"rv-vectorize-cache-v3");
        hash_file(&mut hasher, opt)?;
        hash_file(&mut hasher, plugin)?;
        let llvm_bin = opt.parent().ok_or("LLVM opt has no parent directory")?;
        for compiler in ["clang", "clang++"] {
            let path = llvm_bin.join(compiler);
            if path.is_file() {
                hash_file(&mut hasher, &path)?;
            }
        }
        let mut environment = std::env::vars_os().collect::<Vec<_>>();
        environment.sort_by(|left, right| left.0.cmp(&right.0));
        for (name, value) in environment {
            hasher.update(&(name.len() as u64).to_le_bytes());
            hasher.update(name.as_encoded_bytes());
            hasher.update(&(value.len() as u64).to_le_bytes());
            hasher.update(value.as_encoded_bytes());
        }
        if options.emit_object {
            let llc = opt
                .parent()
                .ok_or("LLVM opt has no parent directory")?
                .join("llc");
            hash_file(&mut hasher, &llc)?;
        }
        hasher.update(
            format!(
                "{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{}:{}:{}:{}",
                options.policy,
                options.vector_width,
                options.engine,
                options.target_triple,
                options.cpu,
                options.features,
                options.verify,
                options.emit_bitcode,
                options.emit_object,
                options.report || options.report_json.is_some() || options.fail_on_missed.is_some()
            )
            .as_bytes(),
        );
        Ok(Self {
            root,
            base: hasher.finalize(),
        })
    }

    pub(super) fn key(
        &self,
        input: &Path,
        arguments: &[String],
        dependencies: &[PathBuf],
    ) -> Result<String, String> {
        let mut hasher = Hasher::new();
        hasher.update(self.base.as_bytes());
        hasher.update(input.as_os_str().as_encoded_bytes());
        for argument in arguments {
            hasher.update(&(argument.len() as u64).to_le_bytes());
            hasher.update(argument.as_bytes());
        }
        for path in dependencies {
            hasher.update(path.as_os_str().as_encoded_bytes());
            hash_file(&mut hasher, path)?;
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub(super) fn lookup(&self, key: &str, output: &Path) -> Result<Option<Diagnostics>, String> {
        let artifact = self.root.join(format!("{key}.artifact"));
        let record = self.root.join(format!("{key}.json"));
        if !artifact.is_file() || !record.is_file() {
            return Ok(None);
        }
        for path in [&artifact, &record] {
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return Err(format!("refusing cache symlink {}", path.display()));
            }
        }
        let record: Record = serde_json::from_slice(
            &fs::read(&record)
                .map_err(|error| format!("cannot read cache {}: {error}", record.display()))?,
        )
        .map_err(|error| format!("invalid cache {}: {error}", record.display()))?;
        if record.schema_version != 3 || file_digest(&artifact)? != record.artifact_hash {
            return Ok(None);
        }
        let parent = output.parent().ok_or("output has no parent")?;
        let temporary = Builder::new()
            .prefix(".rv-cache-")
            .tempfile_in(parent)
            .map_err(|error| format!("cannot create cached output: {error}"))?;
        fs::copy(&artifact, temporary.path())
            .map_err(|error| format!("cannot copy cached artifact: {error}"))?;
        temporary
            .persist(output)
            .map_err(|error| format!("cannot publish cached output: {}", error.error))?;
        Ok(Some(record.diagnostics))
    }

    pub(super) fn store(
        &self,
        key: &str,
        output: &Path,
        diagnostics: &Diagnostics,
    ) -> Result<(), String> {
        let artifact = self.root.join(format!("{key}.artifact"));
        let record = self.root.join(format!("{key}.json"));
        let temporary_artifact = Builder::new()
            .prefix(".rv-cache-")
            .tempfile_in(&self.root)
            .map_err(|error| format!("cannot create cache artifact: {error}"))?;
        fs::copy(output, temporary_artifact.path())
            .map_err(|error| format!("cannot copy cache artifact: {error}"))?;
        let artifact_hash = file_digest(temporary_artifact.path())?;
        temporary_artifact
            .persist(&artifact)
            .map_err(|error| format!("cannot publish cache artifact: {}", error.error))?;
        let mut temporary_record = Builder::new()
            .prefix(".rv-cache-")
            .tempfile_in(&self.root)
            .map_err(|error| format!("cannot create cache record: {error}"))?;
        serde_json::to_writer(
            &mut temporary_record,
            &Record {
                schema_version: 3,
                artifact_hash,
                diagnostics: diagnostics.clone(),
            },
        )
        .map_err(|error| format!("cannot serialize cache record: {error}"))?;
        temporary_record
            .write_all(b"\n")
            .map_err(|error| format!("cannot finish cache record: {error}"))?;
        temporary_record
            .persist(&record)
            .map_err(|error| format!("cannot publish cache record: {}", error.error))?;
        Ok(())
    }
}
