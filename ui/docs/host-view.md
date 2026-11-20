# Host view: letting a host draw the 3D view

Status: **accepted for the interface** (Paul, 2026-11-20; Amy has not commented on specify-ui #10).
Specify 0.3.0 ships the interface with today's views behind it (see "In 0.3.0"). The overlay over a
host-owned camera and the conformance script wait for a real host to test against (see "Later").

## In 0.3.0

- The `HostView` plain-data interface below: show, style and fit faces; camera; pointer events; `rect()`.
- Today's views become its default implementation: `cad-view.ts` (three-cad-viewer) and
  `navigator.ts`/`picker.ts`. The components (`StepDiff`, `SpecifyReview`) are routed through the interface.
- Face identity per quiddity #310 (see Identity), with the check on load.
- One part at a time. `FaceRef.instance?` is reserved for assemblies.
- `three` as a peer dependency and `three-cad-viewer` as an optional peer, only if that falls out of the
  change without extra work. Otherwise packaging stays as it is.
- Tests: the existing browser suites pass on the default view, going through the interface.

## Later (when a real host exists to test against)

- Labels, leaders and drawn PMI as a layer over a host-owned camera. Until then `annotations.ts`,
  `drawn-layer.ts`, `diff-drawn.ts` and `review-view.ts` keep working as they do now, with the default view.
- The adapter conformance script.

## Not proposed

- A transport across an iframe (`postMessage`). Nobody has asked for one. The interface is plain,
  JSON-serialisable data, so a transport can be added if an embedding case appears.

## Why

`StepDiff` and `SpecifyReview` draw their own 3D view today: three-cad-viewer for StepDiff, AssemblyDiffView
and the read-only review (`src/specify/cad-view.ts`), and the package's own three.js navigator for the face
picker (`src/specify/navigator.ts`, `picker.ts`). three-cad-viewer's build contains its own three r184
(bernhard-42/three-cad-viewer#52). So a host that already has a 3D view ships a second viewer and a second
three, and its users see two kinds of view.

This proposal lets a host pass its own view. The components then:
- ask the host to show, colour and frame faces;
- hear from it what is under the pointer and where the camera is;
- keep drawing their own labels, drawn PMI and notes, in a layer over the host's view.

The package's current views become the default implementation of the same interface, used when the host
passes none.

## Who draws what

| | Host | Package |
|---|---|---|
| The part's faces and edges, lighting, background | yes | |
| Camera, controls, orientation cube or gizmo | yes | |
| Face colours and opacity, on request | yes | asks |
| Geometry the host does not have (e.g. StepDiff's "Before" version) | draws a mesh it is given | supplies it |
| Labels, leaders, drawn PMI, notes, the characteristics panel | | yes, in an overlay over the view |
| Which face is under the pointer | yes, reported | |
| Whether a label is hidden behind the part | | yes, against its own copy of the mesh |

The package keeps its own copy of each part's mesh (from the Specify engine) for occlusion tests and for
choosing view directions, so the host needs no ray-casting API beyond reporting picks.

## Dependencies after the change

- `three`: peer dependency, one copy, the host's (the overlays draw with it).
- `three-cad-viewer`: **optional** peer dependency, imported only by the default view. A host that passes its own
  view never loads it.

## Identity: parts and faces

- A **part** is Specify's part index (`engine.parts()`), the same in every call.
- A **face** is a **zero-based** index into that part's faces in Specify's numbering. quiddity #310 fixes
  that order as a depth-first walk of the part. It equals OpenCascade's `TopExp::MapShapes` order only for
  parts made of solids alone; otherwise faces join through `face_sources`, which give each face's STEP
  `ADVANCED_FACE` entity.
- **Adapters identify faces by STEP entity as the general route** (`faceIds: 'step-entity'`). They may use
  MapShapes order (`'mapshapes'`) only for solid-only parts. The check on load guards both.
- For a solid-only part, Specify's face order is OpenCascade's `TopExp::MapShapes` order on the part's
  own solid. haecceity (Specify's reader) follows that order and is
  tested face by face against OpenCascade (specify-core `docs/design.md`). OCCT's map is one-based, so the
  host's `MapShapes` index *n* is Specify's face *n − 1*; the adapter subtracts 1.
- text-to-cad numbers each part's faces with `MapShapes` on the part's own shape, assembly placement removed,
  per part, reading the STEP straight into OCCT. That is the same order, so it matches by number.
- The orders can still differ on unusual files: on one corpus file OCCT's STEP transfer reorders a shell's
  faces while Specify keeps the file's order (specify-core `docs/questions.md` Q13). The check on load below
  catches that. A face without an area or centroid (an unmeasured face) is left out of the sample.
- By **STEP entity** (the general route): for each face, Specify gives the `#N` of the `ADVANCED_FACE` it was read from
  (`snapshot().provenance.face_entities`, already in the WASM engine). The adapter declares which it uses:
  `faceIds: 'mapshapes' | 'step-entity'`, and the package sends the entity list with each part either way.
- **Check on load:** the package sends, for a sample of faces, the face's kind, area and centroid. The host
  answers whether its face with that id matches (within 1e-6 of the part's diagonal). On a mismatch the package
  switches to STEP-entity matching if the host supports it, else gives the host meshes (below), rather than
  highlighting the wrong faces.

## Frames and units

- Points are in the STEP file's units (millimetres for Specify's output), in the root frame of the file. A part
  instance's placement is the one Specify reports (`parts()[i].placements`).
- If the host's world differs (scaled to metres, centred, Y-up), the adapter reports `worldFromStep`, a 4×4
  matrix, once on connect. The package maps points through it before using camera matrices.
- Matrices are column-major arrays of 16 numbers, as three.js's `Matrix4.elements`.

## The interface

All arguments and results are plain JSON-serialisable data. The host passes an object implementing
`HostView` (`<StepDiff view={hostView} …>`, `<SpecifyReview view={hostView} …>`). Plain data would also let a
transport across an iframe be added later. Calls that change the view return nothing; the host applies them in order.

```ts
type Vec3 = [number, number, number];
type Mat4 = number[]; // 16, column-major
interface FaceRef { part: number; face: number }

interface HostViewInfo {
  faceIds: 'mapshapes' | 'step-entity';
  worldFromStep?: Mat4;                 // identity if absent
  canDrawMeshes: boolean;               // false: StepDiff's "Before" ghost is not shown
}

interface CameraState {
  view: Mat4;                           // world → camera
  projection: Mat4;                     // camera → clip
  eye: Vec3;                            // camera position, world
  width: number; height: number;        // the view's size in CSS pixels
}

interface PointerEvent {
  type: 'hover' | 'leave' | 'click' | 'dblclick';
  x: number; y: number;                 // CSS pixels from the view's top left
  hit: FaceRef | null;                  // nearest visible face of a shown part, or of a given mesh
  mesh?: string;                        // set when the hit is on a mesh the package gave
  button?: number; shift?: boolean; ctrl?: boolean; alt?: boolean; meta?: boolean;
}

interface FaceStyle {
  faces: FaceRef[];
  colour: string;                       // CSS colour
  opacity?: number;                     // 1 = solid
}

interface HostView {
  info(): Promise<HostViewInfo>;
  // What is shown
  showParts(parts: number[]): void;                     // all others hidden
  styleFaces(layer: string, styles: FaceStyle[]): void; // replaces that layer's styles; layers stack in call order
  clearStyles(layer: string): void;
  setOpacity(opacity: number): void;                    // whole part, for see-through review
  addMesh(id: string, mesh: PmiMesh, opts?: { transform?: Mat4; colour?: string; opacity?: number; edges?: boolean }): void;
  removeMesh(id: string): void;
  // Camera
  lookFrom(direction: Vec3, opts?: { up?: Vec3; frame?: FaceRef[] | { min: Vec3; max: Vec3 } }): void;
  fit(opts?: { also?: { min: Vec3; max: Vec3 } }): void; // whole visible part, plus what the overlays need
  home(): void;
  // Events
  onCamera(listener: (camera: CameraState) => void): () => void; // every change, and once on subscribe
  onPointer(listener: (event: PointerEvent) => void): () => void;
  onUserMoved(listener: () => void): () => void;                 // the person turned or zoomed the view
  // Placement of the overlay
  rect(): Promise<{ left: number; top: number; width: number; height: number }>; // in the package's page
  dispose(): void;
}
```

Optional members the package uses when present: `roll(quarters)`, `dolly(factor)` (toolbar buttons), and
`flash(faces)` (a brief highlight the host animates its own way). For the check on load: `partFaces` (the
package sends each part's STEP entity list), `checkFaces` (the host answers the sample check),
`HostViewInfo.stepEntities` (the host can match by STEP entity) and `useFaceIds` (the package switches the
host to STEP entities after a mismatch).

## How the components change

- `cad-view.ts` and `navigator.ts` become the default `HostView` implementations. Nothing else imports
  three-cad-viewer or creates a renderer for the part.
- *Later:* overlays (`annotations.ts`, `drawn-layer.ts`, `diff-drawn.ts`, `review-view.ts`) take a
  `CameraState` instead of a `THREE.Camera`, and render into their own canvas and HTML layer over `rect()`.
  In 0.3.0 they keep drawing with the default view as now.
- `look.ts` chooses directions from the package's mesh as now, then calls `lookFrom`.
- Picking (`picker.ts`, `review-view.ts`) listens to `onPointer` instead of ray-casting the view.
- The direct `viewer.setOpacity` call in `review-view.ts` becomes `setOpacity`.

## Testing

- 0.3.0: the existing browser suites keep running on the default view, through the interface.
- *Later:* a fake `HostView` in the browser tests, and an adapter conformance script a host can run (the
  face check on corpus samples, camera events on `lookFrom`, picks on known faces).

## Open questions

1. ~~text-to-cad's face numbering~~: answered 2026-11-18 (`MapShapes` per part, one-based), and tightened by
   quiddity #310: STEP entity is the general route, MapShapes only for solid-only parts (see Identity).
2. ~~Assemblies~~: decided 2026-11-20, one part at a time in 0.3.0. `instance?: number` in `FaceRef` is
   reserved; faces stay numbered per part.
3. ~~Whether the host draws the package's labels itself~~: no. The package keeps drawing them (Paul,
   2026-11-20); the overlay over a host camera comes later.
4. Versioning: the message tag `host-view/1`; additions are optional members, so old adapters keep working.
