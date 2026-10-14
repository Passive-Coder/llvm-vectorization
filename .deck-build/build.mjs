import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { Presentation, PresentationFile } from '@oai/artifact-tool';

const workspaceDir = '/Users/sreerajmuthaiya.a.l/llvm-vectorization';
const buildDir = path.join(workspaceDir, '.deck-build');
const outputDir = path.join(workspaceDir, 'output/pptx');
const finalPath = path.join(outputDir, 'Rust-Loop-Vectorizer-for-LLVM-21-final.pptx');
const skillDir = '/Users/sreerajmuthaiya.a.l/.codex/plugins/cache/openai-primary-runtime/presentations/26.1013.11914/skills/presentations';
const runtimePython = '/Users/sreerajmuthaiya.a.l/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3';

const C = {
  bg: '#24282E', navy: '#071728', panel: '#19242D', white: '#F5F8F8',
  body: '#D1DADD', muted: '#9FB0B8', teal: '#4CD4C9', lime: '#A4E56D',
  line: '#4B5962', pale: '#E8FBF8',
};
const FONT = 'Arial';
const deck = Presentation.create({ slideSize: { width: 1280, height: 720 } });
function asArrayBuffer(bytes) {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
}
const assets = {
  cover: asArrayBuffer(await fs.readFile(path.join(buildDir, 'cover.png'))),
  simd: asArrayBuffer(await fs.readFile(path.join(buildDir, 'simd.png'))),
  tail: asArrayBuffer(await fs.readFile(path.join(buildDir, 'tail.png'))),
  languages: asArrayBuffer(await fs.readFile(path.join(buildDir, 'languages.png'))),
};

function addText(slide, value, x, y, w, h, size = 25, color = C.body, bold = false, opts = {}) {
  const sh = slide.shapes.add({
    geometry: 'textbox',
    position: { left: x, top: y, width: w, height: h },
    fill: 'none', line: { style: 'solid', fill: 'none', width: 0 },
  });
  sh.text = value;
  sh.text.style = {
    typeface: FONT, fontSize: size, bold, color,
    alignment: opts.align ?? 'left', verticalAlignment: opts.valign ?? 'top',
    wrap: 'square', autoFit: 'shrinkText', insets: 0,
  };
  return sh;
}

function rect(slide, x, y, w, h, fill, line = 'none', radius = 0) {
  return slide.shapes.add({
    geometry: radius ? 'roundRect' : 'rect',
    position: { left: x, top: y, width: w, height: h },
    fill,
    line: { style: 'solid', fill: line, width: line === 'none' ? 0 : 1.5 },
    ...(radius ? { borderRadius: radius } : {}),
  });
}

function rule(slide, x, y, w, color = C.line) {
  rect(slide, x, y, w, 1.5, color);
}

function title(slide, value, number) {
  addText(slide, value, 72, 53, 1135, 72, 46, C.white, true);
  rect(slide, 72, 133, 72, 5, C.teal);
  if (number) {
    rule(slide, 72, 664, 1136);
    addText(slide, String(number).padStart(2, '0') + ' / 10', 1094, 674, 114, 22, 16, C.muted, false, { align: 'right' });
  }
}

function bullet(slide, value, x, y, w, h = 68, size = 25, color = C.body) {
  rect(slide, x, y + 10, 7, 7, C.teal);
  addText(slide, value, x + 23, y, w - 23, h, size, color);
}

function label(slide, value, x, y, w) {
  addText(slide, value, x, y, w, 30, 19, C.teal, true);
}

function stage(slide, heading, detail, x, y, w, h) {
  rect(slide, x, y, w, h, C.panel, C.line, 12);
  addText(slide, heading, x + 19, y + 20, w - 38, 34, 23, C.white, true);
  addText(slide, detail, x + 19, y + 66, w - 38, h - 78, 19, C.body);
}

