// Labels on the part: HTML text at a stand-off, a leader to the face, an
// arrowhead where it lands, decluttered on screen as the part turns.
//
// Ported from pmi-assist-viewer (picker/annotations.js), which vendors
// step-pmi-viewer's label markup, Line2 leaders, facing test and declutter.
// Label text is set as DOM text, never as HTML, and the overlay is removed on
// dispose().
import * as THREE from 'three';
import { CSS2DObject, CSS2DRenderer } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { Line2 } from 'three/examples/jsm/lines/Line2.js';
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';

import type { PmiAnalysis, PmiAnchor, PmiMesh } from '../types/pmi';

import { ranges } from './characteristics';
import type { PmiLabel } from './labels';
/** What labels are drawn over: a scene of their own, the view's camera, the part's size. */
export interface AnnotationHost {
  readonly scene: THREE.Scene;
  /** Read each time: a view can change camera (perspective, orthographic). */
  readonly camera: THREE.Camera;
  readonly diagonal: number;
  size(): [number, number];
  onFrame(listener: () => void): void;
}

export const GROUP_COLOUR = {
  answered: '#2f6fb5', default: '#6f8db3', existing: '#707880', general: '#3d8a6e', current: '#c47f00',
} as const;
const LEADER_PX = 2.2;
const STANDOFF = 0.07; // label distance from its face, as a share of the diagonal

interface Placed {
  el: HTMLDivElement;
  inner: HTMLSpanElement;
  tag: CSS2DObject;
  /** Its leader and arrowhead, or a dimension's lines and arrowheads. */
  ink: (Line2 | THREE.Mesh)[];
  world: THREE.Vector3;
  tip: THREE.Vector3;
  at: number;
  /** Whether the part is in the way of where the leader lands, from a ray cast
   * when the view settles. */
  buried: boolean;
  /** Centred on its point, as a dimension's value is; else beside it. */
  centred: boolean;
  /** A leader's line and arrowhead, redrawn from where the label is moved to. */
  leader?: { line: Line2; head: THREE.Mesh };
  /** How far it is moved on screen, in pixels, to stand clear of the others. */
  dx: number;
  dy: number;
}

const UP = new THREE.Vector3(0, 1, 0);

export interface Annotations {
  set(
    labels: readonly PmiLabel[],
    options?: {
      current?: string | null; anchors?: Readonly<Record<string, PmiAnchor>>;
      balloons?: ReadonlyMap<string, readonly number[]>;
    },
  ): void;
  setVisible(on: boolean): void;
  /** Whether labels on the far side of the part are hidden; on by default. */
  setFarSideHidden(on: boolean): void;
  /** What can hide a label changed (the part turned see-through): look again. */
  recheck(): void;
  /** Where the labels being shown stand, for a fit to keep them in view. */
  points(): THREE.Vector3[];
  dispose(): void;
}

