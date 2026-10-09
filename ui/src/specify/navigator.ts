// Camera, lights, tumbling, framing and the orientation cube.
//
// Ported from pmi-assist-viewer (picker/navigator.js), which vendors
// step-pmi-viewer's page script (template.html at 63910de): a factory over one
// container, a press that stays put is a click, and here a dispose() so React
// can mount and unmount it.
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

import type { PmiVec } from '../types/pmi';

const HOME: PmiVec = [0.8, -1.0, 0.62];
const CUBE_PX = 84;
const CUBE_EDGE = 14;
const CUBE_SIDE = 1.7;
const HALF = CUBE_SIDE / 2;
const CUBE_EDGE_BAND = 0.62;
const FLIGHT_MS = 340;
const TURN = 2 * Math.PI; // a full turn across the shorter side of the container
const FIT_AIR = 1.06;
/** Room for a label's text past its point, as a share of the part's diagonal. */
const LABEL_ROOM = 0.06;
const CLICK_PX = 4;

// BoxGeometry orders its materials +X, -X, +Y, -Y, +Z, -Z. Z is up here.
const CUBE_FACES: readonly (readonly [string, PmiVec])[] = [
  ['RIGHT', [1, 0, 0]], ['LEFT', [-1, 0, 0]],
  ['BACK', [0, 1, 0]], ['FRONT', [0, -1, 0]],
  ['TOP', [0, 0, 1]], ['BOTTOM', [0, 0, -1]],
];

export interface Navigator {
  readonly scene: THREE.Scene;
  readonly camera: THREE.PerspectiveCamera;
  readonly diagonal: number;
  fit(): void;
  home(now?: boolean): void;
  roll(quarters: number): void;
  dolly(factor: number): void;
  lookFrom(direction: PmiVec): void;
  rayAt(x: number, y: number): THREE.Raycaster;
  size(): [number, number];
  onClick(listener: (x: number, y: number, event: PointerEvent) => void): void;
  onHover(listener: (x: number | null, y: number | null) => void): void;
  onFrame(listener: () => void): void;
  /** Whether the person has moved the view since it opened. */
  moved(): boolean;
  /** Points to keep in view besides the part when fitting: the labels' own. */
  alsoFrame(points: () => readonly THREE.Vector3[]): void;
  dispose(): void;
}

interface Flight {
  at: number;
  fromPosition: THREE.Vector3; toPosition: THREE.Vector3;
  fromTarget: THREE.Vector3; toTarget: THREE.Vector3;
  fromQuaternion: THREE.Quaternion; toQuaternion: THREE.Quaternion;
  up: THREE.Vector3;
}

/**
 * A navigable 3-D view in ``container``. ``panelRoom()`` is the width a panel
 * covers on the right, so the part is centred in what is left.
 */
