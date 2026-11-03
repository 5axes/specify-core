// For #87: exercise the shipped WASM rematchAnswers, including moved face indices.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine, diff, acceptDefaults } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';

const root = new URL('../', import.meta.url);
await init({ module_or_path: readFileSync(new URL('crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', root)) });
const bytes = revision => gunzipSync(readFileSync(new URL(`tests/fixtures/revisions/named_plate.${revision}.step.gz`, root)));
const name = (feature, n) => `f:named_plate.${feature}.${n};M:named_plate.body`;
const hole = name('body_make_cylinder1', 1);
const bottom = name('body_make_box1', 5);
function face(engine, name) {
  return JSON.parse(engine.snapshot()).analysis.faces.find(f => f.name === name).id;
}
function answers(engine, unnamedBottom) {
  const top = face(engine, name('body_make_box1', 6));
  const base = unnamedBottom ?? face(engine, bottom);
  return {
    'part.material': 'steel',
    [`hole.function:holes:${face(engine, hole)}`]: 'fit:H7',
    'datum.A': [base], 'datum.A.flatness': '0.03',
    'extra.1': { kind: 'parallelism', faces: [top, base], datums: ['A'], value: '0.1' },
    'extra.2': { kind: 'distance', faces: [top], reference: [base], value: '±0.05' },
  };
}
function renamed(bytes, from, to) {
  const source = bytes.toString();
  const literal = `'${from}'`;
  assert.equal(source.split(literal).length, 2, 'one authored face name in the source');
  return Buffer.from(source.replace(literal, `'${to}'`));
}
const oldBytes = bytes('old'), newBytes = bytes('new');
const old = new IntentEngine('old.step', oldBytes), next = new IntentEngine('new.step', newBytes);
try {
  const given = answers(old), expected = answers(next);
  assert.notDeepEqual(given, expected);
  const evidence = old.evidence();
  const automatic = Object.keys(given);
  const rematched = JSON.parse(next.rematchAnswers(JSON.stringify(given), evidence, JSON.stringify(automatic)));
  assert.deepEqual(rematched.answers, expected);
  assert.equal(rematched.unresolved, undefined);
  assert.deepEqual(rematched.automatic, Object.keys(expected));
  const report = JSON.parse(diff('old.step', oldBytes, 'new.step', newBytes, JSON.stringify(given), 'Named rebuild')).report;
  // Diff answer maps now transport the destination evidence beside the same requirements.
  assert.deepEqual(report.answers, {
    ...expected, _specify: { evidence: JSON.parse(next.evidence()), unresolved: {} },
  });
  assert.deepEqual(report.unresolved, []);
  for (const [changed, reason] of [
    [renamed(oldBytes, hole, ''), /not in this revision/],
    [renamed(oldBytes, name('body_make_cylinder2', 1), hole), /ambiguous face name/],
  ]) {
    const engine = new IntentEngine('changed.step', changed);
    try {
      const out = JSON.parse(engine.rematchAnswers(JSON.stringify(given), evidence));
      assert.equal(out.unresolved.length, 1);
      assert.match(out.unresolved[0].reason, reason);
    } finally { engine.free(); }
  }
  // Shared source labels are not saved identities and never refuse a fresh write.
  const sharedBytes = renamed(renamed(oldBytes, hole, 'f:shared.class'), name('body_make_cylinder2', 1), 'f:shared.class');
  const shared = new IntentEngine('shared.step', sharedBytes);
  try {
    const evidence = JSON.parse(shared.evidence());
    assert.equal(evidence.schema, 3);
    assert.equal(evidence.face_names[face(old, hole)], null);
    assert.equal(evidence.face_names[face(old, name('body_make_cylinder2', 1))], null);
    assert.ok(evidence.geometry);
    const written = JSON.parse(shared.writePmi(JSON.stringify(given)));
    const back = new IntentEngine('shared-written.step', Uint8Array.from(written.bytes));
    try { assert.deepEqual(JSON.parse(back.snapshot()).analysis.resume.answers, given); }
    finally { back.free(); }
  } finally { shared.free(); }
  let genericBytes = oldBytes;
  for (const [i, f] of JSON.parse(old.snapshot()).analysis.faces.entries()) {
    genericBytes = renamed(genericBytes, f.name, i === 0 ? '24' : i === 1 ? 'Fill.1' : 'PartBody');
  }
  const generic = new IntentEngine('generic.step', genericBytes);
  try {
    const evidence = JSON.parse(generic.evidence());
    assert.equal(evidence.schema, 2);
    assert.equal(evidence.face_names, undefined);
    assert.ok(JSON.parse(generic.snapshot()).analysis.faces.every(f => f.name === undefined));
    const written = JSON.parse(generic.writePmi(JSON.stringify(given)));
    const back = new IntentEngine('generic-written.step', Uint8Array.from(written.bytes));
    try { assert.deepEqual(JSON.parse(back.snapshot()).analysis.resume.answers, given); }
    finally { back.free(); }
  } finally { generic.free(); }
  const mixedOld = new IntentEngine('mixed-old.step', renamed(oldBytes, bottom, ''));
  const mixedNew = new IntentEngine('mixed-new.step', renamed(newBytes, bottom, ''));
  try {
    const a = answers(mixedOld, face(old, bottom));
    const expected = answers(mixedNew, face(next, bottom));
    const out = JSON.parse(mixedNew.rematchAnswers(JSON.stringify(a), mixedOld.evidence()));
    assert.deepEqual(out.answers, expected);
    assert.equal(out.unresolved, undefined);
  } finally { mixedOld.free(); mixedNew.free(); }
} finally { old.free(); next.free(); }
console.log('STEP face names: WASM rematchAnswers/diff, shifted indices, automatic answers, refusals, shared/generic labels and mixed support passed');


