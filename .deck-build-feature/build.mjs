import fs from "node:fs/promises";
import { FileBlob, PresentationFile } from "@oai/artifact-tool";

const workspaceDir = "/Users/sreerajmuthaiya.a.l/llvm-vectorization";
const buildDir = `${workspaceDir}/.deck-build-feature`;
const sourcePath = `${workspaceDir}/output/pptx/Rust-Loop-Vectorizer-for-LLVM-21-new-changes.pptx`;
const deck = await PresentationFile.importPptx(await FileBlob.load(sourcePath));
if (deck.slides.items.length !== 10) throw new Error("Expected the revised 10-slide source deck");
const original = [...deck.slides.items];

function named(slide, name) {
  const item = slide.shapes.items.find(shape => shape.name === name);
  if (!item) throw new Error(`Missing ${name} on slide ${slide.id}`);
  return item;
}
function replace(slide, name, before, after) {
  const item = named(slide, name);
  if (!String(item.text).includes(before)) throw new Error(`Missing text on ${slide.id}/${name}: ${before}`);
  item.text.replace(before, after);
  if (!String(item.text).includes(after)) throw new Error(`Replacement failed on ${slide.id}/${name}`);
}

const folder = original[1].duplicate();
replace(folder, "Text 0", "THE VECTORIZATION DILEMMA", "NEW PROJECT WORKFLOW");
replace(folder, "Text 1", "Vectorization is a proof-and-planning problem, not opcode substitution", "Project-wide vectorization\nwith isolated file outputs");
replace(folder, "Text 5", "CHALLENGE", "DISCOVERY");
replace(folder, "Text 6", "C++ MONOLITHS", "FOLDER INPUT");
replace(folder, "Text 7", "Traditional C++ LLVM passes are monolithic, prone to memory unsafety, and complex to reason about.", "Walks the tree recursively. Skips symlinks, hidden and generated directories, and non-source assets.");
replace(folder, "Text 10", "LEGALITY", "SELECTION");
replace(folder, "Text 11", "MEMORY PROOFS", "FILE FILTERS");
replace(folder, "Text 12", "Prove memory independence without quadratic compilation slowdowns.", "Uses .gitignore rules and include/exclude patterns to choose files.");
replace(folder, "Text 15", "COST MODELING", "COMPILATION");
replace(folder, "Text 16", "VECTOR TAX", "LLVM MODULES");
replace(folder, "Text 17", "Avoid the “vectorization tax” where SIMD setup and remainder overheads exceed savings.", "Each source file becomes its own LLVM module. A failed file does not stop the other files.");
replace(folder, "Text 20", "RUST + LLVM SYNERGY", "OUTPUT");
replace(folder, "Text 21", "RUST + LLVM", "MIRRORED PATHS");
replace(folder, "Text 22", "Memory safety and zero-cost abstractions in a production pass. A fail-closed LLVM 21 plugin in Rust, separating invariant analysis from IR mutation.", "Keeps relative paths under the output folder. Emits .ll, .bc, or linkable .o only after successful processing.");
replace(folder, "Text 23", "02", "10");

const languages = original[7].duplicate();
replace(languages, "Text 0", "IR VERIFICATION & INSPECTION", "LANGUAGE & BUILD INPUTS");
replace(languages, "Text 1", "Side-by-side IR diff verifies every transformation", "Language adapters feed a common LLVM IR pass");
replace(languages, "Text 3", "Interactive diff mode in the TUI via the [DIFF] button or keyboard shortcuts. GitHub-style coloring: scalar input IR in soft red; emitted vector IR in green. Inspect vector widening, memory alignments, and dispatch branches in real time.", "Adapters compile individual sources to LLVM 21-compatible IR. Existing build systems can supply .ll or .bc modules when project-specific flags or dependencies matter.");
replace(languages, "Text 5", "SCALAR INPUT IR", "SOURCE ADAPTERS");
replace(languages, "Text 7", "; scalar input IR\n%s = fadd float %a, %b\nstore float %s, ptr %pc", "C, C++, Objective-C, Objective-C++\nFortran via flang; Rust via rustc\nSwift via swiftc");
replace(languages, "Text 10", "EMITTED VECTOR IR", "BUILD-SUPPLIED IR");
replace(languages, "Text 12", "; vector widening: <4 x float>\n%v = load <4 x float>, ptr %p, align 16\nbr label %remainder", "compile_commands.json keeps C-family flags\ninput-manifest maps sources to .ll / .bc\nfrontend-config adds other languages");
replace(languages, "Text 13", "08", "11");

