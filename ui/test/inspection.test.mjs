import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine, pmiScene } from '@draftwright/specify-core-wasm';
import { toCsv } from '../src/specify/characteristics.ts';
import { inspectionAnswers } from '../src/specify/inspection.ts';
import { inspectionFromStep } from '../src/specify/saved-inspection.ts';
import { rebuildSavedStep } from '../src/engine/saved-step.ts';
import { as9102 } from '../src/specify/as9102.ts';

await init({ module_or_path: readFileSync(new URL('specify_core_wasm_bg.wasm', import.meta.resolve('@draftwright/specify-core-wasm'))) });
const fixture = (name) => gunzipSync(readFileSync(new URL(`./fixtures/${name}.step.gz`, import.meta.url)));
const shaft = fixture('gramil_shaft_before');
function open(t, bytes = shaft) {
  const engine = new IntentEngine('saved.step', bytes);
  t.after(() => engine.free());
  return engine;
}
function readSaved(t, bytes) {
  const { analysis } = JSON.parse(open(t, bytes).snapshot());
  const scene = JSON.parse(pmiScene(bytes));
  return { analysis, scene, ...inspectionFromStep(analysis, scene) };
}
const write = (engine, answers) => new Uint8Array(JSON.parse(engine.writePmi(JSON.stringify(answers))).bytes);
function assertNumbers(saved) {
  const numbers = saved.analysis.characteristics?.map((row) => row.number).sort((a, b) => a - b) ?? [];
  assert.deepEqual(saved.chars.filter((row) => row.n).map((row) => row.n), numbers, 'every stored number is exported exactly once');
  const csv = toCsv(saved.chars).trimEnd().split('\n').slice(1);
  assert.deepEqual(csv.map((line) => line.split(',')[0]), saved.chars.map((row) => row.n ? String(row.n) : ''));
  const form3 = as9102({ name: 'saved' }, saved.chars, new Set(), saved.notes)[2];
  assert.deepEqual(form3.rows.slice(4).map((row) => row[0]), numbers.map(String));
}

test('saved ISO 2768-m exports every dimension in the written STEP', (t) => {
  const saved = readSaved(t, write(open(t), { 'part.general_tolerance': 'ISO 2768-m' }));
  // Decision 2026-11-19: export stored PMI, so the eleven written dimensions are included.
  // The old twelfth row was an unnumbered class note rather than an individual characteristic.
  assert.equal(saved.chars.length, 11);
  assert.equal(toCsv(saved.chars).trimEnd().split('\n').length, 12);
  assert.equal(as9102({ name: 'shaft' }, saved.chars, new Set(), saved.notes)[2].rows.length, 15);
  assertNumbers(saved);
});

test('Material appears on Form 2 only and Form 3 omits every unnumbered row', (t) => {
  const saved = readSaved(t, write(open(t), { 'part.material': 'Aluminium 6082-T6' }));
  const [, form2, form3] = as9102({ name: 'shaft' }, saved.chars, new Set(), saved.notes);
  assert.match(form2.rows.flat().join(' '), /MATERIAL:? Aluminium 6082-T6/);
  assert.equal(form3.rows.length, 4, 'headers only: Material has no stored characteristic number');
  assert.doesNotMatch(form3.rows.flat().join(' '), /6082/);
  const unnumbered = { n: 0, id: 'unnumbered', source: 'note', group: 'Part', feature: 'Part', text: 'An unnumbered note' };
  assert.equal(as9102({ name: 'shaft' }, [unnumbered], new Set(), saved.notes)[2].rows.length, 4);
  assertNumbers(saved);
});

test('given and kept answers still exclude host suggestions awaiting review', (t) => {
  const engine = open(t);
  const answers = { 'part.material': 'Steel' };
  const defaults = JSON.parse(engine.acceptDefaults(JSON.stringify(answers)));
  const { questions } = JSON.parse(engine.review(JSON.stringify(defaults)));
  const pending = Object.keys(defaults).filter((id) => !(id in answers));
  assert.deepEqual(inspectionAnswers(defaults, questions, ['part.surface_finish', 'hole.function:holes:13'], pending), {
    'part.material': 'Steel', 'part.surface_finish': 'Ra 3.2', 'hole.function:holes:13': defaults['hole.function:holes:13'],
  });
  assert.deepEqual(inspectionAnswers(answers, questions, ['part.surface_finish']), { ...answers, 'part.surface_finish': 'Ra 3.2' });
});

test('spindle exports preserve stored PMI #9 and #16–17 absent from interview reconstruction', (t) => {
  const saved = readSaved(t, fixture('spindle_pmi_after'));
  assertNumbers(saved);
  assert.ok(saved.chars.some((row) => row.n === 9 && row.nominal === 5));
  assert.ok(saved.chars.some((row) => row.n === 16 && row.nominal === 1.5));
  assert.ok(saved.chars.some((row) => row.n === 17 && row.nominal === 2.375));
});

