// The part's PMI as it is drawn (View PMI: "Graphical"), from Specify's pmiScene: haecceity's polylines as
// Line2-family lines and its filled triangles as meshes, both drawn without depth testing so the
// part never hides a callout; text the strokes do not draw as DOM text, set as text, never HTML,
// and never reformatted here. Each item is drawn in its Specify status's colour.
//
// What haecceity generates is drawn as an engineer reads it on a drawing, set in the semantic
// labels' face: its text (``graphic.text[i]`` at ``text_placements[i]``) is set as DOM text, in
// its plane and centred in its cell (a frame's compartment), no smaller than a label; its
// frames, leaders, extension and dimension lines (``polyline_roles``) drawn a little heavier;
// the strokes of its text, which the DOM text replaces, are not drawn. A file's own graphics are
// drawn as the file draws them.
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
//
// The /diff page draws a revision's PMI with it too, each item in its change's colour
// (``colourOf``), and the old revision's removed items as a translucent ghost (``alpha``).
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
// What haecceity generates: its lines a little heavier, as a label's leader is.
const SET_LINE_PX = 2;
const HIT_PX = 6;
// Text set by its stated height, but never smaller than this to be read.
const MIN_TEXT_PX = 7;
// Generated text, set as a label is: never smaller than a label (PmiLab.css's .pmi), and its
// capitals as tall as haecceity's strokes would be (a capital is about 0.7 of the font size).
const SET_TEXT_PX = 11.5;
const CAP_HEIGHT = 0.7;
// How far a plane's x or up may shrink as seen, against a millimetre across the view, before its
// set text is hidden: near edge-on it cannot be read.
const EDGE_ON = 0.25;
// An item lit: a strong blue, read on the part and on white.
const LIT_INK = '#0b62c4';

interface Drawn {
  /** The semantic item, or null for an unlinked graphic. */
  item: string | null;
  name: string;
  source: DrawnGraphic['source'];
  status: DrawnStatus | undefined;
  colour: string;
  /** Its line weight: heavier when its text is set (what haecceity generates). */
  width: number;
  /** The text set for it in the labels' face, hit where it is seen. */
  set: HTMLElement[];
  /** The side it is read from, when that can be told. */
  normal: DrawnPoint | null;
  /** Whether the Show menu shows its kind. */
  shown: boolean;
  /** Whether its plane faces away from the camera. */
  away: boolean;
  polylines: number;
  /** Its lines as drawn, in pairs: what a pointer hits. */
  segments: THREE.Vector3[];
  /** Where all its strokes are, its text's too: what a view frames. */
  extent: THREE.Box3;
  triangles: THREE.Vector3[];
  objects: THREE.Object3D[];
}

export interface DrawnLayer {
  /** The file's own graphics (``source: "file"``) of ``pmi``, or none. */
  set(pmi: DrawnPmi | null): void;
  setVisible(on: boolean): void;
  /**
   * The kinds the Show menu shows, and whether callouts facing away are hidden; items in
   * ``reveal`` are drawn whatever their kind and whichever way they face, as a label chosen is.
   */
  filter(show: DrawnShow, farSide: boolean, reveal?: ReadonlySet<string>): void;
  /** Draws an item's graphics in the highlight colour, or none. */
  light(item: string | null): void;
  /** The linked item drawn within a few pixels of (x, y) in the view, if any. */
  hit(x: number, y: number): string | null;
  /** Where the graphics now shown are, for a view that frames them with the part; null if none. */
  bounds(): THREE.Box3 | null;
  dispose(): void;
}

