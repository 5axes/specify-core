// The package's own view of a part (cad-view.ts, on three-cad-viewer) as a HostView: the view the
// components use when the host gives none. Faces are in Specify's own numbering, so the check on
// load passes as a matter of course.
//
// The viewer's tree is this view's own: a layer's or a mesh's name is its place in the tree, a
// group and a leaf ("2× Ø10 holes/Before"), or a leaf at the top. Labels and drawn PMI are still
// drawn over this view only, through ``cad`` (docs/host-view.md, "Later").
import * as THREE from 'three';

import type { PmiMesh, PmiVec } from '../types/pmi';

import type { CadLeaf, CadView } from './cad-view';
import type { Box, CameraState, FaceRef, FaceStyle, HostPointerEvent, HostView, Mat4, MeshOptions, Vec3 } from './host-view.ts';

/** Where the opening view looks from. */
export const HOME: PmiVec = [0.8, -1.0, 0.62];
/** The part's edges: a leaf of their own, at the top of the tree, kept after the groups. */
export const EDGES = 'Edges';
const EDGE_COLOUR = '#4a5058';
const PLAIN = '#c9ced4';
/** The viewer's transparency, as it opens (createCadView's ``defaultOpacity``). */
const DEFAULT_OPACITY = 0.5;
/** A press that moves further than this is a drag, not a click. */
const CLICK_PX = 4;

export interface DefaultView extends HostView {
  /** The view underneath, for what is drawn over it: labels, drawn PMI, the cube's framing. */
  readonly cad: CadView;
  /** The viewer's tree closed to its first level. */
  collapse(): void;
  /** The viewer's toolbar and tools panel, shown or hidden. */
  showTools(on: boolean): void;
  roll(quarters: number): void;
  dolly(factor: number): void;
}

/** A name's place in the tree: its group, if it has one, and its leaf. */
const placeOf = (name: string): [string | null, string] => {
  const at = name.indexOf('/');
  return at < 0 ? [null, name] : [name.slice(0, at), name.slice(at + 1)];
};
const pathOf = (group: string | null, leaf: string) => `/Part/${group === null ? '' : `${group}/`}${leaf}`;

/** A column-major matrix as a function on points. */
function placing(m: Mat4 | undefined): ((p: [number, number, number]) => [number, number, number]) | undefined {
  if (!m) return undefined;
  return ([x, y, z]) => [
    m[0]! * x + m[4]! * y + m[8]! * z + m[12]!,
    m[1]! * x + m[5]! * y + m[9]! * z + m[13]!,
    m[2]! * x + m[6]! * y + m[10]! * z + m[14]!,
  ];
}

