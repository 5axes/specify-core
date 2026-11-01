import { test } from 'node:test';
import assert from 'node:assert/strict';
import { gzipSync } from 'node:zlib';
import { changedSteps, diffQuery, fileAt, githubLink, handedFiles, reportHtml, stepBytes } from '../src/model/diff.ts';

const sha = 'a8622c3f0e1d2c3b4a5968778695a4b3c2d1e0f9';

test('a diff link names a repository, two commits and relative paths, and nothing else', () => {
  const q = diffQuery(`?repo=pzfreo/step-diff-demo&base=${sha}&head=0153025&path=parts/a%20%23b.step`);
  assert.deepEqual(q, { repo: 'pzfreo/step-diff-demo', base: sha, head: '0153025', path: 'parts/a #b.step', was: 'parts/a #b.step', part: '' });
  assert.equal(diffQuery(`?repo=o/r&base=${sha}&head=${sha}&path=new.step&was=old.step`).was, 'old.step');
  // An assembly's part, as the action's link names it (jq's @uri), or a URLSearchParams one.
  assert.equal(diffQuery(`?repo=o/r&base=${sha}&head=${sha}&path=a.step&part=GRM-04%20drive%20plate`).part, 'GRM-04 drive plate');
  assert.equal(diffQuery(`?repo=o/r&base=${sha}&head=${sha}&path=a.step&part=GRM-02+shaft`).part, 'GRM-02 shaft');
  assert.equal(diffQuery(`?repo=o/r&base=${sha}&head=${sha}&path=a.step&part=${'x'.repeat(500)}`).part, 'x'.repeat(500));
  for (const bad of [
    `?repo=o&base=${sha}&head=${sha}&path=a.step`,
    `?repo=o/r/x&base=${sha}&head=${sha}&path=a.step`,
    `?repo=o/r&base=main&head=${sha}&path=a.step`,
    `?repo=o/r&base=${sha}&head=${sha}&path=../a.step`,
    `?repo=o/r&base=${sha}&head=${sha}&path=/a.step`,
    `?repo=o/r&base=${sha}&head=${sha}&path=a.step&was=x//a.step`,
    `?repo=o/r&base=${sha}&head=${sha}`,
  ]) assert.throws(() => diffQuery(bad), /does not name/, bad);
});

test('files are fetched from raw.githubusercontent.com only, a missing one as null', async () => {
  const asked = [];
  globalThis.fetch = async url => {
    asked.push(url);
    return url.endsWith('missing.json') ? new Response('', { status: 404 }) : new Response('ISO-10303-21;');
  };
  assert.equal(new TextDecoder().decode(await fileAt('o/r', sha, 'parts/a #b.step')), 'ISO-10303-21;');
  assert.equal(await fileAt('o/r', sha, 'missing.json'), null);
  assert.deepEqual(asked, [`https://raw.githubusercontent.com/o/r/${sha}/parts/a%20%23b.step`, `https://raw.githubusercontent.com/o/r/${sha}/missing.json`]);
  globalThis.fetch = async () => new Response('', { status: 500 });
  await assert.rejects(fileAt('o/r', sha, 'a.step'), /GitHub answered 500 for a.step at a8622c3/);
});

