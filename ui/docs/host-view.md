# Host view: letting a host draw the 3D view

Status: **proposed** (2026-11-18). For agreement with Studio / text-to-cad before code.

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
- A **face** is an index into that part's faces in Specify's numbering: OpenCascade's `TopExp::MapShapes`
  order on the part's own solid. haecceity (Specify's reader) follows that order and is tested face by face
  against OpenCascade (specify-core `docs/design.md`).
- A host whose kernel numbers faces the same way (OCCT's face map of the part, per part, not across an
  assembly) uses these indices directly.
- Otherwise the host maps by **STEP entity**: for each face, Specify gives the `#N` of the `ADVANCED_FACE` it
  was read from (`snapshot().provenance.face_entities`). The adapter declares which it uses:
  `faceIds: 'mapshapes' | 'step-entity'`.
- **Check on load:** the package sends, for a sample of faces, the face's kind, area and centroid. The host
  answers whether its face with that id matches (within 1e-6 of the part's diagonal). On a mismatch the package
  says so and falls back to giving the host meshes (below), rather than highlighting the wrong faces.

## Frames and units

- Points are in the STEP file's units (millimetres for Specify's output), in the root frame of the file. A part
  instance's placement is the one Specify reports (`parts()[i].placements`).
- If the host's world differs (scaled to metres, centred, Y-up), the adapter reports `worldFromStep`, a 4×4
  matrix, once on connect. The package maps points through it before using camera matrices.
- Matrices are column-major arrays of 16 numbers, as three.js's `Matrix4.elements`.

## The interface

All arguments and results are plain JSON-serialisable data, so the interface works in the same page or across
an iframe (see Transport). Calls that change the view return nothing; the host applies them in order.

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
`flash(faces)` (a brief highlight the host animates its own way).

## Transport

- **Same page:** the host passes an object implementing `HostView` (`<StepDiff view={hostView} …>`,
  `<SpecifyReview view={hostView} …>`).
- **Across an iframe** (Studio embedding the text-to-cad viewer): the package ships
  `connectHostView(target: Window, origin: string): HostView`, which turns calls into `postMessage` messages
  `{specify: 'host-view/1', id, call, args}` and events into listeners. The adapter inside the iframe uses
  `serveHostView(view, origin)`. Both check the origin. Calls with results answer `{id, result}` or
  `{id, error}`.
- The overlay sits in the package's page, over the iframe's rectangle (`rect()`), with `pointer-events: none`
  except on labels. Pointer events on the geometry happen in the iframe and come back through `onPointer`.

## How the components change

- `cad-view.ts` and `navigator.ts` become the default `HostView` implementations. Nothing else imports
  three-cad-viewer or creates a renderer for the part.
- Overlays (`annotations.ts`, `drawn-layer.ts`, `diff-drawn.ts`, `review-view.ts`) take a `CameraState`
  instead of a `THREE.Camera`, and render into their own canvas and HTML layer over `rect()`.
- `look.ts` chooses directions from the package's mesh as now, then calls `lookFrom`.
- Picking (`picker.ts`, `review-view.ts`) listens to `onPointer` instead of ray-casting the view.
- The direct `viewer.setOpacity` call in `review-view.ts` becomes `setOpacity`.

## Testing

- A fake `HostView` in the browser tests (faces drawn by the test, events scripted), so StepDiff and
  SpecifyReview are tested against the interface as well as against the default view.
- The existing browser suites keep running on the default view.
- An adapter conformance script a host can run: the face check on the corpus samples, camera events on
  `lookFrom`, picks on known faces.

## Open questions

1. text-to-cad's face numbering: `MapShapes` per part, or another order (decides `faceIds`).
2. Assemblies: does the host's view show one part at a time, or the assembly with instances placed? The
   interface addresses faces by part, so an assembly view needs the instance in `FaceRef` (add
   `instance?: number`).
3. Whether the host should also draw the package's labels itself (no overlay layer). Not proposed: it would move
   the PMI drawing rules into every host.
4. Versioning: the message tag `host-view/1`; additions are optional members, so old adapters keep working.
