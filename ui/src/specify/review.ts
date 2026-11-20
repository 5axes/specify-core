// Review mode: pmi-assist's questions as one row per thing on the part, most
// in need of a person first. Pure, no DOM. Ported from pmi-assist-viewer
// (web/review.js).
//
// pmi-assist marks each question's attention: required (no default), check (a
// default it doubts, with a reason), routine (a default from a standard or the
// geometry) or refine (an optional tightening). Everything with a default is
// accepted unless changed; only required and unconfirmed checks need a person.
import type { PmiAnswers, PmiQuestion } from '../types/pmi';

import { codeOf } from './choose.ts';

export interface QuestionGroup {
  readonly key: string;
  readonly title: string;
  readonly questions: PmiQuestion[];
}

export type RowState = 'required' | 'check' | 'set' | 'ok';

/** Rows: the part, its parts-list row (an assembly part's), one per feature in question order,
 * then the datums they are measured from. */
export function groupQuestions(questions: readonly PmiQuestion[]): QuestionGroup[] {
  const groups = new Map<string, QuestionGroup>();
  const add = (key: string, title: string, q: PmiQuestion) => {
    const group = groups.get(key) ?? { key, title, questions: [] };
    group.questions.push(q);
    groups.set(key, group);
  };
  for (const q of questions) {
    if (q.id.startsWith('part.')) add('part', 'Part', q);
    else if (q.id.startsWith('bom.')) add('bom', 'Parts list', q);
    else if (q.id.startsWith('datum.')) add('datums', 'Datums', q);
    else add(q.feature ?? q.id, titleOf(q), q);
  }
  // How the part is made comes before its material: it narrows the materials and sets the
  // defaults (tolerances, holes) the later questions start from.
  groups.get('part')?.questions.sort((a, b) => Number(b.id === 'part.process') - Number(a.id === 'part.process'));
  const order = (key: string) => (key === 'part' ? 0 : key === 'bom' ? 1 : key === 'datums' ? 3 : 2);
  // Each datum is followed by its own control: A, A's flatness, B, B's perpendicularity...
  const datums = groups.get('datums');
  if (datums) datums.questions.sort((a, b) => datumOrder(a.id).localeCompare(datumOrder(b.id)));
  return [...groups.values()].sort((a, b) => order(a.key) - order(b.key));
}

/** ``datum.B`` -> ``B0``, ``datum.B.control`` -> ``B1``: a datum, then what controls it. */
function datumOrder(id: string): string {
  const [, letter = '', rest] = id.split('.');
  return `${letter}${rest ? 1 : 0}`;
}

function titleOf(q: PmiQuestion): string {
  return q.title ?? q.prompt;
}

/** A question's value: its answer, or its default. */
export const answerOf = (q: PmiQuestion, answers: PmiAnswers): unknown =>
  (q.id in answers ? answers[q.id] : q.default);

/** Whether ``q`` still needs a person: unanswered and required, or an unconfirmed check. An
 * optional question is written with Specify's suggestion, so it asks for a check, not an answer. */
export function needs(q: PmiQuestion, answers: PmiAnswers): 'required' | 'check' | null {
  if (q.id in answers) return null;
  if (q.attention === 'required') return q.optional ? 'check' : 'required';
  return q.attention === 'check' ? 'check' : null;
}

/** The row's state, most urgent first. ``open``: the engine's open questions; the Datums row is
 * not answered while one of its own is open (datum C, specify-ui#14). */
export function rowState(group: QuestionGroup, answers: PmiAnswers, open: readonly string[] = []): RowState {
  const states = group.questions.map((q) => needs(q, answers));
  if (states.includes('required')) return 'required';
  if (states.includes('check')) return 'check';
  if (group.key === 'datums' && group.questions.some((q) => open.includes(q.id))) return 'ok';
  return group.questions.some((q) => q.id in answers) ? 'set' : 'ok';
}

/** Whether ``value`` states nothing: no faces, no notes, or a "None …" choice. */
export const isNone = (value: unknown): boolean => (Array.isArray(value) ? value.length === 0
  : typeof value === 'string' && /^none\b/i.test(value.trim()));

/** How an unanswered question whose suggestion states nothing reads (#14): a row like any other. */
export const NONE_SUGGESTED = 'None (suggested)';

/** Whether ``q`` is unanswered with a suggestion that states nothing (coating, other notes, datum C). */
export const noneSuggested = (q: PmiQuestion, answers: PmiAnswers): boolean => !(q.id in answers) && isNone(q.default);

/** A value as a row summary shows it, or '' when it says nothing (general, none). */
export function display(q: PmiQuestion, value: unknown): string {
  if (value === null || value === undefined || value === '') return '';
  if (q.kind === 'faces') {
    if (!Array.isArray(value) || !value.length) return '';
    const letter = /^datum\.([A-H])$/.exec(q.id)?.[1];
    return letter ? `${letter}: ${value.join(', ')}` : `faces ${value.join(', ')}`;
  }
  const text = String(value);
  if (text === 'general' || text === 'none') return '';
  if (q.id.startsWith('hole.position:')) return `⌖${text}${q.frame ? ` ${q.frame}` : ''}`;
  if (q.id.startsWith('hole.frame:')) return '';
  if (q.id.startsWith('hole.location:')) return `located ${text}`;
  if (q.id.startsWith('diameter.runout:')) return `↗${text}${q.frame ? ` ${q.frame}` : ''}`;
  if (q.id.endsWith('.control') || q.id.endsWith('.flatness')) return `${q.id.split('.')[1]} ${text}`;
  return codeOf(q, text);
}

/** One line saying what a row amounts to. */
export function summary(group: QuestionGroup, answers: PmiAnswers): string {
  // A datums row with nothing set is only refining the datums the file has.
  return group.questions.map((q) => display(q, answerOf(q, answers))).filter(Boolean).join(' · ')
    || (group.key === 'datums' ? "the file's datums" : group.key === 'bom' ? "Specify's suggestions" : 'general tolerance');
}

/**
 * The question to open when ``face`` is clicked: a feature on that face before
 * a datum, and within a row the first still needing a person, else the first.
 */
export function questionForFace(groups: readonly QuestionGroup[], face: number): string | null {
  const about = (g: QuestionGroup) => g.questions.some((q) => q.faces.includes(face));
  const group = groups.find((g) => g.key !== 'datums' && g.key !== 'part' && about(g))
    ?? groups.find((g) => g.key === 'datums' && about(g));
  return group?.questions[0]?.id ?? null;
}

/** The first question, in row order, still needing a person. */
export function nextNeeding(groups: readonly QuestionGroup[], answers: PmiAnswers): string | null {
  for (const g of groups) for (const q of g.questions) if (needs(q, answers)) return q.id;
  return null;
}

/** ``answers`` with every check in ``group`` confirmed at its default. */
export function confirmRow(group: QuestionGroup, answers: PmiAnswers): PmiAnswers {
  const next: Record<string, unknown> = { ...answers };
  for (const q of group.questions) if (needs(q, answers) === 'check') next[q.id] = q.default;
  return next;
}

/** What a save "with defaults" writes (docs: ADR P9, written only when approved): each question not
 * answered that has a default, as its prompt and the value written, for the person to confirm. */
export function defaultsToWrite(questions: readonly PmiQuestion[], answers: PmiAnswers): { id: string; prompt: string; value: string }[] {
  return questions
    .filter((q) => !(q.id in answers) && q.default !== undefined && q.default !== null)
    .map((q) => ({ id: q.id, prompt: q.prompt, value: display(q, q.default) || (typeof q.default === 'string' ? q.default : JSON.stringify(q.default)) }));
}
