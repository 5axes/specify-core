// For #79: browser BOM API and native sourced JSON agree, including answered UDAs.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { execFileSync } from 'node:child_process';
import init, { IntentEngine } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
const wasm = readFileSync(new URL('../crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', import.meta.url));
await init({ module_or_path: wasm });
for (const path of process.argv.slice(2).length ? process.argv.slice(2) : ['tests/fixtures/parts/assembly.step']) {
  let bytes = readFileSync(path);
  if (path.endsWith('.gz')) bytes = gunzipSync(bytes);
  const rows = JSON.parse(IntentEngine.bom(bytes));
  const native = JSON.parse(execFileSync('target/release/specify-core', ['bom', path], { encoding: 'utf8' }));
  assert.deepEqual(rows, native);
  assert.ok(rows.length > 0);
  for (const row of rows) {
    for (const [key, field] of Object.entries(row)) {
      if (key !== 'part') assert.ok(['file', 'answered', 'default'].includes(field.source));
    }
  }
}
assert.throws(() => IntentEngine.bom(new Uint8Array(64 * 1024 * 1024 + 1)), /exceeds/);
console.log('WASM BOM parity and input limit passed');

const assembly = readFileSync('tests/fixtures/parts/assembly.step');
const engine = IntentEngine.openPart('assembly', assembly, 0);
const defaults = JSON.parse(engine.acceptDefaults('{}'));
assert.ok(Object.keys(defaults).every(key => !key.startsWith('bom.')));
const ordinary = JSON.parse(engine.writePmi('{}'));
assert.ok(JSON.parse(IntentEngine.bom(Uint8Array.from(ordinary.bytes))).every(row => row.item_number.value === null));
const enabled = JSON.parse(engine.writePmi('{}', undefined, undefined, true));
const enabledRows = JSON.parse(IntentEngine.bom(Uint8Array.from(enabled.bytes)));
assert.equal(enabledRows[0].item_number.value, 1);
assert.equal(enabledRows[0].description.source, 'default');
assert.deepEqual(JSON.parse(IntentEngine.openPart('enabled', Uint8Array.from(enabled.bytes), 0).snapshot()).analysis.resume.answers, {});
console.log('WASM explicit BOM write and suggestion provenance passed');
