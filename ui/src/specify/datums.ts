// The datums row: what each datum is, what is measured from it, and adding or
// removing one. Pure, no DOM. pmi-assist decides what follows (frames are
// rebuilt from whichever datums remain); this only says what the person asked.
import type { PmiAnalysis, PmiAnswers, PmiIntent } from '../types/pmi';

const LETTERS = 'ABCDEFGH';

export interface DatumChip {
  readonly letter: string;
  /** What it is on the part, in words: ``bottom face``, ``⌀10 hole``. */
  readonly text: string;
  /** The file's own: kept as it is, not removable here. */
  readonly existing: boolean;
  /** How many tolerances are measured from it. */
  readonly uses: number;
}

function usesOf(intent: PmiIntent, letter: string): number {
  return intent.requirements.filter((r) => Array.isArray(r.datums) && r.datums.includes(letter)).length;
}

/** The datums ``intent`` has, in letter order. */
export function datumChips(intent: PmiIntent | null, analysis: PmiAnalysis): DatumChip[] {
  const faces = new Map(analysis.faces.map((f) => [f.id, f]));
  return [...(intent?.datums ?? [])]
    .filter((d) => d.faces.length > 0)
    .sort((a, b) => a.letter.localeCompare(b.letter))
    .map((d) => {
      const text = describeFaces(d.faces, analysis, faces);
      return { letter: d.letter, text, existing: Boolean(d.existing), uses: usesOf(intent as PmiIntent, d.letter) };
    });
}

/** The letter a new datum takes: the lowest not in use, so removing C and adding again gives C. */
export function freeLetter(chips: readonly DatumChip[]): string | null {
  const used = new Set(chips.map((c) => c.letter));
  return [...LETTERS].find((x) => !used.has(x)) ?? null;
}

/**
 * Removing ``letter``: the answers that say so, and what to warn about first,
 * if anything. A is the primary: without it nothing can be measured, so the
 * frame goes with it.
 */
export function removal(
  letter: string, chips: readonly DatumChip[], answers: PmiAnswers, intent: PmiIntent | null,
): { answers: PmiAnswers; warning: string | null } {
  const gone = letter === 'A' ? chips.filter((c) => !c.existing).map((c) => c.letter) : [letter];
  const next: Record<string, unknown> = { ...answers };
  for (const x of gone) next[`datum.${x}`] = [];
  // Each tolerance once, however many of the removed datums it cites.
  const uses = (intent?.requirements ?? []).filter(
    (r) => Array.isArray(r.datums) && r.datums.some((x: unknown) => gone.includes(String(x))),
  ).length;
  const others = gone.filter((x) => x !== letter);
  let warning: string | null = null;
  if (others.length) {
    warning = `A is the primary datum: removing it removes ${others.join(' and ')} too`
      + (uses ? `, and the ${uses} tolerance${uses === 1 ? '' : 's'} measured from them.` : '.');
  } else if (uses) {
    warning = `${uses} tolerance${uses === 1 ? ' is' : 's are'} measured from ${letter}; `
      + `${uses === 1 ? 'it' : 'they'} will be measured without it.`;
  }
  return { answers: next, warning };
}

const SIDES: readonly (readonly [number, number, string])[] = [
  [2, 1, 'top'], [2, -1, 'bottom'], [1, 1, 'back'], [1, -1, 'front'], [0, 1, 'right'], [0, -1, 'left'],
];

/**
 * Faces as a person would name them: a flat face by the side it faces
 * (``bottom face``, ``2 front faces``), a cylinder by its size and what it is
 * (``⌀10 hole``, ``⌀10 shaft``).
 */
export function describeFaces(
  ids: readonly number[], analysis: PmiAnalysis,
  faces = new Map(analysis.faces.map((f) => [f.id, f])),
): string {
  const first = faces.get(ids[0] ?? -1);
  if (!first) return `${ids.length} faces`;
  if (first.kind === 'cylinder' && first.radius) {
    const d = Number((2 * first.radius).toFixed(3));
    const family = analysis.features.find((f) => f.faces.includes(first.id) && !f.members)?.family;
    const what = family === 'holes' ? 'hole' : family === 'turned_steps' ? 'shaft' : 'cylinder';
    return `⌀${d} ${what}`;
  }
  const dir = first.direction ?? [0, 0, 0];
  const axis = [0, 1, 2].reduce((a, b) => (Math.abs(dir[b] ?? 0) > Math.abs(dir[a] ?? 0) ? b : a), 0);
  const side = SIDES.find(([k, sign]) => k === axis && Math.sign(dir[axis] ?? 0) === sign)?.[2];
  const many = ids.length > 1 ? `${ids.length} ` : '';
  return side ? `${many}${side} face${ids.length > 1 ? 's' : ''}` : `${many}face${ids.length > 1 ? 's' : ''}`;
}
