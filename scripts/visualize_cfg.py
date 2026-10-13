#!/usr/bin/env python3
"""Generates Graphviz DOT control flow graphs from LLVM IR (.ll) files.

Highlights vector dispatch, vector loop body, vector exit, and scalar remainder blocks.
"""

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path


def parse_llvm_cfg(ll_content: str, target_function: str = None) -> dict:
    """Parses basic blocks and control flow edges from textual LLVM IR."""
    functions = {}
    current_func = None
    current_block = None
    func_regex = re.compile(r"^define\s+.*?@([a-zA-Z0-9_$.]+)\s*\(")
    block_header_regex = re.compile(r"^([a-zA-Z0-9_$.]+):\s*(?:;.*)?$")
    branch_cond_regex = re.compile(r"br\s+i1\s+.*,\s+label\s+%([a-zA-Z0-9_$.]+),\s+label\s+%([a-zA-Z0-9_$.]+)")
    branch_uncond_regex = re.compile(r"br\s+label\s+%([a-zA-Z0-9_$.]+)")

    for line in ll_content.splitlines():
        line_clean = line.strip()

        # Check function start
        func_match = func_regex.match(line_clean)
        if func_match:
            current_func = func_match.group(1)
            functions[current_func] = {"blocks": {}, "edges": []}
            current_block = "entry"
            functions[current_func]["blocks"][current_block] = []
            continue

        if line_clean == "}" and current_func:
            current_func = None
            current_block = None
            continue

        if not current_func:
            continue

        # Check basic block label
        block_match = block_header_regex.match(line_clean)
        if block_match:
            current_block = block_match.group(1)
            if current_block not in functions[current_func]["blocks"]:
                functions[current_func]["blocks"][current_block] = []
            continue

        if current_block is None:
            continue

        functions[current_func]["blocks"][current_block].append(line_clean)

        # Check terminators
        cond_match = branch_cond_regex.search(line_clean)
        if cond_match:
            t_dest, f_dest = cond_match.group(1), cond_match.group(2)
            functions[current_func]["edges"].append((current_block, t_dest, "true"))
            functions[current_func]["edges"].append((current_block, f_dest, "false"))
            continue

        uncond_match = branch_uncond_regex.search(line_clean)
        if uncond_match:
            dest = uncond_match.group(1)
            functions[current_func]["edges"].append((current_block, dest, ""))
            continue

    if target_function:
        return {k: v for k, v in functions.items() if k == target_function}
    return functions


def generate_dot(function_name: str, func_data: dict) -> str:
    """Generates Graphviz DOT representation for a function CFG."""
    lines = [f"digraph \"{function_name}\" {{", "  node [fontname=\"Courier\", shape=box, style=filled];", "  edge [fontname=\"Helvetica\"];"]

    for block_name, instructions in func_data["blocks"].items():
        # Classify block type and styling
        if "rv.vector.dispatch" in block_name:
            style = "fillcolor=\"#FFF4E5\", color=\"#B25E00\", penwidth=2.0"
            role = "[Vector Dispatch]"
        elif "rv.vector.body" in block_name:
            style = "fillcolor=\"#E6F4EA\", color=\"#137333\", penwidth=2.5"
            role = "[Vector Loop Body (SIMD)]"
        elif "rv.vector.exit" in block_name:
            style = "fillcolor=\"#E8F0FE\", color=\"#1A73E8\", penwidth=2.0"
            role = "[Vector Exit / Remainder Bridge]"
        elif block_name in ("loop", "for.body", "while.body"):
            style = "fillcolor=\"#FEF7E0\", color=\"#B06000\", penwidth=1.5"
            role = "[Scalar Remainder / Fallback Loop]"
        elif block_name == "entry":
            style = "fillcolor=\"#F1F3F4\", color=\"#5F6368\", penwidth=1.5"
            role = "[Function Entry]"
        elif "exit" in block_name or any(instr.startswith("ret ") for instr in instructions):
            style = "fillcolor=\"#FCE8E6\", color=\"#C5221F\", penwidth=1.5"
            role = "[Exit]"
        else:
            style = "fillcolor=\"#FFFFFF\", color=\"#BDC1C6\", penwidth=1.0"
            role = ""

        # Format label with instructions summary
        instr_sample = []
        for instr in instructions[:8]:
            escaped = instr.replace('"', '\\"').replace("<", "\\<").replace(">", "\\>")
            instr_sample.append(f"  {escaped}\\l")
        if len(instructions) > 8:
            instr_sample.append(f"  ... ({len(instructions) - 8} more instructions)\\l")

        body_text = "".join(instr_sample)
        header = f"{block_name} {role}".strip()
        label = f"{{{header}|\\l{body_text}}}"
        lines.append(f"  \"{block_name}\" [label=\"{label}\", shape=record, {style}];")

    for src, dst, condition in func_data["edges"]:
        edge_attrs = []
        if condition == "true":
            edge_attrs.append("color=\"#137333\", fontcolor=\"#137333\", label=\"true\"")
        elif condition == "false":
            edge_attrs.append("color=\"#C5221F\", fontcolor=\"#C5221F\", label=\"false\"")
        elif "rv.vector" in src and "rv.vector.body" in dst:
            edge_attrs.append("color=\"#137333\", penwidth=2.0, label=\"backedge\"")

        attr_str = f" [{', '.join(edge_attrs)}]" if edge_attrs else ""
        lines.append(f"  \"{src}\" -> \"{dst}\"{attr_str};")

    lines.append("}\n")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Generate DOT control-flow graphs for vectorized LLVM functions.")
    parser.add_argument("input", type=Path, help="Input LLVM textual IR file (.ll)")
    parser.add_argument("-f", "--function", help="Specific function to visualize (default: all functions)")
    parser.add_argument("-o", "--output", type=Path, help="Output .dot or rendered .png/.svg file")
    args = parser.parse_args()

    if not args.input.is_file():
        print(f"ERROR: Input file not found: {args.input}", file=sys.stderr)
        sys.exit(1)

    ll_content = args.input.read_text(encoding="utf-8")
    parsed_functions = parse_llvm_cfg(ll_content, args.function)

    if not parsed_functions:
        print(f"No functions found matching criteria in {args.input}", file=sys.stderr)
        sys.exit(1)

    combined_dot = []
    for func_name, data in parsed_functions.items():
        combined_dot.append(generate_dot(func_name, data))

    dot_output = "\n".join(combined_dot)

    if args.output:
        suffix = args.output.suffix.lower()
        if suffix in (".png", ".svg", ".pdf"):
            dot_binary = shutil.which("dot")
            if not dot_binary:
                print("WARNING: Graphviz 'dot' binary not found on PATH. Writing raw .dot instead.", file=sys.stderr)
                dot_file = args.output.with_suffix(".dot")
                dot_file.write_text(dot_output, encoding="utf-8")
                print(f"Wrote DOT file to: {dot_file}")
                return

            dot_format = suffix.lstrip(".")
            proc = subprocess.run([dot_binary, f"-T{dot_format}", "-o", str(args.output)],
                                  input=dot_output, text=True, capture_output=True)
            if proc.returncode == 0:
                print(f"Rendered CFG visualization to: {args.output}")
            else:
                print(f"dot error:\n{proc.stderr}", file=sys.stderr)
        else:
            args.output.write_text(dot_output, encoding="utf-8")
            print(f"Wrote DOT graph to: {args.output}")
    else:
        print(dot_output)


if __name__ == "__main__":
    main()
