#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cli="$repo_dir/target/release/rv-vectorize"
opt="${LLVM_SYS_211_PREFIX:-/opt/homebrew/opt/llvm}/bin/opt"
plugin="$repo_dir/target/release/librust_loop_vectorizer.so"
if [ "$(uname -s)" = Darwin ]; then
  plugin="$repo_dir/target/release/librust_loop_vectorizer.dylib"
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/project/sub" "$work/project/target"
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/project/sub/loops.ll"
printf 'arbitrary project asset\n' > "$work/project/readme.txt"
printf 'not LLVM IR\n' > "$work/project/target/ignored.ll"
printf 'int add(int a, int b) { return a + b; }\n' > "$work/project/sub/add.c"
printf 'int mul(int a, int b) { return a * b; }\n' > "$work/project/sub/mul.cpp"
printf 'int spaced(int a) { return a + 1; }\n' > "$work/project/sub/name with spaces.c"
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/project/sub/extra.toy"
printf '{"toy":["/bin/cp","{input}","{output}"]}\n' > "$work/frontends.json"
ln -s "$work/project/sub/loops.ll" "$work/project/linked.ll"
expected=5
if command -v rustc >/dev/null 2>&1; then
  mkdir -p "$work/project/rust/src"
  printf '[package]\nname = "sample"\nversion = "0.1.0"\n' > "$work/project/rust/Cargo.toml"
  printf '%s\n' '#[no_mangle] pub unsafe extern "C" fn add(a: *mut i32, n: usize) { let mut i = 0; while i < n { unsafe { *a.add(i) += 1; } i += 1; } }' > "$work/project/rust/src/lib.rs"
  printf 'this is a module, not a crate root\n' > "$work/project/rust/src/helper.rs"
  expected=$((expected + 1))
fi
if command -v swiftc >/dev/null 2>&1; then
  printf 'public func twice(_ x: Int) -> Int { x * 2 }\n' > "$work/project/sub/twice.swift"
  expected=$((expected + 1))
fi

"$cli" --opt "$opt" --plugin "$plugin" --frontend-config "$work/frontends.json" \
  --report "$work/project" -o "$work/project/vectorized" > "$work/stdout" 2> "$work/stderr"
test -f "$work/project/vectorized/sub/loops.ll"
test -f "$work/project/vectorized/sub/add.c.ll"
test -f "$work/project/vectorized/sub/mul.cpp.ll"
test -f "$work/project/vectorized/sub/name with spaces.c.ll"
test -f "$work/project/vectorized/sub/extra.toy.ll"
if command -v rustc >/dev/null 2>&1; then
  test -f "$work/project/vectorized/rust/src/lib.rs.ll"
  test ! -e "$work/project/vectorized/rust/src/helper.rs.ll"
  grep -q 'rv.vector.body' "$work/project/vectorized/rust/src/lib.rs.ll"
fi
if command -v swiftc >/dev/null 2>&1; then
  test -f "$work/project/vectorized/sub/twice.swift.ll"
fi
test ! -e "$work/project/vectorized/readme.txt"
test ! -e "$work/project/vectorized/target/ignored.ll"
test ! -e "$work/project/vectorized/linked.ll"
grep -q 'load <4 x float>' "$work/project/vectorized/sub/loops.ll"
grep -q 'decision=vectorized' "$work/stderr"
grep -q 'rv-vectorize: input=.*sub/loops.ll' "$work/stderr"
grep -q "processed=$expected" "$work/stdout"

"$cli" --opt "$opt" --plugin "$plugin" --emit-bitcode \
  --frontend-config "$work/frontends.json" "$work/project" -o "$work/bitcode" \
  > "$work/bitcode-stdout"
grep -q "processed=$expected" "$work/bitcode-stdout"
test ! -e "$work/bitcode/vectorized"
test -f "$work/bitcode/sub/loops.ll.bc"
test -f "$work/bitcode/sub/add.c.bc"
"$opt" -passes=verify -disable-output "$work/bitcode/sub/loops.ll.bc"

# A failed optimization must leave a previous output untouched.
printf 'not LLVM IR\n' > "$work/project/sub/bad.ll"
printf 'sentinel\n' > "$work/project/vectorized/sub/bad.ll"
if "$cli" --opt "$opt" --plugin "$plugin" "$work/project" -o "$work/project/vectorized" \
  > "$work/fail-stdout" 2> "$work/fail-stderr"; then
  printf '%s\n' 'invalid IR unexpectedly succeeded' >&2
  exit 1
