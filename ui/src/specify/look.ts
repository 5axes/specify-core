// Turning a part to show some of its faces: where to stand so they are in
// sight, square on to a plane, a little off the axis of a hole or shaft.
// Shared by the face picker and the /diff page.
import * as THREE from 'three';

import type { PmiMesh, PmiVec } from '../types/pmi';

/** What a look turns: a view's camera, the part's size, and a way to stand. */
export interface Viewpoint {
  readonly camera: THREE.Camera;
  readonly diagonal: number;
  lookFrom(direction: PmiVec, around?: THREE.Box3): void;
}

/** How much of the part a close look keeps around a feature, as a share of its diagonal. */
const CLOSE_CONTEXT = 0.35;
/** Points of a feature tested for being in sight. */
const SAMPLES = 24;
/** How far off its axis a hole or shaft is looked at: a deep one, a shallow one. */
const OFF_AXIS = [(12 * Math.PI) / 180, (30 * Math.PI) / 180] as const;
/** What a point seen edge on still counts for: it is in sight, just. */
const EDGE_ON = 0.05;
/** Other directions tried when the part hides a feature: faces, edges and corners of a cube. */
const AROUND: readonly PmiVec[] = [-1, 0, 1].flatMap((x) => [-1, 0, 1].flatMap((y) => [-1, 0, 1]
  .map((z): PmiVec => [x, y, z]))).filter(([x, y, z]) => x || y || z);

export interface LookOptions {
  /** The faces' mesh, the one their ids are in. */
  mesh: PmiMesh;
  /** What can hide them, as drawn. */
  occluder: THREE.Object3D;
  /** The face of ``mesh`` an occluder triangle is, if it is one: a face never hides itself. */
  faceOf: (triangle: number) => number | null;
  /** A face's kind and direction (a plane's normal, a hole's axis), where known. */
  faceInfo?: (face: number) => { kind: string; direction?: PmiVec } | undefined;
}

export type LookAt = (faces: readonly number[], options?: { close?: boolean }) => void;

/** Turns ``nav``'s view to ``faces``, close up with ``close``. */
export function createLook(nav: Viewpoint, { mesh, occluder, faceOf, faceInfo }: LookOptions): LookAt {
  const centre = new THREE.Vector3(...mesh.bbox[0]).add(new THREE.Vector3(...mesh.bbox[1])).multiplyScalar(0.5);

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
      const hit = sight.intersectObject(occluder, false)[0];
      const face = hit?.faceIndex != null ? faceOf(hit.faceIndex) : null;
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
      const f = faceInfo?.(id);
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
    return { direction: best.direction.toArray() as unknown as PmiVec, box };
  }

  return (faces, options) => {
    const view = bestView(faces);
    if (!view) return;
    if (!options?.close) { nav.lookFrom(view.direction); return; }
    // Close, but not so close the feature loses its place on the part.
    const around = view.box.clone();
    const size = around.getSize(new THREE.Vector3()).length();
    around.expandByScalar(Math.max(0, (CLOSE_CONTEXT * nav.diagonal - size) / 2));
    nav.lookFrom(view.direction, around);
  };
}
