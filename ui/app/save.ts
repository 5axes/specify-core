// Save, behind one small interface: today the page writes the STEP with its engine and POSTs the
// bytes (contract of specify-core 0.1.18+ #198); next, the command writes it from the session's
// answers (`POST save {rev, acceptDefaults}` → 200 / 409 stale rev / 422 missing). Only this file
// changes when the contract does.
import type { WorkerClient } from '../src/engine/rpc';
import type { IntentReview } from '../src/types/pmi';
import { needs } from '../src/specify/review';
import type { AnswersStore } from './answers';
import { save as postStep } from './local';

export interface SaveOptions {
  /** Take Specify's default for each question not answered. Only ever the person's explicit choice. */
  acceptDefaults: boolean;
}
export type SaveResult =
  | { ok: true; path: string; bytes: number }
  /** Required questions are unanswered; ``missing`` lists them when known. Nothing was written. */
  | { ok: false; missing: string[]; message: string };

export interface Saver { save(options: SaveOptions): Promise<SaveResult> }

/** The current contract: the engine in the page writes the STEP, the command puts it on disk. */
export function engineSaver(worker: WorkerClient, file: { name: string; bytes: Uint8Array; sourceId: string }, store: AnswersStore): Saver {
  return {
    async save({ acceptDefaults }) {
      const answers = store.get();
      if (!acceptDefaults) {
        // The engine's writePmi does not refuse a required question left open (it writes the part
        // without it), so the page checks first, as the command's 422 will.
        const { questions } = await worker.request<IntentReview>('review', { ...file, answers });
        const missing = questions.filter((q) => needs(q, answers) === 'required').map((q) => q.id);
        if (missing.length) return { ok: false, missing, message: 'required questions are unanswered' };
      }
      const document = await worker.request<{ bytes: Uint8Array }>('write', { ...file, answers, acceptDefaults });
      const written = await postStep(document.bytes);
      return { ok: true, ...written };
    },
  };
}
