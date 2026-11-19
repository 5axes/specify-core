import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine, review } from '@draftwright/specify-core-wasm';
import { characteristics, noteStatements, toCsv } from '../src/specify/characteristics.ts';
import { inspectionAnswers, inspectionCharacteristics } from '../src/specify/inspection.ts';
import { as9102 } from '../src/specify/as9102.ts';

await init({ module_or_path: readFileSync(new URL('specify_core_wasm_bg.wasm', import.meta.resolve('@draftwright/specify-core-wasm'))) });

function shaft(t, answers) {
  const engine = new IntentEngine('shaft.step', gunzipSync(readFileSync(new URL('./fixtures/gramil_shaft_before.step.gz', import.meta.url))));
  t.after(() => engine.free());
  const { analysis } = JSON.parse(engine.snapshot());
  const defaults = JSON.parse(engine.acceptDefaults(JSON.stringify(answers)));
  const preview = JSON.parse(engine.review(JSON.stringify(defaults)));
  return { engine, analysis, defaults, preview, displayed: characteristics(preview.intent, analysis, new Map()) };
}

test('shaft inspection exports preserve Material balloon 12 while excluding every unapproved default', (t) => {
  const answers = { 'part.material': 'Aluminium 6082-T6' };
  const { analysis, preview, displayed } = shaft(t, answers);
  const material = displayed.find((row) => row.source === 'part:material');
  assert.equal(material.id, '12', 'the full preview displays Material as balloon 12');
  // The shared fallback uses this analysis-only review, with exactly these given/kept answers.
  const given = inspectionAnswers(answers, preview.questions);
  const { intent } = JSON.parse(review(JSON.stringify(analysis), JSON.stringify(given)));
  const rows = inspectionCharacteristics(displayed, characteristics(intent, analysis, new Map()));
  assert.deepEqual(rows, [material]);
  assert.equal(rows[0], material, 'filtering retains the displayed characteristic object');
  assert.equal(toCsv(rows).split('\n')[1], '12,,Part,Material: Aluminium 6082-T6,,,,,,');
  const [, form2, form3] = as9102({ name: 'shaft' }, rows, new Set(), noteStatements(intent, analysis));
  assert.deepEqual(form3.rows.slice(4).map((row) => [row[0], row[3]]), [['12', 'Material: Aluminium 6082-T6']]);
  assert.deepEqual(form2.rows.slice(4, 5).map((row) => row[0]), ['MATERIAL Aluminium 6082-T6']);
});

test('the default shared export includes kept answers and excludes host suggestions awaiting review', (t) => {
  const answers = { 'part.material': 'Steel' };
  const { analysis, defaults, preview, displayed } = shaft(t, answers);
  const pending = Object.keys(defaults).filter((id) => !(id in answers));
  const kept = ['part.surface_finish', 'hole.function:holes:13'];
  const given = inspectionAnswers(defaults, preview.questions, kept, pending);
  assert.deepEqual(given, {
    'part.material': 'Steel', 'part.surface_finish': 'Ra 3.2', 'hole.function:holes:13': defaults['hole.function:holes:13'],
  }, 'being in the host answer map alone does not approve a pending suggestion');
  const { intent } = JSON.parse(review(JSON.stringify(analysis), JSON.stringify(given)));
  assert.deepEqual(Object.keys(intent.part).sort(), ['material', 'surface_finish']);
  assert.deepEqual(intent.requirements.map((item) => item.question), ['hole.function:holes:13']);
  assert.deepEqual(intent.general, [], 'unapproved general tolerance and its dependent dimensions are absent');
  const rows = inspectionCharacteristics(displayed, characteristics(intent, analysis, new Map()));
  assert.deepEqual(rows.filter((row) => row.source.startsWith('part:')).map((row) => row.source), ['part:material', 'part:surface_finish']);
  assert.ok(rows.some((row) => row.question === 'hole.function:holes:13'), 'the kept thread is included');
  assert.ok(rows.every((row) => displayed.includes(row)), 'both callback and fallback filtering preserve displayed IDs');
  assert.deepEqual(inspectionAnswers(answers, preview.questions, ['part.surface_finish']), {
    ...answers, 'part.surface_finish': 'Ra 3.2',
  }, 'a kept default need not already be in the host answer map');
});

test('inspection filtering preserves pattern instance IDs and rejects changed limits or datum references', () => {
  const analysis = {
    faces: [{ id: 1, kind: 'cylinder', radius: 5 }, { id: 2, kind: 'cylinder', radius: 5 }],
    features: [
      { id: 'one', faces: [1] }, { id: 'two', faces: [2] },
      { id: 'pattern', faces: [1, 2], members: ['one', 'two'] },
    ], existing: [], part: {},
  };
  const size = { kind: 'size', feature: 'pattern', faces: [1, 2], nominal: 10, upper: 0.1, lower: 0, question: 'size' };
  const intent = (requirements) => ({ requirements, general: [], datums: [], part: {} });
  const preview = characteristics(intent([
    { kind: 'finish', feature: 'one', faces: [1], value: 'Ra 3.2', question: 'finish' }, size,
  ]), analysis, new Map());
  const given = characteristics(intent([size]), analysis, new Map());
  assert.deepEqual(given.map((row) => row.id), ['1.1', '1.2'], 'a fresh numbering would be wrong');
  const filtered = inspectionCharacteristics(preview, given);
  assert.deepEqual(filtered.map((row) => [row.n, row.sub, row.id]), [[2, 1, '2.1'], [2, 2, '2.2']]);
  assert.deepEqual(inspectionCharacteristics(preview, given.map((row) => ({ ...row, upper: 0.5 }))), []);
  assert.deepEqual(inspectionCharacteristics(preview, given.map((row) => ({ ...row, datums: 'B' }))), []);
});
