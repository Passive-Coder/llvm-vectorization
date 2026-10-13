#!/usr/bin/env python3
"""Cross-platform test runner mirroring scripts/test.sh for Windows, macOS, and Linux."""

import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO_DIR = Path(__file__).resolve().parent.parent
BUILD_DIR = REPO_DIR / "build" / "test"


def find_llvm_prefix() -> Path:
    prefix = os.environ.get("LLVM_SYS_211_PREFIX")
    if prefix:
        p = Path(prefix)
        if (p / "bin" / ("llvm-config.exe" if platform.system() == "Windows" else "llvm-config")).is_file():
            return p

    config_path = os.environ.get("LLVM_CONFIG_PATH")
    if config_path:
        p = Path(config_path)
        if p.is_file():
            parent = p.parent.parent
            if parent.is_dir():
                return parent

    candidates = [
        Path("/opt/homebrew/opt/llvm"),
        Path("/usr/local/opt/llvm"),
        Path("/usr/lib/llvm-21"),
        Path(r"C:\Program Files\LLVM"),
        Path(r"C:\Program Files (x86)\LLVM"),
        Path(r"C:\LLVM"),
    ]
    exe_name = "llvm-config.exe" if platform.system() == "Windows" else "llvm-config"
    for candidate in candidates:
        if (candidate / "bin" / exe_name).is_file():
            return candidate

    found = shutil.which(exe_name) or shutil.which("llvm-config-21")
    if found:
        return Path(found).resolve().parent.parent

    print("ERROR: LLVM 21 not found; set LLVM_SYS_211_PREFIX or put llvm-config on PATH.", file=sys.stderr)
    sys.exit(1)


def find_plugin(repo: Path) -> Path:
    system = platform.system()
    if system == "Darwin":
        name = "librust_loop_vectorizer.dylib"
    elif system == "Windows":
        name = "rust_loop_vectorizer.dll"
    else:
        name = "librust_loop_vectorizer.so"

    plugin_path = repo / "target" / "release" / name
    return plugin_path


def run(cmd, **kwargs):
    result = subprocess.run(cmd, cwd=REPO_DIR, text=True, capture_output=True, **kwargs)
    if result.returncode != 0:
        print(f"FAILED: {' '.join(str(c) for c in cmd)}", file=sys.stderr)
        if result.stdout:
            print(f"STDOUT:\n{result.stdout}", file=sys.stderr)
        if result.stderr:
            print(f"STDERR:\n{result.stderr}", file=sys.stderr)
        sys.exit(result.returncode)
    return result


