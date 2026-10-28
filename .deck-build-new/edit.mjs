import fs from "node:fs/promises";
import { FileBlob, PresentationFile } from "@oai/artifact-tool";

const source = "/Users/sreerajmuthaiya.a.l/Downloads/Rust-Loop-Vectorizer-for-LLVM-21 (1).pptx";
const buildDir = "/Users/sreerajmuthaiya.a.l/llvm-vectorization/.deck-build-new";
const deck = await PresentationFile.importPptx(await FileBlob.load(source));

function shape(slideNumber, name) {
  const found = deck.slides.items[slideNumber - 1].shapes.items.find(item => item.name === name);
  if (!found) throw new Error(`Missing ${name} on slide ${slideNumber}`);
  return found;
}

function replace(slideNumber, name, before, after) {
  const item = shape(slideNumber, name);
  if (!String(item.text).includes(before)) {
    throw new Error(`Slide ${slideNumber}, ${name} did not contain: ${before}`);
  }
  item.text.replace(before, after);
  if (!String(item.text).includes(after)) {
    throw new Error(`Slide ${slideNumber}, ${name} replacement failed: ${after}`);
  }
}

replace(7, "Text 1", "rv-vectorize-tui: an interactive Ratatui terminal app", "rv-vectorize-tui now accepts project folders");
replace(7, "Text 3", "Built with Rust's Ratatui and Crossterm, custom warm-palette UI", "Built with Ratatui and Crossterm; now supports folder runs");
replace(7, "Text 3", " input IR, output path, policy, vector width overrides", " source folder, output folder, policy, vector width");
replace(7, "Text 3", " per-loop legality proofs, chosen VF, estimated speedup, timing", " per-file results, loop decisions, chosen VF, timing");

replace(9, "Text 1", "A production CLI with a diagnostic engine", "Project-wide CLI for multiple languages");
replace(9, "Text 3", "Designed for direct integration into Makefiles, CMake toolchains, and continuous integration pipelines.", "Recursively processes source folders, preserves output paths, and reports each file.");
replace(9, "Text 5", "rv-vectorize --report --policy balanced input.ll -o output.ll", "rv-vectorize src/ -o build/vec --report-json report.json");
replace(9, "Text 6", "PASS REMARKS", "LANGUAGE INPUTS");
replace(9, "Text 7", "Emits standard LLVM pass remarks and structured JSON diagnostics for CI regression tracking.", "Adapters for C/C++, Objective-C/++, Fortran, Rust and Swift. Other languages enter through LLVM IR or bitcode.");
replace(9, "Text 8", "IR VERIFIER", "PROJECT REPORTS");
replace(9, "Text 9", "Invokes LLVM IR Verifier after transformation to guarantee structural and SSA integrity.", "Per-file JSON decisions and skip reasons. Parallel jobs and verified cache reuse support CI runs.");

// The supplied deck puts its Gamma badge on two shared layouts.
let removedBadges = 0;
for (const layout of deck.layouts.items) {
  for (const image of [...(layout.images?.items ?? [])]) {
    const { left, top, width, height } = image.frame;
    if (left > 1100 && top > 650 && width < 130 && height < 35) {
      image.delete();
      removedBadges++;
    }
  }
}
if (removedBadges !== 2) throw new Error(`Expected two shared Gamma badges, found ${removedBadges}`);

await fs.mkdir(buildDir, { recursive: true });
for (const [index, slide] of deck.slides.items.entries()) {
  const png = await deck.export({ slide, format: "png", scale: 1 });
  await fs.writeFile(`${buildDir}/edited-slide-${String(index + 1).padStart(2, "0")}.png`, new Uint8Array(await png.arrayBuffer()));
}
const pptx = await PresentationFile.exportPptx(deck);
await pptx.save(`${buildDir}/candidate.pptx`);
console.log(JSON.stringify({ slides: deck.slides.items.length, removedBadges }));
