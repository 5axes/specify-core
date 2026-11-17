import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { bestFitNormal, drawnFromScene, drawnShown, drawnViews, facesAway, graphicNormal, hasFileGraphics, hasGraphics, linksFromPmi, partCentre, readPresentation } from '../src/specify/drawn.ts';

// The spindle's haecceity.presentation/1 and haecceity.pmi/1, from quiddity's json feature
// (test/fixtures/README.md).
const fixture = (kind) => JSON.parse(gunzipSync(readFileSync(`test/fixtures/19-spindle-semantic-links-only.${kind}.json.gz`)));
const spindle = fixture('presentation');
const graphic = { source: 'file', name: 'x', polylines: [[[0, 0, 0], [1, 0, 0]]], triangles: [], text: ['A'], step_ids: [1], unsupported: [] };
const doc = (over = {}) => ({ schema: 'haecceity.presentation/1', length_unit: 'millimetre', items: { 1: [graphic] }, unlinked: [], semantic_ids: ['1'], ...over });

test('the spindle\'s presentation reads, its file graphics by item', () => {
  const read = readPresentation(spindle);
  assert.ok(read.ok, read.reason);
  const items = read.value.items;
  assert.equal(Object.keys(items).length, 13);
  assert.deepEqual(items['6'].map((g) => [g.source, g.name]), [['file', '37.6']]);
  assert.equal(items['12'][0].name, '⌖ ⌀0.2 A');
  // Its text is drawn as strokes: polylines, and nothing to set as text.
  const all = Object.values(items).flat();
  assert.equal(all.reduce((n, g) => n + g.polylines.length, 0), 177);
  assert.equal(all.reduce((n, g) => n + g.text.length, 0), 0);
});

test('a valid document keeps what is drawn as it is', () => {
  const read = readPresentation(doc());
  assert.ok(read.ok);
  assert.deepEqual(read.value.items['1'], [{ source: 'file', name: 'x', polylines: [[[0, 0, 0], [1, 0, 0]]], triangles: [], text: ['A'] }]);
  assert.deepEqual(read.value.semanticIds, ['1']);
});

test('an unknown version or unit is refused with why', () => {
  assert.deepEqual(readPresentation(doc({ schema: 'haecceity.presentation/2' })),
    { ok: false, reason: 'schema is "haecceity.presentation/2", not "haecceity.presentation/1"' });
  assert.match(readPresentation(doc({ length_unit: 'inch' })).reason, /length_unit is "inch"/);
  assert.match(readPresentation(null).reason, /not a JSON object/);
});

test('missing or malformed fields are refused with where', () => {
  const without = (key) => { const d = doc(); delete d[key]; return d; };
  assert.match(readPresentation(without('items')).reason, /items is not an object/);
  assert.match(readPresentation(without('unlinked')).reason, /unlinked is not an array/);
  assert.match(readPresentation(without('semantic_ids')).reason, /semantic_ids is not an array/);
  const bad = (g) => readPresentation(doc({ items: { 1: [{ ...graphic, ...g }] } })).reason;
  assert.match(bad({ source: 'drawn' }), /items\["1"\]\[0\]\.source is "drawn"/);
  assert.match(bad({ polylines: undefined }), /items\["1"\]\[0\]\.polylines is not an array/);
  assert.match(bad({ polylines: [[[0, 0]]] }), /polylines\[0\]\[0\] is not a point/);
  assert.match(bad({ triangles: [[[0, 0, 0], [1, 0, 0]]] }), /triangles\[0\] does not have 3 corners/);
  assert.match(bad({ text: [1] }), /text\[0\] is not a string/);
  assert.match(bad({ name: undefined }), /name is not a string/);
});

test('links: each item\'s faces from the PMI document, refused for another document', () => {
  const presentation = readPresentation(spindle).value;
  const links = linksFromPmi(fixture('pmi'), presentation);
  assert.ok(links.ok, links.reason);
  assert.deepEqual(links.value['3'].faces, [14]); // ⌀3.3 H11 on its bore
  assert.deepEqual(links.value['6'].faces, [7, 11, 13]); // 37.6 from datum B's faces
  assert.deepEqual(links.value['features:95add85520dac4c79ea51bf237b5ea94'].faces, [0]); // datum A
  const other = { ...presentation, semanticIds: [...presentation.semanticIds, 'extra'] };
  assert.match(linksFromPmi(fixture('pmi'), other).reason, /semantic_ids differ/);
  assert.match(linksFromPmi({ schema: 'haecceity.pmi/2', pmi: {} }, presentation).reason, /not a haecceity.pmi\/1/);
});

