import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { linksFromPmi, readPresentation } from '../src/specify/drawn.ts';

// The spindle's haecceity.presentation/1 and haecceity.pmi/1, from quiddity's json feature
// (scripts/fixtures/README.md).
const fixture = (kind) => JSON.parse(gunzipSync(readFileSync(`scripts/fixtures/19-spindle-semantic-links-only.${kind}.json.gz`)));
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

test('fixture links: each item\'s faces from the PMI document, refused for another document', () => {
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
