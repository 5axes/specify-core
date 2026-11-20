import { useEffect, useRef, useState, type ReactNode } from 'react';
import * as THREE from 'three';
import type { WorkerClient } from '../engine/rpc';
import { createIntentEngine } from '../engine/intent-engine';
import { createLook, viewpointOf, type LookAt } from '../specify/look';
import { createDefaultView, pathOf } from '../specify/cad-view';
import type { DefaultView } from '../specify/default-view';
import { subMesh, type HostPointerEvent, type HostView, type Mat4, type MeshOptions, type Vec3 } from '../specify/host-view';
import type { PmiMesh } from '../types/pmi';
import { changeOf, drawsItem, facesOf, movedPmi, type DiffPmi, type PmiChange } from '../model/diff-pmi';
import { createDiffDrawn, PMI_COLOUR, type DiffDrawn } from '../specify/diff-drawn';
import { drawnFromScene, hasGraphics, type DrawnPmi } from '../specify/drawn';
import { reportHtml } from '../model/diff';
import AssemblyDiffView, { type StepFile } from './AssemblyDiffView';
import './StepDiff.css';

/** The STEP diff: two revisions compared with Specify and shown, see ../model/diff.ts. The page around it (the /diff address, where the files came from) is the host's. */

/** A "What changed" line: its text and its features' faces on the new part and the old. */
interface Change { line: string; new: number[]; old: number[] }
interface View {
  new: PmiMesh;
  old: PmiMesh;
  faces: Record<string, 'changed' | 'new' | 'ambiguous'>;
  gone: number[];
  motion: { rotation: number[][]; translation: number[] } | null;
  changes: Change[];
}
/** A comparison's report: its verdict and summary, and its PMI section with the two scenes it compared. */
interface Report { verdict: string; summary: string; pmi?: DiffPmi }
/** ``part``: Specify's index of the part drawn, in the new file (or the old, for a removed part); 0 by default. */
interface Diffed { report: Report; markdown: string; view: View; part?: number }
/** Each revision's PMI scene, as the diff compared them, read for one comparison, or why it could not be. */
interface Scenes { for: Diffed; now: DrawnPmi | null; was: DrawnPmi | null; problem: string }
/**
 * A part of an assembly compared: its status (changed, added, removed, quantity, moved or
 * unchanged), where it is in each file, and, changed, its own diff's report, Markdown and view.
 */
interface PartRow {
  name: string;
  status: string;
  old: { part: number; instances: number } | null;
  new: { part: number; instances: number } | null;
  report?: Report;
  markdown?: string;
  view?: View;
}
interface AssemblyDiffed { report: { verdict: string; summary: string }; markdown: string; parts: PartRow[] }

// The picture's colours (specify-core's revision::picture). The line in hand is lit in
// three-cad-viewer's own highlight blue (LIT).
const LIT = '#89b9e3';
const COLOURS = {
  changed: new THREE.Color(0.88, 0.48, 0.0),
  new: new THREE.Color(0.04, 0.56, 0.24),
  ambiguous: new THREE.Color(0.48, 0.25, 0.75),
  plain: new THREE.Color(0.80, 0.81, 0.83),
  gone: new THREE.Color(0.84, 0.16, 0.16),
};

type Motion = View['motion'];
const place = (p: number[], motion: Motion) => (motion
  ? [0, 1, 2].map(i => motion.rotation[i]![0]! * p[0]! + motion.rotation[i]![1]! * p[1]! + motion.rotation[i]![2]! * p[2]! + motion.translation[i]!)
  : p);

/** A mesh's faces (those `keep` keeps) as one geometry, and each triangle's face: what hides a feature, for look.ts. */
function occluderOf(mesh: PmiMesh) {
  const positions: number[] = [];
  const faceOf: number[] = [];
  mesh.faces.forEach(([start, count], face) => {
    for (let k = start * 3; k < (start + count) * 3; k++) {
      const v = mesh.indices[k]!;
      positions.push(mesh.positions[v * 3]!, mesh.positions[v * 3 + 1]!, mesh.positions[v * 3 + 2]!);
    }
    for (let t = 0; t < count; t++) faceOf.push(face);
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  return { mesh: new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide })), faceOf };
}

// The tree is the report: each "What changed" line a node named by its feature, its faces on
// the new part coloured by kind and its faces Before, the old part's, moved to where the new
// part is. Faces no line names are left at the top.
const KINDS = [
  ['Unchanged', 'plain', null], ['Now (changed)', 'changed', 'changed'], ['Now (new)', 'new', 'new'], ["Now (can't tell)", 'ambiguous', 'ambiguous'],
] as const;
const GONE = 'Gone or moved';
const BEFORE = 'Before';
// A PMI item's faces, lit when it has no callout to light: on the new part, and the old.
const ITEM = 'PMI item';
const ITEM_BEFORE = 'PMI item (before)';
/**
 * Each line's node name: its feature, the line up to its first comma or colon ("2× R1
 * fillets"), made unique. A name is a path step and the viewer writes it as HTML, so it
 * keeps no slashes or markup.
 */
