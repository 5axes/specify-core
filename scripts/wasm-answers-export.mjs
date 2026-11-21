// Answer export through the actual generated JS/WASM boundary: the answers given and exactly
// the defaults approved, by part (ADR P9).
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
  // Every default, each approved with the value offered: what a person who confirmed them sends.
  const every = JSON.parse(old.acceptDefaults(JSON.stringify(given)));
  for (const id of Object.keys(given)) delete every[id];
  const approvals = JSON.stringify({'0': every});
  const text = old.exportAnswers(JSON.stringify(given), approvals);
  const saved = JSON.parse(text);
  assert.equal(old.exportAnswers(JSON.stringify(Object.fromEntries(Object.entries(given).reverse())), approvals), text);
  assert.equal(old.exportAnswers(text), text);
  // No approvals (or another part's), no defaults; a changed value is refused.
  const bare = ({_specify, ...rest}) => rest;
  assert.deepEqual(bare(JSON.parse(old.exportAnswers(JSON.stringify(given)))), given);
  assert.deepEqual(bare(JSON.parse(old.exportAnswers(JSON.stringify(given), JSON.stringify({'1': every})))), given);
  const tolerance = every['part.general_tolerance'];
  assert.ok(tolerance !== undefined);
  assert.deepEqual(bare(JSON.parse(old.exportAnswers(JSON.stringify(given), JSON.stringify({'0': {'part.general_tolerance': tolerance}})))),
    {...given, 'part.general_tolerance': tolerance});
  refuses(() => old.exportAnswers(JSON.stringify(given), JSON.stringify({'0': {'part.general_tolerance': 'changed'}})),
    /part 0: part\.general_tolerance's default is now .*, not the "changed" approved/);
  refuses(() => old.exportAnswers(JSON.stringify(given), JSON.stringify({'part.general_tolerance': tolerance})), /Invalid approvals/);
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
  assert.deepEqual(bare(JSON.parse(old.exportAnswers('{}'))), {});
  // Diff reads the files' PMI only and returns no answers (ADR P11). An answers file carried
  // to the new revision with its datum dropped, as an earlier diff returned one:
  assert.equal(JSON.parse(diff('v1.step', oldBytes, 'v2.step', newBytes, text, 'Split')).report.answers, undefined);
  const carried = {
    'part.material': given['part.material'],
    _specify: { evidence: JSON.parse(next.evidence()), unresolved: {
      'datum.A': 'face 4 has no match in the new part', 'datum.A.flatness': 'its datum datum.A is unresolved' } },
  };
  // Incomplete is normal: carry blocks are kept, their questions left open.
  assert.deepEqual(Object.keys(carried._specify.unresolved).sort(), ['datum.A', 'datum.A.flatness']);
  assert.deepEqual(Object.keys(JSON.parse(next.exportAnswers(JSON.stringify(carried)))._specify.unresolved).sort(), ['datum.A', 'datum.A.flatness']);
  carried['datum.A'] = [4, 8];
  assert.deepEqual(Object.keys(JSON.parse(next.exportAnswers(JSON.stringify(carried)))._specify.unresolved), ['datum.A.flatness']);
  carried['datum.A.flatness'] = saved['datum.A.flatness'];
  assert.deepEqual(JSON.parse(next.exportAnswers(JSON.stringify(carried)))._specify.unresolved, {});
} finally { old.free(); next.free(); }
console.log('Answer export: approved defaults only, deterministic sidecars, resume, stale evidence and carry blocks passed');
