import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { chooseFaceRoute, faceSamples, withFaceRoute } from '../src/specify/host-view.ts';
import { hostViewOf } from '../src/specify/default-view.ts';

// Every member of the interface (docs/host-view.md), as the components call them.
const MEMBERS = ['info', 'showParts', 'styleFaces', 'clearStyles', 'setOpacity', 'addMesh', 'removeMesh', 'lookFrom', 'fit', 'home',
  'onCamera', 'onPointer', 'onUserMoved', 'rect', 'dispose'];

// A two-triangle mesh of two faces.
const mesh = {
  format: 'pmi-assist-mesh', version: 2, binding: { source_sha256: 'x', face_count: 2, loader_version: 2 },
  bbox: [[0, 0, 0], [1, 1, 0]], positions: [0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0], indices: [0, 1, 2, 0, 2, 3],
  faces: [[0, 1], [1, 1]], missing: [], edges: [],
};

/** cad-view's CadView, recording what it is asked. */
function fakeCad() {
  const calls = [];
  const canvas = Object.assign(new EventTarget(), { getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 80 }) });
  const camera = new THREE.PerspectiveCamera();
  const cad = {
    canvas, camera, diagonal: 1, viewer: { setOpacity: (o) => calls.push(['setOpacity', o]) }, overlayHost: canvas,
    size: () => [100, 80], onFrame() {}, onMoved() {}, facesAt: () => [], alsoFrame() {},
    replace: (group, ...leaves) => calls.push(['replace', group, ...leaves.map(l => `${l.name}:${l.faces.join(',')}:${l.colour}`)]),
    show: (path, on) => calls.push(['show', path, on]),
    transparent: (on) => calls.push(['transparent', on]),
    lookFrom: (d, box, up) => calls.push(['lookFrom', d, box && [box.min.toArray(), box.max.toArray()], up]),
    fit() {}, home() {}, roll() {}, dolly() {}, collapse() {}, showTools() {}, light() {}, moved: () => false, dispose: () => calls.push(['dispose']),
  };
  return { cad, calls };
}

test('the default view implements the host view, and its faces always pass the check on load', async () => {
  const { cad, calls } = fakeCad();
  const view = hostViewOf(cad, { part: 3, mesh });
  for (const member of [...MEMBERS, 'roll', 'dolly', 'checkFaces', 'partFaces']) assert.equal(typeof view[member], 'function', member);
  const info = await view.info();
  assert.equal(info.faceIds, 'step-entity');
  assert.equal(info.canDrawMeshes, true);
  const samples = faceSamples([{ id: 0, kind: 'plane', area: 0.5, centroid: [0.6, 0.3, 0] }, { id: 1, kind: 'plane', area: 0.5, centroid: [0.3, 0.6, 0] }]);
  assert.equal(await chooseFaceRoute(view, info, 3, samples, Math.SQRT2), 'step-entity');
  // A layer is a leaf of the tree, in its group; faces of other parts are not this view's.
  view.styleFaces('Hole/Picked', [{ faces: [{ part: 3, face: 1 }, { part: 4, face: 0 }], colour: '#003b73' }]);
  assert.deepEqual(calls.find(c => c[0] === 'replace' && c[1] === 'Hole'), ['replace', 'Hole', 'Picked:1:#003b73']);
  view.clearStyles('Hole/Picked');
  assert.deepEqual(calls.filter(c => c[0] === 'replace' && c[1] === 'Hole').at(-1), ['replace', 'Hole', 'Picked::#c9ced4']);
  // Framing faces frames their points.
  view.lookFrom([0, 0, 1], { frame: [{ part: 3, face: 0 }] });
  assert.deepEqual(calls.at(-1), ['lookFrom', [0, 0, 1], [[0, 0, 0], [1, 1, 0]], undefined]);
  // A camera listener hears the camera at once, as plain data.
  let heard = null;
  view.onCamera(state => { heard = state; })();
  assert.equal(heard.view.length, 16);
  assert.deepEqual([heard.width, heard.height], [100, 80]);
  assert.doesNotThrow(() => JSON.stringify(heard));
});

