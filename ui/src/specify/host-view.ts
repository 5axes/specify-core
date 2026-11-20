// The view a host can give the components to draw the part in (docs/host-view.md): plain,
// JSON-serialisable data both ways. The package's own views are its default implementation
// (default-view.ts). Nothing here imports three.js or a viewer.
import type { PmiFace, PmiMesh } from '../types/pmi';

export type Vec3 = [number, number, number];
/** 16 numbers, column-major, as three.js's ``Matrix4.elements``. */
export type Mat4 = number[];
/**
 * A face: Specify's part index (``engine.parts()``) and the face's zero-based index in Specify's
 * numbering of that part. ``instance`` is reserved for assemblies; one part is shown at a time.
 */
export interface FaceRef { part: number; face: number; instance?: number }

/** How the host knows Specify's faces: by OCCT's ``MapShapes`` order (solid-only parts) or by STEP entity. */
export type FaceIds = 'mapshapes' | 'step-entity';

export interface HostViewInfo {
  faceIds: FaceIds;
  /** Identity if absent. */
  worldFromStep?: Mat4;
  /** false: StepDiff's "Before" ghost is not shown. */
  canDrawMeshes: boolean;
  /** A ``mapshapes`` host that can also match faces by STEP entity, switched by ``useFaceIds``. */
  stepEntities?: boolean;
}

export interface CameraState {
  /** World → camera. */
  view: Mat4;
  /** Camera → clip. */
  projection: Mat4;
  /** The camera's position, in the world. */
  eye: Vec3;
  /** The view's size in CSS pixels. */
  width: number; height: number;
}

/** What the pointer did over the view (the interface's ``PointerEvent``, named apart from the DOM's). */
export interface HostPointerEvent {
  type: 'hover' | 'leave' | 'click' | 'dblclick';
  /** CSS pixels from the view's top left. */
  x: number; y: number;
  /** The nearest visible face of a shown part, or of a mesh the package gave. */
  hit: FaceRef | null;
  /** Set when the hit is on a mesh the package gave: its id. */
  mesh?: string;
  button?: number; shift?: boolean; ctrl?: boolean; alt?: boolean; meta?: boolean;
}

export interface FaceStyle {
  faces: FaceRef[];
  /** A CSS colour. */
  colour: string;
  /** 1 = solid. */
  opacity?: number;
}

/** A face the package checks the host's numbering by: its kind, area and centroid, as Specify measures them. */
export interface FaceSample {
  face: number;
  /** The ``#N`` of the STEP ``ADVANCED_FACE`` it was read from, when known. */
  entity?: number;
  kind: string;
  area: number;
  centroid: Vec3;
}

export interface MeshOptions { transform?: Mat4; colour?: string; opacity?: number; edges?: boolean }
export interface Box { min: Vec3; max: Vec3 }

export interface HostView {
  info(): Promise<HostViewInfo>;
  // What is shown
  /** All others hidden. */
  showParts(parts: number[]): void;
  /** Replaces that layer's styles; layers stack in call order. */
  styleFaces(layer: string, styles: FaceStyle[]): void;
  clearStyles(layer: string): void;
  /** The whole part, for a see-through review. */
  setOpacity(opacity: number): void;
  addMesh(id: string, mesh: PmiMesh, opts?: MeshOptions): void;
  removeMesh(id: string): void;
  // Camera
  lookFrom(direction: Vec3, opts?: { up?: Vec3; frame?: FaceRef[] | Box }): void;
  /** The whole visible part, plus what the overlays need. */
  fit(opts?: { also?: Box }): void;
  home(): void;
  // Events
  /** Every change, and once on subscribe. */
  onCamera(listener: (camera: CameraState) => void): () => void;
  onPointer(listener: (event: HostPointerEvent) => void): () => void;
  /** The person turned or zoomed the view. */
  onUserMoved(listener: () => void): () => void;
  /** Where the view is, in the package's page. */
  rect(): Promise<{ left: number; top: number; width: number; height: number }>;
  dispose(): void;
  // Optional members, used when present
  /** Turn about the line of sight, in quarter turns (a toolbar button). */
  roll?(quarters: number): void;
  /** Move in (under 1) or out (over 1) (a toolbar button). */
  dolly?(factor: number): void;
  /** A brief highlight, animated the host's own way. */
  flash?(faces: FaceRef[]): void;
  /** Specify's faces of a part by STEP entity: sent with each part, whichever ``faceIds`` the host uses. */
  partFaces?(part: number, faces: { entities: readonly number[] }): void;
  /** For each sample, whether the host's face with that id (in ``faceIds``) matches it within ``tolerance``. */
  checkFaces?(part: number, faceIds: FaceIds, samples: FaceSample[], tolerance: number): Promise<boolean[]>;
  /** Number faces this way from now on (a ``stepEntities`` host, after a failed check). */
  useFaceIds?(faceIds: FaceIds): void;
}

