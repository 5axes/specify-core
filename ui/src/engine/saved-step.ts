import type { PmiAnswers } from '../types/pmi';

interface SavedPart {
  snapshot(): string;
  writePmi(answers: string): string;
  free(): void;
}

export interface SavedStepEngine {
  new (name: string, bytes: Uint8Array): SavedPart;
  openPart(name: string, bytes: Uint8Array, part: number): SavedPart;
  parts(name: string, bytes: Uint8Array): string;
}

const hasAnswers = (answers: PmiAnswers) => Object.keys(answers).some((id) => !id.startsWith('_'));

/** 0.2.0 has no download endpoint for Save's separate output. Rebuild only the command's saved
 * revision, without accepting defaults, then inspect the written bytes. Never use live answers. */
export function rebuildSavedStep(Engine: SavedStepEngine, name: string, original: Uint8Array, parts: Record<string, PmiAnswers>): Uint8Array {
  const several = (JSON.parse(Engine.parts(name, original)) as unknown[]).length > 1;
  let bytes = original;
  for (const [index, answers] of Object.entries(parts).sort(([a], [b]) => Number(a) - Number(b))) {
    const file = several ? Engine.openPart(name, bytes, Number(index)) : new Engine(name, bytes);
    try {
      const analysis = JSON.parse(file.snapshot()).analysis;
      // A cleared resumed part is written too. Untouched parts with nothing to write retain
      // their original PMI, including requirements absent from recognition's interview.
      if (!hasAnswers(answers) && !hasAnswers(analysis.resume?.answers ?? {})) continue;
      bytes = new Uint8Array(JSON.parse(file.writePmi(JSON.stringify(answers))).bytes);
    } finally { file.free(); }
  }
  return bytes;
}
