import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSession, createSessionStore, reapply } from '../app/answers.ts';
import { Refused } from '../app/local.ts';
import { DefaultsChanged, FileChanged, NotShown, sessionSaver, Stale } from '../app/save.ts';
// The local app's answers store over the command's session (specify-core docs/open.md): the page's
// changes go up with `PUT answers {rev, part, answers}`, reapplied on a 409; events from others land.

const none = async () => [];

/** A session the store talks to: its rev and part 0's answers, and every PUT it was sent.
 * ``s.during(put)``, when set, runs while a PUT is on its way (after the command applied it). */
function session(rev = 0, answers = {}) {
  const s = { rev, answers, puts: [], during: null };
  s.put = async (at, part, given) => {
    s.puts.push({ at, part, given });
    if (at !== s.rev) throw new Refused('stale', 409, { rev: s.rev, parts: { 0: s.answers } });
    s.rev += 1; s.answers = given;
    const rev = s.rev;
    if (s.during) { const during = s.during; s.during = null; await during(); }
    return { rev };
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
  store.set({ 'part.edges': 'Break 0.2' }, {});
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
  store.set({ a: 1, b: 2, c: 3 }, { a: 1, b: 2 });
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

// B1: the person's edit was made on {a}; while Specify reviewed it the agent's {a, c} arrived. The
// edit's change (b added) goes onto the agent's answers: c is kept, in the page and the session.
test('a page edit made before an agent\'s answer arrived keeps the agent\'s answer', async () => {
  const s = session(0, { a: 1 });
  const store = createSessionStore(0, 0, { a: 1 }, { a: 1 }, s.put);
  const base = store.get();
  s.rev = 1; s.answers = { a: 1, c: 3 };
  store.receive({ rev: 1, part: 0, answers: { a: 1, c: 3 }, source: 'mcp' });
  store.set({ a: 1, b: 2 }, base);
  assert.deepEqual(store.get(), { a: 1, b: 2, c: 3 }, 'the page shows both at once');
  assert.equal(await store.settled(), 2);
  assert.deepEqual(s.answers, { a: 1, c: 3, b: 2 }, 'the session keeps the agent\'s c');
  assert.deepEqual(s.puts.map(p => p.at), [1], 'sent once, at the agent\'s rev');
  assert.deepEqual(store.get(), s.answers);
});

// B2: the agent's change lands while the page's PUT is on its way (its event before the PUT's
// answer). The page shows it, and the rev Save gets is one whose answers the page has shown.
test('an agent\'s change landing during a page PUT is shown, and settled() is its rev', async () => {
  const s = session(0, {});
  const store = createSessionStore(0, 0, {}, {}, s.put);
  const seen = [];
  store.subscribe((answers, source) => seen.push({ answers, source }));
  s.during = async () => {
    // The command applied the page's PUT (rev 1); the agent answers (rev 2) and its event arrives first.
    s.rev = 2; s.answers = { ...s.answers, c: 3 };
    store.receive({ rev: 1, part: 0, answers: { b: 2 }, source: 'page' });
    store.receive({ rev: 2, part: 0, answers: { b: 2, c: 3 }, source: 'mcp' });
  };
  store.set({ b: 2 }, {});
  const rev = await store.settled();
  assert.equal(rev, 2);
  assert.deepEqual(store.get(), { b: 2, c: 3 }, 'the page shows the agent\'s change');
  assert.ok(seen.some(e => e.source === 'mcp' && e.answers.c === 3), 'shown as the agent\'s');
  assert.deepEqual(seen.at(-1).answers, { b: 2, c: 3 });
  // Save: the session's rev 2 is saved only once the page shows rev 2's answers.
  const writes = [];
  const write = async (at) => { writes.push(at); return { path: 'p', bytes: 1, rev: at, report: {}, complete: true, missing: [] }; };
  await assert.rejects(sessionSaver(store, store.session, () => ({ b: 2 }), none, write).save({ defaults: [] }), NotShown);
  assert.deepEqual(writes, [], 'nothing saved that the page has not shown');
  await sessionSaver(store, store.session, () => ({ c: 3, b: 2 }), none, write).save({ defaults: [] });
  assert.deepEqual(writes, [2]);
});

test('a save refused because the file changed on disk is told apart from a stale rev', async () => {
  const store = createSessionStore(0, 4, { a: 1 }, { a: 1 }, async () => ({ rev: 5 }));
  const refuse = rev => async () => { throw new Refused('changed', 409, { rev }); };
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), none, refuse(4)).save({ defaults: [] }), FileChanged);
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), none, refuse(6)).save({ defaults: [] }), Stale);
});

// S1: unsaved is the whole file's, whichever part is shown: answers left unsaved on a part not
// shown count, and after a save the parts shown next are not unsaved for the answers saved.
test('unsaved answers are the session\'s, across parts, against the last save', async () => {
  const puts = [];
  let rev = 0;
  const put = async (at, part, answers) => { puts.push({ at, part, answers }); rev = at + 1; return { rev }; };
  const s = createSession(0, { 0: {}, 1: { m: 'stored' } }, put);
  const first = s.store(0, {});
  first.set({ a: 1 }, {});
  await first.settled();
  assert.equal(s.dirty(), true);
  // Another part shown: the first part's answer is still unsaved.
  first.close();
  const second = s.store(1, { m: 'stored' });
  assert.equal(s.dirty(), true, 'part 0\'s answer is still not on disk');
  // A save of rev 1 (both parts written): nothing unsaved; back on part 0, still nothing.
  s.markSaved(1);
  assert.equal(s.dirty(), false);
  second.close();
  const again = s.store(0, {});
  assert.equal(s.dirty(), false, 'part 0 as the file was opened is not what is on disk after the save');
  again.set({ a: 1, b: 2 }, { a: 1 });
  assert.equal(s.dirty(), true);
});

