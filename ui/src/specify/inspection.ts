import type { PmiAnswers, PmiQuestion } from '../types/pmi';

/** Answers given, plus defaults explicitly kept by the host. Suggestions awaiting review
 * remain unapproved even when the host holds them in its answer map. */
export function inspectionAnswers(answers: PmiAnswers, questions: readonly PmiQuestion[], kept: readonly string[] = [], toReview: readonly string[] = []): PmiAnswers {
  const approved = new Set(kept);
  const pending = new Set(toReview);
  const given = Object.fromEntries(Object.entries(answers).filter(([id]) => !pending.has(id) || approved.has(id)));
  for (const question of questions) {
    if (approved.has(question.id) && !(question.id in given) && question.default != null) given[question.id] = question.default;
  }
  return given;
}
