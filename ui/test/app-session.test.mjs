import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSession, createSessionStore, reapply } from '../app/answers.ts';
import { getDiff, Refused, save as postSave } from '../app/local.ts';
import { acceptedDefaults, defaultParts, DefaultRefused, DefaultsChanged, FileChanged, matchMissing, NotShown, sessionSaver, Stale, writtenParts } from '../app/save.ts';
// The local app's answers store over the command's session (specify-core docs/open.md): the page's
// changes go up with `PUT answers {rev, part, answers}`, reapplied on a 409; events from others land.

const none = async () => [];

// Full answer replay history was removed with reconstruction; retain disk-baseline coverage.
test('a delayed saved revision still tracks disk answers after further edits', () => {
  const metadata = { schema: 2, high_water: 17, entries: [] };
  const saved = { 'part.material': 'Steel', 'part.surface_finish': 'Ra 3.2', _specify_characteristic_numbers: metadata };
  const session = createSession(0, { 0: {} });
  session.receive({ rev: 1, part: 0, answers: saved, source: 'page' });
  session.receive({ rev: 2, part: 0, answers: { 'part.material': 'Brass' }, source: 'page' });
  session.markSaved(1);
  assert.equal(session.dirty(), true);
  assert.deepEqual(session.parts(), { 0: { 'part.material': 'Brass' } });
  session.receive({ rev: 3, part: 0, answers: saved, source: 'page' });
  assert.equal(session.dirty(), false, 'the saved answers remain the disk baseline, independent of current answers');
});

test('closing while Save waits for pending answers prevents the deferred POST', async () => {
  let started;
  const began = new Promise((resolve) => { started = resolve; });
  const session = createSession(0, { 0: {} }, (_rev, _part, _answers, signal) => new Promise((_resolve, reject) => {
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    started();
  }));
  const store = session.store(0);
  store.set({ material: 'Steel' }, {});
  await began;
  const posts = [];
  const saving = sessionSaver(store, session, () => store.get(), none, async (...args) => posts.push(args)).save({ defaults: [] });
  const cancelled = assert.rejects(saving, { name: 'AbortError' });
  session.close();
  await cancelled;
  assert.deepEqual(posts, []);
});

test('closing while Save reviews confirmed defaults prevents the deferred POST', async () => {
  const session = createSession(0, { 0: {} });
  const store = session.store(0);
  const defaults = [{ part: 0, id: 'edges', prompt: 'Edges?', value: '0.2', raw: '0.2' }];
  let finish;
  let started;
  const began = new Promise((resolve) => { started = resolve; });
  const review = () => new Promise((resolve) => { finish = resolve; started(); });
  const posts = [];
  const saving = sessionSaver(store, session, () => ({}), review, async (...args) => posts.push(args)).save({ defaults });
  const cancelled = assert.rejects(saving, { name: 'AbortError' });
  await began;
  session.close();
  finish(defaults);
  await cancelled;
  assert.deepEqual(posts, []);
});

test('assembly missing IDs retain part order, including a part that was cleared', () => {
  const empty = { open: ['material', 'finish'], labels: ['A material', 'A finish'], required: false };
  const answered = { open: ['finish'], labels: ['B finish'], required: true };
  assert.deepEqual(matchMissing(['material', 'finish', 'finish'], [empty, answered]), ['A material', 'A finish', 'B finish']);
  assert.deepEqual(matchMissing(['finish'], [empty, answered]), ['B finish'], 'a joined but untouched part is omitted');
  assert.equal(matchMissing(['material', 'finish', 'finish'], [empty, { ...empty, labels: ['C material', 'C finish'] }, answered]), null,
    'a flat ID list cannot distinguish two empty parts with the same questions');
});

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

