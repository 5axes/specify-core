// Save, behind one small interface: today the page writes the STEP with its engine and POSTs the
// bytes (contract of specify-core 0.1.18+ #198); next, the command writes it from the session's
// answers (`POST save {rev, acceptDefaults}` → 200 / 409 stale rev / 422 missing). Only this file
// changes when the contract does.
import type { WorkerClient } from '../src/engine/rpc';
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
      let document: { bytes: Uint8Array };
      try {
        document = await worker.request('write', { ...file, answers: store.get(), acceptDefaults });
      } catch (error) {
        // The engine's refusal of open questions: shown with its reason, nothing sent to disk.
        const message = error instanceof Error ? error.message : String(error);
        if (acceptDefaults) throw error;
        return { ok: false, missing: [], message };
      }
      const written = await postStep(document.bytes);
      return { ok: true, ...written };
    },
  };
}
