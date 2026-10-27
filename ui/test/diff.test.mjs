import { test } from 'node:test';
import assert from 'node:assert/strict';
import { diffQuery, fileAt, reportHtml } from '../src/model/diff.ts';

const sha = 'a8622c3f0e1d2c3b4a5968778695a4b3c2d1e0f9';

test('a diff link names a repository, two commits and relative paths, and nothing else', () => {
  const q = diffQuery(`?repo=pzfreo/step-diff-demo&base=${sha}&head=0153025&path=parts/a%20%23b.step`);
  assert.deepEqual(q, { repo: 'pzfreo/step-diff-demo', base: sha, head: '0153025', path: 'parts/a #b.step', was: 'parts/a #b.step' });
  assert.equal(diffQuery(`?repo=o/r&base=${sha}&head=${sha}&path=new.step&was=old.step`).was, 'old.step');
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
  assert.match(html, /<li>Ø5 hole &#60;b&#62;: moved <code>X \+2<\/code><\/li>/);
});