/** How faces reach the host after the check on load: its numbering, meshes the package gives, or not at all. */
export type FaceRoute = FaceIds | 'meshes' | 'none';

/** Faces sampled for the check on load. */
const SAMPLES = 12;
/** A face matches within this share of the part's diagonal. */
export const FACE_TOLERANCE = 1e-6;

const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

/**
 * Up to ``count`` faces spread over the part, each with its kind, area and centroid. A face
 * without a measured area or centroid is left out.
 */
export function faceSamples(faces: readonly PmiFace[], entities?: readonly number[], count = SAMPLES): FaceSample[] {
  const measured = faces.filter((f) => finite(f.area) && f.area > 0 && Array.isArray(f.centroid)
    && f.centroid.length === 3 && f.centroid.every(finite));
  const step = Math.max(1, measured.length / count);
  const out: FaceSample[] = [];
  for (let k = 0; k < measured.length && out.length < count; k += step) {
    const f = measured[Math.floor(k)]!;
    const entity = entities?.[f.id];
    out.push({ face: f.id, ...(entity !== undefined ? { entity } : {}), kind: f.kind, area: f.area, centroid: [...f.centroid] as Vec3 });
  }
  return out;
}

/**
 * The check on load: the host's numbering if its faces match the samples; else STEP entities, if
 * it can switch to them and they match; else meshes the package gives, if it can draw them;
 * else none, rather than highlighting the wrong faces. A host that cannot be checked is taken at
 * its word.
 */
export async function chooseFaceRoute(
  view: Pick<HostView, 'checkFaces' | 'useFaceIds'>, info: HostViewInfo, part: number, samples: FaceSample[], diagonal: number,
): Promise<FaceRoute> {
  if (!view.checkFaces || !samples.length) return info.faceIds;
  const tolerance = FACE_TOLERANCE * diagonal;
  const matches = async (ids: FaceIds) => {
    try { return (await view.checkFaces!(part, ids, samples, tolerance)).every(Boolean); } catch { return false; }
  };
  if (await matches(info.faceIds)) return info.faceIds;
  if (info.faceIds !== 'step-entity' && info.stepEntities && samples.every((s) => s.entity !== undefined)
    && await matches('step-entity')) {
    view.useFaceIds?.('step-entity');
    return 'step-entity';
  }
  return info.canDrawMeshes ? 'meshes' : 'none';
}

/** ``mesh`` with only ``faces`` left in it, still numbered as in ``mesh``. */
export function subMesh(mesh: PmiMesh, faces: readonly number[]): PmiMesh {
  const keep = new Set(faces);
  return { ...mesh, faces: mesh.faces.map((range, f) => (keep.has(f) ? range : [range[0], 0] as const)), edges: [] };
}

const PART_MESH = 'specify:part';
const layerMesh = (layer: string, i: number) => `specify:layer:${i}:${layer}`;

