import type { PmiAnswers } from '../src/types/pmi';
import { putAnswers, Refused, type AnswersEvent, type Source } from './local.ts';

export type AnswersSource = Source;

/** One part's answers in the command's session (specify-core docs/open.md, "The session"): the
 * page's own changes go up with `PUT answers`, everyone else's (an agent's, another tab's) come
 * down as `answers` events. What the page shows is always the session's answers, with the page's
 * own changes that are not in the session yet laid over them. */
export interface AnswersStore {
  readonly part: number;
  get(): PmiAnswers;
  /** A change made in this page: from ``base`` (the answers the page showed when it was made) to
   * ``answers``. Only that change is kept and sent, onto whatever the session holds by then, so an
   * answer someone else gave meanwhile is never taken away. */
  set(answers: PmiAnswers, base: PmiAnswers): void;
  /** Each change to what the page should show. ``previous``: the answers before it, to show what
   * someone else (an agent) changed. */
  subscribe(listener: (answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void): () => void;
  /** The session's revision once the page's own changes are all in it; ``get()`` is then its answers. */
  settled(): Promise<number>;
  /** A send that failed for good (not a 409, which is retried), for the page to show. The change is
   * dropped: the page goes back to what the session holds. */
  onProblem(listener: (error: Error) => void): void;
  /** No more updates (another part is shown). */
  close(): void;
}

/** The command's session as this page knows it: every part's answers at ``rev()``, and what is on
 * disk, so unsaved answers are judged for the whole file, whichever part is shown. */
export interface AnswersSession {
  /** Cancelled before the close beacon, shared by answer PUTs and Save POSTs. */
  readonly signal: AbortSignal;
  rev(): number;
  answers(part: number): PmiAnswers;
  /** Every part's answers at ``rev()``, by part index: the parts a save writes (with a file's only part). */
  parts(): Record<string, PmiAnswers>;
  /** An `answers` event (or a `GET answers` resync, as a "start" event). One rev can carry several
   * events (one per part): an event for a (rev, part) already received is ignored, and false is returned. */
  receive(event: AnswersEvent): boolean;
  /** The store for part ``part``. ``onDisk``: its answers stored in the file as opened, so answers
   * an agent gave before the page opened show as unsaved (until a save). */
  store(part: number, onDisk?: PmiAnswers): AnswersStore;
  /** Part ``part``'s answers stored in the file as opened (none for a part Specify did not write),
   * so answers an agent gave it before the page opened count as unsaved, whether or not the part
   * is ever shown. Ignored once the page has saved. */
  fileAnswers(part: number, answers: PmiAnswers): void;
  /** The answers of session revision ``rev`` are on disk (a save's answer, or a `saved` event). */
  markSaved(rev: number): void;
  /** Answers given, in any part, that are not on disk: since the last save, or since the file was opened. */
  dirty(): boolean;
  /** Called on every change that can change ``dirty()``. */
  subscribe(listener: () => void): () => void;
  /** The page is leaving. Abort every in-flight PUT and prevent queued changes or retries. */
  close(): void;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const sorted = (answers: PmiAnswers) => JSON.stringify(Object.entries(answers).sort(([a], [b]) => (a < b ? -1 : 1)));
/** The same answers, in whatever order. */
export const sameAnswers = (a: PmiAnswers, b: PmiAnswers) => sorted(a) === sorted(b);

/** ``base`` with the change from ``before`` to ``after`` applied: what the page changed, put onto
 * answers someone else has changed meanwhile. */
export function reapply(base: PmiAnswers, before: PmiAnswers, after: PmiAnswers): PmiAnswers {
  const next: Record<string, unknown> = { ...base };
  for (const id of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (same(before[id], after[id])) continue;
    if (id in after) next[id] = after[id]; else delete next[id];
  }
  return next as PmiAnswers;
}

/** What is compared for unsaved changes: the answers given, without Specify's own `_` records. */
const asGiven = (answers: PmiAnswers | undefined) => JSON.stringify(Object.fromEntries(Object.entries(answers ?? {}).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1))));
const NONE = asGiven({});

/** How many revisions' answers are remembered, to know what a save of a past revision wrote. */
const HISTORY = 500;

