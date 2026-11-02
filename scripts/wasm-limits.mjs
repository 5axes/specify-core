// Exercise the shipped module's limits/class write, reopen and label contracts.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import init, { IntentEngine, interpret } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
const root = new URL('../', import.meta.url);
await init({ module_or_path: readFileSync(new URL('crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', root)) });
const answers = {
  'part.general_tolerance': 'ISO 2768-mK',
  'extra.size': { kind: 'size', faces: [9], value: '7.8/8.2' },
  'extra.distance': { kind: 'distance', faces: [2], reference: [4], value: '9.8/10.2' },
};
const engine = new IntentEngine('plate.step', readFileSync(new URL('tests/fixtures/parts/plate.step', root)));
try {
  const review = JSON.parse(engine.review('{}'));
  const q = review.questions.find(q => q.id === 'part.general_tolerance');
  assert.equal(q.default, 'ISO 2768-m');
  assert.equal(q.options.length, 16);
  assert.ok(q.options.includes('ISO 2768-mK'));
  const interpreted = JSON.parse(interpret(JSON.stringify(JSON.parse(engine.snapshot()).analysis), '{}', q.id, 'mk'));
  assert.equal(interpreted, 'ISO 2768-mK');
  const write = JSON.parse(engine.writePmi(JSON.stringify(answers)));
  const back = new IntentEngine('limits.step', Uint8Array.from(write.bytes));
  try {
    assert.deepEqual(JSON.parse(back.snapshot()).analysis.resume.answers, answers);
    const scene = JSON.parse(back.pmiScene());
    const labels = Object.values(scene.specify).map(m => m.notation ?? '').join('; ');
    assert.ok(labels.includes('9.8 - 10.2') && labels.includes('⌀7.8 - ⌀8.2'), labels);
    assert.ok(labels.includes('GENERAL TOLERANCES ISO 2768-mK'), labels);
    const dims = scene.pmi.pmi.dimensions;
    assert.equal(dims.filter(d => d.value.tolerance?.limits !== undefined).length, 2);
  } finally { back.free(); }
} finally { engine.free(); }
console.log('WASM limits and ISO 2768-mK round trips passed');