test('text placements read as stated, and a malformed one is refused with where', () => {
  const at = { text_index: 0, origin: [10, 20, 30], x_axis: [1, 0, 0], normal: [0, -1, 0], height: 3, alignment: 'baseline left' };
  const read = readPresentation(doc({ items: { 1: [{ ...graphic, id: 'step:9', plane_id: 'step:8', text_placements: [at] }] }, planes: [], views: [] }));
  assert.ok(read.ok, read.reason);
  assert.deepEqual(read.value.items['1'][0].id, 'step:9');
  assert.deepEqual(read.value.items['1'][0].textPlacements, [{ textIndex: 0, origin: [10, 20, 30], xAxis: [1, 0, 0], normal: [0, -1, 0], height: 3, alignment: 'baseline left' }]);
  // A placeholder: placed, with no text of its own.
  const placeholder = readPresentation(doc({ items: { 1: [{ ...graphic, text_placements: [{ origin: [0, 0, 0], x_axis: [1, 0, 0], normal: [0, 0, 1] }] }] } }));
  assert.deepEqual(placeholder.value.items['1'][0].textPlacements, [{ origin: [0, 0, 0], xAxis: [1, 0, 0], normal: [0, 0, 1] }]);
  const bad = (t) => readPresentation(doc({ items: { 1: [{ ...graphic, text_placements: [{ ...at, ...t }] }] } })).reason;
  assert.match(bad({ origin: [0, 0] }), /text_placements\[0\]\.origin is not a point/);
  assert.match(bad({ height: -1 }), /text_placements\[0\]\.height is not a length/);
  assert.match(bad({ text_index: 1.5 }), /text_index is not an index/);
  assert.match(readPresentation(doc({ items: { 1: [{ ...graphic, id: 3 }] } })).reason, /items\["1"\]\[0\]\.id is not a string/);
});

test('haecceity 0.4.4: polyline roles, a text\'s cell and whether it is stroked read as stated; malformed, refused with where', () => {
  const at = { text_index: 0, origin: [1, 2, 3], x_axis: [1, 0, 0], normal: [0, -1, 0], height: 1.2, alignment: 'centre', cell_box: [[-1.5, -1.2], [1.5, 1.2]], stroked: true };
  const g = { ...graphic, source: 'generated', polylines: [[[0, 0, 0], [1, 0, 0]], [[0, 1, 0], [1, 1, 0]]], polyline_roles: ['frame', 'text'], text_placements: [at] };
  const read = readPresentation(doc({ items: { 1: [g] } }));
  assert.ok(read.ok, read.reason);
  assert.deepEqual(read.value.items['1'][0].polylineRoles, ['frame', 'text']);
  assert.deepEqual(read.value.items['1'][0].textPlacements[0].cellBox, [[-1.5, -1.2], [1.5, 1.2]]);
  assert.equal(read.value.items['1'][0].textPlacements[0].stroked, true);
  const bad = (over, t = {}) => readPresentation(doc({ items: { 1: [{ ...g, ...over, text_placements: [{ ...at, ...t }] }] } })).reason;
  assert.match(bad({ polyline_roles: ['frame'] }), /polyline_roles has 1 roles for 2 polylines/);
  assert.match(bad({ polyline_roles: ['frame', 2] }), /polyline_roles\[1\] is not a string/);
  assert.match(bad({}, { cell_box: [[0, 0]] }), /cell_box does not have 2 corners/);
  assert.match(bad({}, { cell_box: [[0, 0], [1]] }), /cell_box\[1\] is not a point \[x, y\]/);
  assert.match(bad({}, { stroked: 'yes' }), /stroked is not a boolean/);
});

test('links: a stated material is not an item (specify-core 0.1.7, #105: semantic_ids leaves it out)', () => {
  const pmi = { schema: 'haecceity.pmi/1', pmi: { material: { id: 'steel', name: null, density: null } } };
  const links = linksFromPmi(pmi, readPresentation(doc({ items: {}, semantic_ids: [] })).value);
  assert.ok(links.ok, links.reason);
  assert.equal(links.value['part.material'], undefined);
});

