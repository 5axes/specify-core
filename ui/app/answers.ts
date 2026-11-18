import type { PmiAnswers } from '../src/types/pmi';

/** Who changed the answers: the page (SpecifyReview's commits), or, from stage 2, the command's
 * shared session (its event stream, fed by an AI client over MCP). */
export type AnswersSource = 'page' | 'server';

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