// The split-bottom workflow must stay safe through the actual JS/WASM boundary too.
const split = which => gunzipSync(readFileSync(new URL(`tests/fixtures/revisions/plate_split.${which}.step.gz`, root)));
const beforeBytes = split('old'), afterBytes = split('new');
const before = new IntentEngine('plate_v1.step', beforeBytes);
const after = new IntentEngine('plate_v2.step', afterBytes);
try {
  const given = { 'part.material': '6061-T6', 'datum.A': [4], 'datum.B': [1], 'datum.C': [0] };
  const carried = JSON.parse(diff('old.step', beforeBytes, 'new.step', afterBytes, JSON.stringify(given), 'Split')).report.answers;
  const blocked = JSON.stringify(carried);
  const question = JSON.parse(after.review(blocked)).questions.find(q => q.id === 'datum.A');
  assert.equal(question.default, null);
  assert.equal(question.attention, 'required');
  assert.equal(question.reason, `carried answer could not be applied: ${carried._specify.unresolved['datum.A']}`);
  const incomplete = e => typeof e === 'string' && /datum\.A \(carried answer could not be applied: face 4 has no match/.test(e);
  assert.throws(() => after.acceptDefaults(blocked), incomplete);
  const analysis = JSON.stringify(JSON.parse(after.snapshot()).analysis);
  assert.throws(() => acceptDefaults(analysis, blocked), incomplete);
  assert.throws(() => after.writePmi(blocked), incomplete);
  carried['datum.A'] = [4, 8];
  const accepted = after.acceptDefaults(JSON.stringify(carried));
  const written = JSON.parse(after.writePmi(accepted));
  const back = new IntentEngine('resolved.step', Uint8Array.from(written.bytes));
  try {
    const resumed = JSON.parse(back.snapshot()).analysis.resume.answers;
    assert.deepEqual(resumed['datum.A'], [4, 8]);
    assert.equal(resumed._specify, undefined, 'transport is consumed before storing answers');
  } finally { back.free(); }
  const saved = JSON.parse(diff('old.step', beforeBytes, 'old.step', beforeBytes, JSON.stringify(given), 'Unchanged')).report.answers;
  before.writePmi(JSON.stringify(saved));
  assert.throws(() => after.writePmi(JSON.stringify(saved)), e => typeof e === 'string' && /stale answers.*diff/.test(e));
  const rematched = JSON.parse(after.rematchAnswers(JSON.stringify(saved))).answers;
  assert.throws(() => after.acceptDefaults(JSON.stringify(rematched)), e => typeof e === 'string' && /carried answer could not be applied/.test(e));
} finally { before.free(); after.free(); }
console.log('Carried answers: WASM bulk defaults, explicit replacement, unchanged/stale evidence and embedded-evidence rematching passed');