// A pmiScene in small: a datum whose symbol is listed under the datum and its feature, as
// specify-core 0.1.5 lists it, with Specify's overlay for the datum only.
const scene = () => ({
  pmi: {
    schema: 'haecceity.pmi/1',
    pmi: {
      features: [{ id: 'features:f', value: { items: [{ face: 4 }, { face: 2 }] } }],
      datums: [{ id: 'datum:A', value: { features: ['features:f'] } }],
    },
  },
  presentation: doc({
    items: { 'datum:A': [{ ...graphic, id: 'step:7', name: 'Datum A' }], 'features:f': [{ ...graphic, id: 'step:7', name: 'Datum A' }] },
    semantic_ids: ['datum:A', 'features:f'],
  }),
  specify: { 'datum:A': { status: 'answered', question: 'datum.A', number: 1 } },
});

test('a PMI scene: each item\'s faces, and Specify\'s status and question where it has them', () => {
  const read = drawnFromScene(scene());
  assert.ok(read.ok, read.reason);
  assert.deepEqual(read.value.links['datum:A'], { faces: [2, 4], question: 'datum.A', status: 'answered' });
  assert.deepEqual(read.value.links['features:f'], { faces: [2, 4] });
  assert.equal(hasGraphics(read.value), true);
  const empty = drawnFromScene({ ...scene(), presentation: doc({ items: {}, semantic_ids: ['datum:A', 'features:f'] }) });
  assert.equal(hasGraphics(empty.value), false);
  // View PMI opens in Graphical only when a graphic is the file's own, not Specify's.
  assert.equal(hasFileGraphics(read.value), true);
  assert.equal(hasFileGraphics(empty.value), false);
  const generated = drawnFromScene({ ...scene(), presentation: doc({ items: { 'datum:A': [{ ...graphic, source: 'generated' }] }, semantic_ids: ['datum:A', 'features:f'] }) });
  assert.ok(generated.ok, generated.reason);
  assert.equal(hasGraphics(generated.value), true);
  assert.equal(hasFileGraphics(generated.value), false);
  assert.match(drawnFromScene(null).reason, /not a JSON object/);
  assert.match(drawnFromScene({ ...scene(), presentation: { schema: 'x' } }).reason, /schema is "x"/);
  assert.match(drawnFromScene({ ...scene(), pmi: null }).reason, /not a haecceity.pmi\/1/);
});

test('planes, plane ids and saved views read as stated; absent, none', () => {
  const read = readPresentation(doc({
    items: { 1: [{ ...graphic, id: 'step:9', plane_id: 'p1' }] },
    planes: [{ id: 'p1', name: 'Front', origin: [0, 0, 0], x_axis: [1, 0, 0], normal: [0, -1, 0] }],
    views: [{ id: 'v1', name: 'Saved', direction: [0, 1, 0], up: [0, 0, 1], graphics: ['step:9'] }, { id: 'v2', name: 'No camera', graphics: [] }],
  }));
  assert.ok(read.ok, read.reason);
  assert.equal(read.value.items['1'][0].planeId, 'p1');
  assert.deepEqual(read.value.planes, [{ id: 'p1', name: 'Front', origin: [0, 0, 0], xAxis: [1, 0, 0], normal: [0, -1, 0] }]);
  assert.deepEqual(read.value.views, [
    { id: 'v1', name: 'Saved', graphics: ['step:9'], direction: [0, 1, 0], up: [0, 0, 1] },
    { id: 'v2', name: 'No camera', graphics: [] },
  ]);
  assert.deepEqual([readPresentation(doc()).value.planes, readPresentation(doc()).value.views], [[], []]);
  assert.match(readPresentation(doc({ planes: [{ id: 'p', origin: [0, 0], x_axis: [1, 0, 0], normal: [0, 0, 1] }] })).reason, /planes\[0\]\.origin/);
  assert.match(readPresentation(doc({ items: { 1: [{ ...graphic, plane_id: 3 }] } })).reason, /plane_id is not a string/);
});

