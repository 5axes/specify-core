import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
import { mcp } from './mcp-client.mjs';
// The local app as an agent uses it (specify-core docs/session.md): `specify-core mcp` over stdio,
// started as an AI client starts it. The agent opens a part, answers, calls `show {browser: false}`;
// the page loads with the agent's answer, and an answer the agent gives while it is open lands with
// "Updated by your agent". The person's edit in the page is what `changes` returns. The agent's
// `save` and the page's Save write the same naming, and an incomplete save says what is `missing`.
// Then an assembly (every part's stored answers resume when one part is opened) and a part opened
// through a symbolic link (the link's path is kept, Save writes beside the link).
// `npm run test:app` runs it after app-browser.mjs.
//
//   SPECIFY_CORE      the specify-core binary with `mcp`'s `show` (specify-core open-session).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent).
//   SHOTS             a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_CORE ?? 'specify-core';
const app = resolve('dist-app');
const wasm = resolve(process.env.SPECIFY_WASM_DIR ?? 'public/wasm/intent');
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
assert.ok(existsSync(join(app, 'index.html')), 'dist-app is built (npm run build:app)');

const dir = mkdtempSync(join(tmpdir(), 'specify-agent-'));
const env = { SPECIFY_APP_DIR: app, SPECIFY_WASM_DIR: wasm };
const analyse = file => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));
const stored = file => analyse(file).resume?.answers ?? {};
/** The answers a person or agent gave: Specify's own `_` keys left out. */
const given = answers => Object.fromEntries(Object.entries(answers).filter(([id]) => !id.startsWith('_')));

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
async function load(url) {
  const context = await browser.newContext({ viewport: { width: 1400, height: 950 } });
  const requests = [];
  context.on('request', request => requests.push(request.url()));
  const page = await context.newPage();
  page.on('pageerror', error => problems.push(String(error)));
  page.on('console', message => { if (message.type() === 'error') problems.push(`console: ${message.text()}`); });
  await page.goto(url);
  await check(page.locator('.pmi-lab-panel')).toBeVisible();
  return { context, page, requests };
}
const onlyLocal = (requests, url) => {
  const origin = new URL(url).origin;
  assert.deepEqual(requests.filter(u => !u.startsWith(`${origin}/`) && !/^(blob|data):/.test(u)), [], 'no request leaves 127.0.0.1');
};
const question = (page, id) => page.locator(`[data-question="${id}"]`);
const agents = [];

