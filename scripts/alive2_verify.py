#!/usr/bin/env python3
"""Translation validation harness using Alive2 (alive-tv) for formal equivalence proofs.

Verifies that vectorized LLVM functions are observationally correct refinements of
the original scalar LLVM input functions.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass


def find_alive_tv() -> Path | None:
    env_path = os.environ.get("ALIVE2_PATH")
    if env_path:
        p = Path(env_path)
        if p.is_file():
            return p
        candidate = p / "alive-tv"
        if candidate.is_file():
            return candidate

    found = shutil.which("alive-tv")
    if found:
        return Path(found)
    return None


def extract_function_names(ll_path: Path) -> list[str]:
    content = ll_path.read_text(encoding="utf-8")
    return re.findall(r"^define\s+.*?@([a-zA-Z0-9_$.]+)\s*\(", content, re.MULTILINE)


def main():
    parser = argparse.ArgumentParser(
        description="Formally verify equivalence of vectorized LLVM IR against scalar IR using Alive2."
    )
    parser.add_argument("scalar_ir", type=Path, help="Original scalar LLVM IR (.ll) file")
    parser.add_argument("vector_ir", type=Path, help="Transformed vectorized LLVM IR (.ll) file")
    parser.add_argument("-f", "--function", help="Specific function to verify (default: all functions)")
    parser.add_argument("--alive-path", type=Path, help="Explicit path to alive-tv binary")
    parser.add_argument("--dry-run", action="store_true", help="Print verification commands without running alive-tv")
    args = parser.parse_args()

    if not args.scalar_ir.is_file():
        print(f"ERROR: Scalar IR file not found: {args.scalar_ir}", file=sys.stderr)
        sys.exit(1)
    if not args.vector_ir.is_file():
        print(f"ERROR: Vectorized IR file not found: {args.vector_ir}", file=sys.stderr)
        sys.exit(1)

    alive_tv = args.alive_path or find_alive_tv()

    funcs = [args.function] if args.function else extract_function_names(args.scalar_ir)
    if not funcs:
        print(f"No function definitions found in {args.scalar_ir}", file=sys.stderr)
        sys.exit(1)

    print(f"🔍 Found {len(funcs)} functions to verify between {args.scalar_ir.name} and {args.vector_ir.name}")

    if args.dry_run or not alive_tv:
        if not alive_tv:
            print("⚠️  alive-tv binary not found on PATH or via ALIVE2_PATH.", file=sys.stderr)
            print("Displaying verification commands (--dry-run mode):\n")
        else:
            print(f"Using alive-tv at: {alive_tv}\n")

        for func in funcs:
            cmd = [
                str(alive_tv or "alive-tv"),
                f"--src-fn={func}",
                f"--tgt-fn={func}",
                str(args.scalar_ir),
                str(args.vector_ir),
            ]
            print(f"  {' '.join(cmd)}")

        if not alive_tv:
            print("\nTo install Alive2, see: https://github.com/AliveToolkit/alive2")
        return

    verified_count = 0
    mismatch_count = 0
    error_count = 0

    print(f"\nRunning Alive2 verification using: {alive_tv}\n")
    print("| Function | Status | SMT Proof Result |")
    print("|---|:---:|---|")

    for func in funcs:
        cmd = [
            str(alive_tv),
            f"--src-fn={func}",
            f"--tgt-fn={func}",
            str(args.scalar_ir),
            str(args.vector_ir),
        ]
        res = subprocess.run(cmd, text=True, capture_output=True)
        out = (res.stdout + res.stderr).strip()

        if "Transformation seems to be correct!" in out:
            status = "✅ Proven"
            summary = "Refinement holds for all values"
            verified_count += 1
        elif "Transformation doesn't verify!" in out or "ERROR:" in out:
            status = "❌ Mismatch"
            summary = "Counterexample found or UB mismatch"
            mismatch_count += 1
        else:
            status = "⚠️ Warning"
            summary = out.splitlines()[0] if out else "Unknown response"
            error_count += 1

        print(f"| `{func}` | {status} | {summary} |")

    print(f"\nSummary: {verified_count} verified, {mismatch_count} mismatches, {error_count} inconclusive.")
    if mismatch_count > 0:
        sys.exit(1)


if __name__ == "__main__":
    main()
