// The part's PMI as it is drawn ("As drawn"), from Specify's pmiScene: haecceity's polylines as
// Line2-family lines and its filled triangles as meshes, both drawn without depth testing so the
// part never hides a callout; text the strokes do not draw as DOM text, set as text, never HTML,
// and never reformatted here. Each item is drawn in its Specify status's colour.
//
// Drawn in the labels' scene (annotations.ts's host), so the labels' renderers draw it and it
// follows the camera as they do. A click on a linked graphic is reported by item id; graphics
// linked to no semantic item are drawn but cannot be clicked. A graphic listed under several
// items (a datum's symbol, under the datum and its feature) is drawn once, as the item Specify
// knows (the datum), so clicking it reaches that item's question.
//
// The Show menu's kinds hide graphics as they hide labels, by their item's status (drawnShown);
// with the far side hidden, a callout whose annotation plane faces away from the camera is
// hidden too, looked at again as the camera moves.
import * as THREE from 'three';
import { CSS2DObject } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';

import { GROUP_COLOUR, type AnnotationHost } from './annotations';
import {
  drawnShown, facesAway, graphicNormal, type DrawnGraphic, type DrawnPmi, type DrawnPoint, type DrawnShow,
  type DrawnStatus, type TextPlacement,
} from './drawn';

const LINE_PX = 1.5;
const HIT_PX = 6;
// Text set by its stated height, but never smaller than this to be read.
const MIN_TEXT_PX = 7;

interface Drawn {
  /** The semantic item, or null for an unlinked graphic. */
  item: string | null;
  name: string;
  source: DrawnGraphic['source'];
  status: DrawnStatus | undefined;
  /** The side it is read from, when that can be told. */
  normal: DrawnPoint | null;
  /** Whether the Show menu shows its kind. */
  shown: boolean;
  /** Whether its plane faces away from the camera. */
  away: boolean;
  polylines: number;
  segments: THREE.Vector3[];
  triangles: THREE.Vector3[];
  objects: THREE.Object3D[];
}

export interface DrawnLayer {
  /** The file's own graphics (``source: "file"``) of ``pmi``, or none. */
  set(pmi: DrawnPmi | null): void;
  setVisible(on: boolean): void;
  /** The kinds the Show menu shows, and whether callouts facing away are hidden. */
  filter(show: DrawnShow, farSide: boolean): void;
  /** The linked item drawn within a few pixels of (x, y) in the view, if any. */
  hit(x: number, y: number): string | null;
  dispose(): void;
}

