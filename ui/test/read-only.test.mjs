import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readOnlyEngine } from '../src/specify/read-only.ts';

const counting = () => {
  const calls = { questions: 0, interpret: 0, preview: [] };
  const inner = {
    questions: async () => { calls.questions++; return { questions: [] }; },
    interpret: async () => { calls.interpret++; return { value: 'x' }; },
    preview: async (...args) => { calls.preview.push(args); return { shown: args }; },
  };
  return { calls, engine: readOnlyEngine(inner) };
};

test('questions and interpret are refused as read_only without reaching the wrapped engine', async () => {
  const { calls, engine } = counting();
  for (const call of [() => engine.questions('s', { a: 1 }), () => engine.interpret('s', {}, 'q', 'text')]) {
    await assert.rejects(call, (error) => error.code === 'read_only' && /^Read only: /.test(error.message));
  }
  assert.equal(calls.questions, 0);
  assert.equal(calls.interpret, 0);
});

test('preview passes through with the same arguments and result', async () => {
  const { calls, engine } = counting();
  const answers = { 'part.material': 'steel' };
  const result = await engine.preview('s1', answers);
  assert.equal(calls.preview.length, 1);
  assert.equal(calls.preview[0][0], 's1');
  assert.equal(calls.preview[0][1], answers);
  assert.deepEqual(result, { shown: ['s1', answers] });
});
