import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { IntentEngine, characteristicHistory, validateOpening } from './engine-fixture.mjs';
import { exportableAnswers } from '../app/answers.ts';

const shaft = gunzipSync(readFileSync(new URL('./fixtures/gramil_shaft_before.step.gz', import.meta.url)));
const malformed = { 'part.material': 'Steel', _specify: { keep: [], private_state: 'not an answer' },
  _specify_characteristic_numbers: { schema: 2, high_water: 0, entries: [] } };

test('0.2.0 review accepts the old sidecar fixture but opening its scene rejects missing history faces', () => {
  const engine = new IntentEngine('part.step', shaft);
  try {
    assert.doesNotThrow(() => engine.review(JSON.stringify(malformed)), 'review alone cannot validate this fixture');
  } finally { engine.free(); }
  assert.doesNotThrow(() => validateOpening('part.step', shaft, { 'part.material': 'Steel', _specify: malformed._specify }),
    'the unknown metadata member is ignored; it is not the cause');
  assert.throws(() => validateOpening('part.step', shaft, malformed),
    (error) => String(error).includes('_specify_characteristic_numbers: missing field `faces`'));
});

test('real writer history passes every opening operation and is removed from the exact JSON sidecar', () => {
  const given = { 'part.material': 'Steel', 'hole.function:holes:13': 'tapped:M4' };
  const engine = new IntentEngine('part.step', shaft);
  let written;
  try { written = new Uint8Array(JSON.parse(engine.writePmi(JSON.stringify(given))).bytes); }
  finally { engine.free(); }
  const history = characteristicHistory(written);
  assert.equal(history.schema, 2);
  assert.equal(typeof history.faces, 'string');
  assert.ok(history.faces.length > 0);
  assert.ok(history.entries.length > 0);
  const answers = { ...given, _specify: { keep: [] }, _specify_characteristic_numbers: history };
  assert.doesNotThrow(() => validateOpening('part.step', shaft, answers));
  assert.doesNotThrow(() => validateOpening('part.saved.step', written, answers));
  assert.deepEqual(exportableAnswers(answers), { ...given, _specify: { keep: [] } });
});
