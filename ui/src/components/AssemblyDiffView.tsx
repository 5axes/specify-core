import { useEffect, useRef, useState } from 'react';
import type { WorkerClient } from '../engine/rpc';
import { createCadView, pathOf, type CadNode, type CadView } from '../specify/cad-view';
import type { AssemblyPart } from '../types/editor';
import type { PmiMesh } from '../types/pmi';

/** A row of an assembly's comparison: a part, paired by name, and what became of it. */
export interface AssemblyRow {
  name: string;
  status: string;
  old: { part: number; instances: number } | null;
  new: { part: number; instances: number } | null;
}
export interface StepFile { name: string; bytes: Uint8Array }

// The picture's colours (specify-core's revision::picture): a part that changed, was placed a
// different number of times or moved in orange, one added in green, the rest grey, and one
// removed a red ghost where it was.
const COLOUR: Record<string, string> = { changed: '#e07a00', quantity: '#e07a00', moved: '#e07a00', added: '#0a8f3d', removed: '#d62929' };
const PLAIN = '#cfd2d6';

type Placement = number[][];
const placed = (m: Placement) => ([x, y, z]: [number, number, number]) => [0, 1, 2].map(r =>
  m[r]![0]! * x + m[r]![1]! * y + m[r]![2]! * z + m[r]![3]!) as [number, number, number];
// The tree finds nodes by name: none of its separators, and each name once.
const label = (name: string) => name.replace(/[/<>&"]/g, ' ');

/**
 * The new revision of an assembly, whole, in three-cad-viewer: each part at its placements,
 * coloured by what became of it, and each removed part a ghost at its old placements. Picking
 * a changed part opens its own comparison.
 */
export default function AssemblyDiffView({ engine, old, next, rows, lit, onOpen }: {
  engine: WorkerClient;
  /** The part in hand in the parts table, lit here. */
  lit: string | null;
  old: StepFile;
  next: StepFile;
  rows: AssemblyRow[];
  onOpen: (name: string) => void;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const view = useRef<CadView | null>(null);
  const [read, setRead] = useState(0);
  const [error, setError] = useState('');
  const drawn = rows.filter(r => r.new || r.old);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const [oldParts, newParts] = await Promise.all([
          engine.request<AssemblyPart[]>('parts', { name: old.name, bytes: old.bytes }),
          engine.request<AssemblyPart[]>('parts', { name: next.name, bytes: next.bytes }),
        ]);
        const nodes: CadNode[] = [];
        for (const row of drawn) {
          // A part in the new revision is drawn from it; a removed one from the old.
          const [file, parts, at] = row.new ? [next, newParts, row.new.part] : [old, oldParts, row.old!.part];
          const mesh = await engine.request<PmiMesh>('part-mesh', { name: file.name, bytes: file.bytes, part: at });
          if (!live) return;
          setRead(n => n + 1);
          const placements = parts[at]?.placements ?? [];
          nodes.push({
            name: label(row.name),
            leaves: placements.map((m, k) => ({
              name: placements.length > 1 ? `#${k + 1}` : 'Part',
              mesh, faces: mesh.faces.map((_, f) => f), edges: true,
              colour: COLOUR[row.status] ?? PLAIN,
              ...(row.status === 'removed' ? { alpha: 0.35 } : {}),
              place: placed(m),
            })),
          });
        }
        if (!stage.current || !live) return;
        view.current = createCadView(stage.current, nodes, {
          onPick: path => {
            const row = drawn.find(r => path === pathOf(label(r.name)) || path.startsWith(`${pathOf(label(r.name))}/`));
            if (row && row.status === 'changed') onOpen(row.name);
            else if (row) view.current?.light(pathOf(label(row.name)));
          },
        });
      } catch (e) {
        if (live) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => { live = false; view.current?.dispose(); view.current = null; };
  }, [old, next, rows]);

  useEffect(() => { view.current?.light(lit === null ? null : pathOf(label(lit))); }, [lit, read]);

  return (
    <div className="diff-assembly-stage">
      <div className="diff-legend">
        <span><i style={{ background: COLOUR.changed }} />Changed, moved or recounted</span>
        <span><i style={{ background: COLOUR.added }} />Added</span>
        <span><i style={{ background: COLOUR.removed, opacity: 0.5 }} />Removed</span>
        <span><i style={{ background: PLAIN }} />Unchanged</span>
      </div>
      <div ref={stage} className="diff-stage" />
      {read < drawn.length && !error && <p className="diff-status diff-assembly-reading">Reading part {read + 1} of {drawn.length}…</p>}
      {error && <p role="alert" className="diff-error">{error}</p>}
    </div>
  );
}