test('a stale answer response arriving after page close cannot start another PUT', async () => {
  const calls = [];
  let refuse;
  const session = createSession(0, { 0: {} }, (rev, part, answers, signal) => {
    calls.push({ rev, part, answers, signal });
    return new Promise((_resolve, reject) => { refuse = reject; });
  });
  const store = session.store(0);
  store.set({ material: 'Steel' }, {});
  await Promise.resolve();
  session.close();
  assert.equal(calls[0].signal.aborted, true);
  refuse(new Refused('stale', 409, { rev: 1, parts: { 0: { edges: 'Break 0.2' } } }));
  await store.settled();
  assert.equal(calls.length, 1, 'the stale-response retry is cancelled');
  assert.deepEqual(session.parts(), { 0: {} }, 'a late response does not alter the closed session');
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
  // The command's `acceptDefaults` is now the confirmed question ids with the values seen, by part (was a boolean, then ids): `{}` for a plain save.
  assert.deepEqual(writes, [[2, {}], [2, { 0: { c: 2, b: 1 } }]], 'exactly the confirmed ids with the values seen, by part; nothing written for a list that changed');
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
  assert.deepEqual(writes, [[3, { 0: { b: 0.2 } }]]);
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
  assert.deepEqual(writes, [[5, { 0: { edges: 0.5 }, 3: { edges: 0.5 } }]]);
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

// Contract change (specify-core 2026-11-18): the approvals are per part. Approving a default for
// part 0 sends part 0's id only: part 1 has the same question with a default, and is not named.
test('approving a default for one part names that part only', async () => {
  const session = createSession(2, { 0: { a: 1 }, 1: { m: 'Steel' } }, async () => ({ rev: 3 }));
  const store = session.store(0, {});
  const writes = [];
  const write = async (rev, acceptDefaults) => { writes.push(acceptDefaults); return { path: 'p', bytes: 1, rev, report: {}, complete: true, missing: [] }; };
  const defaultsOf = async () => [d(0, 'edges', 0.5), d(1, 'edges', 0.5)];
  await assert.rejects(sessionSaver(store, session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'edges', 0.5)] }), DefaultsChanged, 'the list shown had both: one alone is not it');
  // The person confirmed the list as shown; the body names each id on its own part.
  await sessionSaver(store, session, () => ({ a: 1 }), async () => [d(0, 'edges', 0.5)], write).save({ defaults: [d(0, 'edges', 0.5)] });
  assert.deepEqual(writes, [{ 0: { edges: 0.5 } }], 'part 1 is not named, so it is untouched');
  assert.deepEqual(acceptedDefaults([d(2, 'x', 1), d(0, 'a', 1), d(2, 'y', 1)]), { 0: { a: 1 }, 2: { x: 1, y: 1 } });
  assert.deepEqual(acceptedDefaults([]), {});
});

test('a default the command refuses (400 with its part and question) is reported by them', async () => {
  const store = createSessionStore(0, 2, { a: 1 }, { a: 1 }, async () => ({ rev: 3 }));
  const write = async () => { throw new Refused('no open question with a default', 400, { error: 'no open question with a default', part: 0, question: 'b' }); };
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), async () => [d(0, 'b', 1)], write).save({ defaults: [d(0, 'b', 1)] }),
    (e) => e instanceof DefaultRefused && e.part === 0 && e.question === 'b' && /no open question/.test(e.message));
});

test('the page sends acceptDefaults as the per-part object, never a boolean', async () => {
  const real = globalThis.fetch;
  const sent = [];
  globalThis.fetch = async (url, init) => { sent.push(JSON.parse(init.body)); return new Response('{"path":"p","bytes":1,"rev":4,"report":{},"complete":true,"missing":[]}', { status: 200 }); };
  try {
    await postSave(4, {});
    await postSave(4, { 1: { edges: 0.5 } });
  } finally { globalThis.fetch = real; }
  assert.deepEqual(sent, [{ rev: 4, acceptDefaults: {} }, { rev: 4, acceptDefaults: { 1: { edges: 0.5 } } }]);
});

// A save writes only parts with at least one answer, plus a single-part file's only part.
test('a save writes the parts with answers, and a file\'s only part', () => {
  assert.deepEqual(writtenParts({ 0: { a: 1 }, 1: {}, 2: { _specify: { x: 1 } }, 3: { m: 'Steel' } }, true), [0, 3], 'no answers (or only Specify\'s records): not written');
  assert.deepEqual(writtenParts({ 0: {} }, false), [0], 'a single part is always written');
  assert.deepEqual(writtenParts({}, false), [0]);
  assert.deepEqual(writtenParts({}, true), []);
});

