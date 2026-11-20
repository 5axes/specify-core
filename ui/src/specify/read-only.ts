import type { PmiLabEngine } from '../types/pmi';

type Engine = Pick<PmiLabEngine, 'questions' | 'interpret' | 'preview'>;

/**
 * The engine for a read-only review: `questions` and `interpret` (the calls that change or read back
 * answers) reject with `{ code: 'read_only' }` and never reach `engine`; `preview` (a read) passes through.
 */
export function readOnlyEngine(engine: Engine): Engine {
  const refuse = <T>(): Promise<T> => Promise.reject({ code: 'read_only', message: 'Read only: this review cannot be changed.' });
  return {
    questions: () => refuse(),
    interpret: () => refuse(),
    preview: (sessionId, answers) => engine.preview(sessionId, answers),
  };
}