test('a save heard of before its revision is taken when the revision arrives', () => {
  const s = createSession(3, { 0: { a: 1 } });
  s.store(0, {});
  assert.equal(s.dirty(), true);
  s.markSaved(4);
  s.receive({ rev: 4, part: 0, answers: { a: 1, b: 2 }, source: 'mcp' });
  assert.equal(s.dirty(), false);
});

test('a page change that changes nothing (Specify reviewing the answers it opened with) is not sent', async () => {
  const s = session(0, { a: 1 });
  const store = createSessionStore(0, 0, { a: 1 }, { a: 1 }, s.put);
  store.set({ a: 1 }, { a: 1 });
  await store.settled();
  assert.deepEqual(s.puts, []);
});

// ADR P9: defaults are written only when the person confirmed the list of them. A plain save never
// asks for defaults; a save with defaults asks only when the defaults it would write are the ones confirmed.
const d = (part, id, value) => ({ part, id, prompt: id, value: String(value), raw: JSON.stringify(value) });

test('a save with defaults writes only the defaults confirmed', async () => {
  const store = createSessionStore(0, 2, { a: 1 }, { a: 1 }, async () => ({ rev: 3 }));
  const writes = [];
  const write = async (rev, acceptDefaults) => { writes.push([rev, acceptDefaults]); return { path: 'p', bytes: 1, rev, report: {}, complete: true, missing: [] }; };
  const defaultsOf = async () => [d(0, 'b', 1), d(0, 'c', 2)];
  await sessionSaver(store, store.session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [] });
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'b', 1)] }), DefaultsChanged);
  await sessionSaver(store, store.session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'c', 2), d(0, 'b', 1)] });
  assert.deepEqual(writes, [[2, false], [2, true]], 'defaults asked for only with the confirmed list; nothing written for a list that changed');
});

// Review round 2, item 2: the same questions defaulted to another value (an agent's answer changed
// what a default is) is not what was confirmed.
test('a confirmed default whose value changed before the save is confirmed again', async () => {
  const s = session(2, { a: 1 });
  const store = createSessionStore(0, 2, { a: 1 }, { a: 1 }, s.put);
  const writes = [];
  const write = async (rev, acceptDefaults) => { writes.push([rev, acceptDefaults]); return { path: 'p', bytes: 1, rev, report: {}, complete: true, missing: [] }; };
  // The default of b follows a: 0.1 while a is 1, 0.2 once the agent answers a = 2.
  const defaultsOf = async (parts) => [d(0, 'b', parts['0'].a === 1 ? 0.1 : 0.2)];
  const confirmed = await defaultsOf(store.session.parts());
  store.receive({ rev: 3, part: 0, answers: { a: 2 }, source: 'mcp' });
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 2 }), defaultsOf, write).save({ defaults: confirmed }), DefaultsChanged);
  assert.deepEqual(writes, [], 'nothing written: the list is shown again');
  await sessionSaver(store, store.session, () => ({ a: 2 }), defaultsOf, write).save({ defaults: await defaultsOf(store.session.parts()) });
  assert.deepEqual(writes, [[3, true]]);
});

// Review round 2, item 1 (ADR P9): a save writes every part with answers in the session, so the
// defaults confirmed are every such part's, not only the part shown's.
test('a save with defaults of an assembly checks the defaults of every part it writes', async () => {
  const session = createSession(5, { 0: { a: 1 }, 3: { m: 'Steel' } }, async () => ({ rev: 6 }));
  const store = session.store(0, {});
  const writes = [];
  const write = async (rev, acceptDefaults) => { writes.push([rev, acceptDefaults]); return { path: 'p', bytes: 1, rev, report: {}, complete: true, missing: [] }; };
  const asked = [];
  const defaultsOf = async (parts) => { asked.push(Object.keys(parts).sort()); return Object.keys(parts).map((k) => d(Number(k), 'edges', 0.5)); };
  // Only the part shown's default confirmed: refused, and nothing written.
  await assert.rejects(sessionSaver(store, session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'edges', 0.5)] }), DefaultsChanged);
  assert.deepEqual(asked, [['0', '3']], 'the defaults of every part in the session');
  assert.deepEqual(writes, []);
  await sessionSaver(store, session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(3, 'edges', 0.5), d(0, 'edges', 0.5)] });
  assert.deepEqual(writes, [[5, true]]);
});

// Review round 2, nit: answers an agent gave a part before the page loaded are unsaved, even when
// the page never shows that part (the file stores none for it, or others).
test('answers given to a part not shown before the page loaded are unsaved', () => {
  const fresh = createSession(2, { 0: {}, 4: { 'part.material': 'Steel' } });
  fresh.store(0, {});
  fresh.fileAnswers(4, {});
  assert.equal(fresh.dirty(), true, 'the file stores no answers for part 4');
  const written = createSession(2, { 0: {}, 4: { 'part.material': 'Steel' } });
  written.store(0, {});
  written.fileAnswers(4, { 'part.material': 'Steel' });
  assert.equal(written.dirty(), false, 'what the file stores is not unsaved');
  written.markSaved(2);
  written.fileAnswers(4, {});
  assert.equal(written.dirty(), false, 'after a save, the save is what is on disk');
});
