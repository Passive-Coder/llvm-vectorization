import fs from 'node:fs/promises';
import path from 'node:path';
import { FileBlob, PresentationFile } from '@oai/artifact-tool';

const workspace = '/Users/sreerajmuthaiya.a.l/llvm-vectorization';
const input = path.join(workspace, 'output/pptx/Rust-Loop-Vectorizer-for-LLVM-21-final.pptx');
const out = path.join(workspace, '.deck-build');
const deck = await PresentationFile.importPptx(await FileBlob.load(input));
for (let i = 0; i < deck.slides.items.length; i++) {
  const png = await deck.export({ slide: deck.slides.items[i], format: 'png', scale: 1 });
  await fs.writeFile(path.join(out, `final-slide-${String(i + 1).padStart(2, '0')}.png`), new Uint8Array(await png.arrayBuffer()));
}
const montage = await deck.export({ format: 'webp', montage: true });
await fs.writeFile(path.join(out, 'final-montage.webp'), new Uint8Array(await montage.arrayBuffer()));
const inspection = await deck.inspect({ select: '$.slides[*]', include: ['index','title','elements.kind','elements.text'] });
await fs.writeFile(path.join(out, 'final-inspection.ndjson'), inspection.ndjson);
console.log('Rendered', deck.slides.items.length, 'slides');
