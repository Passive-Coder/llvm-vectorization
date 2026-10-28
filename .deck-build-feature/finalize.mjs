import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const skillDir = "/Users/sreerajmuthaiya.a.l/.codex/plugins/cache/openai-primary-runtime/presentations/26.1013.11914/skills/presentations";
const workspaceDir = "/Users/sreerajmuthaiya.a.l/llvm-vectorization";
const source = path.join(workspaceDir, "output/pptx/Rust-Loop-Vectorizer-for-LLVM-21-new-changes.pptx");
const candidatePath = path.join(workspaceDir, ".deck-build-feature/candidate.pptx");
const finalPath = path.join(workspaceDir, "output/pptx/Rust-Loop-Vectorizer-for-LLVM-21-feature-explainer-v2.pptx");
const { finalizePresentation } = await import(pathToFileURL(path.join(skillDir, "container_tools/artifact_tool_utils.mjs")).href);

await fs.mkdir(path.dirname(finalPath), { recursive: true });
const result = await finalizePresentation({
  workspaceDir,
  candidatePath,
  finalPath,
  pythonExecutable: "/Users/sreerajmuthaiya.a.l/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3",
  integrityValidatorPath: path.join(skillDir, "container_tools/inspect_presentation_package_integrity.py"),
  layoutValidatorPath: path.join(skillDir, "container_tools/inspect_presentation_layout_geometry.py"),
  layoutArgs: [
    "--expected-slide-size-emu", "12191695,6858000",
    "--validate-bullet-geometry",
    "--validate-heading-fit",
  ],
  explicitTotalSlideCount: 13,
  requiredNativeTableOwnerSlides: [],
  requiredNativeChartOwnerSlides: [],
  fontPolicy: {
    basis: "reference",
    families: ["Consolas", "Exo", "Exo Bold", "Exo SemiBold", "Fira Mono", "Fira Mono Medium", "Michroma", "MuseoModerno Bold"],
    referencePath: source,
    referenceSha256: "b046d2d8ec64d8caf928ac159f758b656cf31f30a2f5c95509c1fa07fb58f937",
  },
  verifyArtifactToolImport: true,
  receiptPath: path.join(workspaceDir, ".deck-build-feature/final-validation-v2.json"),
});
console.log(JSON.stringify({ finalPath: result.finalPath, slideCount: result.packageIntegrity.slide_count, sha256: result.finalSha256 }));
