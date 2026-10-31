// A STEP's own graphical PMI on the part ("As drawn"): haecceity's polylines as Line2-family lines
// and its filled triangles as meshes, both drawn without depth testing so the part never hides a
// callout; its text as DOM text, set as text, never HTML, and never reformatted here.
//
// Drawn in the labels' scene (annotations.ts's host), so the labels' renderers draw it and it
// follows the camera as they do. A click on a linked graphic is reported by item id; graphics
// linked to no semantic item are drawn but cannot be clicked.
import * as THREE from 'three';
import { CSS2DObject } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';

import { GROUP_COLOUR, type AnnotationHost } from './annotations';
import type { DrawnGraphic, DrawnPmi } from './drawn';

const LINE_PX = 1.5;
const HIT_PX = 6;
// The file's PMI, in the colour its semantic labels have.
const INK = GROUP_COLOUR.existing;

interface Drawn {
  /** The semantic item, or null for an unlinked graphic. */
  item: string | null;
  polylines: number;
  segments: THREE.Vector3[];
  triangles: THREE.Vector3[];
  objects: THREE.Object3D[];
}

export interface DrawnLayer {
  /** The file's own graphics (``source: "file"``) of ``pmi``, or none. */
  set(pmi: DrawnPmi | null): void;
  setVisible(on: boolean): void;
  /** The linked item drawn within a few pixels of (x, y) in the view, if any. */
  hit(x: number, y: number): string | null;
  dispose(): void;
}

export function createDrawnLayer(
  nav: AnnotationHost,
  container: HTMLElement,
  { onClick }: { onClick: (item: string) => void },
): DrawnLayer {
  const { scene } = nav;
  const material = new LineMaterial({
    color: new THREE.Color(INK).getHex(), linewidth: LINE_PX, worldUnits: false,
    depthTest: false, depthWrite: false, alphaToCoverage: true,
  });
  const fill = new THREE.MeshBasicMaterial({ color: INK, side: THREE.DoubleSide, depthTest: false, depthWrite: false });
  let drawn: Drawn[] = [];
  let visible = false;

  function counts() {
    // What is drawn, for a test to compare with the JSON: nothing while hidden.
    const n = (f: (d: Drawn) => number) => (visible ? drawn.reduce((sum, d) => sum + f(d), 0) : 0);
    container.dataset['drawnPolylines'] = String(n((d) => d.polylines));
    container.dataset['drawnTriangles'] = String(n((d) => d.triangles.length / 3));
    container.dataset['drawnTexts'] = String(n((d) => d.objects.filter((o) => o instanceof CSS2DObject).length));
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
  }

  function place(graphic: DrawnGraphic, item: string | null): Drawn {
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
    // Text where the graphic is drawn: presentation/1 gives no text position, so at the middle of
    // its own lines and triangles; with neither, it has nowhere to go.
    const box = new THREE.Box3().setFromPoints([...segments, ...triangles]);
    for (const [i, text] of graphic.text.entries()) {
      if (box.isEmpty()) break;
      const el = document.createElement('div');
      el.className = 'pmi-drawn';
      const inner = document.createElement('span');
      inner.textContent = text;
      inner.style.color = INK;
      // A graphic's texts one under another.
      if (i) inner.style.transform = `translateY(${1.2 * i}em)`;
      if (item !== null) {
        const id = item;
        inner.style.pointerEvents = 'auto';
        inner.style.cursor = 'pointer';
        inner.addEventListener('pointerdown', (event) => event.stopPropagation());
        inner.addEventListener('click', (event) => { event.stopPropagation(); onClick(id); });
      }
      el.append(inner);
      const tag = new CSS2DObject(el);
      tag.position.copy(box.getCenter(new THREE.Vector3()));
      objects.push(tag);
    }
    for (const o of objects) o.visible = visible;
    scene.add(...objects);
    return { item, polylines: graphic.polylines.filter((l) => l.length > 1).length, segments, triangles, objects };
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

  nav.onFrame(() => {
    const [w, h] = nav.size();
    material.resolution.set(w, h);
  });

  return {
    set(pmi) {
      clear();
      if (pmi) {
        for (const [item, graphics] of Object.entries(pmi.presentation.items)) {
          for (const g of graphics) if (g.source === 'file') drawn.push(place(g, item));
        }
        for (const g of pmi.presentation.unlinked) if (g.source === 'file') drawn.push(place(g, null));
      }
      counts();
    },
    setVisible(on) {
      visible = on;
      for (const d of drawn) for (const o of d.objects) o.visible = on;
      counts();
    },
    hit(x, y) {
      if (!visible) return null;
      p.set(x, y);
      let best: string | null = null;
      let reach = HIT_PX;
      for (const d of drawn) {
        if (d.item === null) continue;
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
      material.dispose();
      fill.dispose();
    },
  };
}
