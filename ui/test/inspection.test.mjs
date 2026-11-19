import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine } from '@draftwright/specify-core-wasm';
import { characteristics, noteStatements, toCsv, balloonsBySource } from '../src/specify/characteristics.ts';
import { inspectionAnswers } from '../src/specify/inspection.ts';
import { withCharacteristicNumbers } from '../src/engine/characteristic-numbers.ts';
import { as9102 } from '../src/specify/as9102.ts';

await init({ module_or_path: readFileSync(new URL('specify_core_wasm_bg.wasm', import.meta.resolve('@draftwright/specify-core-wasm'))) });
const fixture = gunzipSync(readFileSync(new URL('./fixtures/gramil_shaft_before.step.gz', import.meta.url)));
const titles = new Map();

function open(t, bytes = fixture) {
  const engine = new IntentEngine('shaft.step', bytes);
  t.after(() => engine.free());
  return engine;
}

// This is the worker's review-given path, using the engine's own scene allocation for numbers.
function inspect(engine, answers) {
  const { analysis } = JSON.parse(engine.snapshot());
  const review = JSON.parse(engine.review(JSON.stringify(answers)));
  const scene = JSON.parse(engine.pmiScene(JSON.stringify({ answers })));
  const intent = withCharacteristicNumbers(review.intent, scene);
  return { analysis, intent, rows: characteristics(intent, analysis, titles) };
}

function assertExportNumbers(rows, intent, analysis) {
  const csv = toCsv(rows).trimEnd().split('\n').slice(1);
  assert.deepEqual(csv.map((line) => line.split(',')[0]), rows.map((row) => row.n ? String(row.n) : ''));
  const [, , form3] = as9102({ name: 'shaft' }, rows, new Set(), noteStatements(intent, analysis));
  assert.deepEqual(form3.rows.slice(4).map((row) => row[0]), rows.map((row) => row.n ? row.id : ''));
}

test('every ISO 2768-m requirement is exported even when absent from the defaults preview', (t) => {
  const engine = open(t);
  const answers = { 'part.general_tolerance': 'ISO 2768-m' };
  const raw = JSON.parse(engine.review(JSON.stringify(answers))).intent;
  const { rows, intent, analysis } = inspect(engine, answers);
  assert.equal(raw.general.length, 11);
  assert.equal(rows.length, 12, 'all eleven given-only general dimensions and the class note are included');
  assert.deepEqual(rows.filter((row) => row.source.startsWith('general:')).map((row) => row.text).sort(),
    characteristics(raw, analysis, titles).filter((row) => row.source.startsWith('general:')).map((row) => row.text).sort());
  assert.equal(toCsv(rows).trimEnd().split('\n').length, 13);
  assert.equal(as9102({ name: 'shaft' }, rows, new Set(), [])[2].rows.length, 16);
  assertExportNumbers(rows, intent, analysis);
});

test('Material stays unnumbered in display and exports because STEP assigns it no characteristic ID', (t) => {
  const engine = open(t);
  const answers = { 'part.material': 'Aluminium 6082-T6' };
  const { rows, intent, analysis } = inspect(engine, answers);
  assert.equal(rows.length, 1);
  // Changed expectation: Material's old row-order balloon 12 was invented by the UI. The
  // engine's characteristic-number contract treats material as an unnumbered part setting.
  assert.equal(rows[0].n, 0);
  assert.equal(toCsv(rows).split('\n')[1], ',,Part,Material: Aluminium 6082-T6,,,,,,');
  assert.deepEqual([...balloonsBySource(rows)], []);
  assertExportNumbers(rows, intent, analysis);
});

test('given and kept answers exclude unapproved host suggestions before the review is run', (t) => {
  const engine = open(t);
  const answers = { 'part.material': 'Steel' };
  const defaults = JSON.parse(engine.acceptDefaults(JSON.stringify(answers)));
  const { questions } = JSON.parse(engine.review(JSON.stringify(defaults)));
  const pending = Object.keys(defaults).filter((id) => !(id in answers));
  const given = inspectionAnswers(defaults, questions, ['part.surface_finish', 'hole.function:holes:13'], pending);
  assert.deepEqual(given, {
    'part.material': 'Steel', 'part.surface_finish': 'Ra 3.2', 'hole.function:holes:13': defaults['hole.function:holes:13'],
  });
  const { rows, intent, analysis } = inspect(engine, given);
  assert.deepEqual(Object.keys(intent.part).sort(), ['material', 'surface_finish']);
  assert.deepEqual(intent.requirements.map((item) => item.question), ['hole.function:holes:13']);
  assert.deepEqual(intent.general, [], 'no unapproved general tolerance or dependent dimensions');
  assert.equal(rows.length, 3, 'the given material, kept finish and kept thread are all included');
  assertExportNumbers(rows, intent, analysis);
  assert.deepEqual(inspectionAnswers(answers, questions, ['part.surface_finish']), { ...answers, 'part.surface_finish': 'Ra 3.2' });
});