test('the Show menu hides drawn graphics by their item\'s status, as it hides labels', () => {
  const all = { default: true, existing: true, general: true };
  const cases = [
    // source, status, the kind unchecked that hides it
    ['generated', 'general', 'general'],
    ['generated', 'default', 'default'],
    ['generated', 'existing', 'existing'],
    ['file', 'answered', 'existing'],
    ['file', undefined, 'existing'],
  ];
  for (const [source, status, kind] of cases) {
    assert.equal(drawnShown(source, status, all), true, `${source} ${status} shown with everything`);
    assert.equal(drawnShown(source, status, { ...all, [kind]: false }), false, `${source} ${status} hidden without ${kind}`);
    for (const other of Object.keys(all).filter((k) => k !== kind && !(source === 'file' && k === 'existing'))) {
      if (status === other) continue;
      assert.equal(drawnShown(source, status, { ...all, [other]: false }), true, `${source} ${status} kept without ${other}`);
    }
  }
  // Answered, and generated with no status: always drawn, as answered labels are.
  for (const status of ['answered', undefined]) assert.equal(drawnShown('generated', status, { default: false, existing: false, general: false }), true);
});

test('facing: a callout faces away when its plane\'s normal is along the view; its normal from its plane, its text or its strokes', () => {
  // Looking along +y at a plane read from -y: facing; turned 180°, away. Edge on: not away.
  assert.equal(facesAway([0, -1, 0], [0, 1, 0]), false);
  assert.equal(facesAway([0, -1, 0], [0, -1, 0]), true);
  assert.equal(facesAway([0, -1, 0], [1, 0, 0]), false);
  assert.equal(facesAway([0, -1, 0], [-0.6, 0.7, -0.4]), false);
  const planes = [{ id: 'p', name: 'p', origin: [0, 0, 0], xAxis: [1, 0, 0], normal: [0, 0, 2] }];
  const square = [[[0, 5, 0], [10, 5, 0]], [[10, 5, 0], [10, 5, 4]], [[10, 5, 4], [0, 5, 4]]];
  const g = { ...graphic, polylines: square };
  // The plane's, normalised; else the text's.
  assert.deepEqual(graphicNormal({ ...g, planeId: 'p' }, planes), [0, 0, 1]);
  assert.deepEqual(graphicNormal({ ...g, textPlacements: [{ origin: [0, 0, 0], xAxis: [1, 0, 0], normal: [1, 0, 0] }] }, planes), [1, 0, 0]);
  // Else the plane of its strokes (y = 5), turned away from the part's centre.
  const fromBelow = graphicNormal(g, planes, [5, 0, 2]);
  assert.ok(Math.abs(fromBelow[1] - 1) < 1e-9 && Math.abs(fromBelow[0]) < 1e-9 && Math.abs(fromBelow[2]) < 1e-9, `${fromBelow}`);
  const fromAbove = graphicNormal(g, planes, [5, 9, 2]);
  assert.ok(Math.abs(fromAbove[1] + 1) < 1e-9, `${fromAbove}`);
  // Through the centre, or with no centre, it has no outside: never hidden.
  assert.equal(graphicNormal(g, planes, [5, 5, 2]), null);
  assert.equal(graphicNormal(g, planes), null);
  // A slanted plane fits; a line or a spread of points does not.
  const slant = bestFitNormal([[0, 0, 0], [1, 0, 1], [0, 1, 0], [1, 1, 1], [0.5, 0.3, 0.5]]);
  assert.ok(Math.abs(Math.abs(slant[0]) - Math.SQRT1_2) < 1e-9 && Math.abs(slant[1]) < 1e-9 && Math.abs(Math.abs(slant[2]) - Math.SQRT1_2) < 1e-9, `${slant}`);
  assert.equal(bestFitNormal([[0, 0, 0], [1, 1, 1], [2, 2, 2]]), null);
  assert.equal(bestFitNormal([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 1]]), null);
  assert.deepEqual(partCentre([0, 0, 0, 10, 4, -2]), [5, 2, -1]);
});