export function createSession(rev: number, parts: Record<string, PmiAnswers>, put = putAnswers): AnswersSession {
  const requests = new AbortController();
  let state = { rev, parts: { ...parts } };
  // Each revision's answers (as given) by part: what a save of it put on disk.
  const history = new Map<number, Record<string, string>>();
  const remember = () => {
    history.set(state.rev, Object.fromEntries(Object.entries(state.parts).map(([k, a]) => [k, asGiven(a)])));
    for (const old of history.keys()) {
      if (history.size <= HISTORY) break;
      history.delete(old);
    }
  };
  remember();
  // On disk, by part: the file as opened (the session's start, or the part's stored answers when
  // the page opens it) until a save, then that save's answers.
  let onDisk: Record<string, string> = history.get(rev)!;
  let savedRev: number | null = null;
  // A save of a revision not heard of yet: taken as on disk when the session reaches it.
  let awaited: number | null = null;
  const stores = new Set<{ store: AnswersStore; update(source: AnswersSource): void }>();
  const watchers = new Set<() => void>();
  const changed = () => { for (const watcher of watchers) watcher(); };
  let lastSource: AnswersSource = 'start';
  // The (rev, part) of the part events received.
  const heard = new Set<string>();

  /** The session at ``at`` holds ``changes``: every part's answers (``all``), or the parts changed. */
  const known = (at: number, changes: Record<string, PmiAnswers>, all: boolean, source: AnswersSource) => {
    if (requests.signal.aborted || at < state.rev) return;
    state = { rev: at, parts: all ? { ...changes } : { ...state.parts, ...changes } };
    remember();
    if (awaited !== null && at >= awaited) { onDisk = history.get(at)!; awaited = null; }
    for (const s of stores) s.update(source);
    changed();
  };

  const session: AnswersSession = {
    signal: requests.signal,
    rev: () => state.rev,
    answers: (part) => state.parts[String(part)] ?? {},
    parts: () => ({ ...state.parts }),
    receive(event) {
      if ('parts' in event) {
        known(event.rev, event.parts, true, event.source);
        return true;
      }
      // Several events share a rev (one per part), so a repeat is told apart by (rev, part).
      const key = `${event.rev}:${event.part}`;
      if (event.rev < state.rev || heard.has(key)) return false;
      heard.add(key);
      for (const old of heard) { if (heard.size <= HISTORY) break; heard.delete(old); }
      lastSource = event.source;
      known(event.rev, { [String(event.part)]: event.answers }, false, event.source);
      return true;
    },
    store(part, fileAnswers) {
      const key = String(part);
      if (fileAnswers) session.fileAnswers(part, fileAnswers);
      // The page's changes not in the session yet, in order: each from ``before`` to ``after``.
      const pending: { before: PmiAnswers; after: PmiAnswers }[] = [];
      const shown = () => pending.reduce((answers, c) => reapply(answers, c.before, c.after), session.answers(part));
      let current = shown();
      const listeners = new Set<(answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void>();
      const problems = new Set<(error: Error) => void>();
      let chain: Promise<void> = Promise.resolve();
      const update = (source: AnswersSource) => {
        const previous = current;
        current = shown();
        if (sameAnswers(previous, current)) return;
        for (const listener of listeners) listener(current, source, previous);
      };

      /** Sends the oldest pending change, onto the answers the session holds now (others' kept). */
      const send = async () => {
        if (requests.signal.aborted) return;
        const change = pending[0]!;
        for (let attempt = 0; attempt < 8; attempt++) {
          const at = state.rev;
          const base = session.answers(part);
          const wanted = reapply(base, change.before, change.after);
          if (sameAnswers(wanted, base)) return; // The session has it already.
          try {
            const { rev: next } = await put(at, part, wanted, requests.signal);
            // At ``next`` the session holds ``wanted`` for this part, and the others as at ``at``.
            known(next, { [key]: wanted }, false, 'page');
            return;
          } catch (error) {
            if (requests.signal.aborted) return;
            if (!(error instanceof Refused) || error.status !== 409 || typeof error.body.rev !== 'number' || !error.body.parts) throw error;
            // Someone else changed the answers: they are shown, and the change is reapplied over them.
            known(error.body.rev, error.body.parts as Record<string, PmiAnswers>, true, lastSource);
          }
        }
        throw new Error('The answers kept changing while this page sent its own. Reload the page.');
      };

      const store: AnswersStore = {
        part,
        get: () => current,
        set(answers, base) {
          if (requests.signal.aborted || sameAnswers(answers, base)) return;
          pending.push({ before: base, after: answers });
          update('page');
          changed();
          chain = chain.then(send).catch((error: unknown) => {
            for (const listener of problems) listener(error instanceof Error ? error : new Error(String(error)));
          }).finally(() => { pending.shift(); update('start'); changed(); });
        },
        subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
        async settled() { await chain; return state.rev; },
        onProblem(listener) { problems.add(listener); },
        close() { stores.delete(entry); listeners.clear(); },
      };
      const entry = { store, update };
      stores.add(entry);
      return store;
    },
    fileAnswers(part, answers) {
      if (savedRev !== null) return;
      onDisk = { ...onDisk, [String(part)]: asGiven(answers) };
      queueMicrotask(changed);
    },
    markSaved(at) {
      savedRev = at;
      awaited = null;
      const then = history.get(at);
      if (then) onDisk = then;
      // Older than the history: the latest known is the best guess.
      else if (at < state.rev) onDisk = history.get(state.rev)!;
      else awaited = at;
      changed();
    },
    dirty() {
      const now: Record<string, string> = Object.fromEntries(Object.entries(state.parts).map(([k, a]) => [k, asGiven(a)]));
      for (const { store } of stores) now[String(store.part)] = asGiven(store.get());
      return [...new Set([...Object.keys(now), ...Object.keys(onDisk)])].some((k) => (now[k] ?? NONE) !== (onDisk[k] ?? NONE));
    },
    subscribe(listener) { watchers.add(listener); return () => { watchers.delete(listener); }; },
    close() { requests.abort(); },
  };
  return session;
}

/** A session holding part ``part`` only, and that part's store, with the session's calls on it: for tests. */
export function createSessionStore(part: number, rev: number, initial: PmiAnswers, onDisk: PmiAnswers = initial, put = putAnswers) {
  const session = createSession(rev, { [String(part)]: initial }, put);
  const store = session.store(part, onDisk);
  return { ...store, get: store.get, session, receive: session.receive, markSaved: session.markSaved, dirty: session.dirty };
}

/** The questions whose answers differ between ``a`` and ``b``. */
export function changedIds(a: PmiAnswers, b: PmiAnswers): string[] {
  return [...new Set([...Object.keys(a), ...Object.keys(b)])]
    .filter((id) => !id.startsWith('_') && JSON.stringify(a[id]) !== JSON.stringify(b[id]));
}
