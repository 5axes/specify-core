import { test } from 'node:test';
import assert from 'node:assert/strict';
import { characteristics } from '../src/specify/characteristics.ts';

// Two ⌀20 holes in a pattern, as Specify's analysis lists them (CTC-01's shape).
const analysis = {
  faces: [
    { id: 1, kind: 'cylinder', radius: 10 }, { id: 2, kind: 'cylinder', radius: 10 },
  ],
  features: [
    { id: 'holes:1', family: 'holes', faces: [1] },
    { id: 'holes:2', family: 'holes', faces: [2] },
    { id: 'hole_patterns.set:1.2', family: 'hole_patterns', faces: [1, 2], members: ['holes:1', 'holes:2'] },
  ],
  existing: [],
  part: {},
};
const intent = (requirements) => ({ requirements, general: [], datums: [], part: {} });

test('a feature with no question is named by its faces, never by its id', () => {
  const rows = characteristics(intent([
    { kind: 'size', feature: 'holes:1', faces: [1], nominal: 20 },
    { kind: 'size', feature: 'holes:1,holes:2', faces: [], nominal: 20 },
  ]), analysis, new Map());
  assert.deepEqual(rows.map((r) => r.feature), ['2× ⌀20 holes, hole 1', '2× ⌀20 holes']);
});

test('a requirement Specify did not write is not listed', () => {
  const rows = characteristics(intent([
    { kind: 'size', feature: 'holes:1', faces: [1], nominal: 20 },
    { kind: 'size', feature: 'holes:1,holes:2', faces: [], nominal: 20, not_written_reason: 'an answered requirement covers it' },
  ]), analysis, new Map());
  assert.deepEqual(rows.map((r) => r.text), ['⌀20']);
});

test('several features are named once, plural only for a repeated hole or face', () => {
  const mixed = { ...analysis, faces: [...analysis.faces, { id: 3, kind: 'cylinder', radius: 12.5 }],
    features: [...analysis.features, { id: 'holes:3', family: 'holes', faces: [3] }] };
  const rows = characteristics(intent([
    { kind: 'size', feature: 'holes:1,holes:3', faces: [], nominal: 20 },
  ]), mixed, new Map());
  assert.deepEqual(rows.map((r) => r.feature), ['⌀20 hole, ⌀25 hole']);
});

test('a requirement Specify did not write gets no label on the part', async () => {
  const { toLabels } = await import('../src/specify/labels.ts');
  const labels = toLabels(intent([
    { kind: 'size', feature: 'holes:1', faces: [1], nominal: 20 },
    { kind: 'size', feature: 'holes:1,holes:2', faces: [1, 2], nominal: 20, not_written_reason: 'covered' },
  ]), analysis);
  assert.deepEqual(labels.map((l) => l.id.split(':').slice(0, 2).join(':')), ['req:0']);
});

test('a dimension the file states with no value is listed as such, never as 0', () => {
  // CTC-01 states two of its ⌀35 sizes and two locations with no value, only presentation.
  const stated = { ...analysis, existing: [
    { kind: 'dimension', type: 'Size_Diameter', faces: [1], value: 20, upper: 0.1, lower: 0 },
    { kind: 'dimension', type: 'Size_Diameter', faces: [1] },
    { kind: 'dimension', type: 'Location_LinearDistance', faces: [2], faces2: [1] },
  ] };
  const rows = characteristics(intent([]), stated, new Map());
  assert.deepEqual(rows.map((r) => [r.text, r.nominal]), [
    ['⌀20', 20], ['no value stated', null], ['no value stated', null],
  ]);
});
