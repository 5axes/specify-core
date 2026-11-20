// A drawing by default, GD&T on request (docs/WORKFLOW.md): which part of the intent a question
// belongs to, for showing it. The default path's answers and the mode changes are specify-core's
// (its tolerancing module), shared with the MCP server. Pure, no DOM.
import type { PmiAnswers, PmiQuestion } from '../types/pmi';

/** How the tolerancing is specified: not yet (the default path), ± only, or geometric (GD&T). */
export type GdtMode = 'plusminus' | 'geometric';

/** The scheme actually chosen, including older projects that store only the locating answer.
 * The untouched path's automatic ± answer is deliberately not a chosen scheme. */
export function tolerancingMode(edits: { gdt?: GdtMode; answers?: PmiAnswers; auto?: readonly string[] }): GdtMode | undefined {
  if (edits.gdt) return edits.gdt;
  if (edits.auto?.includes('part.locating')) return;
  const locating = edits.answers?.['part.locating'];
  return locating === '± dimensions' ? 'plusminus' : locating === 'position tolerances' ? 'geometric' : undefined;
}

/** The groups of the intent, in the order stage 1 shows them. */
export type IntentPart = 'material' | 'threads' | 'notes' | 'bom' | 'faces' | 'scheme';
export const intentParts: Record<IntentPart, string> = {
  material: 'Material', threads: 'Threads and taps', notes: 'General notes', bom: 'Parts list', faces: 'Not recognised', scheme: 'GD&T',
};

// The general notes: blanket statements for the whole part, in every drawing's default path.
const notes = new Set(['part.general_tolerance', 'part.surface_finish', 'part.coating', 'part.heat_treatment', 'part.edges']);
// A hole's tap and a diameter's thread or knurl are facts about the part; their other options
// (clearance, fits) are tolerancing.
const factOptions = ['tapped:', 'thread:', 'knurl:'];

const valueOf = (q: PmiQuestion, answers: PmiAnswers) => String(q.id in answers ? answers[q.id] : q.default ?? '');

/** The part of the intent ``q`` belongs to, given what it is answered (or proposed) as. */
export function intentPart(q: PmiQuestion, answers: PmiAnswers): IntentPart {
  // How the part is made (Specify 0.1.12) sets its defaults with the material, so it sits beside it.
  if (q.id === 'part.material' || q.id === 'part.process') return 'material';
  if (notes.has(q.id)) return 'notes';
  // A part of an assembly: its row of the parts list (Specify asks these only of assembly parts).
  if (q.id.startsWith('bom.')) return 'bom';
  if ((q.id.startsWith('hole.function:') || q.id.startsWith('diameter.fit:'))
      && factOptions.some(option => valueOf(q, answers).startsWith(option))) return 'threads';
  return 'scheme';
}

/** A question's default as the editor shows it: ± locating where Specify's own is position
 * tolerances (as specify-core's tolerancing::editor_default). */
export function editorDefault(q: PmiQuestion): unknown {
  return q.id === 'part.locating' ? '± dimensions' : q.default;
}
