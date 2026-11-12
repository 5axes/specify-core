// The Specify review's view of a part, on three-cad-viewer (cad-view.ts), with the face
// picker's interface (picker.ts), so the review is unchanged by the move. A face's state --
// plain, a tint from the queue, the current question, picked, under the pointer -- is the
// tree leaf it is in, each leaf one colour; a change replaces only the leaves it touches.
// Labels are drawn over the viewer's canvas in a scene and renderer of their own, from its
// camera: nothing of the editor's is drawn into the viewer's scene.
import * as THREE from 'three';

import type { PmiAnalysis, PmiMesh, PmiVec } from '../types/pmi';

import { createAnnotations } from './annotations';
import { createCadView, type CadLeaf } from './cad-view';
import { partCentre, type DrawnPmi, type DrawnPoint } from './drawn';
import { createDrawnLayer } from './drawn-layer';
import { toLabels, type PmiLabel } from './labels';
import { createLook } from './look';
import type { Picker, PickerEvents } from './picker';
import { expand, faceOfTriangle, indexAnalysis, sameBinding, toggle, type PickMode } from './selection';

const HOME: PmiVec = [0.8, -1.0, 0.62];

// The editor's colour scheme (docs/WORKFLOW.md), chosen to stay apart for colour-blind eyes,
// in the order a face takes them: the first that applies.
const STATES = [
  ['Picked', '#003b73'], // faces picked: chosen, navy
  ['Under the pointer', '#f4f8fb'], // what a click would pick: a slight whitening
  ['Current question', '#1ec8ff'], // active, cyan
  ['Needs an answer', '#eda07a'], // required: vermillion, pale
  ['Default to check', '#f3dc6a'], // check: yellow, pale
  ['Problem', '#8f7fe0'], // a problem on the drawing: violet
  ['Faces', '#c9ced4'],
] as const;
type State = (typeof STATES)[number][0];
const TINT: Record<'required' | 'check' | 'problem', State> = {
  required: 'Needs an answer', check: 'Default to check', problem: 'Problem',
};

/**
 * ``mesh`` and ``analysis`` must be pmi-assist's for the same file (format 2, same binding);
 * a mismatched pair is refused rather than mislabelled.
 */
