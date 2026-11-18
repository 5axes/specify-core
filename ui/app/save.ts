// Save: the command writes the specified STEP from the session's answers (`POST save {rev,
// acceptDefaults}`, specify-core docs/open.md), with the same writer and naming as the agent's
// `save`. It never refuses for open questions: many AP242 files have no complete answer to
// everything, and a part with incomplete PMI is still worth saving. It says what is still open.
import type { AnswersStore } from './answers';
import { save as post, type Saved } from './local';

export interface SaveOptions {
  /** Take Specify's default for each question not answered. Only ever the person's explicit choice. */
  acceptDefaults: boolean;
}

export interface Saver { save(options: SaveOptions): Promise<Saved> }

/** Saves the session as this page last saw it, once the page's own changes are in it. A 409 (the
 * session moved on, or the file changed on disk with --in-place) is thrown as `Refused`. */
export function sessionSaver(store: AnswersStore, write = post): Saver {
  return {
    async save({ acceptDefaults }) {
      const rev = await store.settled();
      return write(rev, acceptDefaults);
    },
  };
}
