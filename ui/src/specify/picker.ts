// A face picker over pmi-assist output. Ported from pmi-assist-viewer
// (picker/picker.js); the picker knows nothing about questions: the page
// decides what to highlight and what a selection means.
import * as THREE from 'three';

import type { PmiAnalysis, PmiIntent, PmiMesh, PmiVec } from '../types/pmi';

import { createAnnotations } from './annotations';
import { toLabels, type LabelGroup, type PmiLabel } from './labels';
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
/** Points of a feature tested for being in sight. */
const SAMPLES = 24;
/** How far off its axis a hole or shaft is looked at: a deep one, a shallow one. */
const OFF_AXIS = [(12 * Math.PI) / 180, (30 * Math.PI) / 180] as const;
/** What a point seen edge on still counts for: it is in sight, just. */
const EDGE_ON = 0.05;
/** Other directions tried when the part hides a feature: faces, edges and corners of a cube. */
const AROUND: readonly PmiVec[] = [-1, 0, 1].flatMap((x) => [-1, 0, 1].flatMap((y) => [-1, 0, 1]
  .map((z): PmiVec => [x, y, z]))).filter(([x, y, z]) => x || y || z);

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
    },
  ): void;
  setLabelsVisible(on: boolean): void;
  /** Whether labels on the far side of the part are hidden. */
  setFarSideHidden(on: boolean): void;
  /** How see-through the part is, 0 (solid) to under 1. */
  setTransparency(amount: number): void;
  setMode(mode: PickMode): void;
  setPickable(on: boolean): void;
  setSelection(faces: readonly number[]): void;
  getSelection(): number[];
  highlight(faces: readonly number[]): void;
  /** Faces tinted by the queue's state, under the selection, hover and focus. */
  tint(states: ReadonlyMap<number, 'required' | 'check' | 'problem'>): void;
  lookAt(faces: readonly number[]): void;
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
  const centre = new THREE.Vector3(...mesh.bbox[0]).add(new THREE.Vector3(...mesh.bbox[1])).multiplyScalar(0.5);

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

  // --- looking at a feature ------------------------------------------------
  // Points inside the faces (triangle centres: a vertex is shared with the
  // face next door) and which way each faces, at most SAMPLES in all.
  function samplesOf(faces: readonly number[]) {
    const triangles: number[] = [];
    for (const id of faces) {
      const [start, count] = mesh.faces[id] ?? [0, 0];
      for (let t = start; t < start + count; t++) triangles.push(t);
    }
    const step = Math.max(1, triangles.length / SAMPLES);
    const out: { at: THREE.Vector3; normal: THREE.Vector3 }[] = [];
    for (let k = 0; k < triangles.length; k += step) {
      const t = triangles[Math.floor(k)] ?? 0;
      const [a, b, c] = [0, 1, 2].map((corner) => {
        const v = mesh.indices[3 * t + corner] ?? 0;
        return new THREE.Vector3(
          mesh.positions[3 * v] ?? 0, mesh.positions[3 * v + 1] ?? 0, mesh.positions[3 * v + 2] ?? 0,
        );
      }) as [THREE.Vector3, THREE.Vector3, THREE.Vector3];
      const normal = b.clone().sub(a).cross(c.clone().sub(a));
      if (normal.lengthSq() < 1e-18) continue;
      out.push({ at: a.add(b).add(c).divideScalar(3), normal: normal.normalize() });
    }
    return out;
  }

  const sight = new THREE.Raycaster();
  // How well ``samples`` are seen from ``eye``: each that nothing but ``faces``
  // hides counts as squarely as it faces the eye -- a bore seen straight down
  // is in sight, but edge on, and counts little.
  function seen(eye: THREE.Vector3, samples: ReturnType<typeof samplesOf>, faces: Set<number>) {
    let score = 0;
    for (const { at, normal } of samples) {
      const along = at.clone().sub(eye);
      const far = along.length();
      sight.set(eye, along.normalize());
      sight.far = far * 1.001;
      const hit = sight.intersectObject(part, false)[0];
      const face = hit?.faceIndex != null ? faceOfTriangle(mesh.faces, hit.faceIndex) : null;
      if (!hit || hit.distance > far * 0.999 || (face !== null && faces.has(face))) {
        score += Math.max(Math.abs(normal.dot(along)), EDGE_ON);
      }
    }
    return score / Math.max(samples.length, 1);
  }

  /**
   * Where to stand to see ``faces``: a plane from in front, a hole or shaft
   * from a little off its axis (straight down it, the bore is a ring), at the
   * end nearer the view -- unless the part is in the way, when the direction
   * that shows most of it wins, the nearer the current view the better.
   */
  function bestView(faces: readonly number[]) {
    const samples = samplesOf(faces);
    if (!samples.length) return null;
    const box = new THREE.Box3().setFromPoints(samples.map((p) => p.at));
    const target = box.getCenter(new THREE.Vector3());
    const heading = nav.camera.position.clone().sub(centre).normalize();

    const normal = new THREE.Vector3();
    let turned = true;
    for (const id of faces) {
      const f = index.faces.get(id);
      if (!f?.direction) continue;
      const direction = new THREE.Vector3(...f.direction);
      if (f.kind !== 'plane' && direction.dot(heading) < 0) direction.negate();
      if (f.kind === 'plane') turned = false;
      normal.add(direction);
    }
    // Square on to a plane; for a hole or shaft, tipped off its axis towards
    // where the view already is (or any side): a little for a deep bore, more
    // for a shallow one, from either end.
    const ideals = [normal.lengthSq() > 1e-9 ? normal.normalize() : heading.clone()];
    const candidates = [...ideals];
    if (turned && normal.lengthSq() > 1e-9) {
      const axis = normal;
      let side = heading.clone().addScaledVector(axis, -heading.dot(axis));
      if (side.lengthSq() < 1e-6) side = new THREE.Vector3(0, 0, 1).cross(axis);
      if (side.lengthSq() < 1e-6) side = new THREE.Vector3(1, 0, 0);
      side.normalize();
      ideals.length = 0;
      for (const end of [axis, axis.clone().negate()]) {
        const tip = (angle: number) =>
          end.clone().multiplyScalar(Math.cos(angle)).addScaledVector(side, Math.sin(angle));
        ideals.push(tip(OFF_AXIS[1]));
        candidates.push(...OFF_AXIS.map(tip));
      }
    }
    candidates.push(...AROUND.map((d) => new THREE.Vector3(...d).normalize()));

    // Seeing it comes first, against the best any direction does; then the
    // ideal, then staying near the current view.
    const room = nav.diagonal * 1.5;
    const set = new Set(faces);
    const sights = candidates.map((d) => seen(target.clone().addScaledVector(d, room), samples, set));
    const clearest = Math.max(...sights, 1e-9);
    let best = { score: -Infinity, direction: candidates[0] ?? heading };
    candidates.forEach((direction, i) => {
      const score = (sights[i] ?? 0) / clearest
        + 0.3 * Math.max(...ideals.map((ideal) => direction.dot(ideal))) + 0.1 * direction.dot(heading);
      if (score > best.score) best = { score, direction };
    });
    return { direction: best.direction.toArray() as unknown as PmiVec };
  }

  paint();
  return {
    navigator: nav,
    missing: mesh.missing,
    annotate(intent, { current = null, show = {}, balloons = new Map(), pending, reveal } = {}) {
      const labels = toLabels(intent, analysis, pending)
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
    lookAt(faces) {
      const view = bestView(faces);
      if (view) nav.lookFrom(view.direction);
    },
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