def main():
    BUILD_DIR.mkdir(parents=True, exist_ok=True)
    llvm_prefix = find_llvm_prefix()
    os.environ["LLVM_SYS_211_PREFIX"] = str(llvm_prefix)

    is_win = platform.system() == "Windows"
    opt = llvm_prefix / "bin" / ("opt.exe" if is_win else "opt")
    clang = llvm_prefix / "bin" / ("clang.exe" if is_win else "clang")
    cxx = llvm_prefix / "bin" / ("clang++.exe" if is_win else "clang++")
    llvm_config = llvm_prefix / "bin" / ("llvm-config.exe" if is_win else "llvm-config")

    plugin = find_plugin(REPO_DIR)
    cli = REPO_DIR / "target" / "release" / ("rv-vectorize.exe" if is_win else "rv-vectorize")

    print(f"Using LLVM prefix: {llvm_prefix}")
    print("Running cargo test...")
    run(["cargo", "test", "--quiet"])

    print("Building release artifacts...")
    run(["cargo", "build", "--release", "--quiet"])

    if not plugin.is_file():
        print(f"ERROR: Pass plugin not found at {plugin}", file=sys.stderr)
        sys.exit(1)

    # 1. vectorizable.ll
    print("Testing vectorizable.ll...")
    vec_ll = BUILD_DIR / "vectorized.ll"
    vec_remarks_file = BUILD_DIR / "vectorized.remarks"
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-report,verify",
               "-S", "tests/fixtures/vectorizable.ll", "-o", str(vec_ll)])
    vec_remarks = res.stderr
    vec_remarks_file.write_text(vec_remarks)

    vector_loops = len(re.findall(r"decision=vectorized", vec_remarks))
    assert vector_loops == 9, f"Expected 9 vectorized loops, got {vector_loops}"

    vec_content = vec_ll.read_text()
    assert "load <4 x float>" in vec_content
    assert "load <4 x i32>" in vec_content
    assert "store <2 x i64>" in vec_content
    assert "zext <4 x i16>" in vec_content
    assert "select <4 x i1>" in vec_content
    assert re.search(r"function=increment_in_place .*decision=vectorized", vec_remarks)
    assert re.search(r"function=increment_in_place_ne .*decision=vectorized", vec_remarks)
    assert re.search(r"function=increment_in_place_ult .*decision=vectorized", vec_remarks)
    assert re.search(r"function=ordered_double_store .*decision=vectorized", vec_remarks)

    # 2. showcase.ll
    print("Testing showcase.ll...")
    showcase_ll = BUILD_DIR / "showcase.ll"
    showcase_remarks_file = BUILD_DIR / "showcase.remarks"
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-report,verify",
               "-S", "tests/fixtures/showcase.ll", "-o", str(showcase_ll)])
    showcase_remarks = res.stderr
    showcase_remarks_file.write_text(showcase_remarks)

    showcase_loops = len(re.findall(r"decision=vectorized", showcase_remarks))
    assert showcase_loops == 14, f"Expected 14 showcase vectorized loops, got {showcase_loops}"
    assert re.search(r"function=mul_add_f64 .*decision=vectorized.*vf=2", showcase_remarks)
    assert re.search(r"function=mask_bits_i8 .*decision=vectorized.*vf=16", showcase_remarks)
    assert re.search(r"function=add_half .*decision=vectorized.*vf=8", showcase_remarks)
    assert re.search(r"function=xor_fold_i16 .*decision=vectorized.*vf=8", showcase_remarks)
    assert re.search(r"function=widen_f32_to_f64 .*decision=vectorized.*vf=2", showcase_remarks)
    assert re.search(r"function=widen_i8_to_i64 .*decision=vectorized.*vf=2", showcase_remarks)
    assert re.search(r"function=scale_globals_i32 .*decision=vectorized.*vf=4", showcase_remarks)
    assert re.search(r"function=neighbor_sum_i32 .*decision=vectorized.*vf=4", showcase_remarks)

    showcase_content = showcase_ll.read_text()
    assert "load <16 x i8>" in showcase_content
    assert "fadd <8 x half>" in showcase_content
    assert "fadd <2 x double>" in showcase_content
    assert "fneg <4 x float>" in showcase_content
    assert "udiv <4 x i32>" in showcase_content
    assert "urem <4 x i32>" in showcase_content
    assert "ashr <4 x i32>" in showcase_content
    assert "fcmp olt <4 x float>" in showcase_content
    assert "sitofp <4 x i32>" in showcase_content
    assert "fpext <2 x float>" in showcase_content
    assert "fptrunc <2 x double>" in showcase_content

    # 3. rejected.ll
    print("Testing rejected.ll...")
    rejected_output = BUILD_DIR / "rejected-output.ll"
    rejected_remarks_file = BUILD_DIR / "rejected.remarks"
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-report,verify",
               "-S", "tests/fixtures/rejected.ll", "-o", str(rejected_output)])
    rejected_remarks = res.stderr
    rejected_remarks_file.write_text(rejected_remarks)

    assert "rv.vector.body" not in rejected_output.read_text(), "Known-unsafe loop was vectorized!"
    assert "reason=loop-carried-memory-dependence" in rejected_remarks
    assert "reason=possible-pointer-alias" in rejected_remarks
    assert "reason=volatile-or-atomic-memory" in rejected_remarks
    assert "reason=unsupported-instruction" in rejected_remarks
    assert "reason=disabled-by-loop-metadata" in rejected_remarks
    assert "reason=loop-value-live-out" in rejected_remarks
    assert "reason=preheader-terminator-must-be-branch" in rejected_remarks
    assert "reason=affine-offset-overflow" in rejected_remarks
    assert "reason=latch-condition-must-be-icmp" in rejected_remarks
    assert re.search(r"function=raw_recurrence .*reason=loop-carried-memory-dependence", rejected_remarks)
    assert re.search(r"function=war_recurrence .*reason=loop-carried-memory-dependence", rejected_remarks)
    assert re.search(r"function=shifted_waw_recurrence .*reason=loop-carried-memory-dependence", rejected_remarks)
    assert re.search(r"function=induction_next_data_use .*reason=induction-next-has-data-use", rejected_remarks)
    assert re.search(r"function=latch_compare_data_use .*reason=latch-compare-has-data-use", rejected_remarks)
    assert "function=optimization_disabled " not in rejected_remarks, "optnone function was analyzed!"

    # 4. rejected-extra.ll
    print("Testing rejected-extra.ll...")
    rejected_extra_output = BUILD_DIR / "rejected-extra-output.ll"
    rejected_extra_remarks_file = BUILD_DIR / "rejected-extra.remarks"
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-report,verify",
               "-S", "tests/fixtures/rejected-extra.ll", "-o", str(rejected_extra_output)])
    rejected_extra_remarks = res.stderr
    rejected_extra_remarks_file.write_text(rejected_extra_remarks)

    extra_rejections = len(re.findall(r"decision=rejected", rejected_extra_remarks))
    assert extra_rejections == 14, f"Expected 14 rejected loops, got {extra_rejections}"
    assert "decision=vectorized" not in rejected_extra_remarks

    # Immutability check
    rejected_extra_baseline = BUILD_DIR / "rejected-extra-baseline.ll"
    run([str(opt), "-passes=verify", "-S", "tests/fixtures/rejected-extra.ll", "-o", str(rejected_extra_baseline)])
    assert rejected_extra_baseline.read_bytes() == rejected_extra_output.read_bytes(), "Rejected module was mutated!"

    # 5. Layout checks
    print("Testing layout checks...")
    for layout_file in ["padded-layout.ll", "narrow-index-layout.ll", "wide-index-layout.ll", "missing-layout.ll"]:
        res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-report,verify",
                   "-disable-output", f"tests/fixtures/{layout_file}"])
        assert "reason=incompatible-target-memory-layout" in res.stderr

    # 6. Policies
    print("Testing policies and forced VF...")
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-report,verify",
               "-disable-output", "tests/fixtures/heuristics.ll"])
    assert re.search(r"function=offset_copy_i32 .*decision=vectorized.*vf=4", res.stderr)

    cons_ll = BUILD_DIR / "conservative.ll"
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-conservative-report,verify",
               "-S", "tests/fixtures/heuristics.ll", "-o", str(cons_ll)])
    assert "decision=rejected reason=not-profitable" in res.stderr
    assert "rv.vector.body" not in cons_ll.read_text()

    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-aggressive-report,verify",
               "-disable-output", "tests/fixtures/heuristics.ll"])
    assert "decision=vectorized" in res.stderr

    forced_ll = BUILD_DIR / "forced-vf8.ll"
    res = run([str(opt), f"-load-pass-plugin={plugin}", "-passes=rust-loop-vectorize-force-vf8-report,verify",
               "-S", "tests/fixtures/heuristics.ll", "-o", str(forced_ll)])
    assert re.search(r"decision=vectorized.*vf=8", res.stderr)
    assert "load <8 x i32>" in forced_ll.read_text()

    # 7. CLI checks
    print("Testing CLI driver...")
    cli_ll = BUILD_DIR / "cli-vectorized.ll"
    res = run([str(cli), "--opt", str(opt), "--plugin", str(plugin), "--report",
               "tests/fixtures/vectorizable.ll", "-o", str(cli_ll)])
    assert "load <4 x float>" in cli_ll.read_text()
    assert "decision=vectorized" in res.stderr

    cli_forced = BUILD_DIR / "cli-forced-vf8.ll"
    run([str(cli), "--opt", str(opt), "--plugin", str(plugin), "--vf", "8",
         "tests/fixtures/heuristics.ll", "-o", str(cli_forced)])
    assert "load <8 x i32>" in cli_forced.read_text()

    # 8. C compile check
    print("Testing C compatibility...")
    sdk_flags = []
    if platform.system() == "Darwin":
        sdk_res = subprocess.run(["xcrun", "--show-sdk-path"], text=True, capture_output=True)
        if sdk_res.returncode == 0:
            sdk_flags = ["-isysroot", sdk_res.stdout.strip()]

    llvm_cxx_flags = run([str(llvm_config), "--cxxflags"]).stdout.strip().split()
    run([str(cxx)] + sdk_flags + llvm_cxx_flags + [
        "-std=c++17", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter",
        "-fsyntax-only", "native/pass_plugin.cpp"
    ])

    run([str(clang)] + sdk_flags + [
        "-std=c11", "-Wall", "-Wextra", "-Werror", "-fsyntax-only",
        "tests/runtime_harness.c", "benchmarks/kernels.c",
        "benchmarks/cloned_kernels.c", "benchmarks/harness.c"
    ])

    # 9. Runtime check
    print("Testing runtime execution harness...")
    runtime_bin = BUILD_DIR / ("runtime-check.exe" if is_win else "runtime-check")
    run([str(clang)] + sdk_flags + [
        "-O2", "-Wno-override-module", "-fno-vectorize", "-fno-slp-vectorize",
        str(vec_ll), "tests/runtime_harness.c", "-o", str(runtime_bin)
    ])
    run([str(runtime_bin)])

    rejected_count = len(re.findall(r"decision=rejected", rejected_remarks))
    print(
        f"verified: {vector_loops}+{showcase_loops} vectorized loops, "
        f"{rejected_count}+{extra_rejections} fail-closed rejections, "
        f"CLI, conservative bailouts, LLVM IR verifier, runtime tails"
    )


if __name__ == "__main__":
    main()
