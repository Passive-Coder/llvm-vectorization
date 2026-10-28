//! Explicit differential correctness and runtime comparison for CI.
use super::Options;
use super::process;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Instant;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    baseline: Vec<String>,
    candidate: Vec<String>,
    #[serde(default)]
    prepare_baseline: Option<Vec<String>>,
    #[serde(default)]
    prepare_candidate: Option<Vec<String>>,
    #[serde(default = "one")]
    iterations: usize,
    #[serde(default)]
    warmup_iterations: usize,
    #[serde(default)]
    max_regression_percent: Option<f64>,
    #[serde(default = "yes")]
    compare_stderr: bool,
}

/// One machine-readable line for the interactive program comparison view.
/// Program output is capped independently of the process capture limit.
#[derive(Serialize)]
struct ProgramReport {
    baseline_stdout: String,
    baseline_stderr: String,
    candidate_stdout: String,
    candidate_stderr: String,
    baseline_median_ms: f64,
    candidate_median_ms: f64,
    baseline_run_ms: f64,
    candidate_run_ms: f64,
    iterations: usize,
    warmup_iterations: usize,
    outputs_match: bool,
    within_regression_limit: bool,
    regression_percent: f64,
}

fn display_output(bytes: &[u8]) -> String {
    const LIMIT: usize = 32 * 1024;
    let end = bytes.len().min(LIMIT);
    let mut value = String::from_utf8_lossy(&bytes[..end]).into_owned();
    if bytes.len() > LIMIT {
        value.push_str("\n[output truncated at 32 KiB]");
    }
    value
}

const fn one() -> usize {
    1
}
const fn yes() -> bool {
    true
}

fn command_from_argv(
    argv: &[String],
    config_dir: &Path,
    input: &Path,
    output: &Path,
) -> Result<Command, String> {
    if argv.is_empty() || argv[0].is_empty() {
        return Err("comparison command needs a nonempty argv array".to_owned());
    }
    let absolute = |path: &Path| -> Result<PathBuf, String> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            std::env::current_dir()
                .map(|current| current.join(path))
                .map_err(|error| format!("cannot resolve comparison path: {error}"))
        }
    };
    let input = absolute(input)?;
    let output = absolute(output)?;
    let replace = |value: &str| {
        value
            .replace("{input}", &input.to_string_lossy())
            .replace("{output}", &output.to_string_lossy())
    };
    let executable = replace(&argv[0]);
    let executable = if executable.contains('/') && !Path::new(&executable).is_absolute() {
        absolute(&config_dir.join(executable))?
    } else {
        PathBuf::from(executable)
    };
    let mut command = Command::new(executable);
    command.args(argv[1..].iter().map(|argument| replace(argument)));
    command.current_dir(config_dir);
    Ok(command)
}