function arrow(slide, x, y, w = 28) {
  slide.shapes.add({
    geometry: 'rightArrow', position: { left: x, top: y, width: w, height: 25 },
    fill: C.teal, line: { style: 'solid', fill: 'none', width: 0 },
  });
}

// 1. Cover
{
  const s = deck.slides.add();
  s.background.fill = C.navy;
  s.images.add({ blob: assets.cover, contentType: 'image/png', alt: 'Abstract data streams forming SIMD lanes', fit: 'cover', position: { left: 0, top: 0, width: 1280, height: 720 } });
  addText(s, 'Rust Loop Vectorizer\nfor LLVM 21', 75, 184, 685, 153, 58, C.white, true);
  addText(s, 'A correctness-first compiler pass and project vectorization CLI', 80, 362, 622, 80, 28, C.pale);
  rect(s, 80, 493, 75, 5, C.teal);
  addText(s, 'Rust pass  ·  LLVM 21 plugin  ·  project-scale tooling', 80, 523, 635, 36, 20, C.body);
  s.speakerNotes.text = 'Source: project README.md and docs/architecture.md. The visual is conceptual and contains no factual labels.';
}

// 2. Problem and goal
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Why loop vectorization is hard', 2);
  bullet(s, 'Scalar loops process one element at a time, even when a CPU can run several lanes together.', 72, 204, 605, 94, 26);
  bullet(s, 'A compiler must prove that grouping iterations preserves control flow and memory behavior.', 72, 317, 605, 94, 26);
  bullet(s, 'This project favors safe transformations and explains every rejection.', 72, 430, 605, 94, 26);
  s.images.add({ blob: assets.simd, contentType: 'image/png', alt: 'One data stream dividing into four parallel lanes', fit: 'cover', position: { left: 728, top: 161, width: 480, height: 443 } });
  addText(s, 'Sequential work', 742, 614, 205, 31, 18, C.body);
  addText(s, 'Parallel lanes', 1013, 614, 195, 31, 18, C.lime, true, { align: 'right' });
}

// 3. Architecture, all labels editable
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'System architecture', 3);
  const y = 227, h = 205, w = 198;
  stage(s, 'Inputs', 'Source files, build metadata, .ll or .bc', 55, y, w, h);
  stage(s, 'LLVM module', 'A frontend emits IR when needed', 298, y, w, h);
  stage(s, 'Rust pass', 'Discover, prove legal, plan, transform', 541, y, w, h);
  stage(s, 'LLVM pass', 'Optional in hybrid or LLVM mode', 784, y, w, h);
  stage(s, 'Verify + emit', 'Checked .ll, .bc or .o output', 1027, y, w, h);
  for (const x of [260, 503, 746, 989]) arrow(s, x, 317, 30);
  rule(s, 73, 491, 1134);
  addText(s, 'The C++ bridge registers the plugin with LLVM’s New Pass Manager. The LLVM-only mode skips the Rust pass.', 74, 515, 1128, 70, 24, C.body);
  s.speakerNotes.text = 'Source: docs/architecture.md. The high-level arrows show hybrid flow; LLVM-only mode bypasses the Rust pass.';
}

// 4. Rust loop subset
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'What the Rust pass vectorizes', 4);
  bullet(s, 'Canonical, single-block count-up loops', 72, 202, 598, 58, 26);
  bullet(s, 'Contiguous loads and stores of integers, half, float, and double', 72, 274, 598, 80, 25);
  bullet(s, 'Arithmetic, shifts, casts, comparisons, and select operations', 72, 370, 598, 82, 25);
  bullet(s, 'Vector factors 2, 4, 8, or 16 after legality checks', 72, 466, 598, 80, 25);
  bullet(s, 'Scalar fallback for short trips and remainder elements', 72, 562, 598, 66, 25);
  s.images.add({ blob: assets.tail, contentType: 'image/png', alt: 'Parallel data lanes with a small scalar remainder path', fit: 'cover', position: { left: 700, top: 170, width: 508, height: 450 } });
  s.speakerNotes.text = 'Source: docs/architecture.md, sections 4 and 5. The image is conceptual; the textual list defines actual pass support.';
}

