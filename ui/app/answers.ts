import type { PmiAnswers } from '../src/types/pmi';
import { putAnswers, Refused, type AnswersEvent, type Source } from './local.ts';

export type AnswersSource = Source;

/** One part's answers in the command's session (specify-core docs/open.md, "The session"): the
 * page's own changes go up with `PUT answers`, everyone else's (an agent's, another tab's) come
 * down as `answers` events. What Save writes is the session's, at ``rev()``. */
export interface AnswersStore {
  get(): PmiAnswers;
  /** A change made in this page: kept here at once and sent to the session. */
  set(answers: PmiAnswers, source: 'page'): void;
  /** ``previous``: the answers before this change, to show what someone else (an agent) changed. */
  subscribe(listener: (answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void): () => void;
  /** An `answers` event from the session. */
  receive(event: AnswersEvent): void;
  /** The session's revision this page has, once its own changes are in. */
  settled(): Promise<number>;
  /** Marks the answers of session revision ``rev`` as on disk (a `saved` event). */
  markSaved(rev: number): void;
  dirty(): boolean;
  /** A send that failed for good (not a 409, which is retried), for the page to show. */
  onProblem(listener: (error: Error) => void): void;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** ``base`` with the change from ``before`` to ``after`` applied: what the page changed, put onto
 * answers someone else has changed meanwhile (a 409's reapply). */
export function reapply(base: PmiAnswers, before: PmiAnswers, after: PmiAnswers): PmiAnswers {
  const next: Record<string, unknown> = { ...base };
  for (const id of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (same(before[id], after[id])) continue;
    if (id in after) next[id] = after[id]; else delete next[id];
  }
  return next as PmiAnswers;
}

/** What is compared for unsaved changes: the answers given, without Specify's own `_` records. */
const asGiven = (answers: PmiAnswers) => JSON.stringify(Object.fromEntries(Object.entries(answers).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1))));

/** ``onDisk``: the answers stored in the file as opened, so answers an agent gave before the page
 * opened show as unsaved. */
export function createSessionStore(part: number, rev: number, initial: PmiAnswers, onDisk: PmiAnswers = initial, put = putAnswers): AnswersStore {
  const key = String(part);
  let current = initial;
  // What the session holds for this part as far as this page knows, and at which revision.
  let server = { rev, answers: initial };
  // What is on disk: the answers of the last save's revision, when this page saw it.
  let saved = asGiven(onDisk);
  let savedRev = -1;
  const history = new Map<number, string>([[rev, asGiven(initial)]]);
  const listeners = new Set<(answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void>();
  const problems = new Set<(error: Error) => void>();
  let pending = 0;
  let chain: Promise<void> = Promise.resolve();
  let lastSource: AnswersSource = 'start';

  const emit = (answers: PmiAnswers, source: AnswersSource) => {
    const previous = current;
    current = answers;
    if (same(previous, answers)) return;
    for (const listener of listeners) listener(answers, source, previous);
  };
  const known = (at: number, answers: PmiAnswers) => {
    if (at < server.rev) return;
    server = { rev: at, answers };
    history.set(at, asGiven(answers));
    if (at === savedRev) saved = asGiven(answers);
  };

  /** Sends one change of the page's: from ``before`` to ``after``, reapplied on a 409. */
  const send = async (before: PmiAnswers, after: PmiAnswers) => {
    for (let attempt = 0; attempt < 8; attempt++) {
      // The page's change, on the answers the session has now (others' changes kept).
      const base = server;
      const wanted = reapply(base.answers, before, after);
      try {
        const { rev: next } = await put(base.rev, part, wanted);
        known(next, wanted);
        // Someone else's change folded in: shown as theirs once the page's own are all in.
        if (pending === 1 && !same(wanted, current)) emit(wanted, lastSource);
        return;
      } catch (error) {
        if (!(error instanceof Refused) || error.status !== 409 || typeof error.body.rev !== 'number' || !error.body.parts) throw error;
        const parts = error.body.parts as Record<string, PmiAnswers>;
        known(error.body.rev, parts[key] ?? {});
      }
    }
    throw new Error('The answers kept changing while this page sent its own. Reload the page.');
  };

  return {
    get: () => current,
    set(answers) {
      const before = current;
      emit(answers, 'page');
      pending++;
      chain = chain.then(() => send(before, answers)).catch((error: unknown) => {
        for (const listener of problems) listener(error instanceof Error ? error : new Error(String(error)));
      }).finally(() => { pending--; });
    },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    receive(event) {
      if (event.rev < server.rev) return;
      const answers = 'parts' in event ? event.parts[key] ?? {} : event.part === part ? event.answers : null;
      if (answers === null) { server = { ...server, rev: event.rev }; return; }
      if (event.source !== 'start') lastSource = event.source;
      known(event.rev, answers);
      // While this page's own changes are on their way, they are reapplied over what arrives.
      if (pending) return;
      emit(answers, event.source);
    },
    async settled() { await chain; return server.rev; },
    markSaved(at) {
      savedRev = at;
      const then = history.get(at);
      if (then !== undefined) saved = then;
      else if (at <= server.rev) saved = asGiven(server.answers);
    },
    dirty: () => asGiven(current) !== saved,
    onProblem(listener) { problems.add(listener); },
  };
}

/** The questions whose answers differ between ``a`` and ``b``. */
export function changedIds(a: PmiAnswers, b: PmiAnswers): string[] {
  return [...new Set([...Object.keys(a), ...Object.keys(b)])]
    .filter((id) => !id.startsWith('_') && JSON.stringify(a[id]) !== JSON.stringify(b[id]));
}
