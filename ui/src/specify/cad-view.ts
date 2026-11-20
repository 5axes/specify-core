// A part in Bernhard Walter's three-cad-viewer, through its published API only: its tree,
// transparency, clipping and measuring, with what the editor needs on top. A tree node is
// lit with the viewer's own tree hover; the faces under the pointer are found by a ray
// against this file's copy of the meshes, so nothing of the editor's is drawn into its scene
// (the package carries its own three.js).
import * as THREE from 'three';
import { CollapseState, Display, Viewer, type Shapes } from 'three-cad-viewer';
import 'three-cad-viewer/css';

import type { PmiMesh, PmiVec } from '../types/pmi';

import { createOrientationCube } from './orientation-cube';
import { lightGroup } from './cad-highlight.ts';

/** A leaf of the tree: some faces of a mesh, in one colour. */
export interface CadLeaf {
  /** Unique among its siblings. */
  name: string;
  mesh: PmiMesh;
  faces: readonly number[];
  colour: string;
  /** Opacity, 1 for a solid. */
  alpha?: number;
  /** The mesh's edges go with this leaf; with no faces, the leaf is the edges alone. */
  edges?: boolean;
  /** Where each point goes, for faces drawn somewhere other than where the mesh has them. */
  place?: (p: [number, number, number]) => [number, number, number];
}

/** A node of the tree with leaves under it. */
export interface CadGroup { name: string; leaves: CadLeaf[] }
export type CadNode = CadLeaf | CadGroup;

/** A face under the pointer: the path of its leaf, and its id in the leaf's mesh. */
export interface CadHit { path: string; face: number }

export interface CadView {
  readonly viewer: Viewer;
  /** The viewer's camera, and the part's diagonal, as look.ts asks of a view. */
  readonly camera: THREE.Camera;
  readonly diagonal: number;
  /** The canvas, for pointer events in its own coordinates. */
  readonly canvas: HTMLElement;
  /** Light everything under one or more tree paths, or nothing. */
  light(path: string | readonly string[] | null): void;
  /** The faces under a point of the canvas, nearest first, of the leaves shown. */
  facesAt(x: number, y: number): CadHit[];
  /** Look along ``direction`` at ``around``, or at the whole part; ``up``, or the way up it had.
   * ``up`` holds only with the trackball: an orbit control keeps +Z up, whatever is asked. */
  lookFrom(direction: PmiVec, around?: THREE.Box3, up?: PmiVec): void;
  /** Show or hide the node at a path. */
  show(path: string, visible: boolean): void;
  /** The viewer's transparency, on or off. */
  transparent(on: boolean): void;
  /** Replace leaves (or add them) under a group, by name: how a face changes colour. A group
   * is made when it is first given leaves, and taken away when it has none left. */
  replace(group: string | null, ...leaves: CadLeaf[]): void;
  /** The canvas's size in CSS pixels, and the element over it to lay overlays in. */
  size(): [number, number];
  readonly overlayHost: HTMLElement;
  /** Called every frame while the view is open. */
  onFrame(listener: () => void): void;
  /** Look along a direction at the whole part (and ``also`` if given), or along the current one. */
  home(direction: PmiVec, also?: THREE.Box3 | null): void;
  /** Look at the whole part, and ``also`` if given, along the current direction. */
  fit(also?: THREE.Box3 | null): void;
  /** Turn the view about the line of sight, in quarter turns. */
  roll(quarters: number): void;
  /** Move in (under 1) or out (over 1). */
  dolly(factor: number): void;
  /** Whether the person has moved the view since it opened. */
  moved(): boolean;
  /** What the orientation cube frames besides the part when it turns the view: the drawn
   * callouts, say. */
  alsoFrame(also: () => THREE.Box3 | null): void;
  /** The viewer's toolbar and its tools panel, shown or hidden (see ``tools``). */
  showTools(on: boolean): void;
  /** The tree opened one level again, its nodes closed. */
  collapse(): void;
  dispose(): void;
}

const ROOT = 'Part';
/** The orientation cube's side in CSS pixels, on a wide view and a narrow one, and its place:
 * at the canvas's top right, below the viewer's toolbar and its tree's filter. */