// 5. Safety and planning
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Safety and vectorization decisions', 5);
  const y = 222, h = 172, w = 245;
  stage(s, 'Discover', 'Find a canonical loop candidate', 68, y, w, h);
  stage(s, 'Prove legal', 'Check dataflow, memory, aliasing and layout', 366, y, w, h);
  stage(s, 'Estimate cost', 'Select a profitable vector width', 664, y, w, h);
  stage(s, 'Transform', 'Emit vectors and keep scalar tail', 962, y, w, h);
  for (const x of [322, 620, 918]) arrow(s, x, 295, 34);
  rule(s, 72, 453, 1136);
  label(s, 'Fail closed', 74, 483, 180);
  addText(s, 'Uncertain aliasing, unsupported instructions, or incompatible target layout cause rejection.', 74, 519, 1090, 54, 25, C.body);
  label(s, 'Planning controls', 74, 595, 205);
  addText(s, 'Conservative, balanced, and aggressive policies. Forced width never bypasses legality.', 280, 595, 900, 43, 22, C.body);
  s.speakerNotes.text = 'Source: docs/architecture.md. Target register width is queried through LLVM TargetTransformInfo when the module has a triple; otherwise the planner uses 128 bits.';
}

// 6. Project and language inputs
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Whole-project and language inputs', 6);
  bullet(s, 'Recursive folder traversal with mirrored output paths', 72, 202, 600, 62, 25);
  bullet(s, 'Built-in adapters: C, C++, Objective-C, Objective-C++, Fortran, Rust, Swift', 72, 283, 600, 89, 24);
  bullet(s, '--compile-commands preserves C-family build flags and working directories', 72, 387, 600, 82, 24);
  bullet(s, '--input-manifest or --frontend-config integrates other LLVM frontends', 72, 486, 600, 83, 24);
  addText(s, 'Requires LLVM 21-compatible IR or bitcode.', 95, 603, 570, 34, 20, C.lime, true);
  s.images.add({ blob: assets.languages, contentType: 'image/png', alt: 'Multiple source documents converging through compilers into one IR cube', fit: 'cover', position: { left: 700, top: 158, width: 508, height: 474 } });
  s.speakerNotes.text = 'Source: README.md, Project directories and languages; docs/advanced-cli-spec.md. Frontend availability and LLVM IR compatibility vary by installed compiler.';
}

// 7. Engines and output choices
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Engines and output choices', 7);
  const xs = [73, 470, 867], widths = [350, 350, 340];
  const entries = [
    ['Rust', 'The project’s conservative pass. It reports its own loop decisions.'],
    ['Hybrid', 'Rust pass followed by LLVM’s Loop Vectorizer for broader LLVM-supported loops.'],
    ['LLVM', 'LLVM’s Loop Vectorizer alone. The Rust pass does not run.'],
  ];
  entries.forEach(([name, detail], i) => {
    addText(s, name, xs[i], 216, widths[i], 57, 38, i === 1 ? C.lime : C.teal, true);
    rule(s, xs[i], 285, widths[i], C.line);
    addText(s, detail, xs[i], 318, widths[i], 167, 25, C.body);
  });
  rule(s, 73, 511, 1134);
  label(s, 'Outputs', 73, 544, 150);
  addText(s, '.ll  readable IR', 267, 540, 260, 48, 26, C.white, true);
  addText(s, '.bc  bitcode', 564, 540, 260, 48, 26, C.white, true);
  addText(s, '.o  linkable object', 846, 540, 350, 48, 26, C.white, true);
  addText(s, 'Target triple, CPU, and feature flags configure planning and object emission.', 73, 604, 1110, 43, 21, C.muted);
  s.speakerNotes.text = 'Source: README.md, Build-aware and advanced project runs. Reductions, runtime pointer checks, and multi-block predication in hybrid/LLVM modes are LLVM transformations, not Rust-pass features.';
}

