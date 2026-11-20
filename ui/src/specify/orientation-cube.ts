// The orientation cube: a labelled cube turned with the view, whose faces, edges and corners
// are directions to look from. Shared by navigator.ts (drawn in a corner of its own renderer)
// and cad-view.ts (drawn on a small canvas of its own over three-cad-viewer's).
import * as THREE from 'three';

import type { PmiVec } from '../types/pmi';

const CUBE_SIDE = 1.7;
const HALF = CUBE_SIDE / 2;
/** How far out from a face's centre (as a share of its half side) a point is on an edge band. */
const CUBE_EDGE_BAND = 0.62;

// BoxGeometry orders its materials +X, -X, +Y, -Y, +Z, -Z. Z is up here.
const CUBE_FACES: readonly (readonly [string, PmiVec])[] = [
  ['RIGHT', [1, 0, 0]], ['LEFT', [-1, 0, 0]],
  ['BACK', [0, 1, 0]], ['FRONT', [0, -1, 0]],
  ['TOP', [0, 0, 1]], ['BOTTOM', [0, 0, -1]],
];

export interface OrientationCube {
  /** The cube's scene and camera, to render into a square viewport. */
  readonly scene: THREE.Scene;
  readonly camera: THREE.OrthographicCamera;
  /** Turn the cube to show the model as a camera with this orientation sees it. */
  follow(cameraQuaternion: THREE.Quaternion): void;
  /** The direction to look from at (x, y) of a square viewport ``side`` pixels across: a face, an
   * edge or a corner; null off the cube. */
  at(x: number, y: number, side: number): PmiVec | null;
  /** Light the face, edge or corner of a direction, or none. */
  mark(direction: PmiVec | null): void;
  dispose(): void;
}

export function createOrientationCube(): OrientationCube {
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-1.5, 1.5, 1.5, -1.5, 0.1, 20);
  camera.position.set(0, 0, 6);
  const materials = CUBE_FACES.map(([name]) => new THREE.MeshBasicMaterial({ map: faceTexture(name) }));
  const cube = new THREE.Mesh(new THREE.BoxGeometry(CUBE_SIDE, CUBE_SIDE, CUBE_SIDE), materials);
  scene.add(cube);
  const patch = new THREE.Mesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshBasicMaterial({ color: 0x2f6fb5, transparent: true, opacity: 0.3, depthWrite: false }),
  );
  patch.visible = false;
  cube.add(patch);
  const ray = new THREE.Raycaster();

  return {
    scene, camera,
    follow(q) { cube.quaternion.copy(q).invert(); },
    at(x, y, side) {
      if (x < 0 || x > side || y < 0 || y > side) return null;
      ray.setFromCamera(new THREE.Vector2((x / side) * 2 - 1, -(y / side) * 2 + 1), camera);
      const hit = ray.intersectObject(cube, false)[0];
      if (!hit) return null;
      const local = cube.worldToLocal(hit.point.clone()).multiplyScalar(2 / CUBE_SIDE);
      const direction = [local.x, local.y, local.z].map(
        (v) => (Math.abs(v) > CUBE_EDGE_BAND ? Math.sign(v) : 0),
      ) as unknown as PmiVec;
      if (direction.some(Boolean)) return direction;
      return CUBE_FACES[hit.face?.materialIndex ?? 0]?.[1] ?? null;
    },
    mark(direction) {
      patch.visible = Boolean(direction);
      if (!direction) return;
      for (let axis = 0; axis < 3; axis++) {
        const out = direction[axis] ?? 0;
        patch.scale.setComponent(axis, out ? (1.02 - CUBE_EDGE_BAND) * HALF : 2 * CUBE_EDGE_BAND * HALF);
        patch.position.setComponent(axis, out ? out * ((1.02 + CUBE_EDGE_BAND) / 2) * HALF : 0);
      }
    },
    dispose() {
      for (const material of materials) {
        material.map?.dispose();
        material.dispose();
      }
      cube.geometry.dispose();
      patch.geometry.dispose();
      (patch.material as THREE.Material).dispose();
    },
  };
}

function faceTexture(text: string): THREE.CanvasTexture {
  const canvas = document.createElement('canvas');
  canvas.width = 128;
  canvas.height = 128;
  const g = canvas.getContext('2d');
  if (g) {
    g.fillStyle = '#e9edf1';
    g.fillRect(0, 0, 128, 128);
    g.strokeStyle = '#98a2ad';
    g.lineWidth = 5;
    g.strokeRect(2.5, 2.5, 123, 123);
    g.fillStyle = '#2b3138';
    g.font = '600 26px ui-sans-serif, -apple-system, sans-serif';
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    g.fillText(text, 64, 66);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  return texture;
}
