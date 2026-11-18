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
  subscribe(listener: (answers: PmiAnswers, source: AnswersSource) => void): () => void;
  /** Marks the current answers as on disk. */
  markSaved(): void;
  dirty(): boolean;
}

export function createAnswersStore(initial: PmiAnswers): AnswersStore {
  let current = initial;
  let saved = JSON.stringify(initial);
  const listeners = new Set<(answers: PmiAnswers, source: AnswersSource) => void>();
  return {
    get: () => current,
    set(answers, source) {
      current = answers;
      for (const listener of listeners) listener(answers, source);
    },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    markSaved() { saved = JSON.stringify(current); },
    dirty: () => JSON.stringify(current) !== saved,
  };
}