export function createNavigator(
  container: HTMLElement,
  { bbox, panelRoom = () => 0 }: { bbox: readonly [PmiVec, PmiVec]; panelRoom?: () => number },
): Navigator {
  const low = new THREE.Vector3(...bbox[0]);
  const high = new THREE.Vector3(...bbox[1]);
  const diagonal = low.distanceTo(high) || 1;
  // What a fit frames: the part, and whatever else is drawn around it -- the
  // labels, which stand off the part and would otherwise be cut off at the edge.
  let alsoFramed: () => readonly THREE.Vector3[] = () => [];
  const framed = () => {
    const box = new THREE.Box3(low.clone(), high.clone());
    const extra = alsoFramed();
    for (const point of extra) box.expandByPoint(point);
    // A label's text runs on from its point, on screen rather than in the model.
    if (extra.length) box.expandByScalar(diagonal * LABEL_ROOM);
    return box;
  };
  const centreOf = () => framed().getCenter(new THREE.Vector3());

  const scene = new THREE.Scene();
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  container.appendChild(renderer.domElement);
  const size = (): [number, number] => [
    container.clientWidth || window.innerWidth, container.clientHeight || window.innerHeight,
  ];

  const camera = new THREE.PerspectiveCamera(36, 1, diagonal / 500, diagonal * 60);
  camera.position.copy(centreOf()).addScaledVector(new THREE.Vector3(...HOME), diagonal);
  camera.up.set(0, 0, 1);
  const controls = new OrbitControls(camera, container);
  controls.target.copy(centreOf());
  controls.enableDamping = true;
  controls.dampingFactor = 0.09;
  // OrbitControls keeps "up" up, which costs the part its roll; its pan and
  // zoom are kept and the rotation is taken over by the tumble below.
  controls.enableRotate = false;
  // Whether the person has moved the view themselves: a press, a wheel or a pinch.
  let moved = false;
  controls.addEventListener('start', () => { moved = true; });

  scene.add(new THREE.HemisphereLight(0xffffff, 0x8a9099, 2.5));
  const key = new THREE.DirectionalLight(0xffffff, 1.7);
  key.position.set(1, -1.3, 1.5);
  const fill = new THREE.DirectionalLight(0xffffff, 0.6);
  fill.position.set(-1.2, 0.8, -0.3);
  scene.add(key, fill);

  // --- orientation cube ---------------------------------------------------
  const cubeScene = new THREE.Scene();
  const cubeCamera = new THREE.OrthographicCamera(-1.5, 1.5, 1.5, -1.5, 0.1, 20);
  cubeCamera.position.set(0, 0, 6);
  const cubeMaterials = CUBE_FACES.map(([name]) => new THREE.MeshBasicMaterial({ map: faceTexture(name) }));
  const cube = new THREE.Mesh(new THREE.BoxGeometry(CUBE_SIDE, CUBE_SIDE, CUBE_SIDE), cubeMaterials);
  cubeScene.add(cube);
  const patch = new THREE.Mesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshBasicMaterial({ color: 0x2f6fb5, transparent: true, opacity: 0.3, depthWrite: false }),
  );
  patch.visible = false;
  cube.add(patch);

  function markCube(direction: PmiVec | null) {
    patch.visible = Boolean(direction);
    if (!direction) return;
    for (let axis = 0; axis < 3; axis++) {
      const out = direction[axis] ?? 0;
      patch.scale.setComponent(axis, out ? (1.02 - CUBE_EDGE_BAND) * HALF : 2 * CUBE_EDGE_BAND * HALF);
      patch.position.setComponent(axis, out ? out * ((1.02 + CUBE_EDGE_BAND) / 2) * HALF : 0);
    }
  }

  const cubeRay = new THREE.Raycaster();
  // Where on the cube a point lands, as a direction to stand in: a face, an
  // edge band or a corner. Coordinates are relative to the container.
  function cubeAt(x: number, y: number): PmiVec | null {
    const [, h] = size();
    const left = CUBE_EDGE;
    const top = h - CUBE_EDGE - CUBE_PX;
    if (x < left || x > left + CUBE_PX || y < top || y > top + CUBE_PX) return null;
    cubeRay.setFromCamera(
      new THREE.Vector2(((x - left) / CUBE_PX) * 2 - 1, -((y - top) / CUBE_PX) * 2 + 1), cubeCamera,
    );
    const hit = cubeRay.intersectObject(cube, false)[0];
    if (!hit) return null;
    const local = cube.worldToLocal(hit.point.clone()).multiplyScalar(2 / CUBE_SIDE);
    const direction = [local.x, local.y, local.z].map(
      (v) => (Math.abs(v) > CUBE_EDGE_BAND ? Math.sign(v) : 0),
    ) as unknown as PmiVec;
    if (direction.some(Boolean)) return direction;
    return CUBE_FACES[hit.face?.materialIndex ?? 0]?.[1] ?? null;
  }

  // --- flights -------------------------------------------------------------
  // Views are turned to, not cut to; orientation travels as a quaternion so
  // going home from upside down does not pass through a degenerate up vector.
  const pose = new THREE.Object3D();
  let flight: Flight | null = null;
  function poseAt(position: THREE.Vector3, up: THREE.Vector3, target: THREE.Vector3) {
    pose.up.copy(up);
    pose.position.copy(position);
    pose.lookAt(target);
    return pose.quaternion.clone();
  }
  function flyTo(position: THREE.Vector3, up: THREE.Vector3, target: THREE.Vector3, now = false) {
    if (now) {
      camera.up.copy(up);
      camera.position.copy(position);
      controls.target.copy(target);
      camera.lookAt(target);
      controls.update();
      flight = null;
      return;
    }
    flight = {
      at: performance.now(),
      fromPosition: camera.position.clone(), toPosition: position.clone(),
      fromTarget: controls.target.clone(), toTarget: target.clone(),
      fromQuaternion: camera.quaternion.clone(), toQuaternion: poseAt(position, up, target),
      up: up.clone(),
    };
  }
  function advanceFlight() {
    if (!flight) return;
    const part = Math.min(1, (performance.now() - flight.at) / FLIGHT_MS);
    const eased = part < 0.5 ? 2 * part * part : 1 - 2 * (1 - part) * (1 - part);
    controls.target.lerpVectors(flight.fromTarget, flight.toTarget, eased);
    camera.position.lerpVectors(flight.fromPosition, flight.toPosition, eased);
    camera.quaternion.slerpQuaternions(flight.fromQuaternion, flight.toQuaternion, eased);
    if (part < 1) return;
    camera.up.copy(flight.up);
    camera.position.copy(flight.toPosition);
    camera.lookAt(controls.target);
    flight = null;
  }

  // How far back the whole box is in view along ``heading``: each corner
  // against the field of view, from where it stands -- a corner nearer the
  // camera than the centre looks bigger, and a long flat part seen at a slant
  // is cropped if only the centre's distance is counted.
  function standoff(heading: THREE.Vector3, up: THREE.Vector3) {
    const across = new THREE.Vector3().crossVectors(heading, up).normalize();
    const over = new THREE.Vector3().crossVectors(across, heading).normalize();
    const box = framed();
    const centre = box.getCenter(new THREE.Vector3());
    const [w, h] = size();
    const clear = Math.max(w - panelRoom(), 1);
    const tanOver = Math.tan((camera.fov * Math.PI) / 360);
    const tanAcross = tanOver * (clear / h);
    let away = 0;
    for (let corner = 0; corner < 8; corner++) {
      const at = new THREE.Vector3(
        corner & 1 ? box.max.x : box.min.x, corner & 2 ? box.max.y : box.min.y,
        corner & 4 ? box.max.z : box.min.z,
      ).sub(centre);
      const reach = Math.max(Math.abs(at.dot(over)) / tanOver, Math.abs(at.dot(across)) / tanAcross);
      away = Math.max(away, at.dot(heading) + reach);
    }
    return away * FIT_AIR;
  }

  /**
   * Stand in ``direction`` from the part, the whole of it fitted to what is left
   * of the screen: turned to a feature or a side, the part is never cropped.
   */
  function lookFrom(direction: PmiVec) {
    const heading = new THREE.Vector3(...direction).normalize();
    const up = new THREE.Vector3(...(Math.abs(heading.z) > 0.9 ? [0, 1, 0] : [0, 0, 1]) as [number, number, number]);
    const centre = centreOf();
    flyTo(centre.clone().addScaledVector(heading, standoff(heading, up)), up, centre);
  }
  function fit() {
    const centre = centreOf();
    const heading = camera.position.clone().sub(controls.target).normalize();
    const over = new THREE.Vector3().setFromMatrixColumn(camera.matrix, 1);
    flyTo(centre.clone().addScaledVector(heading, standoff(heading, over)), over, centre);
  }
  function home(now = false) {
    const heading = new THREE.Vector3(...HOME).normalize();
    const up = new THREE.Vector3(0, 0, 1);
    const centre = centreOf();
    flyTo(centre.clone().addScaledVector(heading, standoff(heading, up)), up, centre, now);
  }
  function roll(quarters: number) {
    const heading = camera.position.clone().sub(controls.target).normalize();
    const over = new THREE.Vector3().setFromMatrixColumn(camera.matrix, 1)
      .applyAxisAngle(heading, (quarters * Math.PI) / 2);
    flyTo(camera.position.clone(), over, controls.target.clone());
  }
  function dolly(factor: number) {
    const offset = camera.position.clone().sub(controls.target);
    const length = Math.max(diagonal * 0.02, offset.length() * factor);
    camera.position.copy(controls.target).add(offset.setLength(length));
    controls.update();
  }

  // --- pointer: tumble, cube, and clicks -------------------------------------
  // A press that stays within a few pixels is a click; anything else is a drag
  // that turns the part. Clicks on the cube snap the view; others go to listeners.
  const touching = new Set<number>();
  const clickListeners: ((x: number, y: number, event: PointerEvent) => void)[] = [];
  const hoverListeners: ((x: number | null, y: number | null) => void)[] = [];
  const frameListeners: (() => void)[] = [];
  let tumbling: { x: number; y: number; id: number } | null = null;
  let press: { x: number; y: number; at: [number, number] } | null = null;
  const local = (event: PointerEvent): [number, number] => {
    const rect = container.getBoundingClientRect();
    return [event.clientX - rect.left, event.clientY - rect.top];
  };
  const tumbleQ = new THREE.Quaternion();
  const tumbleAxis = new THREE.Vector3();

  const onDown = (event: PointerEvent) => {
    moved = true;
    touching.add(event.pointerId);
    if (touching.size > 1) {
      tumbling = null;
      press = null;
      return;
    }
    if (!event.isPrimary || event.button !== 0) return;
    flight = null;
    tumbling = { x: event.clientX, y: event.clientY, id: event.pointerId };
    press = { x: event.clientX, y: event.clientY, at: local(event) };
  };
  const onLocalMove = (event: PointerEvent) => {
    const [x, y] = local(event);
    markCube(tumbling ? null : cubeAt(x, y));
    if (!tumbling) for (const f of hoverListeners) f(x, y);
  };
  const onLeave = () => {
    markCube(null);
    for (const f of hoverListeners) f(null, null);
  };
  const onMove = (event: PointerEvent) => {
    if (!tumbling || event.pointerId !== tumbling.id) return;
    const [w, h] = size();
    const span = Math.min(w, h);
    const dx = ((event.clientX - tumbling.x) / span) * TURN;
    const dy = ((event.clientY - tumbling.y) / span) * TURN;
    tumbling.x = event.clientX;
    tumbling.y = event.clientY;
    tumbleQ.setFromAxisAngle(tumbleAxis.setFromMatrixColumn(camera.matrix, 1), -dx);
    tumbleQ.multiply(new THREE.Quaternion().setFromAxisAngle(
      tumbleAxis.setFromMatrixColumn(camera.matrix, 0), -dy,
    ));
    const offset = camera.position.clone().sub(controls.target).applyQuaternion(tumbleQ);
    camera.position.copy(controls.target).add(offset);
    camera.up.applyQuaternion(tumbleQ);
    camera.lookAt(controls.target);
  };
  const onEnd = (event: PointerEvent) => {
    touching.delete(event.pointerId);
    if (tumbling && event.pointerId === tumbling.id) tumbling = null;
    const p = press;
    press = null;
    if (!p || event.type !== 'pointerup') return;
    if (Math.hypot(event.clientX - p.x, event.clientY - p.y) > CLICK_PX) return;
    const [x, y] = p.at;
    const face = cubeAt(x, y);
    if (face) {
      lookFrom(face);
      return;
    }
    for (const f of clickListeners) f(x, y, event);
  };
  container.addEventListener('pointerdown', onDown);
  container.addEventListener('pointermove', onLocalMove);
  container.addEventListener('pointerleave', onLeave);
  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onEnd);
  window.addEventListener('pointercancel', onEnd);

  // --- size and loop ---------------------------------------------------------
  function resize() {
    if (!container.clientWidth || !container.clientHeight) return;
    const [w, h] = size();
    camera.aspect = w / h;
    camera.setViewOffset(w, h, panelRoom() / 2, 0, w, h);
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
  }
  const observer = new ResizeObserver(resize);
  observer.observe(container);
  resize();
  home(true);

  let frame = 0;
  const tick = () => {
    frame = requestAnimationFrame(tick);
    if (!container.clientWidth || !container.clientHeight) return;
    advanceFlight();
    controls.update();
    for (const f of frameListeners) f();
    renderer.render(scene, camera);
    const [w, h] = size();
    cube.quaternion.copy(camera.quaternion).invert();
    renderer.setScissorTest(true);
    renderer.setViewport(CUBE_EDGE, CUBE_EDGE, CUBE_PX, CUBE_PX);
    renderer.setScissor(CUBE_EDGE, CUBE_EDGE, CUBE_PX, CUBE_PX);
    renderer.autoClear = false;
    renderer.clearDepth();
    renderer.render(cubeScene, cubeCamera);
    renderer.autoClear = true;
    renderer.setScissorTest(false);
    renderer.setViewport(0, 0, w, h);
  };
  tick();

  const ray = new THREE.Raycaster();
  return {
    scene, camera, diagonal, fit, home, roll, dolly, lookFrom, size,
    alsoFrame: (points) => { alsoFramed = points; },
    moved: () => moved,
    /** A ray through container point (x, y), honouring the panel offset. */
    rayAt(x, y) {
      const [w, h] = size();
      ray.setFromCamera(new THREE.Vector2((x / w) * 2 - 1, -(y / h) * 2 + 1), camera);
      return ray;
    },
    onClick: (f) => { clickListeners.push(f); },
    onHover: (f) => { hoverListeners.push(f); },
    onFrame: (f) => { frameListeners.push(f); },
    dispose() {
      cancelAnimationFrame(frame);
      observer.disconnect();
      container.removeEventListener('pointerdown', onDown);
      container.removeEventListener('pointermove', onLocalMove);
      container.removeEventListener('pointerleave', onLeave);
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onEnd);
      window.removeEventListener('pointercancel', onEnd);
      controls.dispose();
      for (const material of cubeMaterials) {
        material.map?.dispose();
        material.dispose();
      }
      cube.geometry.dispose();
      patch.geometry.dispose();
      (patch.material as THREE.Material).dispose();
      renderer.dispose();
      renderer.domElement.remove();
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
  texture.anisotropy = 4;
  return texture;
}
