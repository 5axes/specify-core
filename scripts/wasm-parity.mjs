// Exercise the release's web module directly in Node; no second WASM build or npm deps.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { brotliCompressSync, gzipSync, gunzipSync, constants } from 'node:zlib';
import init, { IntentEngine, review, acceptDefaults, interpret, start } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';

const root = new URL('../', import.meta.url);
const wasm = readFileSync(new URL('crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', root));
await init({ module_or_path: wasm });
start();
const executable = fileURLToPath(new URL('target/release/examples/parity_reference', root));
const project = Buffer.from(JSON.stringify({ revision: 1, drawing: 'front' })).toString('base64');

// Geometric calculations may differ in their last bits between native and WASM libm.
// Keep all ids, strings, array order, reports and written STEP bytes exact.
function compare(actual, expected, path = '$') {
  if (typeof actual === 'number' && typeof expected === 'number') {
    assert.ok(Math.abs(actual - expected) <= 1e-10 * Math.max(1, Math.abs(expected)), `${path}: ${actual} != ${expected}`);
  } else if (actual && expected && typeof actual === 'object' && typeof expected === 'object') {
    assert.equal(Array.isArray(actual), Array.isArray(expected), `${path}: array type`);
    assert.deepEqual(Object.keys(actual).sort(), Object.keys(expected).sort(), `${path}: keys`);
    for (const key of Object.keys(expected)) compare(actual[key], expected[key], `${path}.${key}`);
  } else assert.deepEqual(actual, expected, path);
}
// Delaunay ties on circular planar caps can choose different diagonals across targets.
// Compare the mesh contract per face, including every vertex and oriented surface area.
function meshGeometry(mesh) {
  const { positions, indices, ...metadata } = mesh;
  assert.equal(positions.length % 3, 0);
  assert.equal(indices.length % 3, 0);
  const surfaces = mesh.faces.map(([start, count]) => {
    const vertices = new Map();
    let area = 0;
    const orientedArea = [0, 0, 0];
    for (let i = start * 3; i < (start + count) * 3; i += 3) {
      const triangle = indices.slice(i, i + 3).map(index => {
        assert.ok(Number.isInteger(index) && index >= 0 && index * 3 + 2 < positions.length);
        const point = positions.slice(index * 3, index * 3 + 3);
        vertices.set(JSON.stringify(point), point);
        return point;
      });
      const [a, b, c] = triangle;
      const u = b.map((x, k) => x - a[k]);
      const v = c.map((x, k) => x - a[k]);
      const cross = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]];
      area += Math.hypot(...cross) / 2;
      for (let k = 0; k < 3; k++) orientedArea[k] += cross[k] / 2;
    }
    return { vertices: [...vertices.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([, point]) => point), area, orientedArea };
  });
  return { metadata, surfaces };
}
function throwsString(action, pattern) {
  assert.throws(action, error => typeof error === 'string' && pattern.test(error));
}

const parts = ['plate', 'pin', 'a_placed_part_is_meshed_in_the_frame_the_analysis_uses', 'GRM-02_shaft']
  .map(part => [part, fileURLToPath(new URL(`tests/fixtures/parts/${part}.step`, root))]);
