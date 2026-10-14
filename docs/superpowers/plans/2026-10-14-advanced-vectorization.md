# Advanced Vectorization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make project vectorization reproducible, build-aware, diagnosable, bounded, and usable from native builds, then broaden safe loop coverage.

**Architecture:** Extend the Rust CLI with focused modules for input discovery, process execution, reporting, caching, and comparison. Preserve one LLVM module per translation unit and atomic output publication. Extend the pass only behind explicit legality proofs and differential tests.

**Tech Stack:** Rust 2024, LLVM 21 `opt`/`llc`, Clang JSON compilation databases, shell integration tests, LLVM IR fixtures.

**Spec:** `docs/advanced-cli-spec.md`

## Global Constraints

- LLVM 21 and Rust 1.85 remain the minimum toolchain.
- Never run compilation database command strings through a shell.
- Reject uncertain vectorization legality; never publish failed artifacts.
- Keep existing CLI arguments and test behavior compatible.

## Review Focus

- Duplicate compilation database entries for one source produce distinct paths.
- A header change invalidates any cached C-family artifact.
- Symlinked or overlapping output and cache paths never overwrite inputs.
- Timed-out subprocesses are reaped and previous artifacts remain intact.
- Reports retain deterministic order when `--jobs` exceeds one.

---

### Task 1: Build-aware inputs

**Files:** `src/bin/rv-vectorize.rs`, `src/bin/rv-vectorize/project.rs`, `src/bin/rv-vectorize/inputs.rs`, `tests/advanced_cli.sh`.

**Interfaces:** `inputs::load_compile_commands(path, root) -> Result<Vec<CompileUnit>, String>` and `inputs::load_ir_manifest(path, root) -> Result<Vec<IrUnit>, String>` feed project `WorkItem`s.

- [x] Add failing integration tests for preserved include flags, command-only rejection, duplicate variants, and an arbitrary-language IR manifest.
- [x] Run `sh tests/advanced_cli.sh` and confirm the expected feature failure.
- [x] Implement the loaders and work-item construction without a shell.
- [x] Run the tests and the existing project-mode test.

### Task 2: Output and diagnostics

**Files:** `src/bin/rv-vectorize.rs`, `src/bin/rv-vectorize/project.rs`, `src/bin/rv-vectorize/report.rs`, `src/vectorizer.rs`, `tests/advanced_cli.sh`.

**Interfaces:** `Report` records per-item results; `--emit-object` runs `llc -filetype=obj`; `--report-json` publishes JSON atomically.

- [x] Add failing tests for linkable objects, JSON decisions and source locations, and missed-loop gates.
- [x] Run the tests to verify missing behavior.
- [x] Implement object emission, captured remarks, and atomic JSON reporting.
- [x] Run the tests and existing suite.

### Task 3: Preflight and selection

**Files:** `src/bin/rv-vectorize.rs`, `src/bin/rv-vectorize/project.rs`, `tests/advanced_cli.sh`.

**Interfaces:** `--doctor`, `--include`, `--exclude`, `--no-gitignore`, and `--explain-skips`.

- [x] Add failing tests for doctor, nested ignore rules, override, globs, and skip reasons.
- [x] Run the tests to verify missing behavior.
- [x] Implement the selection and preflight behavior.
- [x] Run the tests and existing suite.

### Task 4: Parallelism, cache, and limits

**Files:** `src/bin/rv-vectorize/project.rs`, `src/bin/rv-vectorize/process.rs`, `src/bin/rv-vectorize/cache.rs`, `src/bin/rv-vectorize.rs`, `tests/advanced_cli.sh`.

**Interfaces:** `--jobs`, `--cache-dir`, `--timeout-seconds`, `--max-memory-mib`, and `--max-input-bytes`.

- [x] Add failing tests for cache hits and invalidation, deterministic parallel reports, input limits, and timeouts.
- [x] Run the tests to verify missing behavior.
- [x] Implement bounded execution and fail-closed caching.
- [x] Run the tests and existing suite.

### Task 5: CI comparison

**Files:** `src/bin/rv-vectorize/compare.rs`, `src/bin/rv-vectorize.rs`, `tests/advanced_cli.sh`.

**Interfaces:** `--compare-config` accepts two argv arrays and optional repeated timing threshold.

- [x] Add failing tests for matching output, mismatched output, and regression threshold.
- [x] Run the tests to verify missing behavior.
- [x] Implement shell-free comparison with the same process limits.
- [x] Run the tests and existing suite.

### Task 6: Compiler-pass coverage

**Files:** `native/pass_plugin.cpp`, `src/vectorizer.rs`, `src/cost.rs`, LLVM fixtures, `scripts/differential_test.py`, architecture docs.

**Interfaces:** The Rust planner reads fixed vector register width from LLVM TTI. `--engine hybrid` runs LLVM's Loop Vectorizer after the Rust pass, and `--engine llvm` runs LLVM's pass alone. The Rust pass retains its existing conservative legality boundary.

- [x] Add failing tests for target-aware VF and hybrid reductions, runtime alias checks, and multi-block predication.
- [x] Verify the baseline lacks each requested capability.
- [x] Bridge TTI register width to the Rust planner and add the explicit LLVM backend modes.
- [x] Run `./scripts/test.sh`, `make lint`, and `make differential` before documenting supported shapes.