/** Labels over ``nav``'s scene, anchored on ``mesh`` faces. */
export function createAnnotations(
  nav: AnnotationHost,
  container: HTMLElement,
  { mesh, analysis, onClick, occluders }: {
    mesh: PmiMesh; analysis: PmiAnalysis; onClick: (label: PmiLabel) => void;
    /** What can stand between the camera and a label: none while the part is see-through. */
    occluders: () => THREE.Object3D[];
  },
): Annotations {
  const { scene, diagonal: d } = nav;
  // An arrowhead with its point at ``tip``, coming from ``from``.
  const point = (head: THREE.Mesh, tip: THREE.Vector3, from: THREE.Vector3) => {
    const along = tip.clone().sub(from).normalize();
    head.quaternion.setFromUnitVectors(UP, along);
    head.position.copy(tip).addScaledVector(along, -d * 0.011);
  };
  const overlay = new CSS2DRenderer();
  overlay.domElement.style.cssText = 'position:absolute;inset:0;pointer-events:none';
  container.appendChild(overlay.domElement);

  const faces = new Map(analysis.faces.map((f) => [f.id, f]));
  const centre = new THREE.Vector3(...mesh.bbox[0]).add(new THREE.Vector3(...mesh.bbox[1]))
    .multiplyScalar(0.5);

  // A point on the face itself (a face's centroid can lie off it -- inside a
  // bore, or in a ring's hole): the face's mesh vertex nearest its centroid.
  function anchorOf(id: number | undefined) {
    if (id === undefined) return null;
    const f = faces.get(id);
    const [start, count] = mesh.faces[id] ?? [0, 0];
    if (!f || !count) return null;
    const c = new THREE.Vector3(...f.centroid);
    let best: THREE.Vector3 | null = null;
    let bestD = Infinity;
    for (let k = start * 3; k < (start + count) * 3; k++) {
      const v = mesh.indices[k] ?? 0;
      const p = new THREE.Vector3(
        mesh.positions[3 * v] ?? 0, mesh.positions[3 * v + 1] ?? 0, mesh.positions[3 * v + 2] ?? 0,
      );
      const dist = p.distanceToSquared(c);
      if (dist < bestD) {
        bestD = dist;
        best = p;
      }
    }
    if (!best) return null;
    // Out from the part: a plane's normal; for a cylinder its axis, whichever
    // way points away from the part's middle.
    const out = new THREE.Vector3(...(f.direction ?? [0, 0, 1]));
    if (f.kind !== 'plane' && out.dot(best.clone().sub(centre)) < 0) out.negate();
    return { tip: best, out: out.normalize() };
  }

  const box = new THREE.Box3(new THREE.Vector3(...mesh.bbox[0]), new THREE.Vector3(...mesh.bbox[1]));
  const corners = [0, 1, 2, 3, 4, 5, 6, 7].map((k) => new THREE.Vector3(
    k & 1 ? box.max.x : box.min.x, k & 2 ? box.max.y : box.min.y, k & 4 ? box.max.z : box.min.z,
  ));

  // The vertex of ``ids``' faces furthest along ``dir``: where an extension
  // line leaves the part rather than crossing it.
  function furthest(ids: readonly number[], dir: THREE.Vector3): THREE.Vector3 | null {
    let best: THREE.Vector3 | null = null;
    let reach = -Infinity;
    for (const id of ids) {
      const [start, count] = mesh.faces[id] ?? [0, 0];
      for (let k = start * 3; k < (start + count) * 3; k++) {
        const v = mesh.indices[k] ?? 0;
        const p = new THREE.Vector3(
          mesh.positions[3 * v] ?? 0, mesh.positions[3 * v + 1] ?? 0, mesh.positions[3 * v + 2] ?? 0,
        );
        if (p.dot(dir) > reach) {
          reach = p.dot(dir);
          best = p;
        }
      }
    }
    return best;
  }

  // Where a length is drawn: from its reference plane, along that plane's
  // normal, to its faces' outer edge or the hole's axis, set out sideways
  // beyond the part. Lengths from one plane are stacked, each further out.
  function spanOf(
    span: NonNullable<PmiLabel['span']>, ids: readonly number[], stacked: Map<string, number>,
  ) {
    const plane = faces.get(span.reference[0] ?? -1);
    const target = faces.get(ids[0] ?? -1);
    if (!plane || plane.kind !== 'plane' || !target) return null;
    const n = new THREE.Vector3(...(plane.direction ?? [0, 0, 1])).normalize();
    const mid = span.axis ? new THREE.Vector3(...span.axis) : new THREE.Vector3(...target.centroid);
    // Out to the side the end lies on; on the axis, the part's widest side.
    const out = mid.clone().sub(centre).addScaledVector(n, -mid.clone().sub(centre).dot(n));
    if (out.lengthSq() < (d * 0.01) ** 2) {
      out.set(0, 0, 0);
      const size = box.getSize(new THREE.Vector3());
      const axes = [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1)];
      const pick = axes.filter((a) => Math.abs(a.dot(n)) < 0.5)
        .sort((a, b) => b.dot(size) - a.dot(size))[0] ?? axes[0];
      out.copy(pick ?? new THREE.Vector3(1, 0, 0));
    }
    out.normalize();
    const to = span.axis ? mid : furthest(ids, out);
    if (!to) return null;
    const from = to.clone().addScaledVector(n, -to.clone().sub(new THREE.Vector3(...plane.centroid)).dot(n));
    const key = [span.reference.join(','), out.toArray().map((v) => v.toFixed(2)).join(',')].join('|');
    const rank = stacked.get(key) ?? 0;
    stacked.set(key, rank + 1);
    const beyond = Math.max(...corners.map((c) => c.clone().sub(to).dot(out)), 0);
    return { from, to, out, off: beyond + d * (0.06 + 0.05 * rank) };
  }

  let placed: Placed[] = [];
  let materials: LineMaterial[] = [];
  let hidden = false;
  let cull = true;

  function clear() {
    for (const p of placed) {
      scene.remove(p.tag, ...p.ink);
      p.el.remove();
      for (const o of p.ink) {
        o.geometry.dispose();
        if (o instanceof THREE.Mesh) (o.material as THREE.Material).dispose();
      }
    }
    for (const m of materials) m.dispose();
    placed = [];
    materials = [];
  }

  function markup(label: PmiLabel): HTMLSpanElement {
    const inner = document.createElement('span');
    inner.className = 'pmi-in';
    const add = (tag: string, className: string, text: string, parent: HTMLElement = inner) => {
      const el = document.createElement(tag);
      if (className) el.className = className;
      el.textContent = text;
      parent.append(el);
      return el;
    };
    if (label.count && label.count > 1) add('span', 'times', `${label.count}× `);
    if (label.kind === 'frame') {
      const frame = add('span', 'fcf', '');
      for (const [i, cell] of label.cells.entries()) add('span', i === 0 ? 'sym' : '', cell, frame);
    } else if (label.kind === 'datum') {
      add('span', 'datum', label.cells[0] ?? '');
    } else {
      inner.append(label.cells[0] ?? '');
      if (label.tolerance?.style === 'symmetric') add('span', 'tolsym', label.tolerance.upper);
      else if (label.tolerance) {
        const pair = add('span', 'tolpair', '');
        add('span', '', label.tolerance.upper, pair);
        add('span', '', label.tolerance.lower ?? '', pair);
      }
      if (label.cells.length > 1) add('span', 'sub', ` ${label.cells.slice(1).join(' ')}`);
    }
    return inner;
  }

  function set(
    labels: readonly PmiLabel[],
    { current = null, anchors = {}, balloons = new Map() }: {
      current?: string | null; anchors?: Readonly<Record<string, PmiAnchor>>;
      balloons?: ReadonlyMap<string, readonly number[]>;
    } = {},
  ) {
    clear();
    const stacked = new Map<string, number>();
    for (const label of labels) {
      // pmi-assist's anchor, found on the solid, where there is one: a point on
      // the face and a leader that leaves through free space. Else a guess.
      const solid = anchors[[...label.faces].sort((a, b) => a - b).join(',')];
      const at = solid
        ? { tip: new THREE.Vector3(...solid.at), out: new THREE.Vector3(...solid.out).normalize() }
        : anchorOf(label.faces[0]);
      const span = label.span ? spanOf(label.span, label.faces, stacked) : null;
      if (!at && !span) continue;
      const colour = label.question && label.question === current
        ? GROUP_COLOUR.current : GROUP_COLOUR[label.group];
      const el = document.createElement('div');
      el.className = `pmi ${label.group}${hidden ? ' hidden' : ''}`;
      el.style.color = colour;
      const inner = markup(label);
      // Its balloon numbers, as the characteristics table numbers them.
      const numbers = balloons.get(label.id.split(':').slice(0, 2).join(':'));
      if (numbers?.length) {
        const balloon = document.createElement('span');
        balloon.className = 'bal';
        balloon.textContent = ranges(numbers);
        balloon.title = `Characteristic ${ranges(numbers)} in the Characteristics table`;
        inner.prepend(balloon);
      }
      // A label is a way into what it says: clicking it, rather than turning
      // the part underneath, goes to its question.
      inner.style.pointerEvents = 'auto';
      inner.style.cursor = 'pointer';
      inner.addEventListener('pointerdown', (event) => event.stopPropagation());
      inner.addEventListener('click', (event) => {
        event.stopPropagation();
        onClick(label);
      });
      el.append(inner);
      const tag = new CSS2DObject(el);
      // Its box's top left at the point, so the label's own transform alone sets where it stands
      // (beside the point, or centred on it), as the declutter below reckons it.
      tag.center.set(0, 0);

      // WebGL draws THREE.Line one pixel wide whatever linewidth says; Line2
      // gives a leader that reads at a glance.
      const faint = label.group === 'default';
      const material = (px: number) => {
        const m = new LineMaterial({
          color: new THREE.Color(colour).getHex(), linewidth: px, worldUnits: false,
          alphaToCoverage: true, transparent: faint, opacity: faint ? 0.6 : 1,
        });
        materials.push(m);
        return m;
      };
      const line = (from: THREE.Vector3, to: THREE.Vector3, px = LEADER_PX) => {
        const geometry = new LineGeometry();
        geometry.setPositions([from.x, from.y, from.z, to.x, to.y, to.z]);
        const l = new Line2(geometry, material(px));
        l.computeLineDistances();
        return l;
      };
      // An arrowhead whose point is at ``tip``, coming from ``from``.
      const arrow = (tip: THREE.Vector3, from: THREE.Vector3) => {
        const head = new THREE.Mesh(
          new THREE.ConeGeometry(d * 0.006, d * 0.022, 12),
          new THREE.MeshBasicMaterial({ color: colour }),
        );
        point(head, tip, from);
        return head;
      };
      let ink: (Line2 | THREE.Mesh)[];
      let leader: Placed['leader'];
      let near: THREE.Vector3;
      let tip: THREE.Vector3;
      if (span) {
        // A dimension clear of the part: extension lines out from the
        // reference plane and the end, the dimension line between them.
        const { from, to, out } = span;
        const a = from.clone().addScaledVector(out, span.off);
        const b = to.clone().addScaledVector(out, span.off);
        const over = d * 0.015;
        near = a.clone().add(b).multiplyScalar(0.5);
        tip = to;
        ink = [
          line(from, a.clone().addScaledVector(out, over), 1),
          line(to, b.clone().addScaledVector(out, over), 1),
          line(a, b), arrow(a, b), arrow(b, a),
        ];
      } else if (at) {
        near = at.tip.clone().addScaledVector(at.out, d * STANDOFF);
        tip = at.tip;
        leader = { line: line(near, at.tip), head: arrow(at.tip, near) };
        ink = [leader.line, leader.head];
      } else {
        continue;
      }
      tag.position.copy(near);
      scene.add(tag, ...ink);
      placed.push({
        el, inner, tag, ink, world: near, tip: tip.clone(),
        at: placed.length, buried: false, centred: Boolean(span), dx: 0, dy: 0,
        ...(leader ? { leader } : {}),
      });
    }
    // Placed while the view is still: whether each is hidden is known now.
    testOcclusion();
  }

  // A label is hidden only when the part is in the way of the point it lands
  // on -- step-pmi-viewer's occlusion test, which it applies on top of a facing
  // test. Facing alone hid a shaft seen edge-on, a feature down a bore in plain
  // sight, and anything whose leader happened to leave on the far side; so it
  // is not used. The rest are moved off one another, the one nearest the camera
  // keeping its place: each to the nearest free spot above, below or to either
  // side, a dimension's value only along its line's normal on screen (up or
  // down). A moved label's leader is redrawn from where it now stands, so it
  // still points at its feature.
  const proj = new THREE.Vector3();
  function declutter() {
    const [w, h] = nav.size();
    const rows: { p: Placed; depth: number; x: number; y: number; bw: number; bh: number }[] = [];
    // Where a label's box starts, left of its point: beside it, or centred on it.
    const left = (r: { p: Placed; x: number; bw: number }) => (r.p.centred ? r.x - r.bw / 2 : r.x + 9);
    for (const p of placed) {
      const away = cull && p.buried;
      for (const o of p.ink) o.visible = !away && !hidden;
      if (away || hidden) {
        p.el.style.visibility = 'hidden';
        continue;
      }
      proj.copy(p.world).project(nav.camera);
      if (proj.z > 1) {
        p.el.style.visibility = 'hidden';
        continue;
      }
      p.el.style.visibility = '';
      rows.push({
        p, depth: proj.z, x: (proj.x * 0.5 + 0.5) * w, y: (-proj.y * 0.5 + 0.5) * h,
        bw: p.inner.offsetWidth || 90, bh: (p.inner.offsetHeight || 15) + 3,
      });
    }
    const resolved = 1e6;
    rows.sort((a, b) => Math.round(a.depth * resolved) - Math.round(b.depth * resolved) || a.p.at - b.p.at);
    const taken: { x: number; bw: number; top: number; bot: number }[] = [];
    for (const r of rows) {
      const box = (dx: number, dy: number) => ({ x: left(r) + dx, bw: r.bw, top: r.y + dy - r.bh / 2, bot: r.y + dy + r.bh / 2 });
      const clear = (b: { x: number; bw: number; top: number; bot: number }) =>
        !taken.some((t) => !(b.x + b.bw <= t.x || t.x + t.bw <= b.x || b.bot <= t.top || t.bot <= b.top));
      let [dx, dy] = [0, 0];
      if (!clear(box(0, 0))) {
        // Across to the point's other side, or a box's width further out.
        const across = r.p.centred ? [0] : [0, -(r.bw + 18), r.bw + 9, -(2 * r.bw + 27)];
        const spots = across.flatMap((x) => Array.from({ length: 41 }, (_, i) => [x, (i % 2 ? 1 : -1) * Math.ceil(i / 2) * r.bh]))
          .sort((a, b) => Math.hypot(a[0] ?? 0, a[1] ?? 0) - Math.hypot(b[0] ?? 0, b[1] ?? 0));
        const free = spots.find(([x, y]) => clear(box(x ?? 0, y ?? 0)));
        if (free) [dx, dy] = [free[0] ?? 0, free[1] ?? 0];
      }
      taken.push(box(dx, dy));
      const x = r.p.centred ? '-50%' : `${(9 + dx).toFixed(1)}px`;
      r.p.inner.style.transform = `translate(${x}, calc(-50% + ${dy.toFixed(1)}px))`;
      if (r.p.leader && (dx !== r.p.dx || dy !== r.p.dy || dx || dy)) {
        // From the label's near edge, at the depth it stands at.
        const edge = dx < 0 ? r.x + dx + 9 + r.bw : r.x + dx;
        const from = dx || dy
          ? new THREE.Vector3((edge / w) * 2 - 1, -((r.y + dy) / h) * 2 + 1, r.depth).unproject(nav.camera)
          : r.p.world;
        r.p.leader.line.geometry.setPositions([from.x, from.y, from.z, r.p.tip.x, r.p.tip.y, r.p.tip.z]);
        r.p.leader.line.computeLineDistances();
        point(r.p.leader.head, r.p.tip, from);
      }
      r.p.dx = dx;
      r.p.dy = dy;
    }
  }

  // Whether the part is actually in the way: the facing test is a proxy that a
  // flange or a bore fools. A ray is exact, and too slow for every frame, so it
  // is cast once the view has held still -- which is when labels are read.
  const occluder = new THREE.Raycaster();
  function testOcclusion() {
    const solids = occluders();
    const bias = d * 0.004;
    const hidden = (point: THREE.Vector3) => {
      const toward = point.clone().sub(nav.camera.position);
      const reach = toward.length();
      occluder.set(nav.camera.position, toward.normalize());
      occluder.near = 0;
      occluder.far = Math.max(reach - bias, 0);
      return occluder.intersectObjects(solids, false).length > 0;
    };
    // Behind the part only if both are: where it lands, and the label itself.
    // A point on a shaft's outline is grazed by the shaft beside it, though
    // the label standing off it is in plain view.
    for (const p of placed) p.buried = solids.length > 0 && hidden(p.tip) && hidden(p.world);
  }

  let frame = 0;
  let live = true;
  let still = 0;
  const lastEye = new THREE.Vector3(Infinity, 0, 0);
  nav.onFrame(() => {
    if (!live) return;
    const [w, h] = nav.size();
    overlay.setSize(w, h);
    for (const m of materials) m.resolution.set(w, h);
    overlay.render(scene, nav.camera);
    if ((frame++ & 1) === 0) declutter();
    // Held still for a few frames running: settled.
    if (lastEye.distanceToSquared(nav.camera.position) > 1e-12) {
      lastEye.copy(nav.camera.position);
      still = 0;
    } else if (still >= 0 && ++still === 6) {
      testOcclusion();
      still = -1;
    }
  });

  return {
    set,
    setVisible(on) {
      hidden = !on;
      for (const p of placed) p.el.classList.toggle('hidden', hidden);
    },
    setFarSideHidden(on) {
      cull = on;
    },
    recheck() {
      still = 0;
    },
    points: () => (hidden ? [] : placed.filter((p) => !(cull && p.buried)).map((p) => p.world.clone())),
    dispose() {
      live = false;
      clear();
      overlay.domElement.remove();
    },
  };
}
