// What a click means, in pmi-assist face indices. Pure, no three.js or DOM.
// Ported from pmi-assist-viewer (picker/selection.js).
//
// The picker only proposes. pmi-assist decides whether faces make a valid
// answer (a datum must be coplanar faces or one hole) and says why not; these
// modes just save clicking every half of a split bore by hand.
import type { PmiAnalysis, PmiBinding, PmiFace, PmiFeature, PmiVec } from '../types/pmi';

export const MODES = ['face', 'feature', 'coplanar'] as const;
export type PickMode = (typeof MODES)[number];

export interface AnalysisIndex {
  readonly faces: ReadonlyMap<number, PmiFace>;
  readonly featuresOf: ReadonlyMap<number, readonly PmiFeature[]>;
}

/** The face owning ``triangle``, given each face's [start, count] triangle range. */
export function faceOfTriangle(
  ranges: readonly (readonly [number, number])[],
  triangle: number,
): number | null {
  let lo = 0;
  let hi = ranges.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const range = ranges[mid];
    if (!range) return null;
    const [start, count] = range;
    if (triangle < start) hi = mid - 1;
    else if (triangle >= start + count) lo = mid + 1;
    else return mid;
  }
  return null;
}

/** Faces by id, and the features each face belongs to, smallest first. */
export function indexAnalysis(analysis: PmiAnalysis): AnalysisIndex {
  const faces = new Map(analysis.faces.map((f) => [f.id, f]));
  const featuresOf = new Map<number, PmiFeature[]>();
  for (const feature of analysis.features) {
    for (const id of feature.faces) {
      const list = featuresOf.get(id) ?? [];
      list.push(feature);
      featuresOf.set(id, list);
    }
  }
  for (const list of featuresOf.values()) list.sort((a, b) => a.faces.length - b.faces.length);
  return { faces, featuresOf };
}

/** The faces a click on ``face`` selects in ``mode``. */
export function expand(index: AnalysisIndex, face: number, mode: PickMode): number[] {
  if (mode === 'feature') return featureFaces(index, face);
  if (mode === 'coplanar') return coplanarFaces(index, face);
  return [face];
}

// The smallest recognised feature on the face. A hole gives its bore -- the
// cylinders at its diameter, not its chamfer or drill point -- because that is
// what a datum or a size refers to.
function featureFaces(index: AnalysisIndex, face: number): number[] {
  const feature = (index.featuresOf.get(face) ?? []).find((f) => f.family !== 'hole_patterns');
  if (!feature) return [face];
  if (feature.family !== 'holes') return [...feature.faces].sort((a, b) => a - b);
  const diameter = feature.record['diameter'];
  const radius = typeof diameter === 'number' ? diameter / 2 : NaN;
  const bores = feature.faces.filter((id) => {
    const f = index.faces.get(id);
    return f?.kind === 'cylinder' && f.radius !== undefined && Math.abs(f.radius - radius) < 1e-6;
  });
  return (bores.length ? bores : [...feature.faces]).sort((a, b) => a - b);
}

// Every plane facing the same way in the same plane: a datum made of two
// flanges is clicked once. The tolerances match pmi-assist's datum check.
function coplanarFaces(index: AnalysisIndex, face: number): number[] {
  const f = index.faces.get(face);
  if (!f || f.kind !== 'plane' || !f.direction) return [face];
  const out: number[] = [];
  for (const g of index.faces.values()) {
    if (g.kind !== 'plane' || !g.direction) continue;
    const facing = dot(g.direction, f.direction) > 1 - 1e-6;
    const offset = dot(sub(g.centroid, f.centroid), f.direction);
    if (facing && Math.abs(offset) < 1e-4) out.push(g.id);
  }
  return out.sort((a, b) => a - b);
}

/**
 * The selection after clicking ``faces``. A click on something wholly
 * selected removes it; anything else adds it, or with ``replace`` stands alone.
 */
export function toggle(
  selection: ReadonlySet<number>,
  faces: readonly number[],
  replace = false,
): Set<number> {
  if (faces.every((id) => selection.has(id))) {
    const next = new Set(selection);
    for (const id of faces) next.delete(id);
    return next;
  }
  return new Set(replace ? faces : [...selection, ...faces]);
}

/** Whether a mesh was made from the file an analysis describes, numbered the same way. */
export function sameBinding(a: PmiBinding, b: PmiBinding): boolean {
  return a.source_sha256 === b.source_sha256 && a.face_count === b.face_count
    && a.loader_version === b.loader_version;
}

const dot = (a: PmiVec, b: PmiVec): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const sub = (a: PmiVec, b: PmiVec): PmiVec => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