/** ``cad``, showing part ``part`` (its faces in ``mesh``), as a HostView. ``cad`` is made with the part's edges as a leaf named ``EDGES``. */
export function hostViewOf(cad: CadView, { part, mesh }: { part: number; mesh: PmiMesh }): DefaultView {
  // Each layer's leaves and each mesh's, by name, as drawn.
  const layers = new Map<string, string[]>();
  const meshes = new Map<string, string>();
  // What each drawn leaf is, by path: a layer's (the part's faces) or a mesh's.
  const leafOf = new Map<string, { mesh?: string }>();
  const groups = new Map<string, number>();
  let partShown = true;
  let opacity = 1;
  let viewerOpacity = DEFAULT_OPACITY;
  const edgesLeaf: CadLeaf = { name: EDGES, mesh, faces: [], colour: EDGE_COLOUR, edges: true };

  // A group gained or lost, counted by its leaves; the edges moved after a new group, so the
  // tree keeps them last.
  const counted = (group: string | null, change: number) => {
    if (group === null) return false;
    const was = groups.get(group) ?? 0;
    const now = was + change;
    if (now > 0) groups.set(group, now); else groups.delete(group);
    return was === 0 && now > 0;
  };
  const put = (group: string | null, leaves: CadLeaf[], owner: { mesh?: string }) => {
    let added = false;
    for (const leaf of leaves) {
      const path = pathOf(group, leaf.name);
      const had = leafOf.has(path);
      const has = leaf.faces.length > 0 || leaf.edges === true;
      if (has) leafOf.set(path, owner); else leafOf.delete(path);
      if (had !== has) added = counted(group, has ? 1 : -1) || added;
    }
    cad.replace(group, ...leaves);
    if (!partShown && !owner.mesh) for (const leaf of leaves) if (leaf.faces.length) cad.show(pathOf(group, leaf.name), false);
    if (added) {
      cad.replace(null, { ...edgesLeaf, edges: false });
      cad.replace(null, edgesLeaf);
      if (!partShown) cad.show(pathOf(null, EDGES), false);
    }
  };

  const ownFaces = (faces: readonly FaceRef[]) => faces.filter((f) => f.part === part && f.instance === undefined).map((f) => f.face);
  const boxOf = (frame: FaceRef[] | Box) => {
    if (!Array.isArray(frame)) return new THREE.Box3(new THREE.Vector3(...frame.min), new THREE.Vector3(...frame.max));
    const box = new THREE.Box3();
    for (const id of ownFaces(frame)) {
      const [start, count] = mesh.faces[id] ?? [0, 0];
      for (let k = 3 * start; k < 3 * (start + count); k++) {
        const v = mesh.indices[k]!;
        box.expandByPoint(new THREE.Vector3(mesh.positions[3 * v], mesh.positions[3 * v + 1], mesh.positions[3 * v + 2]));
      }
    }
    return box.isEmpty() ? undefined : box;
  };

  // --- events ------------------------------------------------------------------
  const cameraListeners = new Set<(camera: CameraState) => void>();
  const pointerListeners = new Set<(event: HostPointerEvent) => void>();
  const movedListeners = new Set<() => void>();
  const cameraState = (): CameraState => {
    const camera = cad.camera;
    const [width, height] = cad.size();
    camera.updateMatrixWorld();
    return {
      view: [...camera.matrixWorldInverse.elements], projection: [...(camera as THREE.Camera).projectionMatrix.elements],
      eye: camera.position.toArray() as Vec3, width, height,
    };
  };
  let lastCamera = '';
  cad.onFrame(() => {
    if (!cameraListeners.size) return;
    const state = cameraState();
    const key = JSON.stringify(state);
    if (key === lastCamera) return;
    lastCamera = key;
    for (const f of cameraListeners) f(state);
  });
  cad.onMoved(() => { for (const f of movedListeners) f(); });

  // What is under a point: the nearest face drawn; seen through a see-through part, a mesh
  // given (a gone face, say) is what is pointed at.
  const hitAt = (x: number, y: number): Pick<HostPointerEvent, 'hit' | 'mesh'> => {
    const hits = cad.facesAt(x, y).filter((h) => leafOf.has(h.path));
    const hit = (opacity < 1 ? hits.find((h) => leafOf.get(h.path)!.mesh !== undefined) : undefined) ?? hits[0];
    if (!hit) return { hit: null };
    const owner = leafOf.get(hit.path)!;
    return { hit: { part, face: hit.face }, ...(owner.mesh !== undefined ? { mesh: owner.mesh } : {}) };
  };
  const canvas = cad.canvas;
  const local = (event: PointerEvent | MouseEvent) => {
    const r = canvas.getBoundingClientRect();
    return [event.clientX - r.left, event.clientY - r.top] as const;
  };
  const keys = (event: PointerEvent | MouseEvent) => ({ button: event.button, shift: event.shiftKey, ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey });
  const emit = (event: HostPointerEvent) => { for (const f of pointerListeners) f(event); };
  // A press that stays put is a click, on the face pressed on: pressing focuses the canvas,
  // which can scroll it under the pointer.
  let down: { x: number; y: number; at: Pick<HostPointerEvent, 'hit' | 'mesh'> } | null = null;
  const onDown = (event: PointerEvent) => {
    down = pointerListeners.size ? { x: event.screenX, y: event.screenY, at: hitAt(...local(event)) } : null;
  };
  const onUp = (event: PointerEvent) => {
    const press = down;
    down = null;
    if (!press || Math.hypot(event.screenX - press.x, event.screenY - press.y) > CLICK_PX) return;
    const [x, y] = local(event);
    emit({ type: 'click', x, y, ...press.at, ...keys(event) });
  };
  const onMove = (event: PointerEvent) => {
    if (event.buttons || !pointerListeners.size) return;
    const [x, y] = local(event);
    emit({ type: 'hover', x, y, ...hitAt(x, y) });
  };
  const onLeave = (event: PointerEvent) => {
    if (!pointerListeners.size) return;
    const [x, y] = local(event);
    emit({ type: 'leave', x, y, hit: null });
  };
  const onDouble = (event: MouseEvent) => {
    if (!pointerListeners.size) return;
    const [x, y] = local(event);
    emit({ type: 'dblclick', x, y, ...hitAt(x, y), ...keys(event) });
  };
  canvas.addEventListener('pointerdown', onDown);
  canvas.addEventListener('pointerup', onUp);
  canvas.addEventListener('pointermove', onMove);
  canvas.addEventListener('pointerleave', onLeave);
  canvas.addEventListener('dblclick', onDouble);
  const listen = <T>(set: Set<T>, listener: T) => { set.add(listener); return () => { set.delete(listener); }; };

  const view: DefaultView = {
    cad,
    async info() { return { faceIds: 'step-entity', canDrawMeshes: true }; },
    showParts(parts) {
      const on = parts.includes(part);
      if (on === partShown) return;
      partShown = on;
      for (const [path, owner] of leafOf) if (!owner.mesh) cad.show(path, on);
      cad.show(pathOf(null, EDGES), on);
    },
    styleFaces(layer, styles: FaceStyle[]) {
      const [group, name] = placeOf(layer);
      const names = styles.map((_, i) => (styles.length === 1 ? name : `${name} (${i + 1})`));
      const gone = (layers.get(layer) ?? []).filter((n) => !names.includes(n));
      layers.set(layer, names);
      put(group, [
        ...gone.map((n): CadLeaf => ({ name: n, mesh, faces: [], colour: PLAIN })),
        ...styles.map((s, i): CadLeaf => ({
          name: names[i]!, mesh, faces: ownFaces(s.faces), colour: s.colour, ...(s.opacity !== undefined ? { alpha: s.opacity } : {}),
        })),
      ], {});
    },
    clearStyles(layer) {
      const [group] = placeOf(layer);
      const names = layers.get(layer);
      if (!names) return;
      layers.delete(layer);
      put(group, names.map((n): CadLeaf => ({ name: n, mesh, faces: [], colour: PLAIN })), {});
    },
    setOpacity(next) {
      opacity = Math.min(Math.max(next, 0), 1);
      cad.transparent(opacity < 1);
      if (opacity < 1 && opacity !== viewerOpacity) { viewerOpacity = opacity; cad.viewer.setOpacity(opacity, false); }
    },
    addMesh(id, given: PmiMesh, opts: MeshOptions = {}) {
      const [group, name] = placeOf(id);
      const place = placing(opts.transform);
      meshes.set(id, name);
      put(group, [{
        name, mesh: given, faces: given.faces.flatMap(([, count], f) => (count ? [f] : [])), colour: opts.colour ?? PLAIN,
        ...(opts.opacity !== undefined ? { alpha: opts.opacity } : {}), ...(opts.edges ? { edges: true } : {}), ...(place ? { place } : {}),
      }], { mesh: id });
    },
    removeMesh(id) {
      if (!meshes.delete(id)) return;
      const [group, name] = placeOf(id);
      put(group, [{ name, mesh, faces: [], colour: PLAIN }], { mesh: id });
    },
    lookFrom(direction, { up, frame } = {}) {
      cad.lookFrom(direction as PmiVec, frame ? boxOf(frame) : undefined, up as PmiVec | undefined);
    },
    fit({ also } = {}) { cad.fit(also ? boxOf(also) : null); },
    home() { cad.home(HOME); },
    onCamera(listener) {
      listener(cameraState());
      return listen(cameraListeners, listener);
    },
    onPointer: (listener) => listen(pointerListeners, listener),
    onUserMoved: (listener) => listen(movedListeners, listener),
    async rect() {
      const r = canvas.getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height };
    },
    roll: (quarters) => cad.roll(quarters),
    dolly: (factor) => cad.dolly(factor),
    // Specify's own numbering: every face is the one it says.
    partFaces() {},
    async checkFaces(_part, _faceIds, samples) { return samples.map(() => true); },
    collapse: () => cad.collapse(),
    showTools: (on) => cad.showTools(on),
    dispose() {
      cameraListeners.clear();
      pointerListeners.clear();
      movedListeners.clear();
      canvas.removeEventListener('pointerdown', onDown);
      canvas.removeEventListener('pointerup', onUp);
      canvas.removeEventListener('pointermove', onMove);
      canvas.removeEventListener('pointerleave', onLeave);
      canvas.removeEventListener('dblclick', onDouble);
      cad.dispose();
    },
  };
  return view;
}

/** The edges leaf a default view's ``cad`` is made with. */
export const edgesOf = (mesh: PmiMesh): CadLeaf => ({ name: EDGES, mesh, faces: [], colour: EDGE_COLOUR, edges: true });
