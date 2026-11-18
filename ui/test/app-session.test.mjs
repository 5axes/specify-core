import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSessionStore, reapply } from '../app/answers.ts';
import { Refused } from '../app/local.ts';
// The local app's answers store over the command's session (specify-core docs/open.md): the page's
// changes go up with `PUT answers {rev, part, answers}`, reapplied on a 409; events from others land.

/** A session the store talks to: its rev and part 0's answers, and every PUT it was sent. */
function session(rev = 0, answers = {}) {
  const s = { rev, answers, puts: [] };
  s.put = async (at, part, given) => {
    s.puts.push({ at, part, given });
    if (at !== s.rev) throw new Refused('stale', 409, { rev: s.rev, parts: { 0: s.answers } });
    s.rev += 1; s.answers = given;
    return { rev: s.rev };
  };
  return s;
}

test('reapply puts the page\'s change onto answers changed meanwhile', () => {
  assert.deepEqual(reapply({ a: 1, b: 2 }, { a: 1 }, { a: 3, c: 4 }), { a: 3, b: 2, c: 4 });
  assert.deepEqual(reapply({ a: 1, b: 2 }, { a: 1, b: 2 }, { b: 2 }), { b: 2 }, 'a cleared answer is cleared');
});

test('a page change is sent at the session\'s rev; on a 409 it is reapplied over the agent\'s', async () => {
  const s = session(0, {});
  const store = createSessionStore(0, 0, {}, {}, s.put);
  // The agent answered meanwhile; the page has not heard yet.
  s.rev = 1; s.answers = { 'part.material': 'Steel' };
  const seen = [];
  store.subscribe((answers, source) => seen.push([answers, source]));
  store.set({ 'part.edges': 'Break 0.2' }, 'page');
  assert.equal(await store.settled(), 2);
  assert.deepEqual(s.answers, { 'part.material': 'Steel', 'part.edges': 'Break 0.2' }, 'the agent\'s answer kept');
  assert.deepEqual(s.puts.map(p => p.at), [0, 1]);
  assert.deepEqual(store.get(), s.answers, 'the page shows both');
  assert.deepEqual(seen.at(-1)[0], s.answers);
});

test('events: an agent\'s change lands, the page\'s own echo changes nothing, another part is ignored', async () => {
  const s = session(3, { a: 1 });
  const store = createSessionStore(0, 3, { a: 1 }, { a: 1 }, s.put);
  const seen = [];
  store.subscribe((answers, source, previous) => seen.push({ answers, source, previous }));
  store.receive({ rev: 4, part: 0, answers: { a: 1, b: 2 }, source: 'mcp' });
  assert.deepEqual(seen, [{ answers: { a: 1, b: 2 }, source: 'mcp', previous: { a: 1 } }]);
  assert.equal(store.dirty(), true);
  store.receive({ rev: 5, part: 1, answers: { z: 1 }, source: 'mcp' });
  store.receive({ rev: 3, part: 0, answers: {}, source: 'page' });
  assert.equal(seen.length, 1, 'other parts and old revisions change nothing');
  s.rev = 5; s.answers = { a: 1, b: 2 };
  store.set({ a: 1, b: 2, c: 3 }, 'page');
  store.receive({ rev: 6, part: 0, answers: { a: 1, b: 2, c: 3 }, source: 'page' });
  assert.equal(await store.settled(), 6);
  assert.equal(seen.length, 2, 'the echo of the page\'s own change is not a change');
  // A save of rev 6 (whoever asked): nothing unsaved.
  store.markSaved(6);
  assert.equal(store.dirty(), false);
});

test('unsaved: the session\'s answers against the file\'s, Specify\'s own records aside', () => {
  const store = createSessionStore(0, 0, { a: 1, _specify: { x: 1 } }, { a: 1 }, async () => ({ rev: 1 }));
  assert.equal(store.dirty(), false);
  const agentFirst = createSessionStore(0, 2, { a: 1 }, {}, async () => ({ rev: 3 }));
  assert.equal(agentFirst.dirty(), true, 'answers given before the page opened are not on disk');
});
