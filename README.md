# Rust Loop Vectorizer for LLVM 21

A correctness-first automatic loop-vectorization pass whose discovery,
dependence analysis, profitability model, and LLVM IR generation are written in
Rust. A small C++ adapter only registers the pass with LLVM's New Pass Manager
and bridges APIs that LLVM does not expose through C.

The project implements the requested end-to-end path:

- detects canonical count-up loops;
- proves a deliberately bounded legality contract;
- performs affine same-base dependence and conservative alias analysis;
- selects a fixed vector factor with conservative, balanced, or aggressive
  profitability policies;
- emits real fixed-width LLVM vector loads, stores, arithmetic, casts,
  comparisons, and selects;
- retains the scalar loop for short inputs and exact scalar remainders;
- reports vector coverage, issued-lane utilization, rejection reasons, and
  per-loop analysis/transformation latency; and
- compares identical scalar input IR against LLVM's built-in Loop Vectorizer
  with native correctness and timing harnesses.

This is a research and teaching implementation for compiler optimization,
code generation, and parallelism modules. It intentionally rejects IR it
cannot prove safe; it is not presented as a replacement for LLVM's production
vectorizer.

## Measured result

On the checked-in 128-loop compile-time corpus, a release build measured
**2.355 µs p50**, **8.790 µs p95**, and **31.404 µs p99** for the custom pass's
combined in-process analysis and transformation. The run used LLVM 21.1.3 on
an Apple M5 and transformed 128/128 loops.

