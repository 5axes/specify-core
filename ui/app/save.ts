// Save, behind one small interface: today the page writes the STEP with its engine and POSTs the
// bytes (contract of specify-core 0.1.18+ #198); next, the command writes it from the session's
// answers (`POST save {rev, acceptDefaults}` → 200 / 409 stale rev). Only this file changes when the
// contract does. Save never refuses for open questions: many AP242 files have no complete answer to
// everything, and a part with incomplete PMI is still worth saving.
import type { WorkerClient } from '../src/engine/rpc';
import type { IntentReview } from '../src/types/pmi';
import { needs } from '../src/specify/review';
import type { AnswersStore } from './answers';
import { save as postStep } from './local';

export interface SaveOptions {
  /** Take Specify's default for each question not answered. Only ever the person's explicit choice. */
  acceptDefaults: boolean;
}
/** What was written, and the questions still open in it (none when complete). */
export interface SaveResult { path: string; bytes: number; open: string[] }

export interface Saver { save(options: SaveOptions): Promise<SaveResult> }

/** The current contract: the engine in the page writes the STEP, the command puts it on disk. */
export function engineSaver(worker: WorkerClient, file: { name: string; bytes: Uint8Array; sourceId: string }, store: AnswersStore): Saver {
  return {
    async save({ acceptDefaults }) {
      const answers = store.get();
      const document = await worker.request<{ bytes: Uint8Array }>('write', { ...file, answers, acceptDefaults });
      const written = await postStep(document.bytes);
      // What is open in the file as written: unanswered questions a person still has to answer or
      // check; with the defaults taken, only the required ones that have no default.
      const { questions } = await worker.request<IntentReview>('review', { ...file, answers });
      const open = questions
        .filter((q) => (acceptDefaults ? needs(q, answers) === 'required' : needs(q, answers) !== null))
        .map((q) => q.id);
      return { ...written, open };
    },
  };
}