test('views: a saved view with a camera, and one a way the shown graphics\' planes face, square to it with its text upright', () => {
  const planes = [
    { id: 'front1', name: 'f1', origin: [0, 0, 0], xAxis: [1, 0, 0], normal: [0, -1, 0] },
    { id: 'front2', name: 'f2', origin: [5, 0, 0], xAxis: [1, 0, 0], normal: [0, -1, 1e-16] },
    { id: 'top', name: 't', origin: [0, 0, 9], xAxis: [0, 1, 0], normal: [0, 0, 1] },
  ];
  const at = (id, name, plane, x) => ({ ...graphic, id, name, planeId: plane, polylines: [[[x, 0, 0], [x + 1, 0, 1]]] });
  const pmi = {
    presentation: {
      items: { a: [at('g1', '10', 'front1', 0)], b: [at('g2', '20', 'front2', 5)], c: [at('g3', '30', 'top', 9)] },
      unlinked: [], semanticIds: ['a', 'b', 'c'], planes,
      views: [{ id: 'v', name: 'Saved', direction: [0, 1, 0], up: [0, 0, 1], graphics: ['g1'] }, { id: 'w', name: 'No camera', graphics: ['g2'] }],
    },
    links: { a: { faces: [], status: 'answered' }, b: { faces: [], status: 'answered' }, c: { faces: [], status: 'general' } },
  };
  const shown = (show) => (g, item) => drawnShown(g.source === 'file' ? 'generated' : g.source, pmi.links[item]?.status, show);
  const views = drawnViews(pmi, shown({ default: true, existing: true, general: true }));
  assert.deepEqual(views.map((v) => v.name), ['Saved', 'Plane 1: 10 · 20', 'Plane 2: 30']);
  const [saved, front, top] = views;
  const near = (actual, expected) => assert.ok(actual.flat().every((x, i) => Math.abs(x - expected.flat()[i]) < 1e-9), `${JSON.stringify(actual)} is ${JSON.stringify(expected)}`);
  // The camera stands against its direction; a plane's front is along its normal, up normal × x.
  near([saved.eye, saved.up, saved.min, saved.max], [[0, -1, 0], [0, 0, 1], [0, 0, 0], [1, 0, 1]]);
  near([front.eye, front.up], [[0, -1, 0], [0, 0, 1]]);
  near([front.min, front.max], [[0, 0, 0], [6, 0, 1]]);
  near([top.eye, top.up], [[0, 0, 1], [-1, 0, 0]]);
  // A kind not shown has no view.
  assert.deepEqual(drawnViews(pmi, shown({ default: true, existing: true, general: false })).map((v) => v.name), ['Saved', 'Plane 1: 10 · 20']);  // A view Specify generates for a graphic of its own (specify-core 0.1.6) is not offered: the
  // planes offer it, grouped.
  const generated = { ...pmi, presentation: { ...pmi.presentation,
    items: { ...pmi.presentation.items, c: [{ ...at('g3', '30', 'top', 9), source: 'generated' }] },
    views: [...pmi.presentation.views, { id: 'g3/plane/view', name: '30', direction: [0, 0, -1], up: [-1, 0, 0], graphics: ['g3'] }] } };
  assert.deepEqual(drawnViews(generated, shown({ default: true, existing: true, general: true })).map((v) => v.name), ['Saved', 'Plane 1: 10 · 20', 'Plane 2: 30']);
  // A generated general dimension hidden by the Show menu: its view is not offered either (it
  // would otherwise frame every graphic, as one with no graphics of its own does).
  assert.deepEqual(drawnViews(generated, shown({ default: true, existing: true, general: false })).map((v) => v.name), ['Saved', 'Plane 1: 10 · 20']);
});

test('looking square at a plane, its text reads along its x axis: /diff\'s removed ⌖ ⌀0.2 A (#12), whose up is -Z', () => {
  // spindle_pmi_before's plane for #12 as pmiScene states it: x along +Y, facing -X, so the
  // text's up (normal × x axis) is -Z, the way an orbit control holding +Z up would turn over.
  const plane = { id: 'step:2099', name: '⌖ ⌀0.2 A', origin: [1.5, -10.587552, 1.9736], xAxis: [0, 1, 0], normal: [-1, 0, -2.22044604925e-16] };
  const frame = { ...graphic, name: '⌖ ⌀0.2 A', planeId: plane.id, polylines: [[[1.5, -10.6, 1.9], [1.5, 0, 1.9]]] };
  const pmi = { presentation: { items: { 'position:holes:15': [frame] }, unlinked: [], semanticIds: ['position:holes:15'], planes: [plane], views: [] }, links: {} };
  const [view] = drawnViews(pmi, () => true);
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const near = (a, b) => a.every((x, k) => Math.abs(x - b[k]) < 1e-9);
  assert.ok(near(view.eye, [-1, 0, 0]), `eye ${view.eye}`);
  assert.ok(near(view.up, [0, 0, -1]), `up ${view.up}`);
  // On screen, right is the look direction (−eye) × up: the plane's x axis, so the text reads left to right.
  assert.ok(near(cross(view.eye.map((x) => -x), view.up), plane.xAxis), 'screen right is the plane\'s x axis');
});
