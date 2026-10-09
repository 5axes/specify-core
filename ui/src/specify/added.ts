// Requirements a person adds where no question asked: which ones the faces
// picked can take, and how each is named in a line. Pure, no DOM; pmi-assist
// checks them against the part when they are given.
import type { PmiAnalysis, PmiAnswers, PmiExtra } from '../types/pmi';

import { CHARACTERISTIC } from './labels';

export const PREFIX = 'extra.';
export type ExtraKind = PmiExtra['kind'];

/** Kinds measured from a datum, necessarily; a profile may be, or not. */
export const NEEDS_DATUM: readonly ExtraKind[] = ['perpendicularity', 'parallelism', 'runout', 'total_runout'];
export const MAY_HAVE_DATUM: readonly ExtraKind[] = [...NEEDS_DATUM, 'profile'];

/** The added requirements among ``answers``, in the order given. */
export function addedOf(answers: PmiAnswers): [string, PmiExtra][] {
  return Object.entries(answers).filter(
    (entry): entry is [string, PmiExtra] =>
      entry[0].startsWith(PREFIX) && Boolean(entry[1]) && typeof entry[1] === 'object',
  );
}

/** A key for the next one: one past the highest given. */
export function nextKey(answers: PmiAnswers): string {
  const n = Math.max(0, ...addedOf(answers).map(([k]) => Number(k.slice(PREFIX.length)) || 0));
  return `${PREFIX}${n + 1}`;
}

/** Called out of the general tolerance: a dimension with a tolerance of its own. */
export const CALL_OUTS: readonly ExtraKind[] = ['size', 'distance'];

/**
 * What ``faces`` can take: flat faces a flatness, an orientation or a distance,
 * a hole or boss a size, fit or distance too.
 */
export function kindsFor(faces: readonly number[], analysis: PmiAnalysis): ExtraKind[] {
  const picked = faces.map((id) => analysis.faces.find((f) => f.id === id));
  if (!picked.length || picked.some((f) => !f)) return [];
  if (picked.every((f) => f?.kind === 'plane')) {
    return ['distance', 'flatness', 'perpendicularity', 'parallelism', 'profile', 'runout', 'finish'];
  }
  const radii = new Set(picked.map((f) => (f?.kind === 'cylinder' ? Number(f.radius?.toFixed(6)) : NaN)));
  if (radii.size === 1 && !radii.has(NaN)) {
    return ['size', 'distance', 'fit', 'perpendicularity', 'parallelism', 'runout', 'total_runout', 'profile', 'finish'];
  }
  // Any faces can be given a profile or a finish.
  return ['profile', 'finish'];
}

export const KIND_TEXT: Record<ExtraKind, string> = {
  flatness: 'Flatness', perpendicularity: 'Perpendicularity', parallelism: 'Parallelism', fit: 'Fit',
  finish: 'Surface finish', profile: 'Profile', runout: 'Runout', total_runout: 'Total runout',
  size: 'Size', distance: 'Distance',
};

/** One line for an added requirement: ``⏥ 0.05``, ``⟂ 0.02 | A``, ``H7 fit``, ``↔ ±0.05``. */
export function describe(extra: PmiExtra): string {
  if (extra.kind === 'fit') return `${extra.value} fit`;
  if (extra.kind === 'size') return `⌀ ${extra.value}`;
  if (extra.kind === 'distance') return `↔ ${extra.value}`;
  if (extra.kind === 'finish') return `√ ${/^ra/i.test(extra.value) ? extra.value : `Ra ${extra.value}`}`;
  const symbol = CHARACTERISTIC[extra.kind] ?? extra.kind;
  const datums = extra.datums?.length ? ` | ${extra.datums.join(' | ')}` : '';
  return `${symbol} ${extra.value}${datums}`;
}
