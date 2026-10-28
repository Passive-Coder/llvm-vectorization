#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cli="$repo_dir/target/release/rv-vectorize"
opt="${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/opt"
plugin="$repo_dir/target/release/librust_loop_vectorizer.so"
if [ "$(uname -s)" = Darwin ]; then
  plugin="$repo_dir/target/release/librust_loop_vectorizer.dylib"
  SDKROOT=$(xcrun --show-sdk-path)
  export SDKROOT
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/project/src" "$work/project/include" "$work/project/generated"
printf '#define SCALE 3\n' > "$work/project/include/value.h"
cat > "$work/project/src/kernel.c" <<'EOF'
#include "value.h"
#ifndef BUILD_FLAG
#error build flag was lost
#endif
int kernel(int *a, int n) {
  for (int i = 0; i < n; ++i) a[i] += SCALE + VARIANT;
  return 0;
}
EOF
python3 - "$work/project" "$work/compile_commands.json" <<'PY'
import json, sys
root, output = sys.argv[1:]
entries = []
for variant in (1, 2):
    entries.append({"directory": root, "file": "src/kernel.c", "arguments": [
        "clang", "-Iinclude", "-DBUILD_FLAG=1", f"-DVARIANT={variant}",
        "-c", "src/kernel.c", "-o", f"kernel-{variant}.o"]})
with open(output, "w") as stream:
    json.dump(entries, stream)
PY
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/compile_commands.json" \
  "$work/project" -o "$work/output" > "$work/build-stdout"
test "$(find "$work/output" -name 'kernel.c*.ll' -type f | wc -l | tr -d ' ')" -eq 2
grep -R -q 'define.*@kernel' "$work/output/src"

python3 - "$work/project" "$work/command-only.json" <<'PY'
import json, sys
root, output = sys.argv[1:]
with open(output, "w") as stream:
    json.dump([{"directory": root, "file": "src/kernel.c",
                "command": "clang -c src/kernel.c -o kernel.o"}], stream)
PY
if "$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/command-only.json" \
  "$work/project" -o "$work/rejected" > "$work/rejected-out" 2> "$work/rejected-err"; then
  printf '%s\n' 'command-only compilation database unexpectedly accepted' >&2
  exit 1
fi
grep -q 'arguments' "$work/rejected-err"

python3 - "$work/compile_commands.json" "$work/exact-duplicates.json" <<'PY'
import json, sys
entry = json.load(open(sys.argv[1]))[0]
with open(sys.argv[2], "w") as stream:
    json.dump([entry, entry], stream)
PY
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/exact-duplicates.json" \
  "$work/project" -o "$work/exact-outputs" > "$work/exact-out"
test "$(find "$work/exact-outputs" -name 'kernel.c*.ll' -type f | wc -l | tr -d ' ')" -eq 2

printf 'any language source\n' > "$work/project/src/script.toy"
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/project/generated/script.ll"
python3 - "$work/manifest.json" <<'PY'
import json, sys
with open(sys.argv[1], "w") as stream:
    json.dump([{"file": "src/script.toy", "ir": "project/generated/script.ll"}], stream)
PY
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/compile_commands.json" \
  --input-manifest "$work/manifest.json" \
  "$work/project" -o "$work/manifest-output" > "$work/manifest-stdout"
test -f "$work/manifest-output/src/script.toy.ll"
grep -q 'rv.vector.body' "$work/manifest-output/src/script.toy.ll"

# Object output must be consumable by the native linker.
printf 'int answer(void) { return 42; }\n' > "$work/answer.c"
printf 'int answer(void); int main(void) { return answer() == 42 ? 0 : 1; }\n' > "$work/main.c"
"$cli" --opt "$opt" --plugin "$plugin" --emit-object \
  "$work/answer.c" -o "$work/answer.o"
if [ "$(uname -s)" = Darwin ]; then
  "${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" -isysroot "$SDKROOT" \
    "$work/answer.o" "$work/main.c" -o "$work/answer-test"
else
  "${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" \
    "$work/answer.o" "$work/main.c" -o "$work/answer-test"
fi
"$work/answer-test"

