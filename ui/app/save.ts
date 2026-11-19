// Save: the command writes the specified STEP from the session's answers (`POST save {rev,
// acceptDefaults}`, specify-core docs/open.md), with the same writer and naming as the agent's
// `save`. It never refuses for open questions: many AP242 files have no complete answer to
// everything, and a part with incomplete PMI is still worth saving. It says what is still open.
import type { PmiAnswers } from '../src/types/pmi';
import type { AnswersSession, AnswersStore } from './answers.ts';
import { Refused, save as post, type AcceptDefaults, type Saved } from './local.ts';

/** A default a save with defaults would write: part ``part``'s question ``id`` set to ``value``
 * (``raw``, the default itself, is what is compared; ``value`` is how it reads). */
export interface Default { part: number; id: string; prompt: string; value: string; raw: string }

export interface SaveOptions {
  /** The defaults the person saw listed and confirmed, of every part the save writes (ADR P9: a
   * default is written only when approved); none for a plain save. */
  defaults: readonly Default[];
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
/** Nothing saved: the command refused a default listed (not an open question with a default on that part). */
export class DefaultRefused extends Error {
  readonly part: number | null;
  readonly question: string | null;
  /** Set when the value confirmed is no longer the default: what the person saw, and what it is now. */
  readonly values: { expected: unknown; current: unknown } | null;
  constructor(message: string, part: number | null, question: string | null, values: { expected: unknown; current: unknown } | null = null) {
    super(message); this.part = part; this.question = question; this.values = values;
  }
}

/** The parts a save writes (specify-core docs/open.md): every part with at least one answer in the
 * session (approved defaults count), and a file's only part. A part with no answers is left as it is. */
export function writtenParts(parts: Record<string, PmiAnswers>, several: boolean): number[] {
  const out = new Set(Object.entries(parts).filter(([, answers]) => Object.keys(answers).some((id) => !id.startsWith('_'))).map(([k]) => Number(k)));
  if (!several) out.add(0);
  return [...out].sort((a, b) => a - b);
}

/** The parts a save with defaults lists the defaults of: every part of the file (``indexes``, and any
 * part the session has), so approving a part's defaults is enough for it to be written (the command
 * applies approvals as answers before it writes). A file of one part is part 0. */
export function defaultParts(parts: Record<string, PmiAnswers>, indexes: readonly number[], several: boolean): number[] {
  if (!several) return [0];
  return [...new Set([...indexes, ...Object.keys(parts).map(Number)])].sort((a, b) => a - b);
}

/** The defaults as the command takes them: each question id confirmed with the value the person
 * saw (``raw`` is that value as JSON), by part index. */
export function acceptedDefaults(defaults: readonly Default[]): AcceptDefaults {
  const out: AcceptDefaults = {};
  for (const d of defaults) (out[String(d.part)] ??= {})[d.id] = JSON.parse(d.raw);
  return out;
}

const given = (answers: PmiAnswers) => JSON.stringify(Object.entries(answers).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1)));

/** The same defaults, part, question and value, in whatever order. */
export function sameDefaults(a: readonly Default[], b: readonly Default[]): boolean {
  const key = (list: readonly Default[]) => list.map((d) => JSON.stringify([d.part, d.id, d.raw])).sort().join('\n');
  return key(a) === key(b);
}

/** Saves the session at the revision the page shows, once the page's own changes are in it.
 * ``shown()``: the answers the page shows, so a save never writes answers the person has not seen.
 * ``defaultsOf(parts)``: the defaults a save with defaults would write for the session's answers
 * ``parts`` (every part's, by index): those of every part the save writes. */
export function sessionSaver(store: AnswersStore, session: Pick<AnswersSession, 'parts'>, shown: () => PmiAnswers, defaultsOf: (parts: Record<string, PmiAnswers>) => Promise<Default[]>, write = post): Saver {
  return {
    async save({ defaults }) {
      const rev = await store.settled();
      const answers = store.get();
      const parts = session.parts();
      if (given(answers) !== given(shown())) throw new NotShown('the answers changed just now');
      // The whole set, part by part, value by value: any default added, gone or changed (an agent
      // answered meanwhile) means a list the person has not confirmed.
      if (defaults.length && !sameDefaults(await defaultsOf(parts), defaults)) throw new DefaultsChanged('the defaults to write changed');
      try {
        // Exactly the defaults the person confirmed, each on the part it was listed for; checked
        // above for every part at this rev (a later rev is a 409).
        return await write(rev, acceptedDefaults(defaults));
      } catch (error) {
        // Both are 409s with the session's rev: when the session is still at the page's rev, it is the file that changed.
        if (error instanceof Refused && error.status === 409 && typeof error.body.rev === 'number') {
          throw error.body.rev === rev ? new FileChanged(error.message) : new Stale(error.message);
        }
        if (error instanceof Refused && error.status === 400 && typeof error.body.question === 'string') {
          const values = 'expected' in error.body && 'current' in error.body ? { expected: error.body.expected, current: error.body.current } : null;
          throw new DefaultRefused(error.message, typeof error.body.part === 'number' ? error.body.part : null, error.body.question, values);
        }
        throw error;
      }
    },
  };
}