In the seven-kernel runtime comparison, all scalar/custom/LLVM variants passed
correctness. The recorded geometric-mean speedups over the scalar control were
**3.009× for this pass** and **2.974× for LLVM LoopVectorize**. These are
cache-hot results from one machine, not universal performance claims. See the
[machine-readable snapshot](docs/benchmark-results.json) and
[research report](docs/research-report.md#53-local-controlled-result) for the
protocol, per-kernel results, and caveats.

## Requirements

- Rust 1.85 or newer
- LLVM 21.x, including `llvm-config`, `opt`, `clang`, `llc`, and `llvm-dis`
- a C++17 compiler compatible with the installed LLVM build
- Python 3.10 or newer for the benchmark driver
- macOS or Linux for the native test/benchmark scripts

For Homebrew LLVM:

```sh
export LLVM_SYS_211_PREFIX="$(brew --prefix llvm)"
export PATH="$LLVM_SYS_211_PREFIX/bin:$PATH"
```

On other systems, set `LLVM_SYS_211_PREFIX` to the LLVM 21 installation prefix
and put that installation's `bin` directory on `PATH`. `LLVM_CONFIG_PATH` may
additionally select the exact `llvm-config` used by this project's C++ bridge,
but the `llvm-sys` dependency itself discovers LLVM through
`LLVM_SYS_211_PREFIX` or `PATH`. Build the plugin and load it with tools from
the same LLVM installation; LLVM's C++ pass-plugin ABI is not a cross-install
compatibility boundary.

## Build and run

```sh
cargo build --release
```

### CLI application

The release build includes `rv-vectorize`, a command-line application that
discovers LLVM 21 and the pass plugin, constructs the pass pipeline, and
propagates LLVM verifier failures:

```sh
target/release/rv-vectorize --report input.ll -o output.ll
```

It accepts textual LLVM IR (`.ll`), bitcode (`.bc`), supported source files,
or an entire project directory. The default policy is
`balanced`, the vector factor is selected automatically, textual IR is written
to stdout when `-o` is omitted, and LLVM verification is enabled. Common
variants are:

```sh
# Favor compile time and require stronger estimated profitability.
target/release/rv-vectorize --policy conservative input.ll -o output.ll

# Force eight lanes after the normal legality checks.
target/release/rv-vectorize --vf 8 --report input.bc -o output.ll

# Emit bitcode and show the fully resolved LLVM command.
target/release/rv-vectorize --emit-bitcode input.ll -o output.bc
target/release/rv-vectorize --dry-run input.ll -o output.ll
```

Run `rv-vectorize --help` for all options. Use `--llvm-prefix`, `--opt`, and
`--plugin` for explicit toolchain selection; the corresponding environment
variables are `LLVM_SYS_211_PREFIX`, `LLVM_CONFIG_PATH`, and
`RV_PLUGIN_PATH`. Fixed `--vf` values bypass profitability but never bypass
dependence or legality checks.

### Project directories and languages

```sh
target/release/rv-vectorize --report path/to/project -o path/to/vectorized-ir
target/release/rv-vectorize --emit-bitcode path/to/project -o path/to/vectorized-bc
target/release/rv-vectorize path/to/project/src/kernel.cpp -o kernel.ll
```

Directory mode recursively processes each LLVM module separately and mirrors
the relative paths into the output directory. The CLI reports `processed`,
`failed`, and `skipped` counts, continues after an individual file fails, and
returns a nonzero status if any file fails. With `--report`, each group of loop
decisions is preceded by its input path. It skips non-source assets,
symlinks, hidden directories, `target`, `node_modules`, and `vendor`. If the
output directory is inside the input tree, it is excluded from scanning and
marked as a generated output directory. Marked output trees from earlier runs
are also excluded on later scans. Existing source directories cannot be
selected as nested output directories. Generated outputs are written through
temporary files, so a compiler or verifier failure preserves any previous
output. Input/output aliases and output symlinks are rejected.

The built-in source adapters are C, C++, Objective-C, Objective-C++ (the LLVM
21 Clang beside `opt`), Fortran (LLVM 21 `flang` or `flang-new`), Rust (`rustc`),
and Swift (`swiftc`). The latter tools must be installed, and their emitted
LLVM IR must be accepted by LLVM 21. Source outputs retain the source
extension, for example
`src/kernel.cpp` becomes `src/kernel.cpp.ll`. A `.ll` file keeps its name;
`.bc` becomes `.bc.ll` when emitting text. `--emit-bitcode` changes the output
suffix to `.bc`.

For any other language with a compiler that emits LLVM IR or bitcode, either
put its generated `.ll`/`.bc` files in the input tree or explicitly supply a
JSON frontend configuration. Each extension maps to an argv array containing
exactly one `{input}` and `{output}` placeholder:

```json
{
  "toy": ["/path/to/toy-compiler", "--emit-llvm", "{input}", "-o", "{output}"]
}
```

```sh
target/release/rv-vectorize --frontend-config frontends.json \
  path/to/project -o path/to/vectorized-ir
```

The CLI executes these arguments directly without a shell. Only use a frontend
configuration you trust, since its programs run with your account privileges.
An entry for a built-in extension replaces that adapter, which can supply
project-specific include paths or compiler flags.

The CLI compiles standalone translation units. Rust crates with external
dependencies, Swift multi-file modules, C/C++ projects needing build-specific
flags, and other complex builds may require producing LLVM IR with the native
build system first. Every language still uses the same conservative loop
legality contract below; support for a frontend does not mean every source
loop is vectorized. `--report` explains each candidate's decision.

### Build-aware and advanced project runs

For a C-family project, pass a [Clang JSON compilation database](https://clang.llvm.org/docs/JSONCompilationDatabase.html)
whose entries have `arguments` arrays. The driver reuses each entry's working
directory, include paths, defines, and target flags. It selects the LLVM 21
Clang beside `opt` and replaces output/dependency-emission flags with LLVM IR
emission. Entries containing only `command` strings are rejected; the driver
never interprets them with a shell. Repeated entries for one source receive
distinct, stable output names.

```sh
target/release/rv-vectorize --compile-commands build/compile_commands.json \
  --emit-object --report-json build/vectorization.json \
  . -o build/vectorized-objects
```

Other build systems can provide LLVM modules through `--input-manifest`. Its
JSON entries map a project-relative source identity to a `.ll` or `.bc` file;
`ir` paths are resolved relative to the manifest file:

```json
[{"file":"src/kernel.toy","ir":"build/kernel.ll"}]
```

Object mode uses LLVM 21 `llc` and writes one linkable `.o` per selected
module. The JSON report lists each source and output path for the native build
to consume. The CLI does not link a whole application on its own.

The default `--engine rust` keeps the bounded Rust legality contract. With
`--engine hybrid`, LLVM's Loop Vectorizer runs after the Rust pass, enabling
LLVM 21's supported reductions, guarded pointer disambiguation, and predicated
multi-block loops. `--engine llvm` runs only LLVM's Loop Vectorizer. A fixed
`--vf` also sets LLVM's vector width in those modes. Rust-loop decisions in
`--report` and the JSON `loops` array describe the Rust pass; the JSON
`llvm_remarks` array records LLVM's own optimization remarks in hybrid and LLVM
modes. The JSON `toolchain` record includes LLVM and Clang versions, the plugin
digest, and selected target settings. LLVM can vectorize a loop that the Rust
pass rejected. `--fail-on-missed` therefore applies only to `--engine rust`.
When the module declares a target triple, the Rust planner
uses LLVM `TargetTransformInfo` for automatic register width. `--cpu`,
`--features`, and `--target-triple` configure LLVM planning and object emission.

Project selection supports repeatable `--include GLOB` and `--exclude GLOB`,
honors nested `.gitignore` files by default, and has `--no-gitignore` and
`--explain-skips` switches. `--jobs N` processes modules in parallel while the
JSON report keeps deterministic file order. `--cache-dir PATH` reuses direct
IR/bitcode and compilation-database artifacts after hashing their content,
compiler arguments, toolchain, plugin, and Clang-discovered dependencies.
Unknown source dependencies and response files bypass the cache. The cache
is available in project mode and must be outside the project and output trees.

`--timeout-seconds`, `--max-memory-mib`, and `--max-input-bytes` bound work per
subprocess or input. Linux uses an address-space limit; macOS monitors resident
memory and terminates a process that exceeds the limit. A failed file leaves
any previous artifact untouched. `--doctor` checks LLVM tools and plugin
loading, then reports whether optional Fortran, Rust, and Swift frontends emit
IR that LLVM 21 can verify.

For CI, `--fail-on-missed N` gates Rust-pass rejections. `--compare-config`
runs explicit baseline and candidate argv arrays after vectorization, checks
exit status and output, and can reject a median runtime regression. This
example uses executable paths or program names in argv arrays; it never sends
a command string to a shell:

```json
{
  "prepare_candidate": ["./link-vectorized", "{output}"],
  "baseline": ["./build/baseline-test"],
  "candidate": ["./build/vectorized-test"],
  "iterations": 5,
  "max_regression_percent": 10
}
```

`{input}` and `{output}` placeholders work in every comparison argument.
`compare_stderr` defaults to `true`; set it to `false` for programs whose
diagnostic text is intentionally variable. Only use comparison and frontend
configurations you trust, since they name programs run with your privileges.
For standalone C, C++, LLVM IR, or bitcode programs with `main`, use
`--compare-auto` instead of a config. It builds temporary normal and vectorized
executables, launches both concurrently once to warm them up, then launches
both concurrently for one timed run. The
normal executable is compiled at `-O0` with loop vectorization, SLP
vectorization, and loop unrolling disabled; the vectorized executable uses
the transformed LLVM IR.
Add `--compare-report-json` to print a machine-readable comparison record with
the displayed run's stdout/stderr and runtime for both programs. The TUI
uses this record; captured output is limited to 32 KiB per stream in the record.

### Interactive TUI application

The project also includes a full-screen terminal application:

```sh
make tui
# or, after cargo build --release:
target/release/rv-vectorize-tui
```

The TUI starts with the runnable `examples/run_program_demo.c` and
`build/tui-vectorized.ll` as its
default input and output. Its editable paths also accept a project directory
and output directory; the side-by-side diff is available for textual single
files. Select **Run program** to vectorize, build, and concurrently run a
standalone C, C++, LLVM IR, or bitcode program. The comparison screen shows
their stdout, stderr, runtimes, and whether the outputs match. The
Compare JSON field is optional for those files. For projects or other languages,
enter a comparison config whose `prepare_baseline` and `prepare_candidate`
commands define how to build the programs for that language and build system.
For a ready-to-run C demo, set Input to `examples/run_program_demo.c`,
Output to `build/run-program-vectorized.ll`, leave Compare JSON blank, then
select **Run program** or press `Ctrl-P`. The reported time is the measured
whole-program wall time of that run, including
process startup; use a longer running benchmark for meaningful speed ratios.
It provides policy and vector-width selectors,
diagnostics/verification/bitcode toggles, and an on-screen results
pane containing the vectorizer's per-loop decisions.

Keyboard controls:

| Key | Action |
|---|---|
| `Tab`, `Shift-Tab`, `↑`, `↓` | Move between controls. |
| `←`, `→` | Move within text or change a selector. |
| `Enter`, `Space` | Toggle a setting or activate the Run button. |
| `Ctrl-R` | Run from anywhere in the application. |
| `Ctrl-P` | Run and compare the normal and vectorized programs. |
| Mouse wheel | Scroll the session pane. |
| `PageUp`, `PageDown` | Scroll the session pane by five rows. |
| `Ctrl-↑`, `Ctrl-↓` | Scroll the session pane one row. |
| `Home`, `End` | Jump to the oldest or newest output, unless a path field is focused. |
| `Esc`, `Ctrl-C` | Exit and restore the terminal. |

The session pane follows the newest output until you scroll up, shows a
scrollbar whenever the transcript overflows, and resumes following once you
scroll back to the bottom. Mouse reporting is enabled while the application
runs, so hold `Shift` to use the terminal's own text selection.

### Direct LLVM invocation

On macOS:

```sh
/opt/homebrew/opt/llvm/bin/opt \
  -load-pass-plugin=target/release/librust_loop_vectorizer.dylib \
  -passes='rust-loop-vectorize-report,verify' \
  -S input.ll -o output.ll
```

On Linux, use `librust_loop_vectorizer.so` and the path to the LLVM 21 `opt`.
Report mode emits one parseable line per discovered candidate, for example:

```text
rv-vectorize: function=add_f32 loop=loop decision=vectorized reason=legal-and-profitable vf=4 estimated_vector_coverage=100% issued_lane_utilization=100% analysis_us=2.834 transform_us=10.708
```

To create suitable scalar IR from C before applying the pass:

```sh
clang -O1 -fno-vectorize -fno-slp-vectorize -fno-unroll-loops \
  -ffp-contract=off -emit-llvm -S kernel.c -o kernel.ll
```

## Policies and pass names

| Pass | Behavior |
|---|---|
| `rust-loop-vectorize` | Balanced policy; automatic power-of-two VF within a 128-bit baseline. |
| `rust-loop-vectorize-report` | Balanced policy with diagnostics and internal timings. |
| `rust-loop-vectorize-conservative` | Requires at least `4*VF` estimated iterations and 1.25× estimated gain. |
| `rust-loop-vectorize-aggressive` | Requires at least `VF` iterations and break-even estimated cost. |
| `rust-loop-vectorize-force-vf2` | Forces VF 2 after all legality checks. |
| `rust-loop-vectorize-force-vf4` | Forces VF 4 after all legality checks. |
| `rust-loop-vectorize-force-vf8` | Forces VF 8 after all legality checks. |
| `rust-loop-vectorize-force-vf16` | Forces VF 16 after all legality checks. |

Append `-report` to the conservative, aggressive, or forced-VF names for
diagnostics. Forced widths bypass the profitability ratio, never legality.

## Supported loop subset

Accepted loops have:

- one loop block, one exit, and one ordinary branch preheader edge;
- one zero-based `i64` induction PHI with unit positive step;
- `eq`, `ne`, or canonical unsigned-`lt` latch comparison against an invariant
  trip count;
- no loop value used outside the loop and no exit PHI;
- direct argument/global bases and one-index `i + constant` GEPs;
- nonvolatile, nonatomic contiguous memory operations using `i8/i16/i32/i64`,
  `half`, `float`, or `double` elements;
- distinct globals, pairs of `noalias` arguments, or proven safe same-base
  dependence distances; and
- a target layout with packed scalar/vector storage and exactly 64-bit GEP
  index arithmetic in the relevant address space. A missing module data layout
  is rejected because generic defaults are not a target contract.

The pass supports lane-wise arithmetic, division/remainder, shifts, bitwise
operations, casts, comparisons, `select`, floating negation, loads, and stores.
It preserves applicable fast-math, integer nowrap, exact, GEP inbounds, and
memory-alignment properties.

Not yet supported: runtime alias versioning, reductions, calls/intrinsics,
multi-block control flow and if-conversion, nonunit/descending induction,
masked tails, scalable vectors, interleaving, outer-loop vectorization, or
instruction-level TargetTransformInfo costing. Each becomes a safe future extension only
with its corresponding legality proof and tests.

## Verification

Run the complete suite:

```sh
./scripts/test.sh
```

It currently covers 45 Rust tests, the CLI and TUI applications, nine positive
LLVM loops plus a fourteen-loop breadth showcase spanning VF 2/4/8/16 and every
supported element type, twenty-nine conservative rejection fixtures covering
distinct named reasons, a byte-for-byte check that a fully rejected module is
left unmodified, policy selection, forced VF, LLVM's verifier, native
differential execution over boundary trip counts, all three supported latch
forms, memory-order cases, guard-page detection of tail over-read/write, and
mixed-language project traversal with output-alias and failure-preservation
checks.

Run the generated differential corpus in quick mode:

```sh
make differential
# differential-test: ok mode=quick ... cases=32 pass-configs=7 \
#   runtime-configs=2 executions=8192 unsupported-probes=9
```

`make differential-full` expands this to 397,824 byte-for-byte scalar/vector
comparisons across natural and forced VFs, `-O0`/`-O2`, boundary and randomized
trip counts, and a UBSan run. Both modes verify every generated module and
confirm that all nine unsupported-shape probes remain scalar. Reproducible
artifacts are written only to `build/differential/`.

Run the reproducible comparison:

```sh
./scripts/benchmark.py --warmups 3 --samples 21 \
  --inner-calls 64 --timing-runs 15
```

The script compiles the kernel source to scalar bitcode once with both LLVM
vectorizers disabled. Scalar, custom, and LLVM variants then fork from that
byte-identical bitcode and use the same backend/link flags. It verifies every
variant, tests edge lengths, records repeated runtime samples, times 128 cloned
loops, and writes full JSON/CSV artifacts to `build/benchmark/`.

Useful development commands:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
make test
```

## Design and research

- [Architecture and correctness contract](docs/architecture.md)
- [Research report and evidence ledger](docs/research-report.md)
- [Recorded benchmark snapshot](docs/benchmark-results.json)

The research report connects this implementation to foundational dependence
work by Allen and Kennedy, Goff–Kennedy–Tseng, and Maydan–Hennessy–Lam; LLVM's
LoopAccessAnalysis and VPlan architecture; target-aware and throttled
profitability research; TSVC evaluation; and translation-validation lessons.

## Repository map

```text
native/pass_plugin.cpp       LLVM New PM adapter and narrow C++ bridges
src/bin/rv-vectorize.rs      CLI driver, discovery, validation, pass execution
src/bin/rv-vectorize-tui.rs  full-screen interactive terminal application
src/vectorizer.rs            discovery, legality, planning inputs, IR rewrite
src/dependence.rs            affine GCD/exact-distance classifier
src/cost.rs                  VF selection, cost score, vector coverage
src/config.rs                heuristic profiles
src/llvm.rs                  borrowed LLVM handle wrappers
tests/fixtures/vectorizable.ll   canonical accepted loops
tests/fixtures/showcase.ll       breadth showcase across VF, types, and opcodes
tests/fixtures/rejected.ll       fail-closed dependence, alias, and CFG cases
tests/fixtures/rejected-extra.ll fail-closed induction, memory, and shape cases
tests/                       positive, negative, runtime, and layout tests
benchmarks/                  fair scalar/custom/LLVM kernel corpus and harness
scripts/test.sh              end-to-end verifier/runtime suite
scripts/differential_test.py generated scalar/vector stress campaign
scripts/benchmark.py         reproducible comparison and JSON/CSV reporting
docs/                        architecture, research, and measured snapshot
```
