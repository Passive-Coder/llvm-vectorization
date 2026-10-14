//! Toolchain compatibility preflight.
use super::{Options, process, resolve_opt, resolve_plugin};
use std::fs;
use std::path::Path;
use std::process::Command;

fn required_tool(path: &Path, version_marker: &str) -> Result<(), String> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot run {}: {error}", path.display()))?;
    if !output.status.success() || !String::from_utf8_lossy(&output.stdout).contains(version_marker)
    {
        return Err(format!(
            "{} does not report LLVM 21 compatibility",
            path.display()
        ));
    }
    Ok(())
}

fn frontend_probe(
    program: &Path,
    suffix: &str,
    source: &str,
    arguments: &[&str],
    opt: &Path,
) -> &'static str {
    if Command::new(program).arg("--version").output().is_err() {
        return "unavailable";
    }
    let Ok(directory) = tempfile::tempdir() else {
        return "probe-failed";
    };
    let input = directory.path().join(format!("probe.{suffix}"));
    let ir = directory.path().join("probe.ll");
    if fs::write(&input, source).is_err() {
        return "probe-failed";
    }
    let mut command = Command::new(program);
    command.args(arguments).arg(&input).arg("-o").arg(&ir);
    let label = input.as_path();
    let limits = Options {
        timeout_seconds: Some(10),
        ..Options::default()
    };
    let Ok(compilation) = process::run(&mut command, label, &limits) else {
        return "compile-failed";
    };
    if !compilation.status.success() {
        return "compile-failed";
    }
    let mut verify = Command::new(opt);
    verify.args(["-passes=verify", "-disable-output"]).arg(&ir);
    let Ok(verification) = process::run(&mut verify, label, &limits) else {
        return "ir-incompatible";
    };
    if verification.status.success() {
        "compatible"
    } else {
        "ir-incompatible"
    }
}

pub(super) fn run(options: &Options) -> Result<(), String> {
    let opt = resolve_opt(options)?;
    let plugin = resolve_plugin(options)?;
    let llvm_bin = opt.parent().ok_or("LLVM opt has no parent directory")?;
    required_tool(&llvm_bin.join("clang"), "version 21.")?;
    required_tool(&llvm_bin.join("clang++"), "version 21.")?;
    required_tool(&llvm_bin.join("llc"), "LLVM version 21.")?;

    let input = tempfile::Builder::new()
        .suffix(".ll")
        .tempfile()
        .map_err(|error| format!("cannot create doctor probe: {error}"))?;
    fs::write(input.path(), "; plugin load probe\n")
        .map_err(|error| format!("cannot write doctor probe: {error}"))?;
    let mut command = Command::new(&opt);
    command
        .arg(format!("-load-pass-plugin={}", plugin.display()))
        .arg("-passes=rust-loop-vectorize,verify")
        .arg("-disable-output")
        .arg(input.path());
    let limits = Options {
        timeout_seconds: Some(10),
        ..Options::default()
    };
    let output = process::run(&mut command, input.path(), &limits)?;
    if !output.status.success() {
        return Err(format!(
            "LLVM plugin probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let version = Command::new(&opt)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot read LLVM version: {error}"))?;
    let version = String::from_utf8_lossy(&version.stdout);
    let line = version
        .lines()
        .find(|line| line.contains("LLVM version 21"))
        .unwrap_or("LLVM version 21")
        .trim();
    println!(
        "{line}\nopt={}\nplugin_path={}\nplugin=ok\nclang=ok\nllc=ok",
        opt.display(),
        plugin.display()
    );
    let flang = if llvm_bin.join("flang").is_file() {
        llvm_bin.join("flang")
    } else {
        llvm_bin.join("flang-new")
    };
    println!(
        "fortran={}",
        frontend_probe(
            &flang,
            "f90",
            "subroutine probe()\nend subroutine\n",
            &["-emit-llvm", "-S"],
            &opt
        )
    );
    println!(
        "rust={}",
        frontend_probe(
            Path::new("rustc"),
            "rs",
            "pub fn probe(x: i32) -> i32 { x + 1 }\n",
            &[
                "--crate-type=lib",
                "--crate-name=rv_doctor",
                "--emit=llvm-ir"
            ],
            &opt
        )
    );
    println!(
        "swift={}",
        frontend_probe(
            Path::new("swiftc"),
            "swift",
            "public func probe(_ x: Int) -> Int { x + 1 }\n",
            &["-emit-ir"],
            &opt
        )
    );
    Ok(())
}