// A joining part event is a change (a new rev, answers possibly empty): the session holds it, a save
// at the older rev is stale, and the defaults a save lists include the part once it has an answer.
test('a part joining the session is held, and counts for a save once it has answers', () => {
  const session = createSession(2, { 0: { a: 1 } });
  const store = session.store(0, {});
  session.receive({ rev: 3, part: 1, answers: {}, source: 'mcp', joined: true });
  assert.equal(session.rev(), 3, 'the join is a new rev: a save at 2 gets a 409 and the page re-lists');
  assert.deepEqual(Object.keys(session.parts()), ['0', '1'], 'part 1 is in the page\'s model');
  assert.deepEqual(writtenParts(session.parts(), true), [0], 'with no answers it is not written');
  session.receive({ rev: 4, part: 1, answers: { m: 'Steel' }, source: 'mcp' });
  assert.deepEqual(writtenParts(session.parts(), true), [0, 1]);
  store.close();
});

test('a save approving defaults is shown as answers on the part: the event carries them', () => {
  const session = createSession(2, { 0: { a: 1 }, 1: { m: 'Steel' } });
  const store0 = session.store(0, {});
  const store1 = session.store(1, {});
  const seen = [];
  store1.subscribe((answers, source, previous) => seen.push({ answers, source, previous }));
  session.receive({ rev: 3, part: 1, answers: { m: 'Steel', edges: 0.5 }, source: 'mcp', defaults: { 1: ['edges'] } });
  assert.deepEqual(seen, [{ answers: { m: 'Steel', edges: 0.5 }, source: 'mcp', previous: { m: 'Steel' } }], 'the approved default is an answer on part 1');
  assert.deepEqual(store0.get(), { a: 1 }, 'part 0 untouched');
});

test('GET diff is read as WASM diff() gives it, with its scenes', async () => {
  const real = globalThis.fetch;
  const body = { report: { verdict: 'ok', pmi: { scenes: { old: {}, new: {} } } }, markdown: '# x', view: { faces: {} } };
  globalThis.fetch = async () => new Response(JSON.stringify(body), { status: 200 });
  try { assert.deepEqual(await getDiff(), body, 'handed on whole: StepDiff draws from it'); } finally { globalThis.fetch = real; }
});

// Contract 2026-11-18: a value that is no longer the server's default is a 400 with `expected` and
// `current`; nothing was written. The saver hands both on.
test('a default whose value no longer matches is refused with what was expected and what it is now', async () => {
  const store = createSessionStore(0, 2, { a: 1 }, { a: 1 }, async () => ({ rev: 3 }));
  const write = async () => { throw new Refused('value changed', 400, { error: 'value changed', part: 0, question: 'b', expected: 1, current: 2 }); };
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), async () => [d(0, 'b', 1)], write).save({ defaults: [d(0, 'b', 1)] }),
    (e) => e instanceof DefaultRefused && e.part === 0 && e.question === 'b' && e.values?.expected === 1 && e.values?.current === 2);
  // Without expected/current it is the other 400 (not an open question with a default).
  const other = async () => { throw new Refused('no', 400, { error: 'no', part: 0, question: 'b' }); };
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), async () => [d(0, 'b', 1)], other).save({ defaults: [d(0, 'b', 1)] }),
    (e) => e instanceof DefaultRefused && e.values === null);
});

test('the value sent is the default as the person saw it, structured values included', () => {
  assert.deepEqual(acceptedDefaults([d(0, 'm', 'ISO 2768-m'), d(1, 'edges', { a: [1, 2] })]), { 0: { m: 'ISO 2768-m' }, 1: { edges: { a: [1, 2] } } });
});

