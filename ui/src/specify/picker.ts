// A face picker over pmi-assist output. Ported from pmi-assist-viewer
// (picker/picker.js); the picker knows nothing about questions: the page
// decides what to highlight and what a selection means.
import * as THREE from 'three';

import type { PmiAnalysis, PmiIntent, PmiMesh } from '../types/pmi';

import { createAnnotations } from './annotations';
import type { DrawnPmi, DrawnShow, DrawnView } from './drawn';
import { toLabels, type LabelGroup, type PmiLabel } from './labels';
import { createLook } from './look';
import { createNavigator, type Navigator } from './navigator';
import {
  expand, faceOfTriangle, indexAnalysis, sameBinding, toggle, type PickMode,
} from './selection';

const COLOURS = {
  plain: new THREE.Color(0xc9ced4),
  // The editor's colour scheme (docs/WORKFLOW.md), chosen to stay apart for colour-blind eyes:
  hover: new THREE.Color(0xf4f8fb), // under the pointer: a slight whitening
  focus: new THREE.Color(0x1ec8ff), // the current question: active, cyan
  selected: new THREE.Color(0x003b73), // faces picked: chosen, navy
  required: new THREE.Color(0xeda07a), // needs an answer: vermillion, pale
  check: new THREE.Color(0xf3dc6a), // a default to check: yellow, pale
  problem: new THREE.Color(0x8f7fe0), // a problem on the drawing: violet
};
export interface PickerEvents {
  change: (faces: number[]) => void;
  hover: (face: number | null) => void;
  face: (face: number) => void;
  label: (label: PmiLabel) => void;
}

export interface Picker {
  readonly navigator: Navigator;
  readonly missing: readonly number[];
  annotate(
    intent: PmiIntent | null,
    options?: {
      current?: string | null; show?: Partial<Record<LabelGroup, boolean>>;
      balloons?: ReadonlyMap<string, readonly number[]>;
      /** Questions still waiting for a person; their defaults are drawn as drafts. */
      pending?: ReadonlySet<string>;
      /** Labels drawn even where their group is not shown: those chosen. */
      reveal?: ReadonlySet<string>;
      /** false hides the datum labels (none until tolerancing is chosen). */
      datums?: boolean;
    },
  ): void;
  /** The review's rows, as the panel names them, with their faces: a view with a tree lists
   * them there. */
  features?(rows: readonly { title: string; faces: readonly number[] }[]): void;
  setLabelsVisible(on: boolean): void;
  /** A STEP's own graphical PMI (View PMI: Graphical), drawn when ``visible``; none without ``pmi``. */
  drawn?(pmi: DrawnPmi | null, visible: boolean): void;
  /** The kinds of drawn PMI the Show menu shows, and whether callouts facing away are hidden. */
  drawnFilter?(show: DrawnShow, farSide: boolean): void;
  /** Turn square to a view of the drawn PMI: along its plane's normal, its text upright. */
  lookSquare?(view: DrawnView): void;
  /** Whether labels on the far side of the part are hidden. */
  setFarSideHidden(on: boolean): void;
  /** The viewer's own toolbar and tools panel, where the view has them. */
  setTools?(on: boolean): void;
  /** How see-through the part is, 0 (solid) to under 1. */
  setTransparency(amount: number): void;
  setMode(mode: PickMode): void;
  setPickable(on: boolean): void;
  setSelection(faces: readonly number[]): void;
  getSelection(): number[];
  highlight(faces: readonly number[]): void;
  /** Faces tinted by the queue's state, under the selection, hover and focus. */
  tint(states: ReadonlyMap<number, 'required' | 'check' | 'problem'>): void;
  /** Turn to see ``faces``; ``close`` also frames them (with some of the part around) instead of the whole part. */
  lookAt(faces: readonly number[], options?: { close?: boolean }): void;
  on<K extends keyof PickerEvents>(name: K, listener: PickerEvents[K]): void;
  dispose(): void;
}

/**
 * ``mesh`` and ``analysis`` must be pmi-assist's for the same file (format 2,
 * same binding); a mismatched pair is refused rather than mislabelled.
 */
