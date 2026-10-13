#!/usr/bin/env python3
"""Compares benchmark results between runs or between custom vectorizer and LLVM.

Generates a Markdown report and detects performance regressions.
"""

import argparse
import json
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass


def load_json(path: Path) -> dict:
    if not path.is_file():
        print(f"ERROR: Benchmark file not found: {path}", file=sys.stderr)
        sys.exit(1)
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as e:
        print(f"ERROR: Failed to parse JSON from {path}: {e}", file=sys.stderr)
        sys.exit(1)


def compare_within_single_file(data: dict) -> str:
    """Compares Custom Pass vs Native LLVM LoopVectorize in a single benchmark result."""
    lines = ["# 🏁 Benchmark Comparison: Custom Pass vs. LLVM LoopVectorize", ""]

    host = data.get("host", {})
    cpu = host.get("cpu", "Unknown CPU")
    llvm_ver = data.get("toolchain", {}).get("llvm", "Unknown")
    lines.append(f"**Host:** {cpu} | **LLVM Version:** {llvm_ver}")
    lines.append("")

    geomean = data.get("runtime_geomean_speedup_vs_scalar", {})
    custom_geo = geomean.get("custom", 1.0)
    llvm_geo = geomean.get("llvm", 1.0)
    geo_diff = ((custom_geo / llvm_geo) - 1.0) * 100.0

    lines.append(f"- **Geometric Mean Speedup (Custom):** `{custom_geo:.3f}x`")
    lines.append(f"- **Geometric Mean Speedup (LLVM):** `{llvm_geo:.3f}x`")
    lines.append(f"- **Relative Advantage:** `{geo_diff:+.2f}%`")
    lines.append("")

    kernels = data.get("runtime_kernel_speedup_vs_scalar", {})
    lines.append("| Kernel | Custom Speedup | LLVM Speedup | Difference | Status |")
    lines.append("|---|---:|---:|---:|:---:|")

    for kernel_name, speeds in kernels.items():
        c_speed = speeds.get("custom", 1.0)
        l_speed = speeds.get("llvm", 1.0)
        diff_pct = ((c_speed / l_speed) - 1.0) * 100.0

        if diff_pct >= 2.0:
            status = "🟢 Faster"
        elif diff_pct <= -2.0:
            status = "🔴 Slower"
        else:
            status = "🟡 Equal"

        lines.append(f"| `{kernel_name}` | {c_speed:.3f}x | {l_speed:.3f}x | {diff_pct:+.2f}% | {status} |")

    lines.append("")
    return "\n".join(lines)


def compare_two_runs(baseline: dict, current: dict, threshold_pct: float) -> (str, bool):
    """Compares two benchmark result snapshots and detects regressions."""
    lines = ["# 📊 Performance Regression Analysis", ""]

    base_geo = baseline.get("runtime_geomean_speedup_vs_scalar", {}).get("custom", 1.0)
    curr_geo = current.get("runtime_geomean_speedup_vs_scalar", {}).get("custom", 1.0)
    geo_change = ((curr_geo / base_geo) - 1.0) * 100.0

    lines.append(f"- **Baseline Geomean Speedup:** `{base_geo:.3f}x`")
    lines.append(f"- **Current Geomean Speedup:** `{curr_geo:.3f}x`")
    lines.append(f"- **Overall Geomean Change:** `{geo_change:+.2f}%`")
    lines.append("")

    lines.append("| Kernel | Baseline Speedup | Current Speedup | Delta | Status |")
    lines.append("|---|---:|---:|---:|:---:|")

    base_kernels = baseline.get("runtime_kernel_speedup_vs_scalar", {})
    curr_kernels = current.get("runtime_kernel_speedup_vs_scalar", {})
    has_regression = False

    for kernel, curr_data in curr_kernels.items():
        curr_speed = curr_data.get("custom", 1.0)
        base_speed = base_kernels.get(kernel, {}).get("custom", curr_speed)
        delta_pct = ((curr_speed / base_speed) - 1.0) * 100.0

        if delta_pct < -threshold_pct:
            status = "🔴 Regression"
            has_regression = True
        elif delta_pct > threshold_pct:
            status = "🟢 Improvement"
        else:
            status = "🟡 Unchanged"

        lines.append(f"| `{kernel}` | {base_speed:.3f}x | {curr_speed:.3f}x | {delta_pct:+.2f}% | {status} |")

    lines.append("")
    if has_regression:
        lines.append(f"> ⚠️ **Warning:** One or more kernels regressed by more than {threshold_pct}%.")
    else:
        lines.append("> ✅ **Clean:** No regressions detected within the tolerance threshold.")

    lines.append("")
    return "\n".join(lines), has_regression


def main():
    parser = argparse.ArgumentParser(description="Analyze and compare benchmark result JSON snapshots.")
    parser.add_argument("file1", type=Path, help="Primary benchmark results JSON file")
    parser.add_argument("file2", type=Path, nargs="?", help="Optional second benchmark results JSON to compare against")
    parser.add_argument("--threshold", type=float, default=3.0, help="Regression tolerance percentage (default: 3.0%%)")
    parser.add_argument("-o", "--output", type=Path, help="Output Markdown report path")
    args = parser.parse_args()

    data1 = load_json(args.file1)

    if args.file2:
        data2 = load_json(args.file2)
        report, regressed = compare_two_runs(data1, data2, args.threshold)
    else:
        report = compare_within_single_file(data1)
        regressed = False

    if args.output:
        args.output.write_text(report, encoding="utf-8")
        print(f"Wrote benchmark report to: {args.output}")
    else:
        print(report)

    if regressed:
        sys.exit(2)


if __name__ == "__main__":
    main()