test('write → readback → export keeps the M4 ID while later saves append numbers', (t) => {
  const bytes = write(open(t), { 'hole.function:holes:13': 'tapped:M4' });
  const saved = readSaved(t, bytes);
  assert.deepEqual(saved.chars.map((row) => row.n), [1]);
  assert.match(saved.chars[0].text, /M4/);
  assertNumbers(saved);
  const resumed = open(t, bytes);
  const answers = { ...saved.analysis.resume.answers, 'datum.A': [0], 'part.surface_finish': 'Ra 3.2' };
  const next = readSaved(t, write(resumed, answers));
  assert.equal(next.chars.find((row) => /M4/.test(row.text)).n, 1);
  assert.ok(next.chars.filter((row) => !/M4/.test(row.text)).every((row) => row.n > 1));
  assertNumbers(next);
});

test('changed limits reach the next saved export and cannot alter the previous snapshot', (t) => {
  const a = { 'extra.1': { kind: 'size', faces: [13], value: '+0.2/-0.1' } };
  const bytes = write(open(t), a);
  const first = readSaved(t, bytes);
  assert.deepEqual(first.chars.map((row) => [row.upper, row.lower]), [[0.2, -0.1]]);
  const live = { ...a, 'extra.1': { ...a['extra.1'], value: '+0.5/-0.3' } };
  // No Save: reading the retained file still describes its original limits.
  assert.deepEqual(readSaved(t, bytes).chars, first.chars);
  const changed = readSaved(t, write(open(t, bytes), live));
  assert.deepEqual(changed.chars.map((row) => [row.n, row.upper, row.lower]), [[first.chars[0].n, 0.5, -0.3]]);
  assert.notEqual(toCsv(changed.chars), toCsv(first.chars));
  assertNumbers(changed);
});

test('changed datum references are read from the saved PMI, with stable requirement IDs', (t) => {
  const answers = {
    'datum.A': [0], 'datum.B': [1],
    'extra.1': { kind: 'perpendicularity', faces: [13], value: '0.05', datums: ['A'] },
  };
  const bytes = write(open(t), answers);
  const first = readSaved(t, bytes);
  const tolerance = first.chars.find((row) => row.upper === 0.05);
  assert.equal(tolerance.datums, 'A');
  const live = { ...answers, 'extra.1': { ...answers['extra.1'], datums: ['B'] } };
  assert.equal(readSaved(t, bytes).chars.find((row) => row.n === tolerance.n).datums, 'A');
  const changed = readSaved(t, write(open(t, bytes), live));
  assert.equal(changed.chars.find((row) => row.n === tolerance.n).datums, 'B');
  assert.match(as9102({ name: 'saved' }, changed.chars, new Set(), changed.notes)[2].rows.flat().join(' '), /datums B/);
  assertNumbers(changed);
});

test('export callback, displayed source and read-only loading describe the same frozen saved STEP', (t) => {
  const bytes = fixture('spindle_pmi_after');
  const displayed = { path: 'spindle.saved.step', bytes };
  const loaded = readSaved(t, displayed.bytes);
  const callback = () => inspectionFromStep(loaded.analysis, JSON.parse(pmiScene(displayed.bytes)));
  const csv = toCsv(callback().chars);
  // Changes to answers, including defaults accepted by an interview review, are never inputs.
  open(t, bytes).review(JSON.stringify({ 'part.material': 'Brass', 'part.general_tolerance': 'ISO 2768-f' }));
  assert.equal(toCsv(callback().chars), csv);
  assert.deepEqual(callback(), inspectionFromStep(loaded.analysis, loaded.scene), 'callback/display parity uses the retained file');
  assert.deepEqual(readSaved(t, bytes).chars, callback().chars, 'read-only loading uses identical stored PMI');
  assertNumbers({ ...loaded, ...callback() });
});

test('the 0.2.0 separate-file fallback rebuilds only the reported saved revision', (t) => {
  const savedAnswers = { 'hole.function:holes:13': 'tapped:M4', 'part.material': 'Steel' };
  const saved = { 0: { ...savedAnswers } };
  const rebuilt = rebuildSavedStep(IntentEngine, 'shaft.step', shaft, saved);
  const expected = write(open(t), savedAnswers);
  const fromReport = readSaved(t, rebuilt);
  assert.deepEqual(fromReport.chars, readSaved(t, expected).chars);
  assert.deepEqual(fromReport.notes, readSaved(t, expected).notes);
  const live = { ...savedAnswers, 'part.material': 'Brass', 'part.surface_finish': 'Ra 0.8' };
  assert.notDeepEqual(live, saved[0]);
  assert.deepEqual(readSaved(t, rebuilt).chars, fromReport.chars);
  assert.deepEqual(readSaved(t, rebuilt).notes, fromReport.notes);
  assertNumbers(fromReport);
  const untouched = fixture('spindle_pmi_after');
  assert.equal(rebuildSavedStep(IntentEngine, 'spindle.step', untouched, {}), untouched, 'nothing written leaves every stored characteristic untouched');
});
