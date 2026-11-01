// The PMI on /diff's 3D view, drawn by the As drawn layer (drawn-layer.ts) over the diff's
// three-cad-viewer view: the new revision's callouts, each in its change's colour, and the old
// revision's removed ones as a translucent red ghost, as removed faces are. The layers are drawn
// in a scene of their own, by renderers of their own over the viewer's canvas, as the Specify
// view's are: nothing is drawn into the viewer's scene.
import * as THREE from 'three';
import { CSS2DRenderer } from 'three/examples/jsm/renderers/CSS2DRenderer.js';

import type { DrawnChange } from '../model/diff-pmi';
import type { PmiVec } from '../types/pmi';
import type { CadView } from './cad-view';
import { drawnViews, type DrawnPmi, type DrawnPoint, type DrawnShow } from './drawn';
import { createDrawnLayer } from './drawn-layer';

// The picture's colours, as the faces have them; unchanged callouts in the file's grey.
export const PMI_COLOUR: Record<DrawnChange, string> = {
  changed: '#c26a00', added: '#0a8f3d', removed: '#d62929', unchanged: '#5d646c',
};
const GHOST = 0.45;

export interface DiffDrawn {
  setVisible(on: boolean): void;
  /** The Show menu's kinds and far side, for unchanged items: changed, added and removed ones are always drawn. */
  filter(show: DrawnShow, farSide: boolean): void;
  /** Lights an item (the new revision's, or a removed one's), or none. */
  light(id: string | null): void;
  /** Turns square to an item's callout, framing it with the part; false if it has none drawn. */
  focus(id: string): boolean;
  /** The item drawn within a few pixels of (x, y), the new revision's first, if any. */
  hit(x: number, y: number): string | null;
  /** The item's callout as haecceity names it, for a readout. */
  name(id: string): string;
  /** Where the callouts shown are, to frame them with the part; null if none. */
  bounds(): THREE.Box3 | null;
  dispose(): void;
}

export function createDiffDrawn(
  cad: CadView,
  { now, was, change, removed, part, onClick }: {
    /** The new revision's scene, and the old one's moved onto the new part; either may be missing. */
    now: DrawnPmi | null;
    was: DrawnPmi | null;
    /** What became of each item the new scene draws, by id; the rest are unchanged. */
    change: ReadonlyMap<string, DrawnChange>;
    /** The old scene's items that were removed. */
    removed: ReadonlySet<string>;
    /** The new part's box: callouts are read from outside it, and framed with it. */
    part: THREE.Box3;
    onClick: (id: string) => void;
  },
): DiffDrawn {
  const scene = new THREE.Scene();
  const ink = new THREE.WebGLRenderer({ alpha: true, antialias: true });
  ink.setPixelRatio(window.devicePixelRatio);
  ink.domElement.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none';
  const text = new CSS2DRenderer();
  text.domElement.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none';
  text.domElement.className = 'diff-pmi-text';
  cad.overlayHost.append(ink.domElement, text.domElement);
  // What each layer draws, for a test to read: data-drawn-* as the Specify view's.
  const tally = (name: string) => {
    const el = document.createElement('div');
    el.className = name;
    el.hidden = true;
    cad.overlayHost.append(el);
    return el;
  };
  const nowTally = tally('diff-pmi-now');
  const wasTally = tally('diff-pmi-was');
  let live = true;
  const centre = part.getCenter(new THREE.Vector3()).toArray() as unknown as DrawnPoint;
  const host = {
    scene, diagonal: cad.diagonal,
    get camera() { return cad.camera; },
    size: () => cad.size(),
    onFrame: (listener: () => void) => cad.onFrame(() => { if (live) listener(); }),
  };
  host.onFrame(() => {
    const [w, h] = cad.size();
    const canvas = cad.canvas as HTMLElement;
    for (const el of [ink.domElement, text.domElement]) {
      el.style.left = `${canvas.offsetLeft}px`;
      el.style.top = `${canvas.offsetTop}px`;
    }
    ink.setSize(w, h);
    text.setSize(w, h);
    ink.render(scene, cad.camera);
    text.render(scene, cad.camera);
  });

  const changed = (id: string) => change.has(id);
  const nowLayer = createDrawnLayer(host, nowTally, {
    onClick, centre,
    colourOf: (item) => PMI_COLOUR[(item !== null && change.get(item)) || 'unchanged'],
    // A graphic several items share (a notes block) is drawn as one that changed, if one did.
    rank: (item) => (changed(item) ? 0 : 1),
  });
  const wasLayer = createDrawnLayer(host, wasTally, {
    onClick, centre, alpha: GHOST,
    colourOf: (item) => (item !== null && removed.has(item) ? PMI_COLOUR.removed : null),
    rank: (item) => (removed.has(item) ? 0 : 1),
  });
  nowLayer.set(now);
  wasLayer.set(was);
  const reveal = new Set([...change.keys(), ...removed]);

  // Square on to an item's graphics: the plane they are on, framed with the part.
  const squareOn = (pmi: DrawnPmi | null, id: string) => {
    if (!pmi?.presentation.items[id]?.length) return null;
    const views = drawnViews(pmi, (_, item) => item === id, centre);
    return views.find((v) => v.key.startsWith('plane:')) ?? views[0] ?? null;
  };
  const inScene = (id: string) => (now?.presentation.items[id]?.length ? now : removed.has(id) ? was : null);

  return {
    setVisible(on) { nowLayer.setVisible(on); wasLayer.setVisible(on); },
    filter(show, farSide) {
      nowLayer.filter(show, farSide, reveal);
      wasLayer.filter(show, farSide, reveal);
    },
    light(id) { nowLayer.light(id); wasLayer.light(id !== null && removed.has(id) ? id : null); },
    focus(id) {
      const pmi = inScene(id);
      const view = pmi && squareOn(pmi, id);
      if (!view) return false;
      const v = (p: DrawnPoint) => new THREE.Vector3(...p);
      const around = new THREE.Box3(v(view.min), v(view.max));
      cad.lookFrom([...view.eye] as PmiVec, part.clone().union(around), view.up ? [...view.up] as PmiVec : undefined);
      return true;
    },
    hit: (x, y) => nowLayer.hit(x, y) ?? wasLayer.hit(x, y),
    name: (id) => inScene(id)?.presentation.items[id]?.[0]?.name ?? '',
    bounds() {
      const a = nowLayer.bounds();
      const b = wasLayer.bounds();
      return a && b ? a.union(b) : a ?? b;
    },
    dispose() {
      live = false;
      nowLayer.dispose();
      wasLayer.dispose();
      ink.dispose();
      for (const el of [ink.domElement, text.domElement, nowTally, wasTally]) el.remove();
    },
  };
}
