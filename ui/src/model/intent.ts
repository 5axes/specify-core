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
 * located by ± dimensions (no datums, no position), plain holes except a proposed tap, and plain
 * diameters except a proposed thread or knurl. Runout is none by default already. Only questions
 * not yet answered are given one.
 */
export function defaultPathAnswers(questions: readonly PmiQuestion[], answers: PmiAnswers): Record<string, string> {
  const added: Record<string, string> = {};
  for (const q of questions) {
    if (q.id in answers) continue;
    if (q.id === 'part.locating') added[q.id] = editorDefault(q) as string;
    else if ((q.id.startsWith('hole.function:') || q.id.startsWith('diameter.fit:'))
      && ![...factOptions, 'general'].some(option => String(q.default ?? '').startsWith(option))) added[q.id] = 'general';
  }
  return added;
}

/** A question's default as the editor applies it: ± locating where Specify's own is position tolerances. */
export function editorDefault(q: PmiQuestion): unknown {
  return q.id === 'part.locating' ? '± dimensions' : q.default;
}

/**
 * The mode of a project saved before the default path: Specify's defaults were then the geometric
 * scheme, so it keeps it, unless it was already located by ± dimensions.
 */
export function inferredMode(answers: PmiAnswers): GdtMode {
  return answers['part.locating'] === '± dimensions' ? 'plusminus' : 'geometric';
}

/** What the default path answered and the person has since left alone. */
export function stillAuto(auto: readonly string[] | undefined, before: PmiAnswers, after: PmiAnswers): string[] | undefined {
  return auto?.filter(id => JSON.stringify(after[id]) === JSON.stringify(before[id]));
}

/**
 * The answers once GD&T is asked for: the default path's are given back, but for ± locating.
 * Either way can be chosen after the other: ± locates by dimensions whatever was answered before,
 * and geometric lets Specify propose the locating again.
 */
export function gdtAnswers(answers: PmiAnswers, auto: readonly string[] | undefined, mode: GdtMode): PmiAnswers {
  const kept = { ...answers };
  for (const id of auto ?? []) if (!(mode === 'plusminus' && id === 'part.locating')) delete kept[id];
  if (mode === 'plusminus') kept['part.locating'] = '± dimensions';
  else if (kept['part.locating'] === '± dimensions') delete kept['part.locating'];
  return kept;
}
