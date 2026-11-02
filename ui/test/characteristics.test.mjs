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
