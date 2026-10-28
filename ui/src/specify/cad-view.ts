// A part in Bernhard Walter's three-cad-viewer (third_party/three-cad-viewer, built here against
// the editor's three.js): its tree, transparency, clipping and measuring, with the editor's
// own needs on top -- light chosen faces, find the face under the pointer, turn to a feature.
// Everything that reaches past three-cad-viewer's documented API is in this file, so an
// upgrade that moves it fails here (scripts/cad-view-browser.mjs) and nowhere else.
import * as THREE from 'three';
import { Display, Viewer } from 'three-cad-viewer';

import type { PmiMesh, PmiVec } from '../types/pmi';

/** A leaf of the tree: some faces of a mesh, in one colour. */
export interface CadLeaf {
  /** Unique within its group, shown in the tree. */
  name: string;
  mesh: PmiMesh;
  faces: readonly number[];
  colour: string;
  /** Opacity, 1 for a solid. */
  alpha?: number;
  /** Drawn through whatever is in front of it, for faces the part now hides. */
  xray?: boolean;
  /** The mesh's edges go with this leaf. */
  edges?: boolean;
  /** Where each point goes, for faces drawn somewhere other than where the mesh has them. */
  place?: (p: [number, number, number]) => [number, number, number];
}

export interface CadGroup { name: string; leaves: CadLeaf[] }

/** A face of a leaf: which group and leaf, and its id in the leaf's mesh. */
export interface CadFace { group: number; leaf: number; face: number }

export interface CadView {
  readonly viewer: Viewer;
  /** The part's camera, and the length of its diagonal, as look.ts asks of a view. */
  readonly camera: THREE.Camera;
  readonly diagonal: number;
  /** Light ``faces`` in ``colour``, everything else back to its own. */
  light(faces: readonly CadFace[], colour?: THREE.ColorRepresentation): void;
  /** The faces under a point of the canvas, nearest first: the one in front, then any of an x-ray leaf behind it. */
  facesAt(x: number, y: number): CadFace[];
  /** Look along ``direction`` at ``around``, or at the whole part. */
  lookFrom(direction: PmiVec, around?: THREE.Box3): void;
  /** Show or hide a group. */
  show(group: number, visible: boolean): void;
  /** The canvas, for pointer events in its own coordinates. */
  readonly canvas: HTMLCanvasElement;
  dispose(): void;
}

const ROOT = 'Part';
// A node's id is its path of names: the tree finds what to show or hide by names.
const pathOf = (...names: string[]) => `/${[ROOT, ...names].join('/')}`;

