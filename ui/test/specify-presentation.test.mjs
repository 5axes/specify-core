import { test } from 'node:test';
import assert from 'node:assert/strict';
import { groupQuestions, isNone, needs, noneSuggested, NONE_SUGGESTED, rowState, summary } from '../src/specify/review.ts';
import { optionHint } from '../src/specify/hints.ts';

test('Specify row titles use typed titles despite edited or translated prompts', () => {
  for (const title of ['6× Ø6.6 holes', 'Turned 2× Ø8']) {
    const q = { id:'feature.question', feature:'f:1', title, prompt:'unrelated wording', faces:[] };
    assert.equal(groupQuestions([q])[0].title, title);
  }
});
test('an assembly part\'s parts-list questions are one row, after the part and before its features', () => {
  const q = (id, feature) => ({ id, ...feature ? { feature } : {}, prompt: id, faces: [] });
  const rows = groupQuestions([q('hole.function:h1', 'f:1'), q('bom.make_buy'), q('part.material'), q('bom.description'), q('datum.A')]);
  assert.deepEqual(rows.map(r => r.title), ['Part', 'Parts list', 'hole.function:h1', 'Datums']);
  assert.deepEqual(rows[1].questions.map(x => x.id), ['bom.make_buy', 'bom.description']);
  assert.equal(summary(rows[1], {}), "Specify's suggestions", 'Unanswered, the row is written with Specify\'s suggestions');
  // Optional: written with Specify's suggestion when not answered, so a check, not a required answer.
  assert.equal(needs({ id: 'bom.make_buy', attention: 'required', optional: true }, {}), 'check');
  assert.equal(needs({ id: 'part.material', attention: 'required' }, {}), 'required');
  assert.equal(needs({ id: 'bom.make_buy', attention: 'required', optional: true }, { 'bom.make_buy': 'made' }), null);
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

test('in the Part row, process comes before material; the other part questions keep their order', () => {
  const q = id => ({ id, prompt: id, faces: [] });
  const [part] = groupQuestions([q('part.material'), q('part.general_tolerance'), q('hole.function:h1'), q('part.process')]);
  assert.deepEqual(part.questions.map(x => x.id), ['part.process', 'part.material', 'part.general_tolerance']);
});

test('specify-ui#14: Datums is not answered while the engine lists datum C open; "none" suggestions read as such', () => {
  const q = (id, def) => ({ id, prompt: id, faces: [], default: def, attention: 'routine', kind: 'faces' });
  const [datums] = groupQuestions([q('datum.A', [1]), q('datum.B', [2]), q('datum.C', [])]);
  const given = { 'datum.A': [1], 'datum.B': [2] };
  assert.equal(rowState(datums, given, ['datum.C']), 'ok', 'not answered while datum C is open');
  assert.equal(rowState(datums, given, []), 'set', 'answered once the engine lists none of its questions open');
  // Other rows keep their rule: answered once any of their questions is.
  const [hole] = groupQuestions([{ ...q('hole.function:h', 'fit:H7'), feature: 'h' }, { ...q('hole.position:h', 0.1), feature: 'h' }]);
  assert.equal(rowState(hole, { 'hole.function:h': 'fit:H7' }, ['hole.position:h']), 'set');
  for (const value of [[], 'None', 'None (as machined)', 'none']) assert.ok(isNone(value), JSON.stringify(value));
  for (const value of [[3], 'Anodised', 'ISO 2768-m']) assert.ok(!isNone(value), JSON.stringify(value));
  assert.ok(noneSuggested(q('part.notes', []), {}));
  assert.ok(!noneSuggested(q('part.notes', []), { 'part.notes': [] }), 'answered none is an answer, not a suggestion');
  assert.ok(!noneSuggested(q('part.material', null), {}), 'no suggestion at all is not a none suggestion');
  assert.equal(NONE_SUGGESTED, 'None (suggested)');
});