fi
grep -q '^sentinel$' "$work/project/vectorized/sub/bad.ll"

# A source and an output path may alias through a symlink; reject that target.
rm "$work/project/sub/bad.ll" "$work/project/vectorized/sub/bad.ll"
rm "$work/project/vectorized/sub/loops.ll"
ln -s "$work/project/sub/loops.ll" "$work/project/vectorized/sub/loops.ll"
if "$cli" --opt "$opt" --plugin "$plugin" "$work/project" -o "$work/project/vectorized" \
  > "$work/alias-stdout" 2> "$work/alias-stderr"; then
  printf '%s\n' 'output symlink unexpectedly succeeded' >&2
  exit 1
fi
test -L "$work/project/vectorized/sub/loops.ll"
grep -q 'define' "$work/project/sub/loops.ll"

# Single-file mode must also protect source aliases and existing outputs.
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/single.ll"
ln -s "$work/single.ll" "$work/alias.ll"
if "$cli" --opt "$opt" --plugin "$plugin" "$work/single.ll" -o "$work/alias.ll" \
  > "$work/single-stdout" 2> "$work/single-stderr"; then
  printf '%s\n' 'single-file output alias unexpectedly succeeded' >&2
  exit 1
fi
grep -q 'define' "$work/single.ll"
printf 'not LLVM IR\n' > "$work/single-bad.ll"
printf 'sentinel\n' > "$work/single-output.ll"
if "$cli" --opt "$opt" --plugin "$plugin" "$work/single-bad.ll" -o "$work/single-output.ll" \
  > "$work/single-fail-stdout" 2> "$work/single-fail-stderr"; then
  printf '%s\n' 'invalid single-file IR unexpectedly succeeded' >&2
  exit 1
fi
grep -q '^sentinel$' "$work/single-output.ll"

# An existing project subdirectory is source content, not an output tree.
mkdir -p "$work/overlap/existing"
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/overlap/main.ll"
printf 'source sentinel\n' > "$work/overlap/existing/main.ll"
if "$cli" --opt "$opt" --plugin "$plugin" "$work/overlap" -o "$work/overlap/existing" \
  > "$work/overlap-stdout" 2> "$work/overlap-stderr"; then
  printf '%s\n' 'existing source directory unexpectedly accepted as output' >&2
  exit 1
fi
grep -q '^source sentinel$' "$work/overlap/existing/main.ll"

# The same source frontends must work for one file, including stdout output.
"$cli" --opt "$opt" --plugin "$plugin" "$work/project/sub/add.c" \
  -o "$work/add-from-source.ll"
grep -q 'define' "$work/add-from-source.ll"
"$cli" --opt "$opt" --plugin "$plugin" "$work/project/sub/mul.cpp" \
  > "$work/mul-from-source.ll"
grep -q 'define' "$work/mul-from-source.ll"

# A leading-dash filename is data, never an option to clang or opt.
cp "$repo_dir/tests/fixtures/vectorizable.ll" "$work/-loops.ll"
(cd "$work" && "$cli" --opt "$opt" --plugin "$plugin" -o leading-dash-output.ll -- -loops.ll)
grep -q 'define' "$work/leading-dash-output.ll"

# Directory mode must not follow symlinked output ancestors.
rm -rf "$work/project/vectorized/sub"
ln -s "$work/project/sub" "$work/project/vectorized/sub"
if "$cli" --opt "$opt" --plugin "$plugin" "$work/project" -o "$work/project/vectorized" \
  > "$work/parent-alias-stdout" 2> "$work/parent-alias-stderr"; then
  printf '%s\n' 'symlinked output subdirectory unexpectedly succeeded' >&2
  exit 1
fi
test -L "$work/project/vectorized/sub"
grep -q 'define' "$work/project/sub/loops.ll"

# Bitcode mode emits verifiable bitcode, and dry-run creates no output tree.
"$cli" --opt "$opt" --plugin "$plugin" --emit-bitcode "$work/project/sub/loops.ll" \
  -o "$work/loops.bc"
"$opt" -passes=verify -disable-output "$work/loops.bc"
"$cli" --opt "$opt" --plugin "$plugin" --dry-run "$work/project" \
  -o "$work/dry-output" > "$work/dry-run"
test ! -e "$work/dry-output"
grep -q 'planned=' "$work/dry-run"

printf '%s\n' 'project mode: ok'