function lineNames(changes: Change[]) {
  const used = new Map<string, number>();
  return changes.map(c => {
    const name = c.line.split(/[,:]/)[0]!.replace(/[/<>&"]/g, ' ').replace(/\s+/g, ' ').trim() || 'Change';
    const n = (used.get(name) ?? 0) + 1;
    used.set(name, n);
    return n > 1 ? `${name} (${n})` : name;
  });
}
const hex = (c: THREE.Color) => `#${c.getHexString()}`;

interface Drawn {
  view: HostView;
  /** The package's own view, when the part is drawn in it: what the PMI is drawn over. */
  own: DefaultView | null;
  /** Whether the person has turned or zoomed the view since it opened. */
  moved(): boolean;
  /**
   * Shows the old part's faces, Before: every line's with ``all``, else only the line in hand's
   * and a removal's, whose Before is all there is to see. Near the new faces they would
   * otherwise streak through them everywhere.
   */
  showBefore(all: boolean, line: number | null): void;
  /** Lights a line's faces, or none. */
  light(line: number | null): void;
  /** The line a face pointed at is in, if any. */
  changeAt(event: HostPointerEvent): number | null;
  /** Turns to a line's faces, through the part if they are all gone from it; none, back to solid. */
  lookAt(line: number | null): void;
  /**
   * Lights a PMI item's faces, on the new part, or the old part's if it has none there; none,
   * nothing. With ``turn``, turns to them too: how an item with no callout is shown.
   */
  lightFaces(faces: { old: number[]; new: number[] } | null, turn?: boolean): void;
  dispose(): void;
}

/** A rigid motion as a column-major matrix. */
const matrixOf = (motion: Motion): Mat4 | undefined => (motion ? [
  motion.rotation[0]![0]!, motion.rotation[1]![0]!, motion.rotation[2]![0]!, 0,
  motion.rotation[0]![1]!, motion.rotation[1]![1]!, motion.rotation[2]![1]!, 0,
  motion.rotation[0]![2]!, motion.rotation[1]![2]!, motion.rotation[2]![2]!, 0,
  motion.translation[0]!, motion.translation[1]!, motion.translation[2]!, 1,
] : undefined);
const SEE_THROUGH = 0.5;

/**
 * The comparison drawn in ``given``, the host's view, or in the package's own in ``container``.
 * ``part`` is Specify's index of the new part. ``onLine`` hears a line picked in the default
 * view's tree.
 */
function draw(container: HTMLElement, view: View, part: number, given: HostView | null, onLine: (line: number) => void): Drawn {
  const names = lineNames(view.changes);
  const lineName = (i: number) => names[i]!;
  const kindOf = (face: number) => view.faces[face] ?? null;
  // Each face's line: the first that names it.
  const lineOf = (side: 'new' | 'old') => {
    const of = new Map<number, number>();
    view.changes.forEach((c, i) => { for (const f of c[side]) if (!of.has(f)) of.set(f, i); });
    return of;
  };
  const newLine = lineOf('new');
  const oldLine = lineOf('old');
  const transform = matrixOf(view.motion);
  const moved = (p: [number, number, number]) => place(p, view.motion) as [number, number, number];
  const kindRow = (face: number) => KINDS.find(([, , kind]) => kind === kindOf(face))!;
  // A face's layer: its line's node and its kind; faces no line names at the top, unchanged,
  // or under Other changes.
  const layerOf = (face: number) => {
    const [name] = kindRow(face);
    const line = newLine.get(face);
    if (line !== undefined) return `${lineName(line)}/${name}`;
    return name === 'Unchanged' ? name : `Other changes/${name}`;
  };
  const newFaces = view.new.faces.map((_, f) => f);
  // The layers in the tree's order: each line's, then the faces no line names.
  const byLayer = new Map<string, { faces: number[]; colour: keyof typeof COLOURS; line: number | null }>();
  for (const [i] of view.changes.entries()) {
    for (const [name, colour] of KINDS) byLayer.set(`${lineName(i)}/${name}`, { faces: [], colour, line: i });
  }
  byLayer.set('Unchanged', { faces: [], colour: 'plain', line: null });
  for (const [name, colour] of KINDS.slice(1)) byLayer.set(`Other changes/${name}`, { faces: [], colour, line: null });
  for (const f of newFaces) byLayer.get(layerOf(f))!.faces.push(f);
  // The old part's faces each line's Before shows, and those no line names.
  const goneOf = (line: number) => view.gone.filter(f => (line < 0 ? !oldLine.has(f) : oldLine.get(f) === line));
  const beforeId = (line: number) => (line < 0 ? GONE : `${lineName(line)}/${BEFORE}`);
  // A line's node and each of its leaves, by tree path; a Before, by its id.
  const lineAt = new Map<string, number>(view.changes.flatMap((_, i) => [
    [pathOf(lineName(i)), i] as const,
    ...[...KINDS.map(([name]) => name), BEFORE].map(leaf => [pathOf(lineName(i), leaf), i] as const),
  ]));
  const beforeLine = new Map(view.changes.map((_, i) => [beforeId(i), i]));

  // What the default view frames: the new part and the old part's gone faces, where they are drawn.
  const bounds = new THREE.Box3(new THREE.Vector3(...view.new.bbox[0]), new THREE.Vector3(...view.new.bbox[1]));
  for (const f of view.gone) {
    const [start, count] = view.old.faces[f] ?? [0, 0];
    for (let k = 3 * start; k < 3 * (start + count); k++) {
      const v = view.old.indices[k]!;
      bounds.expandByPoint(new THREE.Vector3(...moved([view.old.positions[3 * v]!, view.old.positions[3 * v + 1]!, view.old.positions[3 * v + 2]!])));
    }
  }
  // The trackball, as the Specify view has: turning square to a callout sets the way up its plane
  // reads, which an orbit control (up held at +Z) would undo, leaving a callout whose up is -Z
  // upside down.
  const own = given ? null : createDefaultView(container, {
    part, mesh: view.new, control: 'trackball',
    bounds: [bounds.min.toArray(), bounds.max.toArray()],
    onPick: path => { const line = lineAt.get(path); if (line !== undefined) onLine(line); },
  });
  const hv = (given ?? own)!;
  hv.showParts([part]);
  // A host that cannot draw meshes shows no Before.
  let meshes = true;
  const added = new Map<string, number>();
  const addMesh = (id: string, faces: number[], opts: MeshOptions) => {
    if (!meshes || !faces.length) { removeMesh(id); return; }
    hv.addMesh(id, subMesh(view.old, faces), { ...(transform ? { transform } : {}), ...opts });
    added.set(id, faces.length);
  };
  const removeMesh = (id: string) => { if (added.delete(id)) hv.removeMesh(id); };
  void hv.info().then(info => {
    meshes = info.canDrawMeshes;
    if (!meshes) for (const id of [...added.keys()]) removeMesh(id);
  });

  // The line in hand is lit: its faces, and its Before where shown, in LIT.
  let lit: number | null = null;
  const style = (layer: string) => {
    const { faces, colour, line } = byLayer.get(layer)!;
    if (!faces.length) { hv.clearStyles(layer); return; }
    hv.styleFaces(layer, [{ faces: faces.map(face => ({ part, face })), colour: line !== null && line === lit ? LIT : hex(COLOURS[colour]) }]);
  };
  for (const layer of byLayer.keys()) if (byLayer.get(layer)!.faces.length) style(layer);
  // Which Befores are shown, by line (-1, the faces no line names), as last set.
  const shown = new Map<number, boolean>();
  const showGone = (line: number) => {
    if (!shown.get(line)) { removeMesh(beforeId(line)); return; }
    addMesh(beforeId(line), goneOf(line), { colour: line >= 0 && line === lit ? LIT : hex(COLOURS.gone), opacity: 0.35 });
  };

  const occluder = occluderOf(view.new);
  const diagonal = bounds.getSize(new THREE.Vector3()).length() || 1;
  const viewpoint = viewpointOf(hv, diagonal);
  const lookNew: LookAt = createLook(viewpoint, { mesh: view.new, occluder: occluder.mesh, faceOf: t => occluder.faceOf[t] ?? null });
  const oldMoved = { ...view.old, positions: view.old.positions.flatMap((_, i, p) => (i % 3 ? [] : moved([p[i]!, p[i + 1]!, p[i + 2]!]))) };
  const lookOld: LookAt = createLook(viewpoint, { mesh: oldMoved, occluder: occluder.mesh, faceOf: () => null });
  // See-through: from the start when a line has nothing left on the new part, a removed hole
  // say, whose faces are inside what the new part filled in; else while one is picked.
  const removal = (c: Change) => c.new.length === 0 && c.old.length > 0;
  const always = view.changes.some(removal);
  let through = always;
  if (always) hv.setOpacity(SEE_THROUGH);
  let itemFaces = '';
  let userMoved = false;
  const stopMoved = hv.onUserMoved(() => { userMoved = true; });

  return {
    view: hv,
    own,
    moved: () => userMoved,
    showBefore(all, line) {
      view.changes.forEach((c, i) => {
        const visible = all || i === line || removal(c);
        if (shown.get(i) !== visible) { shown.set(i, visible); showGone(i); }
      });
      if (shown.get(-1) !== all) { shown.set(-1, all); showGone(-1); }
    },
    light(line) {
      if (line === lit) return;
      const was = lit;
      lit = line;
      for (const l of [was, line]) {
        if (l === null) continue;
        for (const [name] of KINDS) if (byLayer.get(`${lineName(l)}/${name}`)!.faces.length) style(`${lineName(l)}/${name}`);
        if (shown.get(l)) showGone(l);
      }
    },
    changeAt(event) {
      if (event.mesh !== undefined) return beforeLine.get(event.mesh) ?? null;
      return event.hit && event.hit.part === part ? newLine.get(event.hit.face) ?? null : null;
    },
    lookAt(line) {
      const change = line === null ? null : view.changes[line]!;
      const hides = always || (!!change && removal(change));
      if (hides !== through) { through = hides; hv.setOpacity(hides ? SEE_THROUGH : 1); }
      if (!change) return;
      if (change.new.length) lookNew(change.new, { close: true });
      else if (change.old.length) lookOld(change.old, { close: true });
    },
    lightFaces(faces, turn = false) {
      const on = faces?.new.length ? { new: faces.new, old: [] } : { new: [], old: faces?.old ?? [] };
      const key = JSON.stringify(on);
      if (key !== itemFaces) {
        itemFaces = key;
        if (on.new.length) hv.styleFaces(ITEM, [{ faces: on.new.map(face => ({ part, face })), colour: LIT }]);
        else hv.clearStyles(ITEM);
        addMesh(ITEM_BEFORE, on.old, { colour: LIT, opacity: 0.6 });
      }
      if (!turn) return;
      if (on.new.length) lookNew(on.new, { close: true });
      else if (on.old.length) lookOld(on.old, { close: true });
    },
    dispose() {
      stopMoved();
      viewpoint.dispose();
      occluder.mesh.geometry.dispose();
      if (own) { own.dispose(); return; }
      // The host's view is the host's: only what the comparison put on it is taken off.
      for (const layer of byLayer.keys()) hv.clearStyles(layer);
      hv.clearStyles(ITEM);
      for (const id of [...added.keys()]) removeMesh(id);
    },
  };
}

const CHANGE: Record<string, string> = { quantity: 'quantity changed' };
// A verdict's mark, as the pull request comment gives it, and what it means.
const MARK: Record<string, [string, string]> = {
  ok: ['✅', 'Nothing to decide'], decide: ['⚠️', 'A decision is needed'], problems: ['❌', "Specify's review finds problems"],
};
function Mark({ verdict }: { verdict: string }) {
  const [mark, meaning] = MARK[verdict] ?? MARK.problems!;
  return <span role="img" aria-label={meaning} title={meaning}>{mark}</span>;
}

/** An assembly's parts, those that differ first, each changed one opened to its own comparison. */
function PartsTable({ assembly, onOpen, onPoint }: { assembly: AssemblyDiffed; onOpen: (row: PartRow) => void; onPoint: (name: string | null) => void }) {
  const rows = [...assembly.parts.filter(p => p.status !== 'unchanged'), ...assembly.parts.filter(p => p.status === 'unchanged')];
  const quantity = (p: PartRow) => (p.old && p.new && p.old.instances !== p.new.instances
    ? `${p.old.instances} → ${p.new.instances}` : String((p.new ?? p.old)?.instances ?? ''));
  return (
    <table className="diff-parts" onMouseLeave={() => onPoint(null)}>
      <thead><tr><th>Part</th><th>Quantity</th><th>Change</th><th /></tr></thead>
      <tbody>
        {rows.map(p => (
          <tr key={p.name} className={`is-${p.status}`} onMouseEnter={() => onPoint(p.name)}>
            <td>{p.name}</td>
            <td>{quantity(p)}</td>
            <td>{p.status === 'changed' && p.report ? <><Mark verdict={p.report.verdict} /> changed</> : CHANGE[p.status] ?? p.status}</td>
            <td><button type="button" className="diff-button" onClick={() => onOpen(p)}>Open</button></td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export interface StepDiffProps {
  before: { name: string; bytes: Uint8Array };
  after: { name: string; bytes: Uint8Array };
  /** The answers the revisions are compared with. Compared again when the object changes. */
  answers?: unknown;
  /** The comparison, when the host already has it: exactly what the engine's `diff()` gives for these two files with the
   *  PMI scenes (a part's `{report, markdown, view}`, an assembly's `{report, markdown, parts}`, scenes in
   *  `report.pmi.scenes`). It is drawn as it is and the engine does not compare again. */
  compared?: unknown;
  /** The assembly part open, controlled by the host: its name, or none for the parts list. */
  part?: string | null;
  /** Hears the part the person opens, or none for "All parts". */
  onPart?: (name: string | null) => void;
  onEvent?: (name: 'diff_viewed' | 'diff_failed' | 'diff_change_picked') => void;
  /** The host's title and source lines, first in the header. */
  header?: ReactNode;
  /** An error from the host, shown where this component's own is, ahead of it. */
  hostError?: string;
  /** For a host that serves the engine itself: the URL of its `specify_core_wasm.js` module, resolved against the page.
   *  By default the engine is the dependency @draftwright/specify-core-wasm.
   *  A cross-origin URL needs the host's CSP `script-src` (and `connect-src` for the .wasm) to allow it.
   *  Read only when the component mounts; a later change has no effect. */
  wasmUrl?: string;
  /** The host's own 3D view to draw the part in (docs/host-view.md); by default the package's. */
  view?: HostView | null;
}

const NO_ANSWERS = {};

export default function StepDiff({ before, after, answers = NO_ANSWERS, compared, part: wanted, onPart, onEvent, header, hostError, wasmUrl, view: hostView = null }: StepDiffProps) {
  const [status, setStatus] = useState('');
  const [error, setError] = useState('');
  // What the 3D view draws: a part's comparison, or an assembly's open part's.
  const [diffed, setDiffed] = useState<Diffed | null>(null);
  // An assembly compared, its parts listed, and the part open, if one is.
  const [assembly, setAssembly] = useState<AssemblyDiffed | null>(null);
  const [part, setPart] = useState<string | null>(null);
  // The assembly's two files, for its whole view, and the part pointed at in its table.
  const [files, setFiles] = useState<{ old: StepFile; next: StepFile } | null>(null);
  const [pointedPart, setPointedPart] = useState<string | null>(null);
  // Each part's mesh, read once for the assembly's view and again when it is opened alone.
  const meshes = useRef(new Map<string, PmiMesh>());
  const [notice, setNotice] = useState('');
  const [showGone, setShowGone] = useState(false);
  // The line picked, and the line under the pointer, in the report or on the part.
  const [picked, setPicked] = useState<number | null>(null);
  const [pointed, setPointed] = useState<number | null>(null);
  // Each revision's PMI, drawn on the part, and what of it is shown; the item picked, and the
  // item pointed at, in the PMI changes or on the part.
  const [scenes, setScenes] = useState<Scenes | null>(null);
  const [pmiShow, setPmiShow] = useState({ on: true, defaults: true, existing: true, general: false, farSide: true });
  const [pickedPmi, setPickedPmi] = useState<string | null>(null);
  const [pointedPmi, setPointedPmi] = useState<string | null>(null);
  const pmiDrawn = useRef<DiffDrawn | null>(null);
  const pmiList = useRef<HTMLUListElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const report = useRef<HTMLElement>(null);
  const drawn = useRef<Drawn | null>(null);
  const engine = useRef<WorkerClient | null>(null);
  // Only the latest comparison is shown: picking another file supersedes one still running.
  const run = useRef(0);
  // Picking a change is counted once a visit, not once a click.
  const counted = useRef(false);
  const events = useRef(onEvent);
  events.current = onEvent;
  // The part the host last asked for or was told of, to tell its changes from echoes of our own.
  const handled = useRef<string | null>(null);
  const wantedNow = useRef(wanted);
  wantedNow.current = wanted;
  const choose = (i: number | null) => {
    setPicked(i);
    if (i !== null && !counted.current) { counted.current = true; events.current?.('diff_change_picked'); }
  };

  useEffect(() => {
    engine.current = createIntentEngine({ wasmUrl });
    return () => { run.current++; engine.current?.dispose(); engine.current = null; };
  }, []);

  /**
   * A part of the assembly as a comparison: a changed part's own, or one of a part whose
   * geometry did not change (unchanged, moved, recounted: plain; added: all new; removed: all
   * gone), drawn from the mesh the assembly's view read, so any part can be looked at alone.
   */
  const partDiff = async (row: PartRow, both: { old: StepFile; next: StepFile }): Promise<Diffed> => {
    const [file, at] = row.new ? [both.next, row.new.part] : [both.old, row.old!.part];
    if (row.view) return { report: row.report!, markdown: row.markdown!, view: row.view, part: at };
    const mesh = meshes.current.get(row.name)
      ?? await engine.current!.request<PmiMesh>('part-mesh', { name: file.name, bytes: file.bytes, part: at });
    meshes.current.set(row.name, mesh);
    const all = mesh.faces.map((_, f) => f);
    const counts = row.old && row.new ? `placed ${row.old.instances} → ${row.new.instances} times` : '';
    const summary = {
      added: 'Added in this revision.', removed: 'Removed in this revision.', moved: 'Moved: the same part, placed elsewhere.',
      quantity: `The same part, ${counts}.`,
    }[row.status] ?? 'Unchanged.';
    return {
      part: at,
      report: { verdict: 'ok', summary, ...row.report?.pmi ? { pmi: row.report.pmi } : {} }, markdown: '',
      view: {
        new: mesh, old: mesh, motion: null, changes: [],
        faces: row.status === 'added' ? Object.fromEntries(all.map(f => [String(f), 'new' as const])) : {},
        gone: row.status === 'removed' ? all : [],
      },
    };
  };
  // Only the latest part asked for is shown.
  const shown = useRef(0);
  const show = (row: PartRow | null) => {
    const mine = ++shown.current;
    setPart(row?.name ?? null); setPicked(null); setPointed(null); setNotice('');
    if (!row || !files) { setDiffed(null); return; }
    void partDiff(row, files).then(d => { if (mine === shown.current) setDiffed(d); })
      .catch(e => { if (mine === shown.current) setError(e instanceof Error ? e.message : String(e)); });
  };
  /** Opens an assembly's part, or none (its parts list), and tells the host. */
  const open = (row: PartRow | null) => {
    show(row);
    handled.current = row?.name ?? null;
    onPart?.(row?.name ?? null);
  };
  // The host's part changed (Back and Forward between the list and a part): the comparison is in
  // hand, so nothing is redone.
  useEffect(() => {
    if (!assembly || !files || (wanted || null) === handled.current) return;
    handled.current = wanted || null;
    show(assembly.parts.find(p => p.name === wanted) ?? null);
  }, [wanted, assembly, files]);

  /** Compare the two revisions and show the result unless superseded: an assembly's parts list, or the host's part of it. */
  const compare = async () => {
    const mine = ++run.current;
    setDiffed(null); setAssembly(null); setFiles(null); setPart(null); setNotice(''); meshes.current = new Map(); setError(''); setPicked(null); setPointed(null);
    try {
      const input = { oldName: before.name, old: before.bytes, newName: after.name, new: after.bytes, answers };
      // With the two PMI scenes it compares (each part's, an assembly's), drawn as they are.
      let result: Diffed | AssemblyDiffed;
      if (compared) result = compared as Diffed | AssemblyDiffed;
      else {
        setStatus('Comparing them with Specify…');
        result = await engine.current!.request<Diffed | AssemblyDiffed>('diff', { ...input, title: input.newName, scenes: true });
      }
      if (mine !== run.current) return;
      if ('parts' in result) {
        const both = { old: { name: input.oldName, bytes: input.old }, next: { name: input.newName, bytes: input.new } };
        const want = wantedNow.current || '';
        handled.current = want || null;
        setAssembly(result); setFiles(both);
        const row = want ? result.parts.find(p => p.name === want) : undefined;
        if (row) { setPart(row.name); setDiffed(await partDiff(row, both)); }
        else if (want) setNotice(`The assembly has no part named "${want}".`);
      } else setDiffed(result);
      events.current?.('diff_viewed');
    } catch (e) {
      if (mine === run.current) { setError(e instanceof Error ? e.message : String(e)); events.current?.('diff_failed'); }
    }
  };

  useEffect(() => { void compare(); }, [before, after, answers, compared]);

  useEffect(() => {
    if (!diffed || !stage.current) return;
    const d = draw(stage.current, diffed.view, diffed.part ?? 0, hostView, line => {
      choose(line);
      drawn.current?.lookAt(line);
      report.current?.querySelector(`[data-change="${line}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    });
    drawn.current = d;
    // A callout drawn over the part is nearer than any face.
    const pmiAt = (x: number, y: number) => pmiDrawn.current?.hit(x, y) ?? null;
    const canvas = d.own?.cad.canvas;
    const stop = d.view.onPointer(e => {
      if (e.type === 'leave') { setPointed(null); setPointedPmi(null); return; }
      if (e.type === 'hover') {
        // Over a callout: a hand, its notation as haecceity names it, and its row lit.
        const item = pmiAt(e.x, e.y);
        if (canvas) {
          canvas.style.cursor = item === null ? '' : 'pointer';
          canvas.title = item === null ? '' : pmiDrawn.current?.name(item) ?? '';
        }
        setPointedPmi(item);
        setPointed(item === null ? d.changeAt(e) : null);
        return;
      }
      if (e.type !== 'click') return;
      const item = pmiAt(e.x, e.y);
      if (item !== null) {
        setPickedPmi(item);
        pmiList.current?.querySelector(`[data-pmi="${CSS.escape(item)}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
        return;
      }
      const line = d.changeAt(e);
      choose(line);
      if (line !== null) report.current?.querySelector(`[data-change="${line}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    });
    return () => { stop(); d.dispose(); drawn.current = null; };
  }, [diffed, hostView]);

  const current = pointed ?? picked;
  useEffect(() => { drawn.current?.showBefore(showGone, current); }, [showGone, current, diffed]);
  useEffect(() => {
    drawn.current?.light(current);
    report.current?.querySelectorAll<HTMLElement>('[data-change]').forEach(li => {
      const i = Number(li.dataset.change);
      li.classList.toggle('is-current', i === current);
      li.classList.toggle('is-picked', i === picked);
    });
  }, [current, picked, diffed]);

  // The two PMI scenes the diff compared, as it gave them: the old one moved onto the new part.
  useEffect(() => {
    setScenes(null); setPickedPmi(null); setPointedPmi(null);
    const pmi = diffed?.report.pmi;
    if (!diffed || !pmi) return;
    // Each scene read on its own: one that cannot be drawn leaves the other drawn, and says why.
    const problems: string[] = [];
    const read = (scene: unknown, side: string) => {
      if (scene === undefined) return null;
      const read = drawnFromScene(scene);
      if (read.ok) return read.value;
      problems.push(`${side}: ${read.reason}`);
      return null;
    };
    const now = read(pmi.scenes?.new, 'now');
    const was = read(pmi.scenes?.old, 'before');
    setScenes({ for: diffed, now, was: was && movedPmi(was, diffed.view.motion), problem: problems.join('; ') });
  }, [diffed]);

  // The PMI drawn on the part once both scenes are read, framed with it unless it has been turned.
  const changes = diffed?.report.pmi?.items ?? [];
  useEffect(() => {
    // Drawn over the package's own view; over a host's, not yet (docs/host-view.md).
    const d = drawn.current;
    const cad = d?.own?.cad;
    if (!d || !cad || !diffed || scenes?.for !== diffed || (!scenes.now && !scenes.was)) return;
    const { now, removed, was } = changeOf(diffed.report.pmi?.items ?? []);
    const [min, max] = diffed.view.new.bbox;
    const layers = createDiffDrawn(cad, {
      now: scenes.now, was: scenes.was, change: now, removed, before: was,
      part: new THREE.Box3(new THREE.Vector3(...min), new THREE.Vector3(...max)),
      onClick: id => setPickedPmi(id),
    });
    pmiDrawn.current = layers;
    layers.filter({ default: pmiShow.defaults, existing: pmiShow.existing, general: pmiShow.general }, pmiShow.farSide);
    layers.setVisible(pmiShow.on);
    const bounds = layers.bounds();
    if (!d.moved()) d.view.fit(bounds ? { also: { min: bounds.min.toArray() as Vec3, max: bounds.max.toArray() as Vec3 } } : undefined);
    return () => { layers.dispose(); pmiDrawn.current = null; };
  }, [scenes]);
  useEffect(() => {
    pmiDrawn.current?.filter({ default: pmiShow.defaults, existing: pmiShow.existing, general: pmiShow.general }, pmiShow.farSide);
    pmiDrawn.current?.setVisible(pmiShow.on);
  }, [pmiShow, scenes]);
  // Whether an item has a callout to draw: in the old scene if removed, else the new.
  const drawable = (c: PmiChange) => drawsItem(c.class === 'removed' ? scenes?.was ?? null : scenes?.now ?? null, c.id);
  // An item with no callout is shown on its faces, which the diff gives for each item.
  const facesShown = (id: string | null) => {
    const c = id === null || scenes?.for !== diffed ? undefined : changes.find(c => c.id === id);
    return c && !drawable(c) ? facesOf(c) : null;
  };
  const currentPmi = pointedPmi ?? pickedPmi;
  useEffect(() => {
    pmiDrawn.current?.light(currentPmi);
    drawn.current?.lightFaces(facesShown(currentPmi));
  }, [currentPmi, scenes]);
  /** An item picked in the PMI changes: lit, and turned to; picked again, let go. */
  const pickPmi = (id: string) => {
    const next = id === pickedPmi ? null : id;
    setPickedPmi(next);
    if (next !== null) {
      if (!pmiShow.on) setPmiShow({ ...pmiShow, on: true });
      const faces = facesShown(next);
      if (faces) drawn.current?.lightFaces(faces, true);
      else pmiDrawn.current?.focus(next);
    }
  };

  const lineOf = (target: EventTarget) => {
    const li = (target as Element).closest?.<HTMLElement>('[data-change]');
    return li ? Number(li.dataset.change) : null;
  };
  const pick = (i: number | null) => {
    if (i === null) return;
    const next = i === picked ? null : i;
    choose(next);
    drawn.current?.lookAt(next);
  };

  return (
    <>
      <header>
        {header}
        {assembly && part !== null && (
          <p className="diff-part"><button type="button" className="diff-button diff-back" onClick={() => open(null)}><span aria-hidden="true">←</span> All parts</button><strong>{part}</strong></p>
        )}
      </header>
      {hostError || error ? <p role="alert" className="diff-error">{hostError || error}</p>
        : !diffed && !assembly && status && <p className="diff-status">{status}</p>}
      {/* Kept while a part is open, so coming back to the whole assembly reads nothing again. */}
      {assembly && files && (
        <div className="diff-body diff-body-assembly" hidden={Boolean(diffed)}>
          <section className="diff-view">
            <AssemblyDiffView engine={engine.current!} old={files.old} next={files.next} rows={assembly.parts} lit={pointedPart}
              meshes={meshes.current} onPoint={setPointedPart} onOpen={name => { const row = assembly.parts.find(p => p.name === name); if (row) open(row); }} />
          </section>
          <aside className="diff-panel diff-assembly">
            {notice && <p role="alert" className="diff-error">{notice}</p>}
            <p className="diff-summary"><Mark verdict={assembly.report.verdict} /> {assembly.report.summary}</p>
            <p className="diff-hint">Point at a part to light it; open one, here or on the assembly, to turn it round on its own, a changed one with its changes coloured. ✅ nothing to decide, ⚠️ a decision is needed, ❌ Specify's review finds problems.</p>
            <PartsTable assembly={assembly} onOpen={open} onPoint={setPointedPart} />
          </aside>
        </div>
      )}
      <div className="diff-body" hidden={!diffed}>
        <section className="diff-view">
          <div className="diff-legend">
            <span><i style={{ background: '#e07a00' }} />Now (changed)</span>
            <span><i style={{ background: '#0a8f3d' }} />Now (new)</span>
            <span><i style={{ background: '#7a40bf' }} />Now (can't tell)</span>
            <span><i style={{ background: '#d62929', opacity: 0.5 }} />Before</span>
            <span><i style={{ background: '#cfd2d6' }} />Unchanged</span>
            <span><i style={{ background: LIT }} />Selected</span>
            <label><input type="checkbox" checked={showGone} onChange={e => setShowGone(e.target.checked)} />Show every Before</label>
          </div>
          {scenes?.for === diffed && [scenes.now, scenes.was].some(p => p && hasGraphics(p)) && (
            <div className="diff-legend diff-pmi-legend">
              <label><input type="checkbox" checked={pmiShow.on} onChange={e => setPmiShow({ ...pmiShow, on: e.target.checked })} />PMI</label>
              <span><i style={{ background: PMI_COLOUR.changed }} />Changed</span>
              <span><i style={{ background: PMI_COLOUR.added }} />Added</span>
              <span><i style={{ background: PMI_COLOUR.removed, opacity: 0.5 }} />Removed</span>
              <span><i style={{ background: PMI_COLOUR.unchanged }} />Unchanged</span>
              <details className="diff-pmi-show">
                <summary title="Which unchanged PMI is drawn">Show ▾</summary>
                <div>
                  {(['defaults', 'existing', 'general', 'farSide'] as const).map(key => (
                    <label key={key}>
                      <input type="checkbox" checked={pmiShow[key]} onChange={e => setPmiShow({ ...pmiShow, [key]: e.target.checked })} />
                      {{ defaults: 'Defaults to check', existing: 'PMI from the file', general: 'General dimensions', farSide: 'Hide far side' }[key]}
                    </label>
                  ))}
                </div>
              </details>
            </div>
          )}
          <div ref={stage} className="diff-stage" />
        </section>
        {diffed && (
          <aside className="diff-panel">
            <p className="diff-summary"><Mark verdict={diffed.report.verdict} /> {diffed.report.summary}</p>
            {changes.length > 0 && (
              <section className="diff-pmi" aria-label="PMI changes">
                <h3>PMI changes</h3>
                <p className="diff-hint">Click a row to light its callout and turn the part square to it, a changed one's old callout ghosted beside it; one with no callout lights its faces. Click a callout to find its row.</p>
                {!scenes && <p className="diff-status">Reading the PMI…</p>}
                {scenes?.problem && <p role="alert" className="diff-error">The PMI could not be drawn: {scenes.problem}</p>}
                <ul ref={pmiList} onMouseLeave={() => setPointedPmi(null)}>
                  {changes.map(c => {
                    const can = drawable(c);
                    const anchored = c.faces.new.length > 0 || c.faces.old.length > 0;
                    // Said only of a scene that was drawn: the one the item is in (the old for a removed item).
                    return (
                      <li
                        key={`${c.id}\u0000${c.class}`} data-pmi={c.id} tabIndex={0}
                        className={`is-${c.class}${c.id === pickedPmi ? ' is-picked' : ''}${c.id === (pointedPmi ?? pickedPmi) ? ' is-current' : ''}`}
                        onMouseEnter={() => setPointedPmi(c.id)}
                        onClick={() => pickPmi(c.id)}
                        onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pickPmi(c.id); } }}
                      >
                        <span className="diff-pmi-number">{c.number !== undefined ? `#${c.number}` : ''}</span>
                        <span className="diff-pmi-class">{c.class}</span>
                        <span className="diff-pmi-what">
                          {c.old !== undefined && <span className="diff-pmi-old">{c.old}</span>}
                          {c.old !== undefined && c.new !== undefined && c.new !== c.old && <> → </>}
                          {c.new !== undefined && c.new !== c.old && <span className="diff-pmi-new">{c.new}</span>}
                        </span>
                        <span className="diff-pmi-on">{c.on}{c.reason ? ` · ${c.reason}` : ''}{scenes && (c.class === 'removed' ? scenes.was : scenes.now) && !can ? (anchored ? ' · no callout: shown on its faces' : ' · no callout drawn') : ''}</span>
                      </li>
                    );
                  })}
                </ul>
              </section>
            )}
            {diffed.view.changes.length > 0 && <p className="diff-hint">Point at a line under What changed to light its faces, click it to turn the part to them; click a face to find its line.</p>}
            <article
              ref={report}
              className="diff-report"
              dangerouslySetInnerHTML={{ __html: reportHtml(diffed.markdown, { omit: changes.length > 0 ? ['PMI'] : [] }) }}
              onMouseOver={e => setPointed(lineOf(e.target))}
              onMouseLeave={() => setPointed(null)}
              onClick={e => pick(lineOf(e.target))}
              onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(lineOf(e.target)); } }}
            />
          </aside>
        )}
      </div>
    </>
  );
}
