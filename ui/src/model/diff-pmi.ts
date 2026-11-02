/**
 * The PMI of two revisions as /diff compares them: Specify's diff report's PMI section
 * (`report.pmi`, specify-core 0.1.7), with the two scenes it compared (`pmi.scenes`), joined
 * by item id. Nothing here words or compares PMI, or chooses a scene: the changes and scenes
 * are the engine's, the callouts haecceity's. This is which colour a drawn item takes, which
 * old callout ghosts a changed one, and where the old part's callouts go.
 */
import type { DrawnGraphic, DrawnPlane, DrawnPmi, DrawnPoint, DrawnStatus, TextPlacement } from '../specify/drawn';

/** One item's change, as Specify's diff reports it (`PmiItemChange`). */
export interface PmiChange {
  class: 'added' | 'removed' | 'changed' | 'renumbered';
  /** The item's id in the new revision's scene; a removed item's, in the old one's. */
  id: string;
  /** A changed or renumbered item's id in the old revision's scene, when not ``id``. */
  old_id?: string;
  /** The mesh faces it is on, in each revision (none for a side it is not in, or the part's own). */
  faces: { old: { face: number }[]; new: { face: number }[] };
  kind: string;
  /** Its characteristic number, when it has one. */
  number?: number;
  /** What it is on, as the engine names it. */
  on: string;
  /** Its notation before and after, as the engine words it. */
  old?: string;
  new?: string;
  reason?: string;
  status: DrawnStatus;
}

/** The diff report's PMI section, the parts of it the page reads. */
export interface DiffPmi {
  answers_from: 'given' | 'stored' | 'none';
  new_stored: boolean;
  items: PmiChange[];
  /** The two scenes the diff compared (`pmiScene`'s shape), each in its part's own frame, when asked for. */
  scenes?: { old: unknown; new: unknown };
}

export type DrawnChange = 'changed' | 'added' | 'removed' | 'unchanged';

/**
 * What became of each item: those the new scene draws by their id, the old scene's removed
 * ones, and each changed (or renumbered) item's id in the old scene, whose callout is what it was.
 */
export function changeOf(items: readonly PmiChange[]): { now: Map<string, DrawnChange>; removed: Set<string>; was: Map<string, string> } {
  const now = new Map<string, DrawnChange>();
  const removed = new Set<string>();
  const was = new Map<string, string>();
  for (const c of items) {
    if (c.class === 'removed') removed.add(c.id);
    // Renumbered is a change; added wins over neither, as one item is one or the other.
    else if (c.class === 'added') now.set(c.id, 'added');
    else {
      if (!now.has(c.id)) now.set(c.id, 'changed');
      if (!was.has(c.id)) was.set(c.id, c.old_id ?? c.id);
    }
  }
  return { now, removed, was };
}

/** An item's faces on each revision's mesh, as numbers. */
export const facesOf = (c: PmiChange): { old: number[]; new: number[] } => ({
  old: c.faces.old.map(f => f.face), new: c.faces.new.map(f => f.face),
});

/** Whether a scene draws an item: a graphic of its own, not only a share of another's. */
export const drawsItem = (pmi: DrawnPmi | null, id: string): boolean => (pmi?.presentation.items[id]?.length ?? 0) > 0;

/** A rigid motion: the old part to where the new part is (the diff view's `motion`). */
export interface Motion { rotation: number[][]; translation: number[] }