const info = (faceIds, extra = {}) => ({ faceIds, canDrawMeshes: true, ...extra });
const host = (answers) => {
  const asked = [];
  return { asked, checkFaces: async (_part, ids, samples) => { asked.push(ids); return samples.map(() => answers[ids]); }, useFaceIds: (ids) => asked.push(`use ${ids}`) };
};
const samples = faceSamples([
  { id: 0, kind: 'plane', area: 2, centroid: [0, 0, 0] },
  { id: 1, kind: 'other', area: 0, centroid: [0, 0, 0] },
  { id: 2, kind: 'cylinder', area: 3, centroid: [1, Number.NaN, 0] },
  { id: 3, kind: 'cylinder', area: 3, centroid: [1, 2, 3] },
], [101, 102, 103, 104]);

test('a face without a measured area or centroid is left out of the sample', () => {
  assert.deepEqual(samples, [
    { face: 0, entity: 101, kind: 'plane', area: 2, centroid: [0, 0, 0] },
    { face: 3, entity: 104, kind: 'cylinder', area: 3, centroid: [1, 2, 3] },
  ]);
});

test('the check on load keeps a host numbering that matches', async () => {
  const h = host({ mapshapes: true });
  assert.equal(await chooseFaceRoute(h, info('mapshapes', { stepEntities: true }), 0, samples, 10), 'mapshapes');
  assert.deepEqual(h.asked, ['mapshapes']);
});

test('on a mismatch it switches to STEP entities when the host can match by them', async () => {
  const h = host({ mapshapes: false, 'step-entity': true });
  assert.equal(await chooseFaceRoute(h, info('mapshapes', { stepEntities: true }), 0, samples, 10), 'step-entity');
  assert.deepEqual(h.asked, ['mapshapes', 'step-entity', 'use step-entity']);
});

test('else to meshes, or to nothing when the host cannot draw them', async () => {
  assert.equal(await chooseFaceRoute(host({ mapshapes: false }), info('mapshapes'), 0, samples, 10), 'meshes');
  assert.equal(await chooseFaceRoute(host({ mapshapes: false, 'step-entity': false }), info('mapshapes', { stepEntities: true }), 0, samples, 10), 'meshes');
  assert.equal(await chooseFaceRoute(host({ 'step-entity': false }), info('step-entity', { canDrawMeshes: false }), 0, samples, 10), 'none');
  const failing = { checkFaces: async () => { throw new Error('no'); } };
  assert.equal(await chooseFaceRoute(failing, info('step-entity'), 0, samples, 10), 'meshes');
  // A host that cannot be checked is taken at its word.
  assert.equal(await chooseFaceRoute({}, info('mapshapes'), 0, samples, 10), 'mapshapes');
});

test('through meshes, the part is given as a mesh and styles as meshes of their faces', () => {
  const calls = [];
  let pointer = null;
  const view = {
    showParts: (p) => calls.push(['showParts', p]), styleFaces: (l) => calls.push(['styleFaces', l]), clearStyles: (l) => calls.push(['clearStyles', l]),
    addMesh: (id, m) => calls.push(['addMesh', id, m.faces.map(([, n]) => n)]), removeMesh: (id) => calls.push(['removeMesh', id]),
    onPointer: (f) => { pointer = f; return () => {}; },
  };
  const routed = withFaceRoute(view, 2, mesh);
  routed.styleFaces('Picked', [{ faces: [{ part: 2, face: 1 }], colour: 'navy' }]);
  assert.deepEqual(calls.splice(0), [['styleFaces', 'Picked']]);
  routed.route('meshes');
  assert.deepEqual(calls.splice(0), [
    ['clearStyles', 'Picked'], ['showParts', []], ['addMesh', 'specify:part', [1, 1]], ['addMesh', 'specify:layer:0:Picked', [0, 1]],
  ]);
  const heard = [];
  routed.onPointer((e) => heard.push(e));
  pointer({ type: 'click', x: 1, y: 1, hit: { part: 0, face: 1 }, mesh: 'specify:layer:0:Picked' });
  pointer({ type: 'click', x: 1, y: 1, hit: { part: 2, face: 0 } });
  assert.deepEqual(heard.map(e => e.hit), [{ part: 2, face: 1 }, null]);
  routed.route('none');
  routed.styleFaces('Picked', []);
  assert.deepEqual(calls.splice(0), [['removeMesh', 'specify:layer:0:Picked'], ['removeMesh', 'specify:part'], ['showParts', [2]]]);
});