"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/compile_commands.json" \
  --report-json "$work/report.json" \
  "$work/project" -o "$work/reported" > "$work/reported-stdout"
python3 - "$work/report.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
assert report["schema_version"] == 1
assert "LLVM version 21." in report["toolchain"]["llvm_version"]
assert len(report["toolchain"]["plugin_blake3"]) == 64
assert len(report["files"]) >= 2
assert all(item["status"] == "processed" for item in report["files"])
assert any(loop.get("source", {}).get("line", 0) > 0
           for item in report["files"] for loop in item["loops"])
PY

if "$cli" --opt "$opt" --plugin "$plugin" --fail-on-missed 0 \
  "$repo_dir/tests/fixtures/rejected.ll" -o "$work/rejected.ll" \
  > "$work/missed-stdout" 2> "$work/missed-err"; then
  printf '%s\n' 'missed-loop gate unexpectedly passed' >&2
  exit 1
fi
grep -q 'missed-loop' "$work/missed-err"

# A JSON report may not overwrite an input or an emitted artifact.
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/report-source.ll"
if "$cli" --opt "$opt" --plugin "$plugin" --report-json "$work/report-source.ll" \
  "$work/report-source.ll" -o "$work/report-output.ll" \
  > "$work/report-alias-out" 2> "$work/report-alias-err"; then
  printf '%s\n' 'report/input alias unexpectedly accepted' >&2
  exit 1
fi
cmp -s "$repo_dir/tests/fixtures/vectorizable.ll" "$work/report-source.ll"
grep -q 'report.*alias' "$work/report-alias-err"

"$cli" --opt "$opt" --plugin "$plugin" --doctor > "$work/doctor"
grep -q 'LLVM version 21' "$work/doctor"
grep -q 'plugin=ok' "$work/doctor"
grep -q 'clang=ok' "$work/doctor"
grep -q 'llc=ok' "$work/doctor"
grep -q '^rust=' "$work/doctor"

mkdir -p "$work/select/nested"
for name in keep ignored nested/local nested/omit; do
  cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/select/$name.ll"
done
printf 'ignored.ll\n' > "$work/select/.gitignore"
printf 'local.ll\n' > "$work/select/nested/.gitignore"
"$cli" --opt "$opt" --plugin "$plugin" \
  --include '**/*.ll' --exclude 'nested/omit.ll' --explain-skips \
  "$work/select" -o "$work/selected" > "$work/select-out" 2> "$work/select-err"
test -f "$work/selected/keep.ll"
test ! -e "$work/selected/ignored.ll"
test ! -e "$work/selected/nested/local.ll"
test ! -e "$work/selected/nested/omit.ll"
grep -q 'reason=gitignore' "$work/select-err"
grep -q 'reason=exclude' "$work/select-err"
"$cli" --opt "$opt" --plugin "$plugin" --no-gitignore \
  "$work/select" -o "$work/unignored" > "$work/unignored-out"
test -f "$work/unignored/ignored.ll"
test -f "$work/unignored/nested/local.ll"

# Parallel reports are deterministic, and IR cache invalidates on content changes.
"$cli" --opt "$opt" --plugin "$plugin" --no-gitignore --jobs 3 \
  --cache-dir "$work/cache" --report-json "$work/parallel-1.json" \
  "$work/select" -o "$work/parallel" > "$work/parallel-1-out"
"$cli" --opt "$opt" --plugin "$plugin" --no-gitignore --jobs 3 \
  --cache-dir "$work/cache" --report-json "$work/parallel-2.json" \
  "$work/select" -o "$work/parallel" > "$work/parallel-2-out"
python3 - "$work/parallel-2.json" <<'PY'
import json, sys
files = json.load(open(sys.argv[1]))["files"]
assert len(files) == 4
assert [item["input"] for item in files] == sorted(item["input"] for item in files)
assert all(item["status"] == "cached" for item in files)
PY
corrupt_artifact=$(find "$work/cache" -name '*.artifact' -type f | head -1)
printf 'corrupt cache entry\n' > "$corrupt_artifact"
"$cli" --opt "$opt" --plugin "$plugin" --no-gitignore --jobs 3 \
  --cache-dir "$work/cache" --report-json "$work/parallel-corrupt.json" \
  "$work/select" -o "$work/parallel" > "$work/parallel-corrupt-out"
