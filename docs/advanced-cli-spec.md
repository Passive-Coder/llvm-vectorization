# Advanced CLI specification

The CLI processes LLVM modules from source files, compilation databases, or
build-produced IR manifests. Each translation unit remains a separate module.
The existing conservative vectorization legality contract remains in force.

## Inputs

- `--compile-commands PATH` reads Clang JSON compilation database entries with
  `arguments`, preserving the working directory, preprocessing and target
  flags. It never executes an entry through a shell. `command`-only entries
  fail with a precise error. Duplicate source entries retain distinct outputs.
- `--input-manifest PATH` reads a JSON array of `{file, ir}` entries, where
  `file` is a project-relative source identity and `ir` names an existing LLVM
  IR or bitcode module. This is the language-independent build integration.
- When a compilation database is supplied, listed C-family translation units
  use it; unlisted C-family files are not guessed as standalone builds.
- The project scanner honors nested `.gitignore` files by default and supports
  include/exclude globs, an ignore override, and explicit skip explanations.

## Outputs and diagnostics

- `--emit-object` emits native object files through the selected LLVM 21
  toolchain. It requires an output path and is exclusive with bitcode output.
- `--report-json PATH` writes an atomic, versioned JSON report containing every
  selected file, status, output, candidate loop decisions, errors, and toolchain
  versions, plugin digest, and target settings. Source locations appear when
  debug metadata is present. LLVM backend remarks are listed separately from
  Rust pass loop decisions.
- `--fail-on-missed N` fails the run when more than N reported candidate loops
  are rejected by the Rust engine; the default has no missed-loop gate.
- `--doctor` checks tool discovery, plugin loading, and LLVM version without
  requiring an input module.

## Large project operation

- `--jobs N` processes independent modules concurrently while retaining
  deterministic report order and output paths.
- `--cache-dir PATH` stores completed artifacts under a content-derived key.
  A file may be cached only when its complete dependency set can be accounted
  for. Unknown dependencies force a fresh build. The cache directory is a
  project-mode option and must be outside the source and output trees.
- `--timeout-seconds N`, `--max-memory-mib N`, and `--max-input-bytes N` bound
  subprocesses and input size. A failing unit leaves a previous artifact intact.
- `--compare-config PATH` runs explicit baseline and candidate argv arrays
  after project processing, compares exit status and output, and optionally
  enforces a median runtime regression threshold. It never uses a shell.

## Compiler pass evolution

The Rust planner reads LLVM TargetTransformInfo for fixed vector register width
when the module declares a target triple. Its own legality checks remain
conservative. The optional `--engine hybrid` and `--engine llvm` modes use
LLVM's production Loop Vectorizer for supported reductions, runtime alias
checks, and predicated multi-block loops. Their transformations and remarks
belong to LLVM; extending the Rust pass to cover these shapes remains separate
work requiring legality proofs and differential correctness tests.