export function createReviewView(
  container: HTMLElement,
  { mesh, analysis }: { mesh: PmiMesh; analysis: PmiAnalysis },
): Picker {
  if (mesh.format !== 'pmi-assist-mesh' || mesh.version !== 2) {
    throw new Error(`unsupported mesh ${mesh.format} v${mesh.version}`);
  }
  if (!sameBinding(mesh.binding, analysis.binding)) {
    throw new Error('mesh and analysis are of different files');
  }
  const index = indexAnalysis(analysis);
  const all = mesh.faces.map((_, f) => f);
  const leaf = (state: State, faces: number[]): CadLeaf => ({
    name: state, mesh, faces, colour: STATES.find(([s]) => s === state)![1],
  });
  // The tree is the review's features (its rows, as the panel names them), each feature's faces
  // in the leaf of their state's colour; faces no row is about are the part's others. The edges
  // are a leaf of their own, so they stay as faces move.
  const OTHER = 'Other faces';
  let featureOf = new Map<number, string>();
  const groupOf = (face: number) => featureOf.get(face) ?? OTHER;
  const EDGES: CadLeaf = { name: 'Edges', mesh, faces: [], colour: '#4a5058', edges: true };
  const cad = createCadView(container, [{ name: OTHER, leaves: [leaf('Faces', all)] }, EDGES], {
    control: 'trackball',
    // A calm first look: the viewer's toolbar and tools panel come with "More view tools".
    tools: false,
    // A feature picked in the tree: its question, as picking one of its faces would.
    onPick: (path) => {
      const name = path.split('/')[2];
      const face = all.find((f) => groupOf(f) === name && name !== OTHER);
      if (face !== undefined) for (const f of listeners.face) f(face);
    },
  });
  cad.home(HOME);

  let mode: PickMode = 'face';
  let selection = new Set<number>();
  let focus = new Set<number>();
  let tints = new Map<number, 'required' | 'check' | 'problem'>();
  let hovered = new Set<number>();
  let pickable = true;
  const listeners: { [K in keyof PickerEvents]: PickerEvents[K][] } = { change: [], hover: [], face: [], label: [] };

  // Each face's leaf as drawn, its feature and state; a paint replaces only the leaves whose
  // faces changed.
  const key = (group: string, state: State) => `${group}\u0000${state}`;
  const drawnKey = new Map<number, string>(all.map((f) => [f, key(OTHER, 'Faces')]));
  function stateOf(face: number): State {
    if (selection.has(face)) return 'Picked';
    if (hovered.has(face)) return 'Under the pointer';
    if (focus.has(face)) return 'Current question';
    const tint = tints.get(face);
    return tint ? TINT[tint] : 'Faces';
  }
  function paint() {
    const touched = new Set<string>();
    for (const face of all) {
      const next = key(groupOf(face), stateOf(face));
      const was = drawnKey.get(face)!;
      if (next === was) continue;
      touched.add(next).add(was);
      drawnKey.set(face, next);
    }
    const byGroup = new Map<string, State[]>();
    for (const k of touched) {
      const [group, state] = k.split('\u0000') as [string, State];
      byGroup.set(group, [...(byGroup.get(group) ?? []), state]);
    }
    for (const [group, states] of byGroup) {
      cad.replace(group, ...states.map((state) => leaf(state, all.filter((f) => drawnKey.get(f) === key(group, state)))));
    }
  }

  // What hides a label or a feature: the part, as the editor's own copy of it.
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute([...mesh.positions], 3));
  geometry.setIndex([...mesh.indices]);
  const part = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));
  geometry.computeBoundingBox();
  const partBox = geometry.boundingBox!.clone();
  let seeThrough = false;

  const faceAt = (x: number, y: number) => cad.facesAt(x, y)[0]?.face ?? null;
  const at = (event: PointerEvent) => {
    const r = cad.canvas.getBoundingClientRect();
    return [event.clientX - r.left, event.clientY - r.top] as const;
  };
  // A press that stays put is a click; the face is the one pressed on.
  let down: { x: number; y: number; face: number | null } | null = null;
  const onDown = (event: PointerEvent) => { down = { x: event.screenX, y: event.screenY, face: faceAt(...at(event)) }; };
  const onUp = (event: PointerEvent) => {
    const press = down;
    down = null;
    if (!press || Math.hypot(event.screenX - press.x, event.screenY - press.y) > 4) return;
    // A drawn callout clicked is its semantic item, as its label would be.
    const item = drawnLayer.hit(...at(event));
    if (item !== null) {
      for (const f of listeners.label) f(drawnLabel(item));
      return;
    }
    if (press.face === null) return;
    for (const f of listeners.face) f(press.face);
    if (!pickable) return;
    // Shift adds to the selection; a plain click starts a new one.
    selection = toggle(selection, expand(index, press.face, mode), !event.shiftKey);
    paint();
    const sorted = [...selection].sort((a, b) => a - b);
    for (const f of listeners.change) f(sorted);
  };
  let hoverFace: number | null = null;
  const onMove = (event: PointerEvent) => {
    if (event.buttons) return;
    const point = at(event);
    // Over a drawn callout: a hand, and its notation as haecceity names it.
    const item = drawnLayer.hit(...point);
    cad.canvas.style.cursor = item === null ? '' : 'pointer';
    cad.canvas.title = item === null ? '' : drawnPmi?.presentation.items[item]?.[0]?.name ?? '';
    report(faceAt(...point));
  };
  const report = (face: number | null) => {
    // What a click would pick is lit only while picking; the face under the pointer is
    // reported either way.
    const next = new Set(face === null || !pickable ? [] : expand(index, face, mode));
    if ([...next].join() !== [...hovered].join()) { hovered = next; paint(); }
    if (face !== hoverFace) { hoverFace = face; for (const f of listeners.hover) f(face); }
  };
  const onLeave = () => report(null);
  cad.canvas.addEventListener('pointerdown', onDown);
  cad.canvas.addEventListener('pointerup', onUp);
  cad.canvas.addEventListener('pointermove', onMove);
  cad.canvas.addEventListener('pointerleave', onLeave);

  // The labels' own scene, drawn by a renderer of their own over the canvas.
  const scene = new THREE.Scene();
  const ink = new THREE.WebGLRenderer({ alpha: true, antialias: true });
  ink.setPixelRatio(window.devicePixelRatio);
  ink.domElement.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none';
  cad.overlayHost.append(ink.domElement);
  const labels = document.createElement('div');
  labels.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none';
  cad.overlayHost.append(labels);
  const host = {
    scene, diagonal: cad.diagonal,
    get camera() { return cad.camera; },
    size: () => cad.size(),
    onFrame: (listener: () => void) => cad.onFrame(listener),
  };
  cad.onFrame(() => {
    const [w, h] = cad.size();
    const canvas = cad.canvas as HTMLElement;
    for (const el of [ink.domElement, labels]) {
      el.style.left = `${canvas.offsetLeft}px`;
      el.style.top = `${canvas.offsetTop}px`;
      el.style.width = `${w}px`;
      el.style.height = `${h}px`;
    }
    ink.setSize(w, h, false);
    ink.domElement.style.width = `${w}px`;
    ink.domElement.style.height = `${h}px`;
    ink.render(scene, cad.camera);
  });
  const annotations = createAnnotations(host, labels, {
    mesh, analysis, onClick: (label) => { for (const f of listeners.label) f(label); },
    // A label behind the part is what someone turns transparency up to read.
    occluders: () => (seeThrough ? [] : [part]),
  });
  let framedLabels = false;

  // The file's own graphics, in the labels' scene; a linked one clicked is its item's label.
  let drawnPmi: DrawnPmi | null = null;
  let drawnShowing = false;
  // The part, and the file's callouts when they are drawn: a notes block off the part is in view.
  const drawnBounds = () => (drawnShowing ? drawnLayer.bounds() : null);
  // The orientation cube frames them too, as home and fit do.
  cad.alsoFrame(drawnBounds);
  const drawnLabel = (item: string): PmiLabel => {
    const link = drawnPmi?.links[item];
    return {
      id: `drawn:${item}`, kind: 'note', group: link?.status ?? 'existing',
      cells: [drawnPmi?.presentation.items[item]?.[0]?.name ?? item],
      faces: (link?.faces ?? []).filter((f) => f >= 0 && f < mesh.faces.length),
      ...(link?.question ? { question: link.question } : {}),
    };
  };
  const drawnLayer = createDrawnLayer(host, labels, {
    onClick: (item) => { for (const f of listeners.label) f(drawnLabel(item)); },
    centre: partCentre(mesh.positions),
  });

  const lookAt = createLook(cad, {
    mesh, occluder: part, faceOf: (triangle) => faceOfTriangle(mesh.faces, triangle),
    faceInfo: (face) => index.faces.get(face),
  });

  const navigator = {
    fit: () => cad.fit(drawnBounds()),
    home: () => cad.home(HOME, drawnBounds()),
    roll: (quarters: number) => cad.roll(quarters),
    dolly: (factor: number) => cad.dolly(factor),
  };

  return {
    navigator: navigator as unknown as Picker['navigator'],
    missing: mesh.missing,
    annotate(intent, { current = null, show = {}, balloons = new Map(), pending, reveal, datums = true } = {}) {
      const shown = toLabels(intent, analysis, pending)
        .filter((label) => datums || label.kind !== 'datum' || label.group === 'existing')
        .filter((label) => show[label.group] !== false || reveal?.has(label.id));
      annotations.set(shown, { current, anchors: intent?.anchors ?? {}, balloons });
      // The opening view is chosen before the labels arrive; the first time they do, it is
      // chosen again -- unless the view has been moved.
      if (shown.length && !framedLabels) {
        framedLabels = true;
        if (!cad.moved()) cad.home(HOME, drawnBounds());
      }
    },
    features(rows) {
      // A face in several rows (a hole and its pattern) is in the first; names are made unique
      // and plain, as the viewer's tree finds nodes by name and shows them as HTML.
      const used = new Map<string, number>();
      const next = new Map<number, string>();
      for (const row of rows) {
        const base = row.title.replace(/[/<>&"]/g, ' ').replace(/\s+/g, ' ').trim() || 'Feature';
        const n = (used.get(base) ?? 0) + 1;
        used.set(base, n);
        const name = n > 1 ? `${base} (${n})` : base;
        for (const f of row.faces) if (!next.has(f)) next.set(f, name);
      }
      if ([...next].join() === [...featureOf].join()) return;
      // The tree again, in the panel's order: its features, then the part's other faces and
      // its edges.
      const groups = new Set([...drawnKey.values()].map((k) => k.split('\u0000')[0]!));
      for (const group of groups) cad.replace(group, ...STATES.map(([state]) => leaf(state, [])));
      for (const f of all) drawnKey.set(f, '');
      featureOf = next;
      const order = [...new Set([...next.values()]), OTHER];
      for (const group of order) {
        const states = STATES.map(([state]) => state);
        cad.replace(group, ...states.map((state) => leaf(state, all.filter((f) => groupOf(f) === group && stateOf(f) === state))));
      }
      for (const f of all) drawnKey.set(f, key(groupOf(f), stateOf(f)));
      // The edges, kept until the groups were back (the viewer's tree hides what is added to a
      // node with nothing under it), then moved after them.
      cad.replace(null, { ...EDGES, edges: false });
      cad.replace(null, EDGES);
      cad.collapse();
    },
    setLabelsVisible: (on) => annotations.setVisible(on),
    drawn(pmi, visible) {
      if (pmi !== drawnPmi) { drawnPmi = pmi; drawnLayer.set(pmi); }
      const showing = visible && pmi !== null;
      drawnLayer.setVisible(showing);
      // Turned on: framed once with its callouts, so none starts out of view.
      const turnedOn = showing && !drawnShowing;
      drawnShowing = showing;
      if (turnedOn && !cad.moved()) cad.home(HOME, drawnBounds());
    },
    drawnFilter: (show, farSide) => drawnLayer.filter(show, farSide),
    lookSquare(view) {
      // Square on, framing its graphics with the part: a callout is read where it is on the part.
      const v = (p: DrawnPoint) => new THREE.Vector3(...p);
      const around = partBox.clone().union(new THREE.Box3(v(view.min), v(view.max)));
      cad.lookFrom([...view.eye] as PmiVec, around, view.up ? [...view.up] as PmiVec : undefined);
    },
    setFarSideHidden: (on) => annotations.setFarSideHidden(on),
    setTools: (on) => cad.showTools(on),
    setTransparency(amount) {
      const a = Math.min(Math.max(amount, 0), 0.95);
      seeThrough = a > 0;
      cad.transparent(seeThrough);
      if (seeThrough) cad.viewer.setOpacity(1 - a, false);
      annotations.recheck();
    },
    setMode(next) { mode = next; hovered = new Set(); paint(); },
    setPickable(on) { pickable = on; hovered = new Set(); paint(); },
    setSelection(faces) { selection = new Set(faces); paint(); },
    getSelection: () => [...selection].sort((a, b) => a - b),
    highlight(faces) { focus = new Set(faces); paint(); },
    tint(states) { tints = new Map(states); paint(); },
    lookAt,
    on(name, listener) { (listeners[name] as typeof listener[]).push(listener); },
    dispose() {
      drawnLayer.dispose();
      annotations.dispose();
      cad.canvas.removeEventListener('pointerdown', onDown);
      cad.canvas.removeEventListener('pointerup', onUp);
      cad.canvas.removeEventListener('pointermove', onMove);
      cad.canvas.removeEventListener('pointerleave', onLeave);
      ink.dispose();
      geometry.dispose();
      cad.dispose();
    },
  };
}
