/**
 * The PMI of two revisions as /diff compares them: Specify's diff report's PMI section
 * (`report.pmi`, specify-core 0.1.6) and each revision's PMI scene, joined by item id. Nothing
 * here words or compares PMI: the changes are the engine's, the callouts haecceity's. This is
 * which scene to ask for, which colour a drawn item takes, and where the old part's callouts go.
 */
import type { DrawnGraphic, DrawnPmi, DrawnPoint, DrawnStatus, TextPlacement } from '../specify/drawn';

/** One item's change, as Specify's diff reports it (`PmiItemChange`). */
export interface PmiChange {
  class: 'added' | 'removed' | 'changed' | 'renumbered';
  /** The item's id in the new revision's scene; a removed item's, in the old one's. */
  id: string;
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
}

type Answers = Record<string, unknown>;

/**
 * The answers each revision's scene is asked with, so it holds the items the diff compared
 * (specify-core's revision::pmi::diff): the old part with the answers given, if they were the
 * ones used, else as its file has it; the new part as its file has it when Specify wrote PMI in
 * it, else with the answers carried to it (`report.answers`), if any. Null: the file's own scene.
 */
export function sceneAnswers(pmi: DiffPmi, given: Answers, carried: Answers | undefined): { old: Answers | null; new: Answers | null } {
  return {
    old: pmi.answers_from === 'given' && Object.keys(given).length ? given : null,
    new: !pmi.new_stored && carried && Object.keys(carried).length ? carried : null,
  };
}

export type DrawnChange = 'changed' | 'added' | 'removed' | 'unchanged';

/** What became of each item: those the new scene draws by their id, and the old scene's removed ones. */
export function changeOf(items: readonly PmiChange[]): { now: Map<string, DrawnChange>; removed: Set<string> } {
  const now = new Map<string, DrawnChange>();
  const removed = new Set<string>();
  for (const c of items) {
    if (c.class === 'removed') removed.add(c.id);
    // Renumbered is a change; added wins over neither, as one item is one or the other.
    else if (c.class === 'added') now.set(c.id, 'added');
    else if (!now.has(c.id)) now.set(c.id, 'changed');
  }
  return { now, removed };
}

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
