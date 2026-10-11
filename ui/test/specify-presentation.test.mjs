import { test } from 'node:test';
import assert from 'node:assert/strict';
import { groupQuestions } from '../src/specify/review.ts';
import { optionHint } from '../src/specify/hints.ts';

test('Specify row titles use typed titles despite edited or translated prompts', () => {
  for (const title of ['6× Ø6.6 holes', 'Turned 2× Ø8']) {
    const q = { id:'feature.question', feature:'f:1', title, prompt:'unrelated wording', faces:[] };
    assert.equal(groupQuestions([q])[0].title, title);
  }
});
test('Specify zone hints use typed characteristics despite edited or translated prompts', () => {
  for (const [characteristic, text] of [
    ['flatness', 'two parallel planes'], ['perpendicularity', 'square to the datum'],
    ['parallelism', 'parallel to the datum'], ['position', 'around where it should be'],
    ['runout', 'wobble'],
  ]) {
    const q = { id:'datum.B.control', characteristic, prompt:'unrelated wording' };
    assert(optionHint(q, '0.05')?.includes(text), characteristic);
  }
  assert.equal(optionHint({id:'datum.B.control',prompt:'Flatness but no typed characteristic'}, '0.05'), null);
});
