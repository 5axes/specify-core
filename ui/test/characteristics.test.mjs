import { test } from 'node:test';
import assert from 'node:assert/strict';
import { characteristics, byBalloon, balloonsBySource, toCsv } from '../src/specify/characteristics.ts';

import { as9102 } from '../src/specify/as9102.ts';

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

test('a tolerance the file states with no value is drawn without one, never as 0', async () => {
  const { toLabels } = await import('../src/specify/labels.ts');
  const stated = { ...analysis, existing: [
    { kind: 'geometric_tolerance', type: 'Position', faces: [1], value: 0.75, datums: ['A'] },
    { kind: 'geometric_tolerance', type: 'Position', faces: [2], datums: ['A'] },
  ] };
  const frames = toLabels(intent([]), stated).filter((l) => l.kind === 'frame');
  assert.deepEqual(frames.map((l) => l.cells), [['⌖', '0.75', 'A'], ['⌖', 'A']]);
  const rows = characteristics(intent([]), stated, new Map());
  assert.deepEqual(rows.map((r) => r.text), ['⌖ 0.75', '⌖ no value stated']);
});

test("the part's notes are stated as a drawing's notes block states them", async () => {
  const { noteStatements } = await import('../src/specify/characteristics.ts');
  const part = {
    material: 'Alu_G6061_T6', general_tolerance: 'ISO 2768-mK', surface_finish: 'Ra 3.2',
    coating: 'None (as machined)', heat_treatment: 'Case harden 0.5 mm', edges: 'Remove all burrs and break sharp edges',
  };
  assert.deepEqual(noteStatements(null, { part }).map(n => [n.text, n.question]), [
    // Values as written: their case is notation (mK, Ra, Alu_G6061_T6).
    ['MATERIAL Alu_G6061_T6', 'part.material'],
    ['GENERAL TOLERANCES ISO 2768-mK', 'part.general_tolerance'],
    ['SURFACE FINISH Ra 3.2', 'part.surface_finish'],
    // Said in words, in capitals, but a unit as written; a setting that is none left out.
    ['CASE HARDEN 0.5 mm', 'part.heat_treatment'],
    ['REMOVE ALL BURRS AND BREAK SHARP EDGES', 'part.edges'],
  ]);
  // Notation in words typed in is kept as written; a typed 'none' is left out too.
  const typed = noteStatements({ part: { coating: 'Anodise 10µm, seal 1 h', heat_treatment: 'No treatment', edges: 'Fit H7/g6 edges' } }, { part: {} });
  assert.deepEqual(typed.map(n => n.text), ['ANODISE 10µm, SEAL 1 h', 'FIT H7/g6 EDGES']);
  // The answers given win over the file's.
  assert.equal(noteStatements({ part: { edges: 'No edge note' } }, { part }).some(n => n.question === 'part.edges'), false);
});

// These are origin/main's sequential balloons, pattern/basic grouping and Form 3 contents.
// No number metadata is added: existing three-method hosts must keep these expectations.
test('a three-method host preserves origin/main preview balloons, CSV and Form 3', async () => {
  const preview = { ...intent([
    { kind: 'size', feature: 'hole_patterns.set:1.2', faces: [1, 2], nominal: 20, upper: 0.1 },
    { kind: 'location', feature: 'holes:1', faces: [1], distance: 30, basic: true },
    { kind: 'location', feature: 'holes:2', faces: [2], distance: 40, basic: true },
  ]), datums: [{ letter: 'A', faces: [1] }], part: { material: 'Steel' } };
  const stored = { ...analysis, existing: [
    { kind: 'datum', type: 'A', faces: [1] },
    { kind: 'dimension', type: 'Size_Diameter', faces: [1], value: 20 },
  ] };
  const host = {
    questions: async () => ({ questions: [], intent: preview }),
    interpret: async (_id, _answers, _question, text) => ({ value: text }),
    preview: async () => preview,
  };
  assert.deepEqual(Object.keys(host), ['questions', 'interpret', 'preview']);
  const rows = characteristics(await host.preview('part', {}), stored, new Map());
  assert.deepEqual(rows.map((row) => [row.source, row.n, row.sub, row.id]), [
    ['req:0', 1, 1, '1.1'], ['req:0', 1, 2, '1.2'],
    ['req:1', 2, 1, '2.1'], ['req:2', 2, 2, '2.2'],
    ['existing:1', 3, 0, '3'], ['part:material', 4, 0, '4'],
  ]);
  assert.deepEqual(byBalloon(rows).map((group) => group.map((row) => row.id)), [['1.1', '1.2'], ['2.1', '2.2'], ['3'], ['4']]);
  assert.deepEqual([...balloonsBySource(rows)], [['req:0', [1]], ['req:1', [2]], ['req:2', [2]], ['existing:1', [3]], ['part:material', [4]]]);
  assert.deepEqual(toCsv(rows).trimEnd().split('\n').slice(1).map((line) => line.split(',').slice(0, 2)), [['1', '1'], ['1', '2'], ['2', '1'], ['2', '2'], ['3', ''], ['4', '']]);
  assert.deepEqual(as9102({ name: 'legacy' }, rows, new Set(), [])[2].rows.slice(4).map((row) => [row[0], row[3]]), [
    ['1.1', '⌀20'], ['1.2', '⌀20'], ['2.1', '30 (basic)'], ['2.2', '40 (basic)'], ['3', '⌀20'], ['4', 'Material: Steel'],
  ]);
});