test('write → readback → export preserves the M4 thread ID and appends new requirements above it', (t) => {
  const engine = open(t);
  const answers = { 'hole.function:holes:13': 'tapped:M4' };
  const before = inspect(engine, answers);
  assert.equal(before.rows[0].n, 1, 'the engine assigns the first thread number 1');
  const written = JSON.parse(engine.writePmi(JSON.stringify(answers)));
  const resumed = open(t, new Uint8Array(written.bytes));
  const stored = JSON.parse(resumed.snapshot()).analysis;
  assert.deepEqual(stored.characteristics.map((row) => [row.kind, row.faces, row.number]), [['thread', [13], 1]]);
  const after = inspect(resumed, stored.resume.answers);
  assert.deepEqual(after.rows.map((row) => [row.text, row.n]), before.rows.map((row) => [row.text, row.n]));
  assertExportNumbers(after.rows, after.intent, after.analysis);
  const nextAnswers = { ...stored.resume.answers, 'datum.A': [0], 'part.surface_finish': 'Ra 3.2' };
  const planned = inspect(resumed, nextAnswers);
  assert.equal(planned.rows.find((row) => row.question === 'hole.function:holes:13').n, 1);
  assert.ok(planned.rows.filter((row) => row.question !== 'hole.function:holes:13').every((row) => row.n > 1));
  const next = JSON.parse(resumed.writePmi(JSON.stringify(nextAnswers)));
  const again = open(t, new Uint8Array(next.bytes));
  const readback = inspect(again, JSON.parse(again.snapshot()).analysis.resume.answers);
  assert.deepEqual(readback.rows.map((row) => [row.text, row.n]), planned.rows.map((row) => [row.text, row.n]));
  assert.deepEqual(readback.rows.map((row) => row.n), readback.analysis.characteristics.map((row) => row.number).sort((a, b) => a - b));
  assertExportNumbers(readback.rows, readback.intent, readback.analysis);
});

test('all general-dimension numbers agree with the written STEP, including planned hole depth', (t) => {
  const engine = open(t);
  const answers = { 'part.general_tolerance': 'ISO 2768-m' };
  const before = inspect(engine, answers);
  const written = JSON.parse(engine.writePmi(JSON.stringify(answers)));
  const resumed = open(t, new Uint8Array(written.bytes));
  const after = inspect(resumed, JSON.parse(resumed.snapshot()).analysis.resume.answers);
  const stored = after.analysis.characteristics.map((row) => row.number).sort((a, b) => a - b);
  assert.deepEqual(before.rows.filter((row) => row.n).map((row) => row.n), stored);
  assert.deepEqual(after.rows.filter((row) => row.n).map((row) => row.n), stored);
  assert.deepEqual(after.rows.map((row) => [row.text, row.n]), before.rows.map((row) => [row.text, row.n]));
  assertExportNumbers(after.rows, after.intent, after.analysis);
});

test('imported existing numbers and numbered pattern measurements retain their IDs', () => {
  const analysis = {
    faces: [{ id: 1, kind: 'cylinder', radius: 5 }, { id: 2, kind: 'cylinder', radius: 5 }],
    features: [
      { id: 'one', faces: [1] }, { id: 'two', faces: [2] },
      { id: 'pattern', faces: [1, 2], members: ['one', 'two'] },
    ], existing: [{ kind: 'dimension', type: 'Size_Diameter', faces: [1], value: 10, number: 42 }], part: {},
  };
  const intent = { datums: [], general: [], part: {}, requirements: [
    { kind: 'size', feature: 'pattern', faces: [1, 2], nominal: 10, upper: 0.1, lower: 0, question: 'size', number: 57 },
  ] };
  const rows = characteristics(intent, analysis, titles);
  assert.deepEqual(rows.map((row) => [row.n, row.sub, row.id]), [[42, 0, '42'], [57, 1, '57.1'], [57, 2, '57.2']]);
  assertExportNumbers(rows, intent, analysis);
});