export function createPicker(
  container: HTMLElement,
  { mesh, analysis, panelRoom }: { mesh: PmiMesh; analysis: PmiAnalysis; panelRoom?: () => number },
): Picker {
  if (mesh.format !== 'pmi-assist-mesh' || mesh.version !== 2) {
    throw new Error(`unsupported mesh ${mesh.format} v${mesh.version}`);
  }
  if (!sameBinding(mesh.binding, analysis.binding)) {
    throw new Error('mesh and analysis are of different files');
  }
  const index = indexAnalysis(analysis);
  const nav = createNavigator(container, { bbox: mesh.bbox, ...(panelRoom ? { panelRoom } : {}) });

  // One geometry for the whole part; a face's colour is written into the
  // vertices it owns, which no other face shares.
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute([...mesh.positions], 3));
  geometry.setIndex([...mesh.indices]);
  geometry.computeVertexNormals();
  const colours = new Float32Array(mesh.positions.length);
  const colourAttribute = new THREE.BufferAttribute(colours, 3);
  geometry.setAttribute('color', colourAttribute);
  const material = new THREE.MeshStandardMaterial({
    vertexColors: true, metalness: 0.04, roughness: 0.6, side: THREE.DoubleSide,
    polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1,
  });
  const part = new THREE.Mesh(geometry, material);
  nav.scene.add(part);

  const edgePositions: number[] = [];
  for (const line of mesh.edges) {
    for (let i = 0; i + 5 < line.length; i += 3) edgePositions.push(...line.slice(i, i + 6));
  }
  const edgeGeometry = new THREE.BufferGeometry();
  edgeGeometry.setAttribute('position', new THREE.Float32BufferAttribute(edgePositions, 3));
  const edgeMaterial = new THREE.LineBasicMaterial({ color: 0x4a5058 });
  nav.scene.add(new THREE.LineSegments(edgeGeometry, edgeMaterial));

  // Which vertices each face owns, from its triangles.
  const vertexRange = mesh.faces.map(([start, count]): [number, number] => {
    if (!count) return [0, -1];
    let lo = Infinity;
    let hi = -1;
    for (let k = start * 3; k < (start + count) * 3; k++) {
      const v = mesh.indices[k] ?? 0;
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
    return [lo, hi];
  });

  let mode: PickMode = 'face';
  let selection = new Set<number>();
  let focus = new Set<number>();
  let tints = new Map<number, 'required' | 'check' | 'problem'>();
  let hovered: number[] = [];
  let pickable = true;
  const listeners: { [K in keyof PickerEvents]: PickerEvents[K][] } = {
    change: [], hover: [], face: [], label: [],
  };

  function paint() {
    const hover = new Set(hovered);
    vertexRange.forEach(([lo, hi], face) => {
      const colour = selection.has(face) ? COLOURS.selected
        : hover.has(face) ? COLOURS.hover
          : focus.has(face) ? COLOURS.focus
            : tints.has(face) ? COLOURS[tints.get(face)!]
              : COLOURS.plain;
      for (let v = lo; v <= hi; v++) colour.toArray(colours, v * 3);
    });
    colourAttribute.needsUpdate = true;
  }

  function faceAt(x: number, y: number): number | null {
    const hit = nav.rayAt(x, y).intersectObject(part, false)[0];
    return hit?.faceIndex != null ? faceOfTriangle(mesh.faces, hit.faceIndex) : null;
  }

  nav.onClick((x, y, event) => {
    const face = faceAt(x, y);
    if (face === null) return;
    for (const f of listeners.face) f(face);
    if (!pickable) return;
    // Shift adds to the selection; a plain click starts a new one.
    selection = toggle(selection, expand(index, face, mode), !event.shiftKey);
    paint();
    const sorted = [...selection].sort((a, b) => a - b);
    for (const f of listeners.change) f(sorted);
  });
  let hoverFace: number | null = null;
  nav.onHover((x, y) => {
    const face = x === null || y === null ? null : faceAt(x, y);
    // What a click would select is lit only while picking; the face under the
    // pointer is reported either way.
    const next = face === null || !pickable ? [] : expand(index, face, mode);
    if (next.join() !== hovered.join()) {
      hovered = next;
      paint();
    }
    if (face !== hoverFace) {
      hoverFace = face;
      for (const f of listeners.hover) f(face);
    }
  });

  const annotations = createAnnotations(nav, container, {
    mesh, analysis, onClick: (label) => { for (const f of listeners.label) f(label); },
    // A label behind the part is what someone turns transparency up to read.
    occluders: () => (material.transparent ? [] : [part]),
  });
  nav.alsoFrame(() => annotations.points());
  let framedLabels = false;

  const lookAt = createLook(nav, {
    mesh, occluder: part, faceOf: (triangle) => faceOfTriangle(mesh.faces, triangle),
    faceInfo: (face) => index.faces.get(face),
  });

  paint();
  return {
    navigator: nav,
    missing: mesh.missing,
    annotate(intent, { current = null, show = {}, balloons = new Map(), pending, reveal, datums = true } = {}) {
      const labels = toLabels(intent, analysis, pending)
        .filter((label) => datums || label.kind !== 'datum' || label.group === 'existing')
        .filter((label) => show[label.group] !== false || reveal?.has(label.id));
      annotations.set(labels, { current, anchors: intent?.anchors ?? {}, balloons });
      // The opening view is chosen before the labels arrive; the first time they
      // do, it is chosen again to take them in -- unless the view has been moved.
      if (labels.length && !framedLabels) {
        framedLabels = true;
        if (!nav.moved()) nav.home();
      }
    },
    setLabelsVisible: (on) => annotations.setVisible(on),
    setFarSideHidden: (on) => annotations.setFarSideHidden(on),
    setTransparency(amount) {
      const a = Math.min(Math.max(amount, 0), 0.95);
      material.transparent = a > 0;
      material.opacity = 1 - a;
      // Seen through, what is behind must still be drawn.
      material.depthWrite = a === 0;
      material.needsUpdate = true;
      annotations.recheck();
    },
    setMode(next) {
      mode = next;
      hovered = [];
      paint();
    },
    setPickable(on) {
      pickable = on;
      hovered = [];
      paint();
    },
    setSelection(faces) {
      selection = new Set(faces);
      paint();
    },
    getSelection: () => [...selection].sort((a, b) => a - b),
    highlight(faces) {
      focus = new Set(faces);
      paint();
    },
    tint(states) {
      tints = new Map(states);
      paint();
    },
    lookAt,
    on(name, listener) {
      (listeners[name] as typeof listener[]).push(listener);
    },
    dispose() {
      annotations.dispose();
      geometry.dispose();
      material.dispose();
      edgeGeometry.dispose();
      edgeMaterial.dispose();
      nav.dispose();
    },
  };
}