export function createDrawnLayer(
  nav: AnnotationHost,
  container: HTMLElement,
  { onClick, centre, colourOf, rank, alpha = 1 }: {
    onClick: (item: string) => void;
    centre?: DrawnPoint;
    /** An item's colour (an unlinked graphic's is asked with null), or null to leave it out; by default its status's. */
    colourOf?: (item: string | null, status: DrawnStatus | undefined) => string | null;
    /** Which of several items sharing a graphic it is drawn as: the lowest rank, before the item Specify knows. */
    rank?: (item: string) => number;
    /** Below 1, everything is drawn see-through. */
    alpha?: number;
  },
): DrawnLayer {
  const { scene } = nav;
  // One line and one fill material per colour (a status's, as its semantic labels have it) and weight.
  const inks = new Map<string, { line: LineMaterial; fill: THREE.MeshBasicMaterial }>();
  const ink = (colour: string, width = LINE_PX) => {
    let made = inks.get(`${colour} ${width}`);
    if (!made) {
      made = {
        line: new LineMaterial({
          color: new THREE.Color(colour).getHex(), linewidth: width, worldUnits: false,
          depthTest: false, depthWrite: false, alphaToCoverage: alpha >= 1,
          ...(alpha < 1 ? { transparent: true, opacity: alpha } : {}),
        }),
        fill: new THREE.MeshBasicMaterial({
          color: colour, side: THREE.DoubleSide, depthTest: false, depthWrite: false,
          ...(alpha < 1 ? { transparent: true, opacity: alpha } : {}),
        }),
      };
      inks.set(`${colour} ${width}`, made);
    }
    return made;
  };
  let drawn: Drawn[] = [];
  // Text set by a stated placement, re-oriented and re-sized as the camera moves: a file's by its
  // stated height, generated text (``set``) as a label, centred in its cell.
  // ``cell``: the height its set text may fill, in millimetres; a notes block's line pitch.
  let placed: { inner: HTMLElement; at: TextPlacement; set: boolean; cell?: number }[] = [];
  let visible = false;
  let show: DrawnShow = { default: true, existing: true, general: true };
  let farSide = false;
  let reveal: ReadonlySet<string> = new Set();
  let lit: string | null = null;
  let pmiNow: DrawnPmi | null = null;
  const revealed = (d: { item: string | null }) => d.item !== null && reveal.has(d.item);
  const shownBy = (d: { item: string | null; source: Drawn['source']; status: DrawnStatus | undefined }) =>
    revealed(d) || drawnShown(d.source, d.status, show);
  const on = (d: Drawn) => visible && d.shown && !(farSide && d.away && !revealed(d));

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
    // Each linked item shown, and its colour as drawn, one a line: `item colour`.
    container.dataset['drawnItems'] = [...new Set(shownNow.filter((d) => d.item !== null)
      .map((d) => `${d.item} ${d.item === lit ? LIT_INK : d.colour}`))].join('\n');
  }

  // An item's graphics in its own colour, or lit.
  function paint(d: Drawn) {
    const colour = d.item !== null && d.item === lit ? LIT_INK : d.colour;
    const { line, fill } = ink(colour, d.width);
    for (const o of d.objects) {
      if (o instanceof LineSegments2) o.material = line;
      else if (o instanceof THREE.Mesh) o.material = fill;
      else if (o instanceof CSS2DObject) (o.element.firstElementChild as HTMLElement | null)?.style.setProperty('color', colour);
    }
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
    // What haecceity generates, with its polylines' roles: its text is set here, so its strokes are not drawn.
    const roles = graphic.polylineRoles;
    const setting = graphic.source === 'generated' && roles !== undefined;
    const width = setting ? SET_LINE_PX : LINE_PX;
    const { line: material, fill } = ink(colour, width);
    const lines = graphic.polylines.filter((_, k) => !setting || roles[k] !== 'text');
    const segments: THREE.Vector3[] = [];
    for (const line of lines) {
      for (let k = 1; k < line.length; k++) segments.push(new THREE.Vector3(...line[k - 1]!), new THREE.Vector3(...line[k]!));
    }
    const triangles = graphic.triangles.flatMap((t) => t.map((p) => new THREE.Vector3(...p)));
    const objects: THREE.Object3D[] = [];
    if (segments.length) {
      const geometry = new LineSegmentsGeometry();
      geometry.setPositions(segments.flatMap((p) => [p.x, p.y, p.z]));
      const strokes = new LineSegments2(geometry, material);
      strokes.renderOrder = 10;
      objects.push(strokes);
    }
    if (triangles.length) {
      const geometry = new THREE.BufferGeometry().setFromPoints(triangles);
      const mesh = new THREE.Mesh(geometry, fill);
      mesh.renderOrder = 10;
      objects.push(mesh);
    }
    // A file's text strings (its text literals) have no strokes, so they are set here, where the
    // file places them, or, with no stated placement, at the middle of the graphic's own lines and
    // triangles. haecceity strokes the text of what it generates into the polylines; with their
    // roles stated, that text is set here instead, each at its placement, and without them its
    // strokes are drawn.
    const texts = graphic.source === 'file' || setting ? graphic.text : [];
    const block = setting && graphic.text.length > 1 && !roles.includes('frame');
    const box = new THREE.Box3().setFromPoints([...segments, ...triangles]);
    const set: HTMLElement[] = [];
    // A block's lines share its smallest cell, so no line is set taller than the pitch.
    const heights = (graphic.textPlacements ?? []).flatMap((t) => (t.cellBox ? [Math.abs(t.cellBox[1][1] - t.cellBox[0][1])] : []));
    const pitch = block && heights.length ? Math.min(...heights) : undefined;
    for (const [i, text] of texts.entries()) {
      const at = graphic.textPlacements?.find((t) => t.textIndex === i);
      if (setting && !at) continue;
      if (!at && box.isEmpty()) continue;
      const el = document.createElement('div');
      el.className = `pmi-drawn${at ? ' pmi-drawn-placed' : ''}${setting ? ' pmi-drawn-set' : ''}`;
      const inner = document.createElement('span');
      inner.textContent = text;
      inner.style.color = colour;
      if (alpha < 1) inner.style.opacity = String(alpha);
      // Unplaced, a graphic's texts one under another.
      if (!at && i) inner.style.transform = `translateY(${1.2 * i}em)`;
      if (setting) {
        // As a semantic label is set (PmiLab.css's .pmi); clicked and pointed at through the
        // canvas (``hit``), so a drag that starts on it still turns the view.
        inner.style.fontFamily = "'IBM Plex Mono', ui-monospace, monospace";
        inner.style.fontWeight = '600';
        inner.style.fontVariantNumeric = 'tabular-nums';
        set.push(inner);
      } else if (item !== null) {
        const id = item;
        inner.style.pointerEvents = 'auto';
        inner.style.cursor = 'pointer';
        inner.addEventListener('pointerdown', (event) => event.stopPropagation());
        inner.addEventListener('click', (event) => { event.stopPropagation(); onClick(id); });
      }
      el.append(inner);
      const tag = new CSS2DObject(el);
      if (at && setting && at.cellBox) {
        // In its cell, which haecceity gives in the plane's x and up from the origin: centred, as
        // in a frame's compartment or on a dimension line; a block of lines (a notes block) from
        // its cells' left, so its lines start together whatever the face's widths.
        const [[x0, y0], [x1, y1]] = at.cellBox;
        const x = new THREE.Vector3(...at.xAxis).normalize();
        const y = new THREE.Vector3(...at.normal).cross(x).normalize();
        const centre = new THREE.Vector3(...at.origin).addScaledVector(x, block ? x0 : (x0 + x1) / 2).addScaledVector(y, (y0 + y1) / 2);
        tag.position.copy(centre);
        placed.push({ inner, at: { ...at, origin: [centre.x, centre.y, centre.z], alignment: block ? 'middle left' : 'centre' }, set: true, cell: pitch ?? Math.abs(y1 - y0) });
      } else if (at) {
        tag.position.set(...at.origin);
        placed.push({ inner, at, set: setting });
      } else tag.position.copy(box.getCenter(new THREE.Vector3()));
      objects.push(tag);
    }
    scene.add(...objects);
    return {
      item, name: graphic.name, source: graphic.source, status, colour, width, set,
      normal: graphicNormal(graphic, pmiNow?.presentation.planes ?? [], centre),
      shown: shownBy({ item, source: graphic.source, status }), away: false,
      polylines: lines.filter((l) => l.length > 1).length, segments, triangles, objects,
      extent: new THREE.Box3().setFromPoints([...graphic.polylines.flat(), ...graphic.triangles.flat()].map((q) => new THREE.Vector3(...q))),
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
  const across = new THREE.Vector2();
  const right = new THREE.Vector3();
  function orient() {
    right.set(1, 0, 0).applyQuaternion(nav.camera.quaternion);
    for (const { inner, at, set, cell: height } of placed) {
      const origin = new THREE.Vector3(...at.origin);
      if (!screen(origin, o)) continue;
      const x = new THREE.Vector3(...at.xAxis).normalize();
      const y = new THREE.Vector3(...at.normal).cross(x).normalize();
      const size = at.height ?? 0;
      if (set) {
        // Generated text lies in its plane as its strokes would: the text's x and up are the
        // plane's x and up as seen, so it shears and foreshortens with the plane and sits square in
        // its cell; seen from behind, it reads forwards. Sized by a millimetre across the view, so
        // a plane seen at a slant squeezes its text rather than growing it: capitals as tall as the
        // strokes', never smaller than a label but never taller than its cell, so a notes block's
        // lines stay apart. Near edge-on it cannot be read, and is left to its plane's lines.
        screen(along.copy(origin).add(x), ex);
        screen(along.copy(origin).add(y), up);
        screen(along.copy(origin).add(right), across);
        const [ax, ay, ux, uy] = [ex.x - o.x, ex.y - o.y, up.x - o.x, up.y - o.y];
        const mm = o.distanceTo(across);
        const edgeOn = mm < 1e-6 || Math.min(Math.hypot(ax, ay), Math.hypot(ux, uy)) / mm < EDGE_ON;
        inner.style.visibility = edgeOn ? 'hidden' : '';
        if (edgeOn) continue;
        const flip = ax * -uy - ay * -ux < 0 ? -1 : 1;
        const m = [flip * ax / mm, flip * ay / mm, -ux / mm, -uy / mm].map((n) => n.toFixed(4)).join(', ');
        const own = (mm * size) / CAP_HEIGHT;
        const cell = height ? mm * height : Infinity;
        inner.style.fontSize = `${Math.min(Math.max(own, SET_TEXT_PX), cell).toFixed(1)}px`;
        inner.style.transform = `matrix(${m}, 0, 0) ${shift(at.alignment)}`;
        continue;
      }
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
        const ranked = (item: string) => rank?.(item) ?? 0;
        const items = Object.entries(pmi.presentation.items)
          .sort(([a], [b]) => ranked(a) - ranked(b) || known(a) - known(b));
        const seen = new Set<string>();
        const once = (g: DrawnGraphic) => {
          if (g.id === undefined) return true;
          if (seen.has(g.id)) return false;
          seen.add(g.id);
          return true;
        };
        const colourFor = (item: string | null, status: DrawnStatus | undefined) =>
          (colourOf ? colourOf(item, status) : GROUP_COLOUR[status ?? 'existing']);
        for (const [item, graphics] of items) {
          const status = pmi.links[item]?.status;
          const colour = colourFor(item, status);
          // A graphic left out here is still seen, so another item sharing it is not drawn with it.
          for (const g of graphics) if (once(g) && colour !== null) drawn.push(place(g, item, status, colour));
        }
        const loose = colourFor(null, undefined);
        for (const g of pmi.presentation.unlinked) if (once(g) && loose !== null) drawn.push(place(g, null, undefined, loose));
        for (const d of drawn) paint(d);
      }
      turn();
      apply();
    },
    setVisible(next) {
      visible = next;
      if (next) { turn(); orient(); }
      apply();
    },
    filter(next, hideFar, chosen = new Set()) {
      show = next;
      farSide = hideFar;
      reveal = chosen;
      for (const d of drawn) d.shown = shownBy(d);
      turn();
      apply();
    },
    light(item) {
      if (item === lit) return;
      const was = lit;
      lit = item;
      for (const d of drawn) if (d.item !== null && (d.item === was || d.item === item)) paint(d);
      counts();
    },
    bounds() {
      const box = new THREE.Box3();
      for (const d of drawn) {
        if (!on(d)) continue;
        box.union(d.extent);
        for (const o of d.objects) if (o instanceof CSS2DObject) box.expandByPoint(o.position);
      }
      return box.isEmpty() ? null : box;
    },
    hit(x, y) {
      if (!visible) return null;
      p.set(x, y);
      let best: string | null = null;
      let reach = HIT_PX;
      for (const d of drawn) {
        if (d.item === null || !on(d)) continue;
        // Its set text, where it is seen: on the text renderer's overlay, which lies over the view.
        for (const el of d.set) {
          const r = el.getBoundingClientRect();
          const view = el.parentElement?.parentElement?.getBoundingClientRect();
          if (!r.width || !view || el.style.visibility === 'hidden') continue;
          if (x >= r.left - view.left && x <= r.right - view.left && y >= r.top - view.top && y <= r.bottom - view.top) return d.item;
        }
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