python3 - "$work/parallel-corrupt.json" <<'PY'
import json, sys
files = json.load(open(sys.argv[1]))["files"]
assert any(item["status"] == "processed" for item in files)
PY
printf '; modified\n' >> "$work/select/keep.ll"
"$cli" --opt "$opt" --plugin "$plugin" --no-gitignore --jobs 3 \
  --cache-dir "$work/cache" --report-json "$work/parallel-3.json" \
  "$work/select" -o "$work/parallel" > "$work/parallel-3-out"
python3 - "$work/parallel-3.json" <<'PY'
import json, sys
files = json.load(open(sys.argv[1]))["files"]
assert next(item for item in files if item["input"].endswith("keep.ll"))["status"] == "processed"
assert sum(item["status"] == "cached" for item in files) == 3
PY

"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/compile_commands.json" --cache-dir "$work/c-cache" \
  --report-json "$work/c-cache-1.json" "$work/project" -o "$work/c-output" \
  > "$work/c-cache-1-out"
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/compile_commands.json" --cache-dir "$work/c-cache" \
  --report-json "$work/c-cache-2.json" "$work/project" -o "$work/c-output" \
  > "$work/c-cache-2-out"
python3 - "$work/c-cache-2.json" <<'PY'
import json, sys
files = json.load(open(sys.argv[1]))["files"]
assert all(item["status"] == "cached" for item in files if item["input"].endswith("kernel.c"))
PY
printf '#define SCALE 4\n' > "$work/project/include/value.h"
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/compile_commands.json" --cache-dir "$work/c-cache" \
  --report-json "$work/c-cache-3.json" "$work/project" -o "$work/c-output" \
  > "$work/c-cache-3-out"
python3 - "$work/c-cache-3.json" <<'PY'
import json, sys
files = json.load(open(sys.argv[1]))["files"]
assert all(item["status"] == "processed" for item in files if item["input"].endswith("kernel.c"))
PY

printf '%s\n' '-Iinclude -DBUILD_FLAG=1 -DVARIANT=1' > "$work/project/opts.rsp"
python3 - "$work/project" "$work/response-commands.json" <<'PY'
import json, sys
root, output = sys.argv[1:]
with open(output, "w") as stream:
    json.dump([{"directory": root, "file": "src/kernel.c",
                "arguments": ["clang", "@opts.rsp", "-c", "src/kernel.c", "-o", "kernel.o"]}], stream)
PY
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/response-commands.json" --cache-dir "$work/response-cache" \
  --report-json "$work/response-1.json" "$work/project" -o "$work/response-output" \
  > "$work/response-1-out"
"$cli" --opt "$opt" --plugin "$plugin" \
  --compile-commands "$work/response-commands.json" --cache-dir "$work/response-cache" \
  --report-json "$work/response-2.json" "$work/project" -o "$work/response-output" \
  > "$work/response-2-out"
python3 - "$work/response-2.json" <<'PY'
import json, sys
files = json.load(open(sys.argv[1]))["files"]
assert next(item for item in files if item["input"].endswith("kernel.c"))["status"] == "processed"
PY

# A source-size rejection must preserve a previous result.
printf 'sentinel\n' > "$work/limited.ll"
if "$cli" --opt "$opt" --plugin "$plugin" --max-input-bytes 1 \
  "$repo_dir/tests/fixtures/vectorizable.ll" -o "$work/limited.ll" \
  > "$work/limit-out" 2> "$work/limit-err"; then
  printf '%s\n' 'input-size limit unexpectedly passed' >&2
  exit 1
fi
grep -q '^sentinel$' "$work/limited.ll"
grep -q 'max-input-bytes' "$work/limit-err"

