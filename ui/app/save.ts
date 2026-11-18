// Save: the command writes the specified STEP from the session's answers (`POST save {rev,
// acceptDefaults}`, specify-core docs/open.md), with the same writer and naming as the agent's
// `save`. It never refuses for open questions: many AP242 files have no complete answer to
// everything, and a part with incomplete PMI is still worth saving. It says what is still open.
import type { PmiAnswers } from '../src/types/pmi';
import type { AnswersStore } from './answers.ts';
import { Refused, save as post, type Saved } from './local.ts';

export interface SaveOptions {
  /** The questions whose defaults the person saw listed and confirmed (ADR P9: a default is written
   * only when approved); none for a plain save. */
  defaults: readonly string[];
}

export interface Saver { save(options: SaveOptions): Promise<Saved> }

/** Nothing saved: the session holds answers the page has not shown yet (they are being shown). */
export class NotShown extends Error {}
/** Nothing saved: the session moved on since the page's rev (an agent answered); ``rev`` is the session's. */
export class Stale extends Error {}
/** Nothing saved: (`--in-place`) the file was changed on disk by another program since it was opened. */
export class FileChanged extends Error {}
/** Nothing saved: the defaults a save would write are no longer the ones the person confirmed. */
export class DefaultsChanged extends Error {}

const given = (answers: PmiAnswers) => JSON.stringify(Object.entries(answers).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1)));

/** Saves the session at the revision the page shows, once the page's own changes are in it.
 * ``shown()``: the answers the page shows, so a save never writes answers the person has not seen.
 * ``defaultsOf(answers)``: the questions a save with defaults would default for ``answers``. */
export function sessionSaver(store: AnswersStore, shown: () => PmiAnswers, defaultsOf: (answers: PmiAnswers) => Promise<string[]>, write = post): Saver {
  return {
    async save({ defaults }) {
      const rev = await store.settled();
      const answers = store.get();
      if (given(answers) !== given(shown())) throw new NotShown('the answers changed just now');
      if (defaults.length) {
        const now = await defaultsOf(answers);
        if ([...now].sort().join('\n') !== [...defaults].sort().join('\n')) throw new DefaultsChanged('the defaults to write changed');
      }
      try {
        // TODO(specify-core `POST save {acceptDefaults: [question ids]}`): send ``defaults`` itself,
        // so only those are defaulted. Until then the boolean, which defaults every open question
        // that has one: exactly the confirmed list, checked above at this rev (a later rev is a 409).
        return await write(rev, defaults.length > 0);
      } catch (error) {
        // Both are 409s with the session's rev: when the session is still at the page's rev, it is the file that changed.
        if (error instanceof Refused && error.status === 409 && typeof error.body.rev === 'number') {
          throw error.body.rev === rev ? new FileChanged(error.message) : new Stale(error.message);
        }
        throw error;
      }
    },
  };
}
