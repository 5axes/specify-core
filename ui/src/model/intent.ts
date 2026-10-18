// A drawing by default, GD&T on request (docs/WORKFLOW.md): which part of the intent a question
// belongs to, for showing it. The default path's answers and the mode changes are specify-core's
// (its tolerancing module), shared with the MCP server. Pure, no DOM.
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

/** A question's default as the editor shows it: ± locating where Specify's own is position
 * tolerances (as specify-core's tolerancing::editor_default). */
export function editorDefault(q: PmiQuestion): unknown {
  return q.id === 'part.locating' ? '± dimensions' : q.default;
}