# Timeout kills an unresponsive custom frontend and preserves its artifact.
mkdir -p "$work/slow-project"
printf 'slow source\n' > "$work/slow-project/slow.slow"
cat > "$work/slow-compiler" <<'EOF'
#!/bin/sh
sleep 5
cp "$1" "$2"
EOF
chmod +x "$work/slow-compiler"
python3 - "$work/slow-frontends.json" "$work/slow-compiler" <<'PY'
import json, sys
with open(sys.argv[1], "w") as stream:
    json.dump({"slow": [sys.argv[2], "{input}", "{output}"]}, stream)
PY
mkdir -p "$work/slow-output"
printf 'sentinel\n' > "$work/slow-output/slow.slow.ll"
if "$cli" --opt "$opt" --plugin "$plugin" --timeout-seconds 1 \
  --frontend-config "$work/slow-frontends.json" \
  "$work/slow-project" -o "$work/slow-output" \
  > "$work/slow-out" 2> "$work/slow-err"; then
  printf '%s\n' 'timed-out frontend unexpectedly passed' >&2
  exit 1
fi
grep -q '^sentinel$' "$work/slow-output/slow.slow.ll"
grep -q 'timed out' "$work/slow-err"

mkdir -p "$work/memory-project" "$work/memory-output"
printf 'memory source\n' > "$work/memory-project/large.mem"
printf 'sentinel\n' > "$work/memory-output/large.mem.ll"
cat > "$work/memory-compiler.py" <<'PY'
import pathlib, sys, time
allocation = bytearray(256 * 1024 * 1024)
allocation[0] = 1
time.sleep(2)
pathlib.Path(sys.argv[2]).write_bytes(pathlib.Path(sys.argv[1]).read_bytes())
PY
python3 - "$work/memory-frontends.json" "$work/memory-compiler.py" <<'PY'
import json, sys
with open(sys.argv[1], "w") as stream:
    json.dump({"mem": ["python3", sys.argv[2], "{input}", "{output}"]}, stream)
PY
if "$cli" --opt "$opt" --plugin "$plugin" --max-memory-mib 32 \
  --frontend-config "$work/memory-frontends.json" \
  "$work/memory-project" -o "$work/memory-output" \
  > "$work/memory-out" 2> "$work/memory-err"; then
  printf '%s\n' 'memory-limited frontend unexpectedly passed' >&2
  exit 1
fi
grep -q '^sentinel$' "$work/memory-output/large.mem.ll"

# Explicit baseline/candidate runners compare behavior without shell parsing.
cat > "$work/baseline-run" <<'EOF'
#!/bin/sh
printf 'same result\n'
EOF
cat > "$work/candidate-run" <<'EOF'
#!/bin/sh
printf 'same result\n'
EOF
chmod +x "$work/baseline-run" "$work/candidate-run"
python3 - "$work/compare.json" "$work/baseline-run" "$work/candidate-run" <<'PY'
import json, sys
with open(sys.argv[1], "w") as stream:
    json.dump({"baseline": [sys.argv[2]], "candidate": [sys.argv[3]],
               "iterations": 2}, stream)
PY
"$cli" --opt "$opt" --plugin "$plugin" --compare-config "$work/compare.json" \
  "$repo_dir/tests/fixtures/vectorizable.ll" -o "$work/compared.ll" \
  > "$work/compare-out"
grep -q 'comparison=passed' "$work/compare-out"
"$cli" --opt "$opt" --plugin "$plugin" --compare-config "$work/compare.json" \
  --compare-report-json "$repo_dir/tests/fixtures/vectorizable.ll" \
  -o "$work/compared.ll" > "$work/compare-json-out"
python3 - "$work/compare-json-out" <<'PY'
import json, sys
record = next(line.split("=", 1)[1] for line in open(sys.argv[1])
              if line.startswith("program_comparison_json="))
report = json.loads(record)
assert report["baseline_stdout"] == "same result\n"
assert report["candidate_stdout"] == "same result\n"
assert report["outputs_match"] is True
assert report["iterations"] == 2
assert report["baseline_median_ms"] >= 0
assert report["candidate_median_ms"] >= 0
assert report["baseline_run_ms"] >= 0
assert report["candidate_run_ms"] >= 0
PY
"$cli" --opt "$opt" --plugin "$plugin" --compare-auto --compare-report-json \
  "$repo_dir/examples/run_program_demo.c" -o "$work/auto-program.ll" \
  > "$work/auto-program-out"