// 8. Developer workflow
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Developer workflow and large projects', 8);
  label(s, 'Inspect results', 72, 205, 500);
  bullet(s, 'Text and JSON reports show file status and loop decisions.', 72, 251, 515, 80, 23);
  bullet(s, '--doctor checks tools, plugin loading, and optional frontends.', 72, 346, 515, 90, 23);
  bullet(s, 'The terminal UI exposes paths, policy, width, and a results pane.', 72, 455, 515, 91, 23);
  rect(s, 627, 203, 2, 390, C.line);
  label(s, 'Run at scale', 674, 205, 510);
  bullet(s, '--jobs runs modules concurrently; --cache-dir reuses verified artifacts.', 674, 251, 527, 88, 23);
  bullet(s, 'Time, memory, and input-size limits bound each job.', 674, 358, 527, 80, 23);
  bullet(s, 'CI gates missed loops and compares baseline/candidate behavior.', 674, 455, 527, 92, 23);
  rule(s, 72, 603, 1136);
  addText(s, 'Atomic writes preserve earlier outputs when compilation or verification fails.', 74, 620, 1120, 34, 21, C.lime);
  s.speakerNotes.text = 'Source: README.md and docs/advanced-cli-spec.md. --fail-on-missed measures Rust-engine rejections only. Cache reuse is limited to inputs with accounted dependencies.';
}

// 9. Verified results, editable chart
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Verification and measured results', 9);
  label(s, 'Local seven-kernel benchmark', 73, 205, 500);
  addText(s, 'All scalar, Rust-pass, and LLVM variants passed correctness.', 73, 249, 478, 84, 25, C.body);
  addText(s, '128 / 128', 73, 356, 470, 68, 48, C.lime, true);
  addText(s, 'cloned loops transformed', 73, 415, 470, 40, 24, C.body);
  addText(s, '2.355 µs', 73, 485, 470, 62, 40, C.teal, true);
  addText(s, 'median in-pass analysis + transform', 73, 535, 470, 40, 22, C.body);
  addText(s, 'Quick differential suite: 8,192 executions', 73, 607, 510, 33, 20, C.muted);
  const chart = s.charts.add('bar', {
    position: { left: 585, top: 192, width: 622, height: 391 },
    categories: ['Rust pass', 'LLVM Loop Vectorizer'],
    series: [{
      name: 'Geometric mean speedup', values: [3.009, 2.974], fill: C.teal,
      points: [{ idx: 0, fill: C.teal }, { idx: 1, fill: C.lime }],
    }],
    barOptions: { direction: 'column', grouping: 'clustered', gapWidth: 78 },
    hasLegend: false,
    chartFill: C.bg, chartLine: { style: 'solid', fill: 'none', width: 0 },
    plotAreaFill: C.bg, plotAreaLine: { style: 'solid', fill: 'none', width: 0 },
    xAxis: { visible: true, textStyle: { fill: C.body, fontSize: 17 }, line: { style: 'solid', fill: C.line, width: 1 } },
    yAxis: {
      visible: true, title: { text: 'Speedup vs scalar (×)', textStyle: { fill: C.body, fontSize: 16 } },
      min: 0, max: 3.5, majorUnit: 0.5, numberFormatCode: '0.0',
      textStyle: { fill: C.body, fontSize: 16 },
      line: { style: 'solid', fill: C.line, width: 1 },
      majorGridlines: { style: 'solid', fill: '#43505A', width: 1 },
    },
    dataLabels: { showValue: true, position: 'outEnd', numberFormatCode: '0.000"×"', textStyle: { fill: C.white, fontSize: 18, bold: true } },
  });
  const { applyPresentationChartFont } = await import(pathToFileURL(path.join(skillDir, 'container_tools/artifact_tool_utils.mjs')).href);
  applyPresentationChartFont(chart, { fontFamily: FONT });
  addText(s, 'Source: docs/benchmark-results.json. Apple M5, LLVM 21.1.3, seven cache-hot kernels. Local results, not application-wide speedups.', 590, 593, 616, 61, 17, C.muted);
  s.speakerNotes.text = 'Source: docs/benchmark-results.json (generated 2026-09-07) and scripts/differential_test.py quick run. Benchmark: seven kernels, Apple M5, LLVM 21.1.3, cache-hot measurements. Rust pass geometric mean 3.008593× and LLVM 2.973528× versus scalar. The speedup difference is small and not a general claim of superiority.';
}