/** The old revision's scene moved as its faces are, onto the new part. */
export function movedPmi(pmi: DrawnPmi, motion: Motion | null): DrawnPmi {
  if (!motion) return pmi;
  const turn = (v: DrawnPoint): DrawnPoint => [0, 1, 2].map(i =>
    motion.rotation[i]![0]! * v[0] + motion.rotation[i]![1]! * v[1] + motion.rotation[i]![2]! * v[2]) as unknown as DrawnPoint;
  const put = (p: DrawnPoint): DrawnPoint => { const t = turn(p); return [t[0] + motion.translation[0]!, t[1] + motion.translation[1]!, t[2] + motion.translation[2]!]; };
  const text = (t: TextPlacement): TextPlacement => ({ ...t, origin: put(t.origin), xAxis: turn(t.xAxis), normal: turn(t.normal) });
  const graphic = (g: DrawnGraphic): DrawnGraphic => ({
    ...g,
    polylines: g.polylines.map(line => line.map(put)),
    triangles: g.triangles.map(([a, b, c]) => [put(a), put(b), put(c)] as const),
    ...(g.textPlacements ? { textPlacements: g.textPlacements.map(text) } : {}),
  });
  const p = pmi.presentation;
  return {
    links: pmi.links,
    presentation: {
      ...p,
      items: Object.fromEntries(Object.entries(p.items).map(([id, gs]) => [id, gs.map(graphic)])),
      unlinked: p.unlinked.map(graphic),
      planes: p.planes.map(plane => ({ ...plane, origin: put(plane.origin), xAxis: turn(plane.xAxis), normal: turn(plane.normal) })),
      views: p.views.map(v => ({ ...v, ...(v.direction ? { direction: turn(v.direction) } : {}), ...(v.up ? { up: turn(v.up) } : {}) })),
    },
  };
}

/**
 * A changed item's old callout set beside its new one: moved up its own plane (its y axis, the
 * plane's normal × x axis) clear of the new one when they would overlap, as haecceity puts both
 * in one place for one requirement. Where they do not overlap, or the plane is not known, as drawn.
 */
export function besideNew(old: readonly DrawnGraphic[], now: readonly DrawnGraphic[], planes: readonly DrawnPlane[]): DrawnGraphic[] {
  const first = old[0];
  const plane = first?.planeId !== undefined ? planes.find(p => p.id === first.planeId) : undefined;
  const frame = plane ?? first?.textPlacements?.[0];
  if (!first || !frame || !now.length) return [...old];
  const unit = (v: readonly number[]) => { const n = Math.hypot(v[0]!, v[1]!, v[2]!) || 1; return [v[0]! / n, v[1]! / n, v[2]! / n]; };
  const x = unit(frame.xAxis);
  const n = unit(frame.normal);
  const up = unit([n[1]! * x[2]! - n[2]! * x[1]!, n[2]! * x[0]! - n[0]! * x[2]!, n[0]! * x[1]! - n[1]! * x[0]!]);
  const dot = (a: readonly number[], b: readonly number[]) => a[0]! * b[0]! + a[1]! * b[1]! + a[2]! * b[2]!;
  const extent = (gs: readonly DrawnGraphic[], axis: number[]) => {
    const along = gs.flatMap(g => [...g.polylines.flat(), ...g.triangles.flat()]).map(p => dot(p, axis));
    return along.length ? [Math.min(...along), Math.max(...along)] as const : null;
  };
  const [ox, nx, oy, ny] = [extent(old, x), extent(now, x), extent(old, up), extent(now, up)];
  if (!ox || !nx || !oy || !ny) return [...old];
  const overlap = (a: readonly [number, number], b: readonly [number, number]) => a[0] < b[1] && b[0] < a[1];
  if (!overlap(ox, nx) || !overlap(oy, ny)) return [...old];
  const by = ny[1] - oy[0] + 0.25 * (oy[1] - oy[0]);
  const put = (p: DrawnPoint): DrawnPoint => [p[0] + up[0]! * by, p[1] + up[1]! * by, p[2] + up[2]! * by];
  return old.map(g => ({
    ...g,
    polylines: g.polylines.map(line => line.map(put)),
    triangles: g.triangles.map(([a, b, c]) => [put(a), put(b), put(c)] as const),
    ...(g.textPlacements ? { textPlacements: g.textPlacements.map(t => ({ ...t, origin: put(t.origin) })) } : {}),
  }));
}