test('the report renders without its heading and summary, and escapes what it shows', () => {
  const html = reportHtml([
    '### a.step: Specify', '', '✅ 1 hole moved.', '', '| | Before | After |', '|---|---|---|', '| Volume | 1 | 2 |', '',
    '**What changed**', '', '- Ø5 hole <b>: moved `X +2`', '',
  ].join('\n'));
  assert.doesNotMatch(html, /a\.step: Specify|1 hole moved/);
  assert.match(html, /<thead><tr><th><\/th><th>Before<\/th><th>After<\/th><\/tr><\/thead><tbody><tr><td>Volume<\/td><td>1<\/td><td>2<\/td><\/tr><\/tbody>/);
  assert.match(html, /<p><strong>What changed<\/strong><\/p>/);
  assert.match(html, /<li data-change="0" tabindex="0">Ø5 hole &#60;b&#62;: moved <code>X \+2<\/code><\/li>/);
});

test('only the "What changed" lines are numbered, in order', () => {
  const html = reportHtml([
    '**What changed**', '', '- a', '- b', '', '**Answers carried to a changed feature**', '', '- c', '',
  ].join('\n'));
  assert.match(html, /<li data-change="0" tabindex="0">a<\/li><li data-change="1" tabindex="0">b<\/li>/);
  assert.match(html, /<li>c<\/li>/);
});

test('a picked file is read as it is, or unpacked when gzipped', async () => {
  const step = 'ISO-10303-21;\nHEADER;\nENDSEC;\n';
  const read = async bytes => new TextDecoder().decode(await stepBytes(new Blob([bytes])));
  assert.equal(await read(new TextEncoder().encode(step)), step);
  assert.equal(await read(gzipSync(step)), step);
  await assert.rejects(read(new Uint8Array([0x1f, 0x8b, 0, 1, 2])));
});

test('a pasted pull request or commit address is read, and nothing else', () => {
  assert.deepEqual(githubLink('https://github.com/pzfreo/step-diff-demo/pull/2'), { kind: 'pull', repo: 'pzfreo/step-diff-demo', number: 2 });
  assert.deepEqual(githubLink(' github.com/o/r.x/pull/12/files?diff=split '), { kind: 'pull', repo: 'o/r.x', number: 12 });
  assert.deepEqual(githubLink(`https://github.com/o/r/commit/${sha.toUpperCase()}`), { kind: 'commit', repo: 'o/r', sha });
  for (const bad of ['https://gitlab.com/o/r/pull/2', 'https://github.com/o/r/issues/2', 'https://github.com/o/r/pull/x',
    'https://github.com/o/r/commit/zz', 'https://evil.example/github.com/o/r/pull/2', 'github.com/o/pull/2']) {
    assert.equal(githubLink(bad), null, bad);
  }
});

test('a pull request\'s or commit\'s changed STEP files, each as a diff link', async () => {
  const head = 'b'.repeat(40), parent = 'c'.repeat(40);
  const asked = [];
  const files = [
    { filename: 'parts/a.step', status: 'modified' },
    { filename: 'parts/new.step', status: 'renamed', previous_filename: 'parts/old.stp' },
    { filename: 'parts/added.step', status: 'added' },
    { filename: 'parts/gone.step', status: 'removed' },
    { filename: 'README.md', status: 'modified' },
  ];
  globalThis.fetch = async url => {
    asked.push(String(url));
    if (url.endsWith('/pulls/2')) return Response.json({ base: { sha }, head: { sha: head } });
    if (url.includes('/pulls/2/files')) return Response.json(files);
    if (url.endsWith(`/commits/${head}`)) return Response.json({ sha: head, parents: [{ sha: parent }], files });
    return new Response('', { status: 404 });
  };
  const pull = await changedSteps({ kind: 'pull', repo: 'o/r', number: 2 });
  assert.deepEqual(pull.files.map(f => [f.path, f.was]), [['parts/a.step', 'parts/a.step'], ['parts/new.step', 'parts/old.stp']]);
  assert.deepEqual(diffQuery(pull.files[1].query), { repo: 'o/r', base: sha, head, path: 'parts/new.step', was: 'parts/old.stp', part: '' });
  assert.ok(asked.every(u => u.startsWith('https://api.github.com/repos/o/r/')), 'only GitHub\'s API, for that repository');
  const commit = await changedSteps({ kind: 'commit', repo: 'o/r', sha: head });
  assert.equal(diffQuery(commit.files[0].query).base, parent, 'a commit against its parent');
  await assert.rejects(changedSteps({ kind: 'pull', repo: 'o/r', number: 9 }), /private/);
  globalThis.fetch = async () => new Response('', { status: 403 });
  await assert.rejects(changedSteps({ kind: 'pull', repo: 'o/r', number: 2 }), /limiting/);
});

test('another page hands over two files: Blobs, ArrayBuffers or typed arrays, named', async () => {
  const files = handedFiles({ type: 'draftwright:diff',
    before: { name: '/home/me/parts/a.step.gz', data: new Uint8Array([1, 2]).buffer },
    after: { name: 'C:\\parts\\b.step', data: new Blob(['ISO']) } });
  assert.equal(files.before.name, 'a.step.gz');
  assert.equal(files.after.name, 'b.step');
  assert.deepEqual([...new Uint8Array(await files.before.data.arrayBuffer())], [1, 2]);
  assert.equal(await files.after.data.text(), 'ISO');
  assert.equal(handedFiles({ type: 'draftwright:diff', before: { name: 'a', data: new Uint16Array([7]) }, after: { name: 'b', data: new Blob([]) } }).before.data.size, 2);
  for (const bad of [
    null, 'draftwright:diff', { type: 'other', before: { name: 'a', data: new Blob([]) }, after: { name: 'b', data: new Blob([]) } },
    { type: 'draftwright:diff', before: { name: 'a', data: new Blob([]) } },
    { type: 'draftwright:diff', before: { name: '', data: new Blob([]) }, after: { name: 'b', data: new Blob([]) } },
    { type: 'draftwright:diff', before: { name: 'a', data: 'ISO-10303-21;' }, after: { name: 'b', data: new Blob([]) } },
  ]) assert.equal(handedFiles(bad), null);
});

test('the PMI scenes asked for are those the diff compared, and each item takes its change', async () => {
  const { changeOf, drawsItem, movedPmi, sceneAnswers } = await import('../src/model/diff-pmi.ts');
  const given = { 'part.material': 'Steel' };
  const carried = { 'part.material': 'Steel', 'datum.A': [3] };
  // Answers given and no Specify PMI in the new file: both planned, the new with what carried.
  assert.deepEqual(sceneAnswers({ answers_from: 'given', new_stored: false, items: [] }, given, carried), { old: given, new: carried });
  // Both files written by Specify: each as its file has it.
  assert.deepEqual(sceneAnswers({ answers_from: 'stored', new_stored: true, items: [] }, {}, carried), { old: null, new: null });
  // Nothing to plan with: the files' own.
  assert.deepEqual(sceneAnswers({ answers_from: 'none', new_stored: false, items: [] }, {}, {}), { old: null, new: null });
  const items = [
    { class: 'removed', id: 'position:holes:15', kind: 'geometric_tolerance', number: 12, on: 'x', old: '⌖ ⌀0.2 A', status: 'answered' },
    { class: 'changed', id: 'position:holes:14', kind: 'geometric_tolerance', number: 13, on: 'x', old: 'a', new: 'b', status: 'answered' },
    { class: 'renumbered', id: 'position:holes:14', kind: 'geometric_tolerance', number: 13, on: 'x', status: 'answered' },
    { class: 'renumbered', id: 'size:holes:14', kind: 'dimension', number: 3, on: 'x', status: 'answered' },
    { class: 'added', id: 'perpendicularity:datum B', kind: 'geometric_tolerance', number: 18, on: 'x', new: '⟂ 0.05 A', status: 'answered' },
  ];
  const { now, removed } = changeOf(items);
  assert.deepEqual([...now], [['position:holes:14', 'changed'], ['size:holes:14', 'changed'], ['perpendicularity:datum B', 'added']]);
  assert.deepEqual([...removed], ['position:holes:15']);
  // The old part's callouts go where its faces go.
  const graphic = { source: 'file', name: 'A', polylines: [[[1, 0, 0], [0, 1, 0]]], triangles: [], text: [], textPlacements: [{ origin: [1, 0, 0], xAxis: [1, 0, 0], normal: [0, 0, 1] }] };
  const pmi = { links: {}, presentation: { items: { a: [graphic] }, unlinked: [], semanticIds: ['a'], planes: [], views: [] } };
  assert.equal(movedPmi(pmi, null), pmi);
  const turned = movedPmi(pmi, { rotation: [[0, -1, 0], [1, 0, 0], [0, 0, 1]], translation: [10, 0, 0] });
  assert.deepEqual(turned.presentation.items.a[0].polylines[0], [[10, 1, 0], [9, 0, 0]]);
  assert.deepEqual(turned.presentation.items.a[0].textPlacements[0], { origin: [10, 1, 0], xAxis: [0, 1, 0], normal: [0, 0, 1] });
  assert.ok(drawsItem(pmi, 'a'));
  assert.ok(!drawsItem(pmi, 'b'));
  assert.ok(!drawsItem(null, 'a'));
});

test('a section the page lists itself (its PMI changes) is left out of the report', () => {
  const md = ['# x.step', '', 'Summary', '', '**What changed**', '', '- a face moved', '', '**PMI**', '', '- #13 changed', '', '**Answers not carried**', '', '- none'].join('\n');
  const kept = reportHtml(md);
  assert.match(kept, /#13 changed/);
  const html = reportHtml(md, { omit: ['PMI'] });
  assert.doesNotMatch(html, /#13 changed|>PMI</);
  assert.match(html, /a face moved/);
  assert.match(html, /Answers not carried/);
});
