// A drawing by default, GD&T on request (docs/WORKFLOW.md): which part of the intent a question
// belongs to, and the answers that keep Specify to the default path until GD&T is asked for.
// Pure, no DOM.
import type { PmiAnswers, PmiQuestion } from '../types/pmi';

/** How the tolerancing is specified: not yet (the default path), ± only, or geometric (GD&T). */
export type GdtMode = 'plusminus' | 'geometric';

/** The groups of the intent, in the order stage 1 shows them. */
export type IntentPart = 'material' | 'threads' | 'notes' | 'scheme';
export const intentParts: Record<IntentPart, string> = {
  material: 'Material', threads: 'Threads and taps', notes: 'General notes', scheme: 'GD&T',
};

// The general notes: blanket statements for the whole part, in every drawing's default path.
const notes = new Set(['part.general_tolerance', 'part.surface_finish', 'part.coating', 'part.heat_treatment', 'part.edges']);
// A hole's tap and a diameter's thread or knurl are facts about the part; their other options
// (clearance, fits) are tolerancing.
const factOptions = ['tapped:', 'thread:', 'knurl:'];

const valueOf = (q: PmiQuestion, answers: PmiAnswers) => String(q.id in answers ? answers[q.id] : q.default ?? '');

/** The part of the intent ``q`` belongs to, given what it is answered (or proposed) as. */
export function intentPart(q: PmiQuestion, answers: PmiAnswers): IntentPart {
  if (q.id === 'part.material') return 'material';
  if (notes.has(q.id)) return 'notes';
  if ((q.id.startsWith('hole.function:') || q.id.startsWith('diameter.fit:'))
      && factOptions.some(option => valueOf(q, answers).startsWith(option))) return 'threads';
  return 'scheme';
}

/**
 * The answers the default path adds, so that Specify proposes no tolerancing scheme: holes
 * located by ± dimensions (no datums, no position), and plain holes except a proposed tap.
 * Only questions not yet answered are given one.
 */
export function defaultPathAnswers(questions: readonly PmiQuestion[], answers: PmiAnswers): Record<string, string> {
  const added: Record<string, string> = {};
  for (const q of questions) {
    if (q.id in answers) continue;
    if (q.id === 'part.locating') added[q.id] = '± dimensions';
    else if (q.id.startsWith('hole.function:') && !String(q.default ?? '').startsWith('tapped:')) added[q.id] = 'general';
  }
  return added;
}

// Answers that only GD&T gives: a project saved with any of them was already tolerancing.
const schemePrefixes = ['datum.', 'hole.position:', 'hole.frame:', 'hole.location:', 'diameter.runout:', 'extra.'];

/** The mode a project saved before the default path was already in, from its answers. */
export function inferredMode(answers: PmiAnswers): GdtMode | undefined {
  const ids = Object.keys(answers);
  if (ids.some(id => schemePrefixes.some(prefix => id.startsWith(prefix)))) return 'geometric';
  const holes = ids.filter(id => id.startsWith('hole.function:') || id.startsWith('diameter.fit:'));
  if (holes.some(id => !['general', ...factOptions].some(option => String(answers[id]).startsWith(option)))) {
    return answers['part.locating'] === '± dimensions' ? 'plusminus' : 'geometric';
  }
  return undefined;
}
