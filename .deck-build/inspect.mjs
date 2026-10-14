import fs from 'node:fs/promises';
import { FileBlob, PresentationFile } from '@oai/artifact-tool';

const source = '/Users/sreerajmuthaiya.a.l/Downloads/Rust-Loop-Vectorizer-for-LLVM-21.pptx';
const deck = await PresentationFile.importPptx(await FileBlob.load(source));
const result = await deck.inspect({
  select: '$.slides[*]',
  include: ['id','index','title','elements.id','elements.kind','elements.text','elements.position','elements.style','elements.alt'],
});
await fs.writeFile('.deck-build/inspection.ndjson', result.ndjson);
console.log('slides', deck.slides.items.length);
console.log('size', JSON.stringify(deck.slideSize));
console.log(result.ndjson.slice(0, 6000));
for (const i of [0, 2, 8]) {
  const blob = await deck.export({ slide: deck.slides.items[i], format: 'png', scale: 1 });
  await fs.writeFile(`.deck-build/import-slide-${i+1}.png`, new Uint8Array(await blob.arrayBuffer()));
}
