import type { PmiAnswers, PmiQuestion } from '../types/pmi';
import type { Characteristic } from './characteristics';

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

// Sources' array indexes change when defaults are removed. Match the requirement itself,
// including its limits and datum references, without comparing its balloon number.
const identity = (row: Characteristic) => JSON.stringify([
  row.source.split(':')[0], row.question, row.feature, row.group, row.text, row.nominal,
  row.upper, row.lower, row.datums, row.basic, [...row.faces].sort((a, b) => a - b),
]);

/** Keep only stated rows, preserving the displayed rows' balloon and instance IDs. */
export function inspectionCharacteristics(displayed: readonly Characteristic[], stated: readonly Characteristic[]): Characteristic[] {
  const allowed = new Set(stated.map(identity));
  return displayed.filter((row) => allowed.has(identity(row)));
}