const CUBE_PX = 84;
const CUBE_PX_NARROW = 64;
const CUBE_RIGHT = 10;
const CUBE_TOP = 64;
/** A node's path: the viewer's tree finds what to show, hide or light by names. */
export const pathOf = (...names: string[]) => `/${[ROOT, ...names].join('/')}`;
const isGroup = (node: CadNode): node is CadGroup => 'leaves' in node;

/** A leaf's faces as one triangle soup, each face its own vertices, and each triangle's face. */
function soupOf(leaf: CadLeaf) {
  const place = leaf.place ?? ((p: [number, number, number]) => p);
  const positions: number[] = [];
  const triangles: number[] = [];
  const perFace: number[] = [];
  const faceOf: number[] = [];
  for (const id of leaf.faces) {
    const [start, count] = leaf.mesh.faces[id] ?? [0, 0];
    const local = new Map<number, number>();
    for (let k = 3 * start; k < 3 * (start + count); k++) {
      const v = leaf.mesh.indices[k]!;
      let i = local.get(v);
      if (i === undefined) {
        i = positions.length / 3;
        local.set(v, i);
        positions.push(...place([leaf.mesh.positions[3 * v]!, leaf.mesh.positions[3 * v + 1]!, leaf.mesh.positions[3 * v + 2]!]));
      }
      triangles.push(i);
    }
    perFace.push(count);
    for (let t = 0; t < count; t++) faceOf.push(id);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(triangles);
  geometry.computeVertexNormals();
  geometry.computeBoundingSphere();
  return { geometry, positions, triangles, perFace, faceOf };
}

/** A leaf in three-cad-viewer's flat format. */
function shapeOf(leaf: CadLeaf, soup: ReturnType<typeof soupOf>) {
  const place = leaf.place ?? ((p: [number, number, number]) => p);
  const edges: number[] = [];
  const perEdge: number[] = [];
  if (leaf.edges) {
    // A polyline of n points is n - 1 segments, each written as its two end points.
    for (const line of leaf.mesh.edges) {
      const n = Math.floor(line.length / 3);
      if (n < 2) continue;
      for (let i = 0; i + 1 < n; i++) {
        edges.push(...place([line[3 * i]!, line[3 * i + 1]!, line[3 * i + 2]!]), ...place([line[3 * i + 3]!, line[3 * i + 4]!, line[3 * i + 5]!]));
      }
      perEdge.push(n - 1);
    }
  }
  return {
    vertices: soup.positions, normals: Array.from(soup.geometry.getAttribute('normal').array as Float32Array),
    triangles: soup.triangles, triangles_per_face: soup.perFace,
    edges, segments_per_edge: perEdge, obj_vertices: [],
    face_types: soup.perFace.map(() => 0), edge_types: perEdge.map(() => 0),
  };
}

/**
 * ``nodes`` drawn in ``container``, sized to it. ``onPick`` hears the path of a node picked in
 * the viewer's tree (or a face double-clicked on its canvas). The viewer's own bounding box for
 * what was picked is never shown.
 */
export function createCadView(
  container: HTMLElement, nodes: CadNode[],
  { onPick, control = 'orbit', tools = true }: { onPick?: (path: string) => void; control?: 'orbit' | 'trackball';
    /** Whether the viewer shows its toolbar and tools panel; ``showTools`` changes it later. */ tools?: boolean } = {},
): CadView {
  const box = new THREE.Box3();
  // Each leaf drawn, by path, with what a ray needs to find its faces.
  const drawn = new Map<string, { mesh: THREE.Mesh; faceOf: number[] }>();
  // The groups in the tree, by path.
  const made = new Set<string>();
  const part = (leaf: CadLeaf, ...names: string[]) => {
    if (!leaf.faces.length) {
      // The mesh's edges alone: a leaf that stays while faces move between the others.
      const lines = shapeOf(leaf, soupOf(leaf));
      // Drawn, so it can be replaced, with nothing for a ray to find.
      drawn.set(pathOf(...names, leaf.name), { mesh: new THREE.Mesh(new THREE.BufferGeometry()), faceOf: [] });
      for (let i = 0; i < lines.edges.length; i += 3) box.expandByPoint(new THREE.Vector3(lines.edges[i], lines.edges[i + 1], lines.edges[i + 2]));
      return {
        version: 3, name: leaf.name, id: pathOf(...names, leaf.name), type: 'edges',
        shape: { ...lines, vertices: [], normals: [], triangles: [], triangles_per_face: [], face_types: [] },
        state: [3, 1], color: leaf.colour, alpha: 1, width: 1, loc: null, accuracy: null, bb: null,
      };
    }
    const soup = soupOf(leaf);
    const path = pathOf(...names, leaf.name);
    drawn.set(path, { mesh: new THREE.Mesh(soup.geometry, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide })), faceOf: soup.faceOf });
    for (let i = 0; i < soup.positions.length; i += 3) box.expandByPoint(new THREE.Vector3(soup.positions[i], soup.positions[i + 1], soup.positions[i + 2]));
    return {
      version: 3, name: leaf.name, id: path, type: 'shapes', subtype: 'faces', shape: shapeOf(leaf, soup),
      state: [1, leaf.edges ? 1 : 3], color: leaf.colour, alpha: leaf.alpha ?? 1, renderback: true, loc: null, accuracy: null, bb: null,
    };
  };
  const parts = nodes.flatMap((node): unknown[] => {
    const shown = (leaf: CadLeaf) => leaf.faces.length > 0 || leaf.edges === true;
    if (!isGroup(node)) return shown(node) ? [part(node)] : [];
    const leaves = node.leaves.filter(shown).map(leaf => part(leaf, node.name));
    if (leaves.length) made.add(pathOf(node.name));
    return leaves.length ? [{ version: 3, name: node.name, id: pathOf(node.name), loc: null, parts: leaves }] : [];
  });
  const bb = { xmin: box.min.x, xmax: box.max.x, ymin: box.min.y, ymax: box.max.y, zmin: box.min.z, zmax: box.max.z };
  const shapes = { version: 3, name: ROOT, id: pathOf(), loc: null, parts, bb } as unknown as Shapes;

  // Hidden by a class, not by the viewer's own ``tools: false``: with that, hovering a node in its
  // tree (how ``light`` lights a pick) no longer lights its faces on the part.
  container.classList.toggle('cad-tools-hidden', !tools);
  const size = () => [Math.max(container.clientWidth, 200), Math.max(container.clientHeight, 200)] as const;
  const [width, height] = size();
  // Studio mode is off: it fetches its lighting from polyhaven.org, which the CSP refuses.
  const display = new Display(container, {
    cadWidth: width, height, treeWidth: 260, glass: true, theme: 'light', tools: true,
    pinning: false, studioTool: false, zscaleTool: false, explodeTool: false,
    measureTools: true, selectTool: true, zebraTool: true, externalMeasurementBackend: false,
  });
  // Lit softly enough that a face's colour still reads.
  const options = { target: box.getCenter(new THREE.Vector3()).toArray(), up: 'Z' as const, control };
  const viewer: Viewer = new Viewer(display, options, change => {
    const picked = (change as { lastPick?: { new?: { path: string; name: string } | null } }).lastPick?.new;
    if (!picked) return;
    // Never the viewer's box for what was picked: it reads as part of the model. It draws the
    // box after it says what was picked.
    setTimeout(() => { viewer.removeLastBbox(); viewer.update(true, false); });
    onPick?.(`${picked.path}/${picked.name}`);
  });
  viewer.render(shapes, {
    ambientIntensity: 0.55, directIntensity: 0.7, metalness: 0.04, roughness: 0.6,
    edgeColor: 0x4a5058, defaultOpacity: 0.5, normalLen: 0,
  }, options);
  // The orientation cube says which way the view faces; the viewer's XYZ marker in the corner
  // is off by default (the toolbar's axes button still shows the axes on the part).
  // The viewer builds its marker as it renders, so it is hidden each frame, not once.
  const hideMarker = () => { try { viewer.orientationMarker.setVisible(false); } catch { /* not rendered yet */ } };
  hideMarker();
  // The tree opened one level, its nodes closed. Set once the caller has set up what is shown:
  // the viewer's tree closes again as nodes are first shown or hidden.
  // On a narrow view (a phone), its tools and tree start folded too: they would cover half the
  // part. Its Tools toggle opens them.
  const opened = setTimeout(() => {
    viewer.collapseNodes(CollapseState.ROOT, false);
    if (container.clientWidth < 700) viewer.showToolsPanel(false);
  });

  const canvas = viewer.getCanvas() as HTMLElement;
  // The canvas takes what the container has left after the viewer's own toolbar, margins and
  // borders, measured as drawn.
  const fit = () => {
    const root = container.firstElementChild as HTMLElement | null;
    let [chromeX, chromeY] = [0, 0];
    if (root) {
      const outer = root.getBoundingClientRect();
      const style = getComputedStyle(root);
      const inner = canvas.getBoundingClientRect();
      chromeX = Math.max(0, outer.width + parseFloat(style.marginLeft) + parseFloat(style.marginRight) - inner.width);
      chromeY = Math.max(0, outer.height + parseFloat(style.marginTop) + parseFloat(style.marginBottom) - inner.height);
    }
    const [w, h] = size();
    viewer.resizeCadView(Math.max(w - chromeX, 200), 260, Math.max(h - chromeY, 160), true);
  };
  fit();
  const resized = new ResizeObserver(fit);
  resized.observe(container);
  const hidden = new Set<string>();
  const ray = new THREE.Raycaster();
  // Overlays sit over the canvas, in its parent, sized to it.
  const overlayHost = canvas.parentElement as HTMLElement;
  if (getComputedStyle(overlayHost).position === 'static') overlayHost.style.position = 'relative';
  let moved = false;
  const press = () => { moved = true; };
  canvas.addEventListener('pointerdown', press);
  canvas.addEventListener('wheel', press, { passive: true });
  const frames: (() => void)[] = [];
  let ticking = 0;
  const tick = () => { hideMarker(); for (const f of frames) f(); ticking = requestAnimationFrame(tick); };
  const vector = (v: readonly number[]) => new THREE.Vector3(v[0], v[1], v[2]);

  // The orientation cube, on a small canvas of its own at the canvas's top right, turned with the
  // viewer's camera each frame. A face, edge or corner clicked (or tapped) turns the view to look
  // from there; the rest of the viewer's canvas is left to the viewer.
  const cube = createOrientationCube();
  const cubeInk = new THREE.WebGLRenderer({ alpha: true, antialias: true });
  cubeInk.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  const cubeCanvas = cubeInk.domElement;
  cubeCanvas.className = 'cad-view-cube';
  cubeCanvas.title = 'Click a face, edge or corner to look from there';
  cubeCanvas.style.cssText = 'position:absolute;z-index:5;touch-action:none';
  overlayHost.append(cubeCanvas);
  let cubeSide = 0;
  let alsoFramed: () => THREE.Box3 | null = () => null;
  const cubeAt = (event: PointerEvent) => {
    const r = cubeCanvas.getBoundingClientRect();
    return cube.at(event.clientX - r.left, event.clientY - r.top, r.width);
  };
  let cubePress: { x: number; y: number } | null = null;
  const onCubeDown = (event: PointerEvent) => {
    if (!event.isPrimary || event.button !== 0) return;
    cubePress = { x: event.clientX, y: event.clientY };
  };
  const onCubeUp = (event: PointerEvent) => {
    const p = cubePress;
    cubePress = null;
    if (!p || Math.hypot(event.clientX - p.x, event.clientY - p.y) > 4) return;
    const direction = cubeAt(event);
    if (!direction) return;
    moved = true;
    // Z up, unless looking along it; then the back of the part is at the top.
    const up: PmiVec = Math.abs(direction[2]) / Math.hypot(...direction) > 0.9 ? [0, 1, 0] : [0, 0, 1];
    const also = alsoFramed();
    view.lookFrom(direction, also ? box.clone().union(also) : undefined, up);
  };
  const onCubeMove = (event: PointerEvent) => {
    const direction = event.pointerType === 'touch' ? null : cubeAt(event);
    cube.mark(direction);
    cubeCanvas.style.cursor = direction ? 'pointer' : '';
    // Where the pointer is, for a test to find a face by.
    cubeCanvas.dataset['direction'] = direction?.join(',') ?? '';
  };
  const onCubeLeave = () => { cubePress = null; cube.mark(null); };
  cubeCanvas.addEventListener('pointerdown', onCubeDown);
  cubeCanvas.addEventListener('pointerup', onCubeUp);
  cubeCanvas.addEventListener('pointermove', onCubeMove);
  cubeCanvas.addEventListener('pointerleave', onCubeLeave);
  cubeCanvas.addEventListener('pointercancel', onCubeLeave);
  const cubeQuaternion = new THREE.Quaternion();
  frames.push(() => {
    const r = canvas.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const side = r.width < 500 ? CUBE_PX_NARROW : CUBE_PX;
    if (side !== cubeSide) {
      cubeSide = side;
      cubeInk.setSize(side, side);
    }
    cubeCanvas.style.left = `${canvas.offsetLeft + r.width - side - CUBE_RIGHT}px`;
    cubeCanvas.style.top = `${canvas.offsetTop + CUBE_TOP}px`;
    cube.follow(cubeQuaternion.fromArray(viewer.getCameraQuaternion()));
    cubeInk.render(cube.scene, cube.camera);
  });
  ticking = requestAnimationFrame(tick);
  const heading = () => vector(viewer.getCameraPosition()).sub(vector(viewer.getCameraTarget())).normalize();
  // The node in hand stays lit: the viewer's own pointer hover, on its canvas or leaving a row
  // of its tree, clears the same highlight, so it is put back after each move. Over the tree,
  // the viewer's hover of a row is left to show.
  let lit: string | null = null;
  let unlightGroup = () => {};
  let frame = 0;
  const relight = (event: Event) => {
    if ((event.target as Element | null)?.closest?.('.tcv_cad_tree')) return;
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => viewer.handleTreeHover(lit));
  };
  for (const type of ['pointermove', 'pointerleave', 'pointerup']) container.addEventListener(type, relight, true);

  const view: CadView = {
    viewer,
    canvas,
    get camera() { return viewer.camera.getCamera() as unknown as THREE.Camera; },
    diagonal: box.getSize(new THREE.Vector3()).length(),
    light(path) {
      unlightGroup();
      unlightGroup = () => {};
      lit = typeof path === 'string' ? path : path?.length === 1 ? path[0] : null;
      viewer.handleTreeHover(lit);
      if (Array.isArray(path) && path.length > 1 && viewer.nestedGroup.highlight) {
        unlightGroup = lightGroup(viewer.nestedGroup.highlight, path);
        viewer.update(true, false);
      }
    },
    facesAt(x, y) {
      const rect = canvas.getBoundingClientRect();
      ray.setFromCamera(new THREE.Vector2((2 * x) / rect.width - 1, 1 - (2 * y) / rect.height), viewer.camera.getCamera() as unknown as THREE.Camera);
      const hits: (CadHit & { distance: number })[] = [];
      for (const [path, { mesh, faceOf }] of drawn) {
        if ([...hidden].some(h => path === h || path.startsWith(`${h}/`))) continue;
        for (const hit of ray.intersectObject(mesh, false)) {
          if (hit.faceIndex != null) hits.push({ path, face: faceOf[hit.faceIndex]!, distance: hit.distance });
        }
      }
      return hits.sort((a, b) => a.distance - b.distance).map(({ path, face }) => ({ path, face }));
    },
    lookFrom(direction, around, upward) {
      const target = (around ?? box).getCenter(new THREE.Vector3());
      const span = (around ?? box).getSize(new THREE.Vector3()).length();
      // Placed outright, at the distance the camera keeps: a relative placement is from the
      // viewer's own idea of the target, which the trackball does not keep up to date.
      const distance = vector(viewer.getCameraPosition()).distanceTo(vector(viewer.getCameraTarget()));
      const eye = target.clone().addScaledVector(new THREE.Vector3(...direction).normalize(), distance);
      viewer.setCameraTarget(target.toArray() as [number, number, number], false);
      viewer.setCameraPosition(eye.toArray() as [number, number, number], false, false);
      // Aimed at the target, with the way up asked for or the way up it had (or Z, if looking
      // along it): the trackball does not turn the camera to its target by itself.
      const turned = new THREE.Quaternion(...viewer.getCameraQuaternion());
      let up = upward ? new THREE.Vector3(...upward).normalize() : new THREE.Vector3(0, 1, 0).applyQuaternion(turned);
      const along = target.clone().sub(eye).normalize();
      if (Math.abs(up.dot(along)) > 0.999) up = new THREE.Vector3(0, 0, 1);
      const aim = new THREE.Matrix4().lookAt(eye, target, up);
      viewer.setCameraQuaternion(new THREE.Quaternion().setFromRotationMatrix(aim).toArray() as [number, number, number, number], false);
      viewer.setCameraZoom(Math.min(8, Math.max(0.2, box.getSize(new THREE.Vector3()).length() / Math.max(span, 1e-6))), false);
    },
    show(path, visible) {
      if (visible) hidden.delete(path); else hidden.add(path);
      viewer.setState(path, visible ? [1, 1] : [0, 0]);
    },
    transparent(on) { viewer.setTransparent(on); },
    size() { const r = canvas.getBoundingClientRect(); return [r.width, r.height]; },
    overlayHost,
    onFrame(listener) { frames.push(listener); },
    home(direction, also) { this.lookFrom(direction, also ? box.clone().union(also) : undefined); },
    fit(also) { this.lookFrom(heading().toArray() as unknown as PmiVec, also ? box.clone().union(also) : undefined); },
    roll(quarters) {
      const q = new THREE.Quaternion(...viewer.getCameraQuaternion());
      q.multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), (quarters * Math.PI) / 2));
      viewer.setCameraQuaternion(q.toArray() as [number, number, number, number], false);
    },
    dolly(factor) { viewer.setCameraZoom(Math.min(40, Math.max(0.05, viewer.getCameraZoom() / factor)), false); },
    moved: () => moved,
    alsoFrame(also) { alsoFramed = also; },
    showTools(on) { container.classList.toggle('cad-tools-hidden', !on); fit(); },
    collapse() { viewer.collapseNodes(CollapseState.ROOT, false); },
    replace(group, ...leaves) {
      const names = group === null ? [] : [group];
      const groupPath = pathOf(...names);
      const has = (path: string) => [...drawn.keys()].some(p => p.startsWith(`${path}/`));
      for (const leaf of leaves) {
        const path = pathOf(...names, leaf.name);
        if (drawn.has(path)) { viewer.removePart(path, { skipBounds: true }); drawn.get(path)!.mesh.geometry.dispose(); drawn.delete(path); }
        if (!leaf.faces.length && !leaf.edges) continue;
        if (group !== null && !made.has(groupPath)) {
          // A group the tree does not have yet: added with its first leaf, its ids relative to
          // the root, as the viewer prefixes them.
          const shape = part(leaf, ...names) as { id: string };
          shape.id = `/${group}/${leaf.name}`;
          viewer.addPart(pathOf(), { version: 3, name: group, id: `/${group}`, loc: null, parts: [shape] } as unknown as Shapes, { skipBounds: true });
          made.add(groupPath);
        } else {
          viewer.addPart(groupPath, part(leaf, ...names) as unknown as Shapes, { skipBounds: true });
        }
      }
      if (group !== null && !has(groupPath) && made.delete(groupPath)) viewer.removePart(groupPath, { skipBounds: true });
      // Once for the lot: the viewer's bounds, clipping and tree.
      viewer.updateBounds();
    },
    dispose() {
      cancelAnimationFrame(ticking);
      frames.length = 0;
      canvas.removeEventListener('pointerdown', press);
      canvas.removeEventListener('wheel', press);
      cubeCanvas.removeEventListener('pointerdown', onCubeDown);
      cubeCanvas.removeEventListener('pointerup', onCubeUp);
      cubeCanvas.removeEventListener('pointermove', onCubeMove);
      cubeCanvas.removeEventListener('pointerleave', onCubeLeave);
      cubeCanvas.removeEventListener('pointercancel', onCubeLeave);
      cube.dispose();
      cubeInk.dispose();
      clearTimeout(opened);
      cancelAnimationFrame(frame);
      for (const type of ['pointermove', 'pointerleave', 'pointerup']) container.removeEventListener(type, relight, true);
      resized.disconnect();
      for (const { mesh } of drawn.values()) mesh.geometry.dispose();
      unlightGroup();
      viewer.dispose();
      container.replaceChildren();
    },
  };
  return view;
}
