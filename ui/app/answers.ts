import type { PmiAnswers } from '../src/types/pmi';

/** Who changed the answers, as the command's session will say in its `answers` events: the page
 * (SpecifyReview's commits), an AI client over MCP, or the session's first state. Today only the
 * page sets them; the next contract feeds the store from `GET events` and sends the page's own
 * changes with `PUT answers {rev, part, answers}` (a store over the session, same interface). */
export type AnswersSource = 'page' | 'mcp' | 'start';

/** The part's answers for this run: what Save writes and what the unsaved-changes guard compares. */
export interface AnswersStore {
  get(): PmiAnswers;
  set(answers: PmiAnswers, source: AnswersSource): void;
  /** ``previous``: the answers before this change, to show what someone else (an agent) changed. */
  subscribe(listener: (answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void): () => void;
  /** Marks the current answers as on disk. */
  markSaved(): void;
  dirty(): boolean;
}

export function createAnswersStore(initial: PmiAnswers): AnswersStore {
  let current = initial;
  let saved = JSON.stringify(initial);
  const listeners = new Set<(answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void>();
  return {
    get: () => current,
    set(answers, source) {
      const previous = current;
      current = answers;
      for (const listener of listeners) listener(answers, source, previous);
    },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    markSaved() { saved = JSON.stringify(current); },
    dirty: () => JSON.stringify(current) !== saved,
  };
}

/** The questions whose answers differ between ``a`` and ``b``. */
export function changedIds(a: PmiAnswers, b: PmiAnswers): string[] {
  return [...new Set([...Object.keys(a), ...Object.keys(b)])]
    .filter((id) => !id.startsWith('_') && JSON.stringify(a[id]) !== JSON.stringify(b[id]));
}