/** A leaf's tessellation in three-cad-viewer's flat format: each face its own vertices. */
function shapeOf(leaf: CadLeaf) {
  const { mesh, faces } = leaf;
  const place = leaf.place ?? ((p: [number, number, number]) => p);
  const positions: number[] = [];
  const triangles: number[] = [];
  const perFace: number[] = [];
  for (const id of faces) {
    const [start, count] = mesh.faces[id] ?? [0, 0];
    const local = new Map<number, number>();
    for (let k = 3 * start; k < 3 * (start + count); k++) {
      const v = mesh.indices[k]!;
      let i = local.get(v);
      if (i === undefined) {
        i = positions.length / 3;
        local.set(v, i);
        positions.push(...place([mesh.positions[3 * v]!, mesh.positions[3 * v + 1]!, mesh.positions[3 * v + 2]!]));
      }
      triangles.push(i);
    }
    perFace.push(count);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(triangles);
  geometry.computeVertexNormals();
  const normals = Array.from(geometry.getAttribute('normal').array as Float32Array);
  geometry.dispose();
  const edges: number[] = [];
  const perEdge: number[] = [];
  if (leaf.edges) {
    // A polyline of n points is n - 1 segments, each written as its two end points.
    for (const line of mesh.edges) {
      const n = Math.floor(line.length / 3);
      if (n < 2) continue;
      for (let i = 0; i + 1 < n; i++) {
        edges.push(...place([line[3 * i]!, line[3 * i + 1]!, line[3 * i + 2]!]), ...place([line[3 * i + 3]!, line[3 * i + 4]!, line[3 * i + 5]!]));
      }
      perEdge.push(n - 1);
    }
  }
  return {
    vertices: positions, normals, triangles, triangles_per_face: perFace,
    edges, segments_per_edge: perEdge, obj_vertices: [],
    face_types: perFace.map(() => 0), edge_types: perEdge.map(() => 0),
  };
}

/** The part as three-cad-viewer's tree, and its bounding box. */
function shapesOf(groups: CadGroup[]) {
  const box = new THREE.Box3();
  const parts = groups.map((group, g) => ({
    version: 3, name: group.name, id: pathOf(group.name), loc: null,
    parts: group.leaves.filter(leaf => leaf.faces.length).map(leaf => {
      const shape = shapeOf(leaf);
      for (let i = 0; i < shape.vertices.length; i += 3) box.expandByPoint(new THREE.Vector3(shape.vertices[i], shape.vertices[i + 1], shape.vertices[i + 2]));
      return {
        version: 3, name: leaf.name, id: pathOf(group.name, leaf.name),
        type: 'shapes', subtype: 'faces', shape, state: [1, leaf.edges ? 1 : 3],
        color: leaf.colour, alpha: leaf.alpha ?? 1, renderback: true, loc: null, accuracy: null, bb: null,
      };
    }),
  }));
  const bb = { xmin: box.min.x, xmax: box.max.x, ymin: box.min.y, ymax: box.max.y, zmin: box.min.z, zmax: box.max.z };
  return { shapes: { version: 3, name: ROOT, id: `/${ROOT}`, loc: null, parts, bb }, box };
}

/** ``groups`` drawn in ``container``, sized to it. */
export function createCadView(container: HTMLElement, groups: CadGroup[]): CadView {
  const { shapes, box } = shapesOf(groups);
  const size = () => [Math.max(container.clientWidth, 200), Math.max(container.clientHeight, 200)] as const;
  const [width, height] = size();
  const display = new Display(container, {
    cadWidth: width, height, treeWidth: 220, glass: true, theme: 'light', tools: true,
    pinning: false, studioTool: false, zscaleTool: false, explodeTool: false,
  });
  // The tree opened to its leaves; lit softly enough that a face's colour still reads.
  const options = { target: box.getCenter(new THREE.Vector3()).toArray(), up: 'Z', collapse: 2 };
  const viewer = new Viewer(display, options, () => {});
  viewer.render(shapes, {
    ambientIntensity: 0.55, directIntensity: 0.7, metalness: 0.04, roughness: 0.6,
    edgeColor: 0x4a5058, defaultOpacity: 0.5, normalLen: 0,
  }, options);

  // Each face's pick-buffer component, both ways, from three-cad-viewer's registry: face k of a
  // leaf is "<leaf path>/faces/faces_<k>", k its place in the leaf's faces.
  const ids = new Map<string, number>();
  const faceOfId = new Map<number, CadFace>();
  const kept = groups.map(group => group.leaves.map((leaf, l) => ({ leaf, l })).filter(({ leaf }) => leaf.faces.length));
  const leafAt = new Map(groups.flatMap((group, g) => kept[g]!.map(({ leaf, l }) => [pathOf(group.name, leaf.name), { g, l, leaf }] as const)));
  for (const info of viewer.nestedGroup.registry.entries()) {
    const m = info.topo === 'face' ? /^(.*)\/faces\/faces_(\d+)$/.exec(info.path) : null;
    const owner = m ? leafAt.get(m[1]!) : undefined;
    if (!m || !owner) continue;
    const face = { group: owner.g, leaf: owner.l, face: owner.leaf.faces[Number(m[2])]! };
    ids.set(`${face.group}:${face.leaf}:${face.face}`, info.id);
    faceOfId.set(info.id, face);
  }

  // A leaf drawn through the part: its faces skip the depth test, and stay that way when the
  // viewer's own transparency is toggled.
  const xrays = groups.flatMap((group, g) => kept[g]!.filter(({ leaf }) => leaf.xray)
    .map(({ leaf }) => viewer.nestedGroup.groups[pathOf(group.name, leaf.name)]));
  const xray = () => {
    for (const object of xrays) {
      const front = object?.front;
      if (!front) continue;
      front.material.depthTest = false;
      front.renderOrder = 1;
    }
  };
  xray();

  // The pick buffer sees only what is in front; an x-ray leaf is drawn through the part, so it
  // is also found by a ray, against its own faces where they are drawn.
  const rays = groups.flatMap((group, g) => kept[g]!.filter(({ leaf }) => leaf.xray).map(({ leaf, l }) => {
    const shape = shapeOf({ ...leaf, edges: false });
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.Float32BufferAttribute(shape.vertices, 3));
    geometry.setIndex(shape.triangles);
    const faceOf = shape.triangles_per_face.flatMap((n, k) => Array<number>(n).fill(leaf.faces[k]!));
    return { g, l, mesh: new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide })), faceOf };
  }));
  const ray = new THREE.Raycaster();

  const highlight = viewer.nestedGroup.highlight!;
  let lit: number[] = [];
  const hidden = new Set<number>();
  const canvas = viewer.renderer.domElement as HTMLCanvasElement;
  const fit = new ResizeObserver(() => { const [w, h] = size(); viewer.resizeCadView(w, 220, h, true); });
  fit.observe(container);

  return {
    viewer,
    canvas,
    get camera() { return viewer.camera.getCamera(); },
    diagonal: box.getSize(new THREE.Vector3()).length(),
    light(faces, colour = 0x1ec8ff) {
      for (const id of lit) highlight.setSelected(id, false);
      lit = faces.map(f => ids.get(`${f.group}:${f.leaf}:${f.face}`)).filter((id): id is number => id !== undefined);
      highlight.uniforms.uHighlightSelectedColor.value.set(colour);
      for (const id of lit) highlight.setSelected(id, true);
      xray();
      viewer.update(true, false);
    },
    facesAt(x, y) {
      const hit = viewer.idPicker?.pickAt(x, y);
      const front = hit ? faceOfId.get(hit.id) : undefined;
      const out = front ? [front] : [];
      const rect = canvas.getBoundingClientRect();
      ray.setFromCamera(new THREE.Vector2((2 * x) / rect.width - 1, 1 - (2 * y) / rect.height), viewer.camera.getCamera());
      for (const { g, l, mesh, faceOf } of rays) {
        if (hidden.has(g)) continue;
        for (const h of ray.intersectObject(mesh, false)) if (h.faceIndex != null) out.push({ group: g, leaf: l, face: faceOf[h.faceIndex]! });
      }
      return out;
    },
    lookFrom(direction, around) {
      const target = (around ?? box).getCenter(new THREE.Vector3());
      const span = (around ?? box).getSize(new THREE.Vector3()).length();
      viewer.setCameraTarget(target.toArray() as [number, number, number], false);
      viewer.setCameraPosition(direction as unknown as [number, number, number], true, false);
      viewer.setCameraZoom(Math.min(8, Math.max(0.5, box.getSize(new THREE.Vector3()).length() / Math.max(span, 1e-6))), false);
    },
    show(group, visible) {
      if (visible) hidden.delete(group); else hidden.add(group);
      const on = visible ? 1 : 0;
      viewer.setState(pathOf(groups[group]!.name), [on, on]);
    },
    dispose() {
      fit.disconnect();
      for (const { mesh } of rays) mesh.geometry.dispose();
      viewer.dispose(); container.replaceChildren();
    },
  };
}
