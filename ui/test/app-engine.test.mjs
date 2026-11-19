import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
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

test('the fixture loads both glue and WASM from SPECIFY_WASM_DIR', () => {
  const selected = mkdtempSync(join(tmpdir(), "specify selected engine's-"));
  writeFileSync(join(selected, 'package.json'), JSON.stringify({ type: 'module' }));
  writeFileSync(join(selected, 'specify_core_wasm.js'), `
export default function init({ module_or_path }) {
  if (module_or_path[0] !== 37) throw new Error('Wrong WASM bytes selected');
}
export class IntentEngine { static selected = true; }
export function pmiScene() {}`);
  writeFileSync(join(selected, 'specify_core_wasm_bg.wasm'), new Uint8Array([37]));
  const fixture = new URL('./engine-fixture.mjs', import.meta.url).href;
  const result = execFileSync(process.execPath, ['--input-type=module', '-e',
    `const {wasmDirectory, IntentEngine} = await import(${JSON.stringify(fixture)}); console.log(JSON.stringify({path:wasmDirectory, selected:IntentEngine.selected}));`],
    { encoding: 'utf8', env: { ...process.env, SPECIFY_WASM_DIR: selected } });
  assert.deepEqual(JSON.parse(result), { path: selected, selected: true });
});

test('missing engine files report the fetch command before the test suite starts', () => {
  const dir = mkdtempSync(join(tmpdir(), 'specify-missing-engine-'));
  const noWasm = join(dir, 'no-wasm');
  mkdirSync(noWasm);
  writeFileSync(join(noWasm, 'specify_core_wasm.js'), '');
  const check = new URL('./engine-path.mjs', import.meta.url);
  for (const selected of [join(dir, 'not-fetched'), noWasm]) {
    const result = spawnSync(process.execPath, [fileURLToPath(check)], { encoding: 'utf8', env: { ...process.env, SPECIFY_WASM_DIR: selected } });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /run npm run fetch:wasm/);
    assert.ok(result.stderr.includes(selected));
    assert.doesNotMatch(result.stderr, /ERR_MODULE_NOT_FOUND|ENOENT/);
  }
});