// 10. Scope and next research
{
  const s = deck.slides.add(); s.background.fill = C.bg; title(s, 'Scope and next steps', 10);
  label(s, 'Rust pass today', 73, 208, 525);
  bullet(s, 'Real fixed-width vector IR and explainable decisions', 73, 257, 520, 73, 24);
  bullet(s, 'A bounded single-block loop legality contract', 73, 352, 520, 70, 24);
  bullet(s, 'Unsupported loops stay scalar in Rust-only mode', 73, 443, 520, 73, 24);
  rect(s, 626, 202, 2, 350, C.line);
  label(s, 'Future Rust-pass research', 674, 208, 520);
  bullet(s, 'Richer target throughput and register-pressure costing', 674, 257, 520, 78, 24);
  bullet(s, 'Scalable vectors, masked tails, and interleaving', 674, 352, 520, 72, 24);
  bullet(s, 'Legality proofs for reductions and multi-block loops', 674, 443, 520, 74, 24);
  rule(s, 73, 586, 1135);
  addText(s, 'Hybrid mode delegates broader loop shapes to LLVM’s production vectorizer.', 73, 608, 1130, 39, 24, C.lime, true);
  s.speakerNotes.text = 'Source: docs/architecture.md and README.md. The future items are not implemented by the Rust pass. LLVM-backed modes may vectorize some of those shapes.';
}

await fs.mkdir(buildDir, { recursive: true });
await fs.mkdir(outputDir, { recursive: true });
for (let i = 0; i < deck.slides.items.length; i++) {
  const blob = await deck.export({ slide: deck.slides.items[i], format: 'png', scale: 1 });
  await fs.writeFile(path.join(buildDir, `slide-${String(i + 1).padStart(2, '0')}.png`), new Uint8Array(await blob.arrayBuffer()));
}
const montage = await deck.export({ format: 'webp', montage: true });
await fs.writeFile(path.join(buildDir, 'montage.webp'), new Uint8Array(await montage.arrayBuffer()));

const { finalizePresentation } = await import(pathToFileURL(path.join(skillDir, 'container_tools/artifact_tool_utils.mjs')).href);
const stagingDir = path.join(workspaceDir, '.codex-finalizer');
await fs.mkdir(stagingDir, { recursive: true });
const candidatePath = path.join(stagingDir, 'candidate-vectorizer.pptx');
await (await PresentationFile.exportPptx(deck)).save(candidatePath);
const result = await finalizePresentation({
  workspaceDir,
  candidatePath,
  finalPath,
  pythonExecutable: runtimePython,
  integrityValidatorPath: path.join(skillDir, 'container_tools/inspect_presentation_package_integrity.py'),
  layoutValidatorPath: path.join(skillDir, 'container_tools/inspect_presentation_layout_geometry.py'),
  layoutArgs: ['--expected-slide-size-emu', '12192000,6858000', '--validate-bullet-geometry', '--validate-heading-fit'],
  explicitTotalSlideCount: 10,
  requiredNativeChartOwnerSlides: [9],
  materializeLiteralChartWorkbooks: true,
  fontPolicy: { basis: 'design', families: [FONT] },
  verifyArtifactToolImport: true,
  receiptPath: path.join(stagingDir, 'vectorizer-final.validation.json'),
});
console.log(JSON.stringify(result));