python3 - "$work/auto-program-out" <<'PY'
import json, sys
record = next(line.split("=", 1)[1] for line in open(sys.argv[1])
              if line.startswith("program_comparison_json="))
report = json.loads(record)
assert report["baseline_stdout"] == "sum=1571328\n"
assert report["candidate_stdout"] == "sum=1571328\n"
assert report["outputs_match"] is True
assert report["iterations"] == 1
assert report["warmup_iterations"] == 1
PY
cat > "$work/parallel-check.py" <<'PY'
#!/usr/bin/env python3
import pathlib, sys, time
name, directory = sys.argv[1:]
directory = pathlib.Path(directory)
(directory / name).touch()
other = directory / ("candidate" if name == "baseline" else "baseline")
deadline = time.monotonic() + 3
while not other.exists() and time.monotonic() < deadline:
    time.sleep(0.01)
if not other.exists():
    sys.exit("programs did not overlap")
print("parallel result")
PY
chmod +x "$work/parallel-check.py"
python3 - "$work/parallel.json" "$work/parallel-check.py" "$work" <<'PY'
import json, sys
config, program, directory = sys.argv[1:]
with open(config, "w") as stream:
    json.dump({"baseline": [program, "baseline", directory],
               "candidate": [program, "candidate", directory]}, stream)
PY
"$cli" --opt "$opt" --plugin "$plugin" --compare-config "$work/parallel.json" \
  "$repo_dir/tests/fixtures/vectorizable.ll" -o "$work/parallel.ll" \
  > "$work/parallel-out"
grep -q 'comparison=passed' "$work/parallel-out"
printf '#!/bin/sh\nprintf "different result\\n"\n' > "$work/candidate-run"
chmod +x "$work/candidate-run"
if "$cli" --opt "$opt" --plugin "$plugin" --compare-config "$work/compare.json" \
  "$repo_dir/tests/fixtures/vectorizable.ll" -o "$work/compared.ll" \
  > "$work/mismatch-out" 2> "$work/mismatch-err"; then
  printf '%s\n' 'mismatched comparison unexpectedly passed' >&2
  exit 1
fi
grep -q 'comparison outputs differ' "$work/mismatch-err"
"$cli" --opt "$opt" --plugin "$plugin" --compare-config "$work/compare.json" \
  --compare-report-json "$repo_dir/tests/fixtures/vectorizable.ll" \
  -o "$work/compared.ll" > "$work/mismatch-json-out" 2> "$work/mismatch-json-err" && exit 1
python3 - "$work/mismatch-json-out" <<'PY'
import json, sys
record = next(line.split("=", 1)[1] for line in open(sys.argv[1])
              if line.startswith("program_comparison_json="))
report = json.loads(record)
assert report["outputs_match"] is False
assert report["baseline_stdout"] == "same result\n"
assert report["candidate_stdout"] == "different result\n"
PY

cat > "$work/baseline-run" <<'EOF'
#!/bin/sh
sleep 0.02
printf 'same result\n'
EOF
cat > "$work/candidate-run" <<'EOF'
#!/bin/sh
sleep 0.25
printf 'same result\n'
EOF
chmod +x "$work/baseline-run" "$work/candidate-run"
python3 - "$work/compare-slow.json" "$work/baseline-run" "$work/candidate-run" <<'PY'
import json, sys
with open(sys.argv[1], "w") as stream:
    json.dump({"baseline": [sys.argv[2]], "candidate": [sys.argv[3]],
               "iterations": 3, "max_regression_percent": 20}, stream)
PY
if "$cli" --opt "$opt" --plugin "$plugin" --compare-config "$work/compare-slow.json" \
  "$repo_dir/tests/fixtures/vectorizable.ll" -o "$work/compared.ll" \
  > "$work/regression-out" 2> "$work/regression-err"; then
  printf '%s\n' 'runtime regression unexpectedly passed' >&2
  exit 1
fi
grep -q 'runtime regression' "$work/regression-err"

