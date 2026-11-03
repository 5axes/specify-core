// Complete answer export through the actual generated JS/WASM boundary.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine, diff } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
const root = new URL('../', import.meta.url);
await init({module_or_path: readFileSync(new URL('crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', root))});
const bytes = which => gunzipSync(readFileSync(new URL(`tests/fixtures/revisions/plate_split.${which}.step.gz`, root)));
const oldBytes = bytes('old'), newBytes = bytes('new');
const old = new IntentEngine('v1.step', oldBytes), next = new IntentEngine('v2.step', newBytes);
const refuses = (action, pattern) => assert.throws(action, e => typeof e === 'string' && pattern.test(e));
try {
  const given = {'part.material':'6061-T6', 'datum.A':[4], 'datum.B':[1], 'datum.C':[0]};
  const text = old.exportAnswers(JSON.stringify(given));
  const saved = JSON.parse(text);
  assert.equal(old.exportAnswers(JSON.stringify(Object.fromEntries(Object.entries(given).reverse()))), text);
  assert.equal(old.exportAnswers(text), text);
  assert.deepEqual(saved._specify.evidence, JSON.parse(old.evidence()));
  assert.deepEqual(Object.keys(saved), Object.keys(saved).sort());
  const {_specify, ...plain} = saved;
  assert.deepEqual(plain, JSON.parse(old.acceptDefaults(JSON.stringify(given))));
  assert.deepEqual(JSON.parse(old.review(text)).open, []);
  const written = JSON.parse(old.writePmi(text));
  const back = new IntentEngine('written.step', Uint8Array.from(written.bytes));
  try { assert.deepEqual(JSON.parse(back.snapshot()).analysis.resume.answers, plain); }
  finally { back.free(); }
  refuses(() => next.exportAnswers(text), /stale answers/);
  refuses(() => next.writePmi(text), /stale answers/);
  refuses(() => old.exportAnswers('{}'), /part\.material/);
  const carried = JSON.parse(diff('v1.step', oldBytes, 'v2.step', newBytes, text, 'Split')).report.answers;
  assert.equal(carried['part.material'], given['part.material']);
  refuses(() => next.exportAnswers(JSON.stringify(carried)), /datum\.A \(carried answer could not be applied:/);
  carried['datum.A'] = [4, 8];
  assert.deepEqual(JSON.parse(next.exportAnswers(JSON.stringify(carried)))._specify.unresolved, {});
} finally { old.free(); next.free(); }
console.log('Complete answer export: deterministic sidecars, resume, stale evidence and carry blocks passed');
