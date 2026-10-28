// A part in Bernhard Walter's three-cad-viewer, through its published API only: its tree,
// transparency, clipping and measuring, with what the editor needs on top. A tree node is
// lit with the viewer's own tree hover; the faces under the pointer are found by a ray
// against this file's copy of the meshes, so nothing of the editor's is drawn into its scene
// (the package carries its own three.js).
import * as THREE from 'three';
import { Display, Viewer, type Shapes } from 'three-cad-viewer';
import 'three-cad-viewer/css';

import type { PmiMesh, PmiVec } from '../types/pmi';

/** A leaf of the tree: some faces of a mesh, in one colour. */
export interface CadLeaf {
  /** Unique among its siblings. */
  name: string;
  mesh: PmiMesh;
  faces: readonly number[];
  colour: string;
  /** Opacity, 1 for a solid. */
  alpha?: number;
  /** The mesh's edges go with this leaf. */
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
  /** Light everything under a tree path, or nothing. */
  light(path: string | null): void;
  /** The faces under a point of the canvas, nearest first, of the leaves shown. */
  facesAt(x: number, y: number): CadHit[];
  /** Look along ``direction`` at ``around``, or at the whole part. */
  lookFrom(direction: PmiVec, around?: THREE.Box3): void;
  /** Show or hide the node at a path. */
  show(path: string, visible: boolean): void;
  /** The viewer's transparency, on or off. */
  transparent(on: boolean): void;
  dispose(): void;
}

const ROOT = 'Part';
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

/** ``nodes`` drawn in ``container``, sized to it. */
export function createCadView(container: HTMLElement, nodes: CadNode[]): CadView {
  const box = new THREE.Box3();
  // Each leaf drawn, by path, with what a ray needs to find its faces.
  const drawn = new Map<string, { mesh: THREE.Mesh; faceOf: number[] }>();
  const part = (leaf: CadLeaf, ...names: string[]) => {
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
    if (!isGroup(node)) return node.faces.length ? [part(node)] : [];
    const leaves = node.leaves.filter(leaf => leaf.faces.length).map(leaf => part(leaf, node.name));
    return leaves.length ? [{ version: 3, name: node.name, id: pathOf(node.name), loc: null, parts: leaves }] : [];
  });
  const bb = { xmin: box.min.x, xmax: box.max.x, ymin: box.min.y, ymax: box.max.y, zmin: box.min.z, zmax: box.max.z };
  const shapes = { version: 3, name: ROOT, id: pathOf(), loc: null, parts, bb } as unknown as Shapes;

  const size = () => [Math.max(container.clientWidth, 200), Math.max(container.clientHeight, 200)] as const;
  const [width, height] = size();
  // Studio mode is off: it fetches its lighting from polyhaven.org, which the CSP refuses.
  const display = new Display(container, {
    cadWidth: width, height, treeWidth: 220, glass: true, theme: 'light', tools: true,
    pinning: false, studioTool: false, zscaleTool: false, explodeTool: false,
    measureTools: true, selectTool: true, zebraTool: true, externalMeasurementBackend: false,
  });
  // The tree opened to its leaves; lit softly enough that a face's colour still reads.
  const options = { target: box.getCenter(new THREE.Vector3()).toArray(), up: 'Z' as const, collapse: 2 };
  const viewer = new Viewer(display, options, () => {});
  viewer.render(shapes, {
    ambientIntensity: 0.55, directIntensity: 0.7, metalness: 0.04, roughness: 0.6,
    edgeColor: 0x4a5058, defaultOpacity: 0.5, normalLen: 0,
  }, options);

  const canvas = viewer.getCanvas() as HTMLElement;
  const fit = new ResizeObserver(() => { const [w, h] = size(); viewer.resizeCadView(w, 220, h, true); });
  fit.observe(container);
  const hidden = new Set<string>();
  const ray = new THREE.Raycaster();

  return {
    viewer,
    canvas,
    get camera() { return viewer.camera.getCamera() as unknown as THREE.Camera; },
    diagonal: box.getSize(new THREE.Vector3()).length(),
    light(path) { viewer.handleTreeHover(path); },
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
    lookFrom(direction, around) {
      const target = (around ?? box).getCenter(new THREE.Vector3());
      const span = (around ?? box).getSize(new THREE.Vector3()).length();
      viewer.setCameraTarget(target.toArray() as [number, number, number], false);
      viewer.setCameraPosition(direction as unknown as [number, number, number], true, false);
      viewer.setCameraZoom(Math.min(8, Math.max(0.5, box.getSize(new THREE.Vector3()).length() / Math.max(span, 1e-6))), false);
    },
    show(path, visible) {
      if (visible) hidden.delete(path); else hidden.add(path);
      viewer.setState(path, visible ? [1, 1] : [0, 0]);
    },
    transparent(on) { viewer.setTransparent(on); },
    dispose() {
      fit.disconnect();
      for (const { mesh } of drawn.values()) mesh.geometry.dispose();
      viewer.dispose();
      container.replaceChildren();
    },
  };
}
