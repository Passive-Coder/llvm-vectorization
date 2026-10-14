//! Explicit differential correctness and runtime comparison for CI.
use super::Options;
use super::process;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
    max_regression_percent: Option<f64>,
    #[serde(default = "yes")]
    compare_stderr: bool,
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
    let replace = |value: &str| {
        value
            .replace("{input}", &input.to_string_lossy())
            .replace("{output}", &output.to_string_lossy())
    };
    let executable = replace(&argv[0]);
    let executable = if executable.contains('/') && !Path::new(&executable).is_absolute() {
        config_dir.join(executable)
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
    let mut baseline_times = Vec::with_capacity(config.iterations);
    let mut candidate_times = Vec::with_capacity(config.iterations);
    for _ in 0..config.iterations {
        let (baseline, baseline_ms) =
            execute(&config.baseline, config_dir, input, output, options)?;
        let (candidate, candidate_ms) =
            execute(&config.candidate, config_dir, input, output, options)?;
        if baseline.stdout != candidate.stdout
            || config.compare_stderr && baseline.stderr != candidate.stderr
        {
            return Err("comparison outputs differ between baseline and candidate".to_owned());
        }
        baseline_times.push(baseline_ms);
        candidate_times.push(candidate_ms);
    }
    let baseline_ms = median(&mut baseline_times);
    let candidate_ms = median(&mut candidate_times);
    let regression = if baseline_ms == 0.0 {
        0.0
    } else {
        (candidate_ms / baseline_ms - 1.0) * 100.0
    };
    if config
        .max_regression_percent
        .is_some_and(|limit| regression > limit)
    {
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