const engines = original[4].duplicate();
replace(engines, "Text 0", "DETERMINISTIC COST MODELING", "EXECUTION MODES");
replace(engines, "Text 1", "Vectorizable does not mean profitable", "Three engines define loop coverage");
replace(engines, "Text 3", "Short trip counts and remainder epilogue branches can degrade runtime. Operation weights: Loads/Stores 2, Div/Rem 4, Arithmetic/Select 1, Loop Setup 6. Unknown dynamic bounds default to 64.", "The Rust pass stays conservative. Hybrid and LLVM modes use LLVM's Loop Vectorizer for supported reductions, runtime alias checks, and predicated multi-block loops.");
replace(engines, "Text 5", "PROFILE 01", "DEFAULT");
replace(engines, "Text 6", "CONSERVATIVE", "RUST");
replace(engines, "Text 8", "Min Trip Count:", "Pipeline:");
replace(engines, "Text 8", "4 * VF", "Rust pass");
replace(engines, "Text 8", "Cost Ratio:", "Legality:");
replace(engines, "Text 8", "≥ 1.25x", "bounded");
replace(engines, "Text 8", "High certainty bar. Protects cold code and branch-heavy loops against any epilogue penalties.", "Uses custom legality, cost planning, and SIMD code generation. Uncertain loops stay scalar with a remark.");
replace(engines, "Text 9", "LATENCY CRITICAL", "--engine rust");
replace(engines, "Text 12", "DEFAULT", "HYBRID");
replace(engines, "Text 13", "BALANCED", "RUST + LLVM");
replace(engines, "Text 15", "Min Trip Count:", "Pipeline:");
replace(engines, "Text 15", "2 * VF", "Rust + LLVM");
replace(engines, "Text 15", "Cost Ratio:", "Coverage:");
replace(engines, "Text 15", "≥ 1.08x", "broader loops");
replace(engines, "Text 15", "Default compiler configuration. Balances 100% issued-lane utilization against scalar remainder cost.", "Runs the Rust pass first, then LLVM's Loop Vectorizer. LLVM may vectorize loops the Rust pass rejects.");
replace(engines, "Text 16", "GENERAL PRODUCTION", "--engine hybrid");
replace(engines, "Text 18", "PROFILE 03", "LLVM ONLY");
replace(engines, "Text 19", "AGGRESSIVE", "LLVM");
replace(engines, "Text 21", "Min Trip Count:", "Pipeline:");
replace(engines, "Text 21", "1 * VF", "LLVM pass");
replace(engines, "Text 21", "Cost Ratio:", "Coverage:");
replace(engines, "Text 21", "≥ 1.00x", "LLVM rules");
replace(engines, "Text 21", "Transforms any loop where SIMD operations break even, maximizing vector throughput.", "Runs LLVM's Loop Vectorizer alone. LLVM remarks explain its optimization decisions.");
replace(engines, "Text 22", "HPC & NUMERICAL COMPUTE", "--engine llvm");
replace(engines, "Text 23", "05", "12");

for (const [slide, footerName] of [[folder, "Text 23"], [languages, "Text 13"], [engines, "Text 23"]]) {
  const footer = named(slide, footerName);
  footer.position = { ...footer.position, left: 1200, width: 79.97 };
}

const benchmarkPage = original[9].shapes.add({
  geometry: "rect",
  name: "Page 13",
  position: { left: 1200, top: 678.062, width: 79.97, height: 19.875 },
  fill: "none",
  line: { style: "solid", fill: "none", width: 0 },
});
benchmarkPage.text = "13";
benchmarkPage.text.typeface = "Fira Mono";
benchmarkPage.text.fontSize = 14;
benchmarkPage.text.color = "#9D9D9D";
benchmarkPage.text.verticalAlignment = "middle";
benchmarkPage.text.insets = { top: 0, right: 0, bottom: 0, left: 0 };

deck.slides.reorder([...original.slice(0, 9), folder, languages, engines, original[9]].map(slide => `sl/${slide.id}`));
if (deck.slides.items.length !== 13) throw new Error("Expected 13 slides after insertion");
const expected = ["Project-wide vectorization", "Language adapters feed", "Three engines define"];
for (let index = 0; index < expected.length; index++) {
  if (!String(named(deck.slides.items[index + 9], "Text 1").text).includes(expected[index])) {
    throw new Error(`New slide ${index + 10} is out of order`);
  }
}

await fs.mkdir(buildDir, { recursive: true });
for (let index = 0; index < deck.slides.items.length; index++) {
  const png = await deck.export({ slide: deck.slides.items[index], format: "png", scale: 1 });
  await fs.writeFile(`${buildDir}/draft-slide-${String(index + 1).padStart(2, "0")}.png`, new Uint8Array(await png.arrayBuffer()));
}
await (await PresentationFile.exportPptx(deck)).save(`${buildDir}/candidate.pptx`);
console.log(JSON.stringify({ slides: deck.slides.items.length, newSlides: [10, 11, 12] }));