/**
 * ``view`` with faces sent by ``route``. Through the host's own numbering, as they are. Through
 * meshes, the part is hidden and given as a mesh, each style a mesh of its faces over it, and a
 * pick on them is the face it is. With none, styles are not sent. The route can change after the
 * check on load: the styles set so far are sent again the new way.
 */
export function withFaceRoute(view: HostView, part: number, mesh: PmiMesh): HostView & { route(route: FaceRoute): void } {
  let route: FaceRoute = 'step-entity';
  const layers = new Map<string, FaceStyle[]>();
  const sent = new Map<string, number>();
  const unsend = (layer: string) => {
    for (let i = 0; i < (sent.get(layer) ?? 0); i++) view.removeMesh(layerMesh(layer, i));
    sent.delete(layer);
  };
  const send = (layer: string, styles: FaceStyle[]) => {
    if (route === 'none') return;
    if (route !== 'meshes') { if (styles.length) view.styleFaces(layer, styles); else view.clearStyles(layer); return; }
    unsend(layer);
    styles.forEach((s, i) => view.addMesh(layerMesh(layer, i), subMesh(mesh, s.faces.filter((f) => f.part === part).map((f) => f.face)),
      { colour: s.colour, ...(s.opacity !== undefined ? { opacity: s.opacity } : {}) }));
    sent.set(layer, styles.length);
  };
  // Each member called on ``view`` itself: a host's view may be a class instance.
  return {
    info: () => view.info(),
    setOpacity: (opacity) => view.setOpacity(opacity),
    addMesh: (id, m, opts) => view.addMesh(id, m, opts),
    removeMesh: (id) => view.removeMesh(id),
    lookFrom: (direction, opts) => view.lookFrom(direction, opts),
    fit: (opts) => view.fit(opts),
    home: () => view.home(),
    onCamera: (listener) => view.onCamera(listener),
    onUserMoved: (listener) => view.onUserMoved(listener),
    rect: () => view.rect(),
    dispose: () => view.dispose(),
    ...(view.roll ? { roll: (quarters: number) => view.roll!(quarters) } : {}),
    ...(view.dolly ? { dolly: (factor: number) => view.dolly!(factor) } : {}),
    ...(view.flash ? { flash: (faces: FaceRef[]) => view.flash!(faces) } : {}),
    ...(view.partFaces ? { partFaces: (p: number, faces: { entities: readonly number[] }) => view.partFaces!(p, faces) } : {}),
    ...(view.checkFaces ? { checkFaces: (p: number, ids: FaceIds, samples: FaceSample[], tolerance: number) => view.checkFaces!(p, ids, samples, tolerance) } : {}),
    ...(view.useFaceIds ? { useFaceIds: (ids: FaceIds) => view.useFaceIds!(ids) } : {}),
    route(next) {
      if (next === route) return;
      // Taken back the old way, then sent the new way.
      for (const layer of layers.keys()) { if (route === 'meshes') unsend(layer); else if (route !== 'none') view.clearStyles(layer); }
      if (route === 'meshes') { view.removeMesh(PART_MESH); view.showParts([part]); }
      route = next;
      if (route === 'meshes') { view.showParts([]); view.addMesh(PART_MESH, mesh, { edges: true }); }
      for (const [layer, styles] of layers) send(layer, styles);
    },
    styleFaces(layer, styles) { layers.set(layer, styles); send(layer, styles); },
    clearStyles(layer) { layers.delete(layer); send(layer, []); },
    showParts(parts) { if (route !== 'meshes') view.showParts(parts); },
    onPointer(listener) {
      return view.onPointer((event) => {
        if (route !== 'meshes') { listener(event); return; }
        // Through meshes, only the package's own meshes are the part's faces.
        const ours = event.mesh === PART_MESH || event.mesh?.startsWith('specify:layer:');
        const { mesh: _mesh, ...rest } = event;
        listener(ours ? { ...rest, hit: event.hit && { ...event.hit, part } } : { ...rest, hit: null });
      });
    },
  };
}