export function createDrawnLayer(
  nav: AnnotationHost,
  container: HTMLElement,
  { onClick, centre }: { onClick: (item: string) => void; centre?: DrawnPoint },
): DrawnLayer {
  const { scene } = nav;
  // One line and one fill material per colour: a status's, as its semantic labels have it.
  const inks = new Map<string, { line: LineMaterial; fill: THREE.MeshBasicMaterial }>();
  const ink = (colour: string) => {
    let made = inks.get(colour);
    if (!made) {
      made = {
        line: new LineMaterial({
          color: new THREE.Color(colour).getHex(), linewidth: LINE_PX, worldUnits: false,
          depthTest: false, depthWrite: false, alphaToCoverage: true,
        }),
        fill: new THREE.MeshBasicMaterial({ color: colour, side: THREE.DoubleSide, depthTest: false, depthWrite: false }),
      };
      inks.set(colour, made);
    }
    return made;
  };
  let drawn: Drawn[] = [];
  // Text set by a stated placement, re-oriented and re-sized as the camera moves.
  let placed: { inner: HTMLElement; at: TextPlacement }[] = [];
  let visible = false;
  let show: DrawnShow = { default: true, existing: true, general: true };
  let farSide = false;
  let pmiNow: DrawnPmi | null = null;
  const on = (d: Drawn) => visible && d.shown && !(farSide && d.away);

  function counts() {
    // What is drawn, for a test to compare with the JSON: nothing while hidden.
    const shownNow = drawn.filter(on);
    const n = (f: (d: Drawn) => number) => shownNow.reduce((sum, d) => sum + f(d), 0);
    container.dataset['drawnPolylines'] = String(n((d) => d.polylines));
    container.dataset['drawnTriangles'] = String(n((d) => d.triangles.length / 3));
    container.dataset['drawnTexts'] = String(n((d) => d.objects.filter((o) => o instanceof CSS2DObject).length));
    container.dataset['drawnNames'] = shownNow.map((d) => d.name).join('\n');
    // By the Show menu's kinds, for a test of it: general, default, and the file's (its own or existing).
    const by = (f: (d: Drawn) => boolean) => String(shownNow.filter(f).length);
    container.dataset['drawnGraphics'] = String(shownNow.length);
    container.dataset['drawnGeneral'] = by((d) => d.status === 'general');
    container.dataset['drawnDefault'] = by((d) => d.status === 'default');
    container.dataset['drawnFile'] = by((d) => d.source === 'file' || d.status === 'existing');
  }

  // Each graphic shown or not, as the menu and the camera say.
  function apply() {
    for (const d of drawn) for (const o of d.objects) o.visible = on(d);
    counts();
  }
  const look = new THREE.Vector3();
  function turn(): boolean {
    nav.camera.getWorldDirection(look);
    const direction: DrawnPoint = [look.x, look.y, look.z];
    let changed = false;
    for (const d of drawn) {
      const away = d.normal !== null && facesAway(d.normal, direction);
      if (away !== d.away) { d.away = away; changed = true; }
    }
    return changed;
  }

  function clear() {
    for (const d of drawn) {
      scene.remove(...d.objects);
      for (const o of d.objects) {
        if (o instanceof CSS2DObject) o.element.remove();
        else if (o instanceof THREE.Mesh) o.geometry.dispose();
      }
    }
    drawn = [];
    placed = [];
  }

  function place(graphic: DrawnGraphic, item: string | null, status: DrawnStatus | undefined, colour: string): Drawn {
    const { line: material, fill } = ink(colour);
    const segments: THREE.Vector3[] = [];
    for (const line of graphic.polylines) {
      for (let k = 1; k < line.length; k++) segments.push(new THREE.Vector3(...line[k - 1]!), new THREE.Vector3(...line[k]!));
    }
    const triangles = graphic.triangles.flatMap((t) => t.map((p) => new THREE.Vector3(...p)));
    const objects: THREE.Object3D[] = [];
    if (segments.length) {
      const geometry = new LineSegmentsGeometry();
      geometry.setPositions(segments.flatMap((p) => [p.x, p.y, p.z]));
      const lines = new LineSegments2(geometry, material);
      lines.renderOrder = 10;
      objects.push(lines);
    }
    if (triangles.length) {
      const geometry = new THREE.BufferGeometry().setFromPoints(triangles);
      const mesh = new THREE.Mesh(geometry, fill);
      mesh.renderOrder = 10;
      objects.push(mesh);
    }
    // haecceity strokes the text of what it generates into the polylines; a file's text strings
    // (its text literals) have no strokes, so they are set here, where the file places them, or,
    // with no stated placement, at the middle of the graphic's own lines and triangles.
    const texts = graphic.source === 'file' ? graphic.text : [];
    const box = new THREE.Box3().setFromPoints([...segments, ...triangles]);
    for (const [i, text] of texts.entries()) {
      const at = graphic.textPlacements?.find((t) => t.textIndex === i);
      if (!at && box.isEmpty()) continue;
      const el = document.createElement('div');
      el.className = at ? 'pmi-drawn pmi-drawn-placed' : 'pmi-drawn';
      const inner = document.createElement('span');
      inner.textContent = text;
      inner.style.color = colour;
      // Unplaced, a graphic's texts one under another.
      if (!at && i) inner.style.transform = `translateY(${1.2 * i}em)`;
      if (item !== null) {
        const id = item;
        inner.style.pointerEvents = 'auto';
        inner.style.cursor = 'pointer';
        inner.addEventListener('pointerdown', (event) => event.stopPropagation());
        inner.addEventListener('click', (event) => { event.stopPropagation(); onClick(id); });
      }
      el.append(inner);
      const tag = new CSS2DObject(el);
      if (at) {
        tag.position.set(...at.origin);
        placed.push({ inner, at });
      } else tag.position.copy(box.getCenter(new THREE.Vector3()));
      objects.push(tag);
    }
    scene.add(...objects);
    return {
      item, name: graphic.name, source: graphic.source, status,
      normal: graphicNormal(graphic, pmiNow?.presentation.planes ?? [], centre),
      shown: drawnShown(graphic.source, status, show), away: false,
      polylines: graphic.polylines.filter((l) => l.length > 1).length, segments, triangles, objects,
    };
  }

  const a = new THREE.Vector2();
  const b = new THREE.Vector2();
  const c = new THREE.Vector2();
  const p = new THREE.Vector2();
  const v = new THREE.Vector3();
  function screen(point: THREE.Vector3, into: THREE.Vector2): boolean {
    const [w, h] = nav.size();
    v.copy(point).project(nav.camera);
    into.set((v.x * 0.5 + 0.5) * w, (-v.y * 0.5 + 0.5) * h);
    return v.z <= 1;
  }
  function nearSegment(): number {
    const ab = b.clone().sub(a);
    const t = ab.lengthSq() ? Math.min(Math.max(p.clone().sub(a).dot(ab) / ab.lengthSq(), 0), 1) : 0;
    return a.clone().addScaledVector(ab, t).distanceTo(p);
  }
  function inside(): boolean {
    const cross = (u: THREE.Vector2, w: THREE.Vector2) => (w.x - u.x) * (p.y - u.y) - (w.y - u.y) * (p.x - u.x);
    const [d1, d2, d3] = [cross(a, b), cross(b, c), cross(c, a)];
    return !((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0));
  }

  // A placed text's alignment point is its tag's position; the text is turned to its x axis as
  // seen and sized to its stated height as seen.
  // Where the alignment point is on the text: "baseline left", "centre", "top right" and so on;
  // "centre" alone centres both ways. Unstated, at the left of the baseline.
  const shift = (alignment = '') => {
    const words = alignment.toLowerCase().split(/[\s_-]+/);
    const has = (...w: string[]) => w.some((x) => words.includes(x));
    const centre = has('centre', 'center');
    const x = has('left') ? '0' : has('right') ? '-100%' : centre ? '-50%' : '0';
    const y = has('top') ? '0' : has('bottom') ? '-100%' : has('middle') || (centre && !has('baseline')) ? '-50%' : '-80%';
    return `translate(${x}, ${y})`;
  };
  const o = new THREE.Vector2();
  const ex = new THREE.Vector2();
  const up = new THREE.Vector2();
  const along = new THREE.Vector3();
  function orient() {
    for (const { inner, at } of placed) {
      const origin = new THREE.Vector3(...at.origin);
      if (!screen(origin, o)) continue;
      const x = new THREE.Vector3(...at.xAxis).normalize();
      const y = new THREE.Vector3(...at.normal).cross(x).normalize();
      const size = at.height ?? 0;
      // A length on the page: the height, or a unit, along each axis.
      screen(along.copy(origin).addScaledVector(x, size || 1), ex);
      screen(along.copy(origin).addScaledVector(y, size || 1), up);
      const angle = Math.atan2(ex.y - o.y, ex.x - o.x);
      const px = size ? Math.max(o.distanceTo(up), MIN_TEXT_PX) : 11.5;
      inner.style.fontSize = `${px.toFixed(1)}px`;
      inner.style.transform = `rotate(${angle.toFixed(4)}rad) ${shift(at.alignment)}`;
    }
  }

  // Which way the camera looks and which way is up, for a test of turning square to a plane.
  const upward = new THREE.Vector3();
  const fixed = (u: THREE.Vector3) => u.toArray().map((x) => (Math.abs(x) < 5e-4 ? 0 : x).toFixed(3)).join(',');
  nav.onFrame(() => {
    const [w, h] = nav.size();
    for (const { line } of inks.values()) line.resolution.set(w, h);
    nav.camera.getWorldDirection(look);
    upward.set(0, 1, 0).applyQuaternion(nav.camera.quaternion);
    const camera = `${fixed(look)} ${fixed(upward)}`;
    if (container.dataset['camera'] !== camera) container.dataset['camera'] = camera;
    if (!visible) return;
    if (farSide && turn()) apply();
    if (placed.length) orient();
  });

  return {
    set(pmi) {
      clear();
      pmiNow = pmi;
      if (pmi) {
        // Items Specify knows first, so a graphic under several is drawn as the one it knows.
        const known = (item: string) => (pmi.links[item]?.status ? 0 : 1);
        const items = Object.entries(pmi.presentation.items).sort(([a], [b]) => known(a) - known(b));
        const seen = new Set<string>();
        const once = (g: DrawnGraphic) => {
          if (g.id === undefined) return true;
          if (seen.has(g.id)) return false;
          seen.add(g.id);
          return true;
        };
        for (const [item, graphics] of items) {
          const status = pmi.links[item]?.status;
          const colour = GROUP_COLOUR[status ?? 'existing'];
          for (const g of graphics) if (once(g)) drawn.push(place(g, item, status, colour));
        }
        for (const g of pmi.presentation.unlinked) if (once(g)) drawn.push(place(g, null, undefined, GROUP_COLOUR.existing));
      }
      turn();
      apply();
    },
    setVisible(next) {
      visible = next;
      if (next) { turn(); orient(); }
      apply();
    },
    filter(next, hideFar) {
      show = next;
      farSide = hideFar;
      for (const d of drawn) d.shown = drawnShown(d.source, d.status, show);
      turn();
      apply();
    },
    hit(x, y) {
      if (!visible) return null;
      p.set(x, y);
      let best: string | null = null;
      let reach = HIT_PX;
      for (const d of drawn) {
        if (d.item === null || !on(d)) continue;
        for (let k = 0; k + 1 < d.segments.length; k += 2) {
          if (!screen(d.segments[k]!, a) || !screen(d.segments[k + 1]!, b)) continue;
          const dist = nearSegment();
          if (dist < reach) { reach = dist; best = d.item; }
        }
        for (let k = 0; k + 2 < d.triangles.length; k += 3) {
          if (screen(d.triangles[k]!, a) && screen(d.triangles[k + 1]!, b) && screen(d.triangles[k + 2]!, c) && inside()) {
            return d.item;
          }
        }
      }
      return best;
    },
    dispose() {
      clear();
      for (const { line, fill } of inks.values()) { line.dispose(); fill.dispose(); }
    },
  };
}