// One rev can carry several `answers` events (one per part); a repeat of (rev, part) is ignored.
test('events are deduped by (rev, part): one rev may carry one event per part', () => {
  const session = createSession(2, { 0: { a: 1 }, 1: { m: 'Steel' } });
  const store0 = session.store(0, {});
  const store1 = session.store(1, {});
  const seen = [];
  store0.subscribe((answers) => seen.push(['0', answers]));
  store1.subscribe((answers) => seen.push(['1', answers]));
  assert.equal(session.receive({ rev: 3, part: 0, answers: { a: 1, e: 0.5 }, source: 'mcp', defaults: { 0: ['e'], 1: ['e'] } }), true);
  assert.equal(session.receive({ rev: 3, part: 1, answers: { m: 'Steel', e: 0.5 }, source: 'mcp', defaults: { 0: ['e'], 1: ['e'] } }), true, 'the same rev, another part: heard');
  assert.equal(session.receive({ rev: 3, part: 0, answers: { a: 1, e: 0.5 }, source: 'mcp' }), false, 'the same rev and part again: ignored');
  assert.equal(session.rev(), 3);
  assert.deepEqual(store0.get(), { a: 1, e: 0.5 });
  assert.deepEqual(store1.get(), { m: 'Steel', e: 0.5 });
  assert.equal(seen.length, 2, 'each part told once');
});

test('a repeated event does not undo what a later resync holds', () => {
  const session = createSession(2, { 0: { a: 1 } });
  session.receive({ rev: 3, part: 0, answers: { a: 2 }, source: 'mcp' });
  session.receive({ rev: 4, parts: { 0: { a: 3 } }, source: 'start' });
  assert.equal(session.receive({ rev: 3, part: 0, answers: { a: 2 }, source: 'mcp' }), false);
  assert.deepEqual(session.answers(0), { a: 3 });
});

// A failed write rolls its approvals back (a further rev): the session ends as it was, so the
// approvals the page was told of are gone from what it shows.
test('approvals shown as answers are shown rolled back when the save failed', () => {
  const session = createSession(2, { 0: { a: 1 } });
  const store = session.store(0, { a: 1 });
  session.receive({ rev: 3, part: 0, answers: { a: 1, e: 0.5 }, source: 'page', defaults: { 0: ['e'] } });
  assert.deepEqual(store.get(), { a: 1, e: 0.5 });
  assert.equal(session.dirty(), true);
  session.receive({ rev: 4, parts: { 0: { a: 1 } }, source: 'start' }); // GET answers after the failure
  assert.deepEqual(store.get(), { a: 1 }, 'not shown as applied');
  assert.equal(session.dirty(), false, 'nothing unsaved either');
});

// Save with defaults lists every part of the file, answered or not: approving a part's defaults is
// enough for the command to write it (specify-core docs/open.md: approvals are applied as answers
// first, and a part with an approval is a part the session changed). Plain save is unchanged.
test('save with defaults covers every part of an assembly, even where no part has an answer', async () => {
  const session = createSession(5, {}, async () => ({ rev: 6 }));
  const store = session.store(0, {});
  assert.deepEqual(defaultParts(session.parts(), [0, 1, 2], true), [0, 1, 2], 'every part of the file, though the session holds no answers');
  assert.deepEqual(defaultParts({ 3: { a: 1 } }, [0, 1], true), [0, 1, 3], 'and a part only the session has');
  assert.deepEqual(defaultParts({}, [0], false), [0], 'a single part still works');
  const writes = [];
  const write = async (rev, acceptDefaults) => { writes.push([rev, acceptDefaults]); return { path: 'p', bytes: 1, rev, report: {}, complete: false, missing: [] }; };
  const defaultsOf = async (parts) => defaultParts(parts, [0, 1, 2], true).map((k) => d(k, 'edges', 0.5));
  const listed = await defaultsOf(session.parts());
  await sessionSaver(store, session, () => ({}), defaultsOf, write).save({ defaults: listed });
  assert.deepEqual(writes, [[5, { 0: { edges: 0.5 }, 1: { edges: 0.5 }, 2: { edges: 0.5 } }]], 'acceptDefaults names every part');
  // Left one part's default out of what was confirmed: it is not the list that would be written.
  await assert.rejects(sessionSaver(store, session, () => ({}), defaultsOf, write).save({ defaults: listed.slice(0, 2) }), DefaultsChanged);
  assert.equal(writes.length, 1);
  // A plain save of the same untouched assembly sends nothing for any part (so none is written).
  await sessionSaver(store, session, () => ({}), defaultsOf, write).save({ defaults: [] });
  assert.deepEqual(writes[1], [5, {}]);
  assert.deepEqual(writtenParts(session.parts(), true), [], 'plain save: no part has answers, so none is written');
  store.close();
});