parts.push(['spindle', fileURLToPath(new URL('tests/fixtures/spindle.step', root))]);
const corpus = process.env.QUIDDITY_CORPUS || process.env.SPECIFY_CORPUS;
if (corpus) parts.push(['GRM-03_thumbwheel_drive_screw', join(corpus, 'gramel/GRM-03_thumbwheel_drive_screw.step')]);
for (const [part, path] of parts) {
  const name = `${part}.step`;
  const bytes = readFileSync(path);
  const engine = new IntentEngine(name, bytes);
  try {
    const snapshot = JSON.parse(engine.snapshot());
    const initial = JSON.parse(engine.review('{}'));
    const depth = initial.questions.find(q => q.id.startsWith('hole.full_thread:'));
    const question = initial.questions.find(q =>
      part === 'spindle' ? q.options?.includes('thread:M10') :
      part.startsWith('GRM-') ? q.id === depth.id.replace('hole.full_thread:', 'hole.function:') :
      /^(hole.function:|diameter.fit:)/.test(q.id));
    assert.ok(question, `${part}: a diameter choice to interpret`);
    assert.ok(question.diameter > 0, 'presented question preserves diameter');
    assert.ok(initial.open.includes(question.id), 'unanswered question remains open despite preview defaults');
    const text = part === 'spindle' ? 'M10' : part.startsWith('GRM-') ? 'M2' : question.id.startsWith('hole.') ? 'H7' : 'h6';
    const chosen = JSON.parse(interpret(JSON.stringify(snapshot.analysis), '{}', question.id, text));
    assert.ok(chosen.callout, 'diameter interpretation includes callout');
    const answers = { 'part.material': 'steel', [question.id]: chosen.value };
    if (part.startsWith('GRM-')) {
      assert.equal(depth.kind, 'text');
      // This pinned GRM-03 has a 3.8 mm drill (2.5 mm full thread); #21's
      // reported 8 mm drill is covered as an explicit core rules input.
      assert.equal(Number(depth.default), part.startsWith('GRM-03') ? 2.5 : 2);
      assert.equal(depth.attention, 'refine');
      assert.ok(!initial.open.includes(depth.id), 'depth is an optional override');
      answers[depth.id] = part.startsWith('GRM-03') ? '2.3' : '1.8';
    }
    const answersJson = JSON.stringify(answers);
    assert.ok(!JSON.parse(engine.review(answersJson)).open.includes(question.id), 'explicit answer closes question');
    const native = spawnSync(executable, [], {
      cwd: fileURLToPath(root), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
      input: JSON.stringify({ name, path, answers, project, question_id: question.id, text }),
    });
    assert.equal(native.status, 0, native.error?.message ?? native.stderr);
    const expected = JSON.parse(native.stdout);
    compare(JSON.parse(engine.pmiScene(answersJson)), expected.pmi_scene);
    const { mesh: actualMesh, ...actualSnapshot } = snapshot;
    const { mesh: expectedMesh, ...expectedSnapshot } = expected.snapshot;
    compare(actualSnapshot, expectedSnapshot);
    compare(meshGeometry(actualMesh), meshGeometry(expectedMesh), '$.mesh');
    compare(JSON.parse(engine.review(answersJson)), expected.review);
    compare(JSON.parse(review(JSON.stringify(snapshot.analysis), answersJson)), expected.free_review);
    assert.deepEqual(JSON.parse(interpret(JSON.stringify(snapshot.analysis), answersJson, question.id, text)), expected.interpret);
    const bare = JSON.parse(engine.writePmi(answersJson));
    const stored = JSON.parse(engine.writePmi(answersJson, project));
    assert.deepEqual(bare, expected.bare, 'bare STEP bytes and report');
    assert.deepEqual(stored, expected.project, 'project STEP bytes and report');
    const acceptedJson = engine.acceptDefaults(answersJson);
    const accepted = JSON.parse(acceptedJson);
    assert.deepEqual(accepted, expected.accepted, 'native/session accepted answers');
    assert.deepEqual(JSON.parse(acceptDefaults(JSON.stringify(snapshot.analysis), answersJson)), accepted, 'free accepted answers');
    assert.deepEqual(JSON.parse(engine.acceptDefaults(acceptedJson)), accepted, 'session acceptance is idempotent');
    assert.deepEqual(JSON.parse(acceptDefaults(JSON.stringify(snapshot.analysis), acceptedJson)), accepted, 'free acceptance is idempotent');
    for (const [id, value] of Object.entries(answers)) assert.deepEqual(accepted[id], value, `${id}: supplied answer preserved`);
    const defaults = JSON.parse(engine.acceptDefaults('{}'));
    assert.deepEqual(defaults, expected.defaults_without_answers);
    assert.ok(!Object.hasOwn(defaults, 'part.material'), 'no invented default for material');
    assert.ok(JSON.parse(engine.review(JSON.stringify(defaults))).open.includes('part.material'));
    const acceptedReview = JSON.parse(engine.review(acceptedJson));
    compare(acceptedReview, expected.accepted_review);
    compare(JSON.parse(review(JSON.stringify(snapshot.analysis), acceptedJson)), expected.accepted_free_review);
    assert.deepEqual(acceptedReview.open, [], 'accepted defaults are answered');
    assert.equal(acceptedReview.intent.part.general_tolerance, accepted['part.general_tolerance']);
    assert.ok(acceptedReview.intent.datums.length > 0);
    assert.ok(!acceptedReview.problems.some(p => p.code === 'awaits_datum_frame'));
    const state = { answers: accepted, auto: [], gdt: null };
    const settled = JSON.parse(engine.settle(JSON.stringify(state)));
    compare(settled, expected.accepted_settle);
    assert.deepEqual(settled.state.answers, accepted, 'settle preserves accepted answers');
    const acceptedWrite = JSON.parse(engine.writePmi(acceptedJson));
    assert.deepEqual(acceptedWrite, expected.accepted_write, 'accepted PMI matches native apply-with-defaults bytes/report');
    const acceptedReopened = new IntentEngine(name, Uint8Array.from(acceptedWrite.bytes));
    try { assert.deepEqual(JSON.parse(acceptedReopened.snapshot()).analysis.resume.answers, accepted); }
    finally { acceptedReopened.free(); }
    const edited = { ...accepted, 'part.general_tolerance': 'ISO 2768-f' };
    assert.equal(JSON.parse(engine.acceptDefaults(JSON.stringify(edited)))['part.general_tolerance'], 'ISO 2768-f');
    assert.equal(JSON.parse(engine.review(JSON.stringify(edited))).intent.part.general_tolerance, 'ISO 2768-f');
    assert.ok(stored.report.written.length > 0, 'PMI written');
    assert.ok(Buffer.from(stored.bytes).includes(Buffer.from(`DESCRIPTIVE_REPRESENTATION_ITEM('draftwright editor project','${project}')`)));
    const reopened = new IntentEngine(name, Uint8Array.from(stored.bytes));
    try { assert.deepEqual(JSON.parse(reopened.snapshot()).analysis.resume.answers, answers); }
    finally { reopened.free(); }
    // #35: a carried payload, the original part, and saved answers matched with face evidence.
    const payload = { name: 'an app', value: { revision: 2, note: '‘α’ it\'s' } };
    const carried = JSON.parse(engine.writePmi(answersJson, null, JSON.stringify(payload)));
    const evidence = engine.evidence();
    const carrying = new IntentEngine(name, Uint8Array.from(carried.bytes));
    try {
      assert.deepEqual(JSON.parse(carrying.payload('an app')), payload.value);
      assert.equal(carrying.payload('another app'), undefined);
      // The spindle came with datums and their presentation, which the writer replaces, so
      // it has no original to give back; every other part's is its bytes exactly.
      const original = carrying.original();
      if (part === 'spindle') assert.equal(original, undefined, 'replaced PMI: no original');
      else assert.deepEqual(Buffer.from(original), Buffer.from(bytes), 'original part bytes');
      assert.deepEqual(JSON.parse(carrying.evidence()), JSON.parse(evidence));
      const rematched = JSON.parse(carrying.rematchAnswers(answersJson, evidence, '[]'));
      assert.deepEqual(rematched.answers, answers);
      assert.deepEqual(rematched.automatic, []);
      // Without evidence (a legacy project) answers are trusted by feature ID: those on this
      // part's IDs are kept, and an ID the part lacks is left open with its value.
      const given = { ...answers, 'hole.function:holes:9999': 'fit:H7' };
      const unknown = JSON.parse(carrying.rematchAnswers(JSON.stringify(given)));
      assert.deepEqual(unknown.unresolved.map(u => [u.id, u.answer]), [['hole.function:holes:9999', 'fit:H7']]);
      assert.deepEqual(unknown.answers, answers);
      assert.ok(unknown.unverified);
      // Evidence that contradicts the part leaves every answer on a feature or a datum open.
      const other = JSON.stringify({ ...JSON.parse(evidence), faces: '0'.repeat(64) });
      const mismatched = JSON.parse(carrying.rematchAnswers(JSON.stringify(given), other));
      const geometric = id => /^(hole|diameter|datum)\./.test(id);
      assert.deepEqual(mismatched.unresolved.map(u => [u.id, u.answer]),
        Object.entries(given).filter(([id]) => geometric(id)));
      assert.deepEqual(Object.keys(mismatched.answers), Object.keys(given).filter(id => !geometric(id)));
      assert.ok(mismatched.unverified);
    } finally { carrying.free(); }
    assert.equal(engine.original(), undefined, 'a bare part records no original');
    throwsString(() => engine.writePmi('{}', project, JSON.stringify(payload)), /not both/);
    throwsString(() => engine.writePmi('{}', null, '{"name":"pmi-assist answers","value":1}'), /must be named/);
    throwsString(() => engine.writePmi('{}', null, '{"value":1}'), /Invalid payload/);
    throwsString(() => engine.rematchAnswers('{}', '{"schema":3}'), /Invalid evidence/);
    throwsString(() => engine.review('[]'), /Invalid answers/);
    throwsString(() => engine.acceptDefaults('[]'), /Invalid answers/);
    throwsString(() => acceptDefaults('{', '{}'), /Invalid analysis/);
    throwsString(() => acceptDefaults(JSON.stringify(snapshot.analysis), 'null'), /Invalid answers/);
    throwsString(() => acceptDefaults('{}', '{}'), /faces/);
    throwsString(() => engine.writePmi('{}', 'not base64!'), /not base64/);
    throwsString(() => engine.writePmi('{}', ''), /not base64/);
    throwsString(() => review('{', '{}'), /Invalid analysis/);
    throwsString(() => interpret(JSON.stringify(snapshot.analysis), '{}', 'missing', 'H7'), /No choice question/);
    console.log(`${part}: snapshot, review, interpretation, acceptDefaults, settle, exact PMI bytes/report, project and resume parity passed`);
  } finally { engine.free(); }
  throwsString(() => new IntentEngine(name, gzipSync(bytes)), /Decompress/);
}
// #21: the accepted tap's derived depth must survive without a new depth answer,
// including a save of an alpha.7 file. quiddity-rust#137 adds callouts, General notes and
// new aspects to the current numbered byte pin. The native thread_depth_help test compares
// every legacy semantic readback field and part setting to the unchanged alpha.7 input.
// #77 leaves this M2-only pin unchanged: it writes no size/location dimensions.
// v0.4.1 regenerates GENERAL NOTES and plane views; refresh only the numbered output golden.
const alpha7 = gunzipSync(readFileSync(new URL('tests/fixtures/thread-depth-alpha7.step.gz', root)));
const numberedTap = gunzipSync(readFileSync(new URL('tests/fixtures/thread-depth-numbered.step.gz', root)));
const savedTap = new IntentEngine('shaft.step', alpha7);
let tapAnswers;
try { tapAnswers = JSON.parse(savedTap.snapshot()).analysis.resume.answers; }
finally { savedTap.free(); }
for (const [index, bytes] of [readFileSync(new URL('tests/fixtures/parts/GRM-02_shaft.step', root)), alpha7].entries()) {
  const engine = new IntentEngine('shaft.step', bytes);
  try {
    const reviewed = JSON.parse(engine.review(JSON.stringify(tapAnswers)));
    const depth = reviewed.questions.find(q => q.id.startsWith('hole.full_thread:'));
    assert.equal(depth.attention, 'refine');
    assert.ok(!reviewed.open.includes(depth.id));
    assert.ok(!Object.hasOwn(tapAnswers, depth.id));
    assert.equal(reviewed.intent.requirements.find(r => r.kind === 'thread').full_thread, 2);
    const written = Buffer.from(JSON.parse(engine.writePmi(JSON.stringify(tapAnswers))).bytes);
    if (index === 0) assert.deepEqual(written, numberedTap, 'implicit M2 tap writes exact numbered STEP bytes');
    assert.match(written.toString(), /MEASURE_WITH_UNIT\(LENGTH_MEASURE\(2(?:\.0*)?\),#\d+\)REPRESENTATION_ITEM\('minimum full thread'\)/);
  } finally { engine.free(); }
}
console.log('alpha.7 M2 tap: implicit depth, exact numbered STEP bytes and resave passed');
for (const [part, pattern] of [['assembly', /one distinct part/]]) {
  throwsString(() => new IntentEngine(`${part}.step`, readFileSync(new URL(`tests/fixtures/parts/${part}.step`, root))), pattern);
}
console.log(`WASM sizes: raw ${wasm.length} bytes; gzip ${gzipSync(wasm, { level: 9 }).length} bytes; Brotli ${brotliCompressSync(wasm, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length} bytes`);
// Duplicate the wrapped plate's occurrence to exercise the one-placement refusal.
const wrapped = readFileSync(new URL('tests/fixtures/parts/wrapped.step', root), 'utf8');
const twice = wrapped.replace('ENDSEC;\nEND-ISO', `#500 = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#501,#503);
#501 = ( REPRESENTATION_RELATIONSHIP('','',#32,#10)
REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#454)
SHAPE_REPRESENTATION_RELATIONSHIP() );
#503 = PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#504);
#504 = NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','plate','',#5,#27,$);
ENDSEC;\nEND-ISO`);
throwsString(() => new IntentEngine('twice.step', Buffer.from(twice)), /one instance/);
// #2: an assembly's parts are listed, one is opened and written into the original file, and
// the other part does not see its project; a part placed twice opens as part 0.
{
  const bytes = readFileSync(new URL('tests/fixtures/parts/assembly.step', root));
  const brass = JSON.stringify({ 'part.material': 'brass' });
  const listed = JSON.parse(IntentEngine.parts('assembly.step', bytes));
  assert.deepEqual(listed.map(p => [p.part, p.name, p.instances, p.written]), [[0, 'plate', 1, false], [1, 'pin', 2, false]]);
  throwsString(() => IntentEngine.openPart('assembly.step', bytes, 2), /there is no part 2/);
  let written;
  const pin = IntentEngine.openPart('assembly.step', bytes, 1);
  try {
    assert.equal(JSON.parse(pin.snapshot()).provenance.part, 1);
    written = Buffer.from(JSON.parse(pin.writePmi(brass, project)).bytes);
  } finally { pin.free(); }
  assert.deepEqual(JSON.parse(IntentEngine.parts('assembly.step', written)).map(p => p.written), [false, true]);
  const plate = IntentEngine.openPart('assembly.step', written, 0);
  try {
    assert.equal(plate.payload('draftwright editor project'), undefined);
    assert.deepEqual(Buffer.from(plate.original()), bytes);
  } finally { plate.free(); }
  const reopened = IntentEngine.openPart('assembly.step', written, 1);
  try {
    assert.deepEqual(JSON.parse(reopened.snapshot()).analysis.resume.answers, JSON.parse(brass));
    assert.deepEqual(JSON.parse(reopened.payload('draftwright editor project')), { revision: 1, drawing: 'front' });
  } finally { reopened.free(); }
  const placedTwice = IntentEngine.openPart('twice.step', Buffer.from(twice), 0);
  try {
    assert.equal(JSON.parse(IntentEngine.parts('twice.step', Buffer.from(twice)))[0].instances, 2);
    const out = Buffer.from(JSON.parse(placedTwice.writePmi(JSON.stringify({ 'part.material': 'steel' }))).bytes);
    assert.equal(JSON.parse(IntentEngine.parts('twice.step', out))[0].written, true);
  } finally { placedTwice.free(); }
}
console.log('assemblies: parts listed, a part opened, written into the original file and reopened');