# The hybrid pipeline handles reductions and runtime alias checks that the
# narrow Rust legality contract currently rejects.
cat > "$work/advanced.c" <<'EOF'
#include <stddef.h>
float reduce(const float *a, size_t n) {
  float sum = 0;
  for (size_t i = 0; i < n; ++i) sum += a[i];
  return sum;
}
void alias(float *a, float *b, size_t n) {
  for (size_t i = 0; i < n; ++i) a[i] += b[i];
}
void conditional(float *restrict a, const float *restrict b, size_t n) {
  for (size_t i = 0; i < n; ++i) if (b[i] > 0) a[i] += b[i];
}
EOF
"${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" -O1 \
  -fno-vectorize -fno-slp-vectorize -S -emit-llvm \
  "$work/advanced.c" -o "$work/advanced.ll"
"$cli" --opt "$opt" --plugin "$plugin" --engine hybrid \
  --report-json "$work/advanced-report.json" \
  "$work/advanced.ll" -o "$work/advanced-vectorized.ll"
grep -q 'llvm.vector.reduce' "$work/advanced-vectorized.ll"
grep -q 'vector.memcheck' "$work/advanced-vectorized.ll"
python3 - "$work/advanced-report.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
assert report["engine"] == "hybrid"
assert any("vectorized loop" in remark for remark in report["files"][0]["llvm_remarks"])
PY
"$cli" --opt "$opt" --plugin "$plugin" --engine hybrid --vf 4 \
  "$work/advanced.ll" -o "$work/advanced-forced.ll"
grep -q 'pred.store.if' "$work/advanced-forced.ll"
"$opt" -passes=verify -disable-output "$work/advanced-vectorized.ll"
cat > "$work/advanced-main.c" <<'EOF'
#include <stddef.h>
float reduce(const float *, size_t);
void alias(float *, float *, size_t);
void conditional(float *, const float *, size_t);
int main(void) {
  float a[64], b[64];
  for (int i = 0; i < 64; ++i) { a[i] = 1; b[i] = 2; }
  if (reduce(a, 64) != 64) return 1;
  alias(a, b, 64);
  if (a[0] != 3 || a[63] != 3) return 2;
  alias(a, a, 64);
  if (a[0] != 6 || a[63] != 6) return 3;
  for (int i = 0; i < 64; ++i) b[i] = (i & 1) ? -2 : 2;
  conditional(a, b, 64);
  if (a[0] != 8 || a[1] != 6 || a[62] != 8 || a[63] != 6) return 4;
  return 0;
}
EOF
if [ "$(uname -s)" = Darwin ]; then
  "${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" -isysroot "$SDKROOT" \
    "$work/advanced-vectorized.ll" "$work/advanced-main.c" -o "$work/advanced-test"
else
  "${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" \
    "$work/advanced-vectorized.ll" "$work/advanced-main.c" -o "$work/advanced-test"
fi
"$work/advanced-test"
if [ "$(uname -s)" = Darwin ]; then
  "${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" -isysroot "$SDKROOT" \
    "$work/advanced-forced.ll" "$work/advanced-main.c" -o "$work/advanced-forced-test"
else
  "${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/clang" \
    "$work/advanced-forced.ll" "$work/advanced-main.c" -o "$work/advanced-forced-test"
fi
"$work/advanced-forced-test"

# The Rust planner uses TargetTransformInfo when a target is specified.
python3 - "$repo_dir/tests/fixtures/vectorizable.ll" "$work/x86.ll" <<'PY'
import pathlib, sys
source = pathlib.Path(sys.argv[1]).read_text()
pathlib.Path(sys.argv[2]).write_text(source.replace(
    "target datalayout", 'target triple = "x86_64-unknown-linux-gnu"\ntarget datalayout', 1))
PY
"$cli" --opt "$opt" --plugin "$plugin" --cpu haswell --report \
  "$work/x86.ll" -o "$work/x86-vectorized.ll" 2> "$work/x86-remarks"
grep -q 'function=add_f32 .*decision=vectorized.*vf=8' "$work/x86-remarks"

printf '%s\n' 'advanced CLI: ok'