try {
  // One part: the agent and the person on one session.
  {
    const part = join(dir, 'part.step');
    writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const opened = await agent.call('open', { path: part });
    const doc = opened.doc;
    await agent.call('answer', { doc, set: { 'part.material': 'Aluminium 6082-T6' } });
    const shown = await agent.call('show', { doc, browser: false });
    assert.equal(shown.mode, 'specify');
    assert.match(shown.url, /^http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\/$/);
    const { context, page, requests } = await load(shown.url);
    await check(page).toHaveTitle('part.step — Specify');
    await check(page.locator('.app-path')).toHaveText(part);
    // The agent's answer before the page opened is there, as the session's.
    await check(page.locator('.pmi-lab-panel')).toContainText('6082');
    await check(page.locator('.app-dirty')).toBeVisible();
    await check(page.locator('.pmi-lab-resumed')).toHaveCount(0);
    // An answer the agent gives now lands in the open page: said, its question opened and lit.
    await agent.call('answer', { doc, set: { 'part.general_tolerance': 'ISO 2768-f' } });
    await check(page.locator('.app-agent')).toContainText('Updated by your agent');
    await check(page.locator('.app-agent')).toContainText('part.general_tolerance');
    await check(question(page, 'part.general_tolerance')).toBeVisible();
    await check(question(page, 'part.general_tolerance')).toHaveClass(/app-agent-lit/);
    await check(question(page, 'part.general_tolerance')).toContainText('2768-f');
    if (shots) await page.screenshot({ path: join(shots, 'agent-update.png') });
    // The person changes the material in the page: the agent's `changes` returns it, as the page's.
    const { rev } = await agent.call('changes', { doc, since: 0 });
    // An answered part setting is a one-line row: open it to change it.
    await page.locator('.pmi-lab-panel').getByRole('button', { name: /^Material/ }).click();
    const box = question(page, 'part.material').locator('input');
    await box.fill('Aluminium 6061-T6');
    await box.press('Enter');
    await check.poll(async () => (await agent.call('changes', { doc, since: rev })).changes
      .some(c => c.source === 'page' && c.set?.['part.material'] === 'Aluminium 6061-T6'), { timeout: 30000 }).toBe(true);
    const asked = await agent.call('questions', { doc, state: 'answered' });
    assert.ok(JSON.stringify(asked).includes('Aluminium 6061-T6'), 'the agent reads the page\'s answer');
    // The agent saves (no defaults): part.specified.step beside the part, incomplete, saying what is missing.
    const byAgent = await agent.call('save', { doc, accept_defaults: false });
    assert.equal(byAgent.path, join(dir, 'part.specified.step'));
    assert.equal(byAgent.complete, false);
    assert.ok(byAgent.missing.includes('part.surface_finish'), `missing: ${byAgent.missing}`);
    assert.ok(!byAgent.missing.includes('part.material'));
    // The page hears of it, whoever saved.
    await check(page.locator('.app-saved')).toContainText('Your agent saved');
    await check(page.locator('.app-saved')).toContainText(byAgent.path);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    await check(page.locator('.app-target')).toHaveText(join(dir, 'part.specified-2.step'));
    // The page's Save writes the next name, from the same answers, and says what is still open.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(join(dir, 'part.specified-2.step'));
    await check(page.locator('.app-saved')).toContainText('still open');
    await check(page.locator('.app-saved')).toContainText('part.surface_finish');
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    const want = { 'part.material': 'Aluminium 6061-T6', 'part.general_tolerance': 'ISO 2768-f' };
    assert.deepEqual(given(stored(byAgent.path)), want, 'the agent\'s save stores the session\'s answers');
    assert.deepEqual(given(stored(join(dir, 'part.specified-2.step'))), want, 'the page\'s save stores the same');
    // A save of a revision the page has not seen is refused, with the current one.
    const origin = new URL(shown.url).origin;
    const stale = await fetch(`${shown.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: origin }, body: JSON.stringify({ rev: 0, acceptDefaults: false }) });
    assert.equal(stale.status, 409);
    assert.equal(typeof (await stale.json()).rev, 'number');
    assert.ok(!existsSync(join(dir, 'part.specified-3.step')), 'nothing written for a stale save');
    onlyLocal(requests, shown.url);
    await context.close();
    agent.stop();
  }

  // An assembly: answers given to two parts and saved; a new agent opening one part resumes both.
  {
    const asm = join(dir, 'asm.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const first = await mcp(cli, { cwd: dir, env }); agents.push(first);
    const listed = await first.call('open', { path: asm });
    assert.equal(listed.needs_part, true);
    const [a, b] = listed.parts.map(p => p.part);
    const docA = (await first.call('open', { path: asm, part: a })).doc;
    const docB = (await first.call('open', { path: asm, part: b })).doc;
    await first.call('answer', { doc: docA, set: { 'part.material': 'Steel 1.4301' } });
    await first.call('answer', { doc: docB, set: { 'part.material': 'Brass CW614N' } });
    const written = await first.call('save', { doc: docA, accept_defaults: false });
    assert.equal(written.path, join(dir, 'asm.specified.step'));
    first.stop();
    const second = await mcp(cli, { cwd: dir, env }); agents.push(second);
    const docB2 = (await second.call('open', { path: written.path, part: b })).doc;
    const shown = await second.call('show', { doc: docB2, browser: false });
    const session = await (await fetch(`${shown.url}answers`)).json();
    assert.equal(session.parts[String(a)]?.['part.material'], 'Steel 1.4301', `the other part's stored answers resume: ${JSON.stringify(session.parts).slice(0, 300)}`);
    assert.equal(session.parts[String(b)]?.['part.material'], 'Brass CW614N');
    // The page shows one part at a time and offers the others.
    const { context, page, requests } = await load(shown.url);
    await check(page.locator('.app-part select')).toBeVisible();
    await check(page.locator('.pmi-lab-panel')).toContainText(/1\.4301|CW614N/);
    const showing = Number(await page.locator('.app-part select').inputValue());
    const other = showing === a ? b : a;
    await page.locator('.app-part select').selectOption(String(other));
    await check(page.locator('.pmi-lab-panel')).toContainText(other === a ? '1.4301' : 'CW614N');
    // The agent answers the part not shown now: the page says so, by name, and offers to show it.
    const hidden = await second.call('open', { path: written.path, part: showing });
    await second.call('answer', { doc: hidden.doc, set: { 'part.general_tolerance': 'ISO 2768-f' } });
    const name = listed.parts.find(p => p.part === showing).name;
    await check(page.locator('.app-agent')).toContainText(`Updated by your agent: ${name}`);
    await page.getByRole('button', { name: 'Show it' }).click();
    await check(page.locator('.app-part select')).toHaveValue(String(showing));
    await check(page.locator('.pmi-lab-panel')).toContainText('2768-f');
    onlyLocal(requests, shown.url);
    await context.close();
    second.stop();
  }

  // A part opened through a symbolic link: the link's path is shown and Save writes beside it.
  {
    const real = join(dir, 'real');
    mkdirSync(real);
    writeFileSync(join(real, 'shaft.step'), gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const link = join(dir, 'linked.step');
    symlinkSync(join(real, 'shaft.step'), link);
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const { doc } = await agent.call('open', { path: link });
    const shown = await agent.call('show', { doc, browser: false });
    const config = await (await fetch(`${shown.url}config`)).json();
    assert.equal(config.path, link, 'config.path is the link as given');
    assert.equal(config.saveTarget, join(dir, 'linked.specified.step'));
    const { context, page } = await load(shown.url);
    await check(page.locator('.app-path')).toHaveText(link);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(join(dir, 'linked.specified.step'));
    assert.ok(existsSync(join(dir, 'linked.specified.step')) && !existsSync(join(real, 'shaft.specified.step')), 'saved beside the link');
    await context.close();
    agent.stop();
  }
  assert.deepEqual(problems, [], 'no page errors');
  console.log('app-agent passed');
} finally {
  for (const agent of agents) agent.stop();
  await browser.close();
}