fn execute(
    argv: &[String],
    config_dir: &Path,
    input: &Path,
    output: &Path,
    options: &Options,
) -> Result<(Output, f64), String> {
    let mut command = command_from_argv(argv, config_dir, input, output)?;
    let start = Instant::now();
    let result = process::run(&mut command, input, options)?;
    let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    if !result.status.success() {
        return Err(format!(
            "comparison command {} failed with {}: {}",
            command.get_program().to_string_lossy(),
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok((result, milliseconds))
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        values[middle - 1].midpoint(values[middle])
    } else {
        values[middle]
    }
}

type TimedOutput = (Output, f64);

fn execute_pair(
    config: &Config,
    config_dir: &Path,
    input: &Path,
    output: &Path,
    options: &Options,
) -> Result<(TimedOutput, TimedOutput), String> {
    let (baseline_result, candidate_result) = thread::scope(|scope| {
        let baseline_task =
            scope.spawn(|| execute(&config.baseline, config_dir, input, output, options));
        let candidate_result = execute(&config.candidate, config_dir, input, output, options);
        let baseline_result = baseline_task
            .join()
            .map_err(|_| "baseline comparison worker panicked".to_owned());
        (baseline_result, candidate_result)
    });
    Ok((baseline_result??, candidate_result?))
}

/// Generate a temporary build recipe for a standalone program. Projects and
/// other languages keep using explicit comparison configs for their build steps.
pub(super) fn run_auto(input: &Path, output: &Path, options: &Options) -> Result<(), String> {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let compiler = match extension {
        "c" | "ll" | "bc" => if cfg!(target_os = "macos") { "/usr/bin/clang" } else { "clang" },
        "cc" | "cpp" | "cxx" => if cfg!(target_os = "macos") { "/usr/bin/clang++" } else { "clang++" },
        _ => return Err("automatic program comparison supports standalone C, C++, LLVM IR, and bitcode files; use --compare-config for this input".to_owned()),
    };
    if extension == "ll" {
        let module = fs::read_to_string(input)
            .map_err(|error| format!("cannot read LLVM IR program {}: {error}", input.display()))?;
        if !module.contains("@main(") && !module.contains("@\"main\"(") {
            return Err("automatic program comparison needs a main function; use a runnable source file or --compare-config with a test harness".to_owned());
        }
    }
    let temporary = tempfile::tempdir()
        .map_err(|error| format!("cannot create comparison build directory: {error}"))?;
    let baseline = temporary.path().join("normal-program");
    let candidate = temporary.path().join("vectorized-program");
    let baseline_path = baseline.to_string_lossy().into_owned();
    let candidate_path = candidate.to_string_lossy().into_owned();
    let config = serde_json::json!({
        "prepare_baseline": [compiler, "-O0", "-fno-vectorize", "-fno-slp-vectorize", "-fno-unroll-loops", "{input}", "-lm", "-o", baseline_path],
        "prepare_candidate": [compiler, "-O1", "-x", "ir", "{output}", "-lm", "-o", candidate_path],
        "baseline": [baseline_path],
        "candidate": [candidate_path],
        "iterations": 1,
        "warmup_iterations": 1
    });
    let config_path = temporary.path().join("compare.json");
    fs::write(&config_path, config.to_string())
        .map_err(|error| format!("cannot write automatic comparison config: {error}"))?;
    run(&config_path, input, output, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_comparison_explains_ir_without_main() {
        let error = run_auto(
            Path::new("tests/fixtures/vectorizable.ll"),
            Path::new("build/unused.ll"),
            &Options::default(),
        )
        .expect_err("the fixture is a library module, not a program");
        assert!(error.contains("main function"));
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn run(
    path: &Path,
    input: &Path,
    output: &Path,
    options: &Options,
) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot read compare config {}: {error}", path.display()))?;
    if metadata.len() > 1024 * 1024 {
        return Err("compare config exceeds 1 MiB".to_owned());
    }
    let config: Config = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("cannot read compare config: {error}"))?,
    )
    .map_err(|error| format!("invalid compare config: {error}"))?;
    if config.iterations == 0 || config.iterations > 100 {
        return Err("comparison iterations must be 1 through 100".to_owned());
    }
    if config.warmup_iterations > 10 {
        return Err("comparison warmup_iterations must be at most 10".to_owned());
    }
    if config
        .max_regression_percent
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err("max_regression_percent must be a nonnegative finite number".to_owned());
    }
    let config_dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if let Some(argv) = &config.prepare_baseline {
        execute(argv, config_dir, input, output, options)?;
    }
    if let Some(argv) = &config.prepare_candidate {
        execute(argv, config_dir, input, output, options)?;
    }
    for _ in 0..config.warmup_iterations {
        execute_pair(&config, config_dir, input, output, options)?;
    }
    let mut baseline_times = Vec::with_capacity(config.iterations);
    let mut candidate_times = Vec::with_capacity(config.iterations);
    let mut shown_outputs = None;
    let mut outputs_match = true;
    for _ in 0..config.iterations {
        let ((baseline, baseline_ms), (candidate, candidate_ms)) =
            execute_pair(&config, config_dir, input, output, options)?;
        let matched = baseline.stdout == candidate.stdout
            && (!config.compare_stderr || baseline.stderr == candidate.stderr);
        if shown_outputs.is_none() || !matched {
            shown_outputs = Some((
                baseline.stdout,
                baseline.stderr,
                candidate.stdout,
                candidate.stderr,
                baseline_ms,
                candidate_ms,
            ));
        }
        baseline_times.push(baseline_ms);
        candidate_times.push(candidate_ms);
        if !matched {
            outputs_match = false;
            if !options.compare_report_json {
                return Err("comparison outputs differ between baseline and candidate".to_owned());
            }
            break;
        }
    }
    let completed_iterations = baseline_times.len();
    let baseline_ms = median(&mut baseline_times);
    let candidate_ms = median(&mut candidate_times);
    let regression = if baseline_ms == 0.0 {
        0.0
    } else {
        (candidate_ms / baseline_ms - 1.0) * 100.0
    };
    let within_regression_limit = config
        .max_regression_percent
        .is_none_or(|limit| regression <= limit);
    if options.compare_report_json {
        let (
            baseline_stdout,
            baseline_stderr,
            candidate_stdout,
            candidate_stderr,
            baseline_run_ms,
            candidate_run_ms,
        ) = shown_outputs.expect("at least one comparison iteration");
        let report = ProgramReport {
            baseline_stdout: display_output(&baseline_stdout),
            baseline_stderr: display_output(&baseline_stderr),
            candidate_stdout: display_output(&candidate_stdout),
            candidate_stderr: display_output(&candidate_stderr),
            baseline_median_ms: baseline_ms,
            candidate_median_ms: candidate_ms,
            baseline_run_ms,
            candidate_run_ms,
            iterations: completed_iterations,
            warmup_iterations: config.warmup_iterations,
            outputs_match,
            within_regression_limit,
            regression_percent: regression,
        };
        println!(
            "program_comparison_json={}",
            serde_json::to_string(&report)
                .map_err(|error| format!("cannot serialize comparison report: {error}"))?
        );
    }
    if !outputs_match {
        return Err("comparison outputs differ between baseline and candidate".to_owned());
    }
    if !within_regression_limit {
        return Err(format!(
            "runtime regression {regression:.1}% exceeds configured threshold"
        ));
    }
    println!(
        "comparison=passed iterations={} baseline_median_ms={baseline_ms:.3} candidate_median_ms={candidate_ms:.3} regression_percent={regression:.1}",
        config.iterations
    );
    Ok(())
}
