import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
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
// Then an assembly (every part's stored answers resume when one part is opened; an agent's answer
// to a part not shown is unsaved; Save with defaults lists every written part's defaults), answers
// the page cannot show (said, retried, Save explains), and a part opened
// through a symbolic link (the link's path is kept, Save writes beside the link), and a diff the agent
// opened, which `show` serves in diff mode.
// `npm run test:app` runs it after app-browser.mjs.
//
//   SPECIFY_CORE      the specify-core binary with `mcp`'s `show` (specify-core open-session).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent).
//   SPECIFY_APP_BUILT_IN=1  serve the app and engine built into SPECIFY_CORE (ADR P12) instead
//                     of setting SPECIFY_APP_DIR=dist-app and SPECIFY_WASM_DIR.
//   SHOTS             a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_CORE ?? 'specify-core';
const app = resolve('dist-app');
const wasm = resolve(process.env.SPECIFY_WASM_DIR ?? 'public/wasm/intent');
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const builtIn = process.env.SPECIFY_APP_BUILT_IN === '1';
if (!builtIn) assert.ok(existsSync(join(app, 'index.html')), 'dist-app is built (npm run build:app)');

const dir = mkdtempSync(join(tmpdir(), 'specify-agent-'));
// Empty overrides are unset: the command's own app and engine.
const env = builtIn ? { SPECIFY_APP_DIR: '', SPECIFY_WASM_DIR: '' } : { SPECIFY_APP_DIR: app, SPECIFY_WASM_DIR: wasm };
const analyse = (file, part) => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', ...(part === undefined ? [] : ['--part', String(part)]), file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));
const stored = file => analyse(file).resume?.answers ?? {};
const stored2 = (file, part) => analyse(file, part).resume?.answers ?? {};
/** The answers a person or agent gave: Specify's own `_` keys left out. */
const given = answers => Object.fromEntries(Object.entries(answers).filter(([id]) => !id.startsWith('_')));

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
async function load(url, ready = '.pmi-lab-panel') {
  const context = await browser.newContext({ viewport: { width: 1400, height: 950 } });
  const requests = [];
  context.on('request', request => requests.push(request.url()));
  // Every message the page posts to a worker (the engine), by operation.
  await context.addInitScript(() => {
    window.__engineOps = [];
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (message, ...rest) {
      if (message && typeof message.operation === 'string') window.__engineOps.push(message.operation);
      return post.call(this, message, ...rest);
    };
  });
  const page = await context.newPage();
  page.on('pageerror', error => problems.push(String(error)));
  page.on('console', message => { if (message.type() === 'error') problems.push(`console: ${message.text()}`); });
  await page.goto(url);
  await check(page.locator(ready).first()).toBeVisible();
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
    // An answer the agent gives now lands in the open page: said (by the question's short label,
    // #217), its question opened and lit. The note and the light stay until dismissed or the next change.
    await agent.call('answer', { doc, set: { 'part.general_tolerance': 'ISO 2768-f' } });
    await check(page.locator('.app-agent')).toContainText('Updated by your agent');
    await check(page.locator('.app-agent')).toContainText('General tolerance');
    await check(page.locator('.app-agent')).not.toContainText('Which general tolerance applies');
    await check(question(page, 'part.general_tolerance')).toBeVisible();
    await check(question(page, 'part.general_tolerance')).toHaveClass(/app-agent-lit/);
    await check(question(page, 'part.general_tolerance')).toContainText('2768-f');
    if (shots) await page.screenshot({ path: join(shots, 'agent-update.png') });
    await page.locator('.app-agent').getByRole('button', { name: 'Dismiss' }).click();
    await check(page.locator('.app-agent')).toHaveCount(0);
    await check(page.locator('.app-agent-lit')).toHaveCount(0);
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
    const byAgent = await agent.call('save', { doc, accept_defaults: {} });
    assert.equal(byAgent.path, join(dir, 'part.specified.step'));
    assert.equal(byAgent.complete, false);
    assert.ok(byAgent.missing.includes('part.surface_finish'), `missing: ${byAgent.missing}`);
    assert.ok(!byAgent.missing.includes('part.material'));
    const agentStored = given(stored(byAgent.path));
    // The page hears of it, whoever saved, and says what is still unanswered.
    await check(page.locator('.app-saved')).toContainText('Your agent saved');
    await check(page.locator('.app-saved')).toContainText(byAgent.path);
    await check(page.locator('.app-saved')).toContainText('still unanswered');
    // By the question's short label, not its prompt (#217).
    await check(page.locator('.app-saved')).toContainText('Surface finish');
    await check(page.locator('.app-saved')).not.toContainText('What surface finish applies');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The session's copy: later saves update it (#217).
    await check(page.locator('.app-target')).toHaveText(byAgent.path);
    // The page's Save updates it, from the same answers, and says what is still open.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).not.toContainText('Your agent saved');
    await check(page.locator('.app-saved')).toContainText(byAgent.path);
    await check(page.locator('.app-saved')).toContainText('still open');
    await check(page.locator('.app-saved')).toContainText('Surface finish');
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    const want = { 'part.material': 'Aluminium 6061-T6', 'part.general_tolerance': 'ISO 2768-f' };
    assert.deepEqual(agentStored, want, 'the agent\'s save stores the session\'s answers');
    assert.deepEqual(given(stored(byAgent.path)), want, 'the page\'s save stores the same');
    assert.ok(!existsSync(join(dir, 'part.specified-2.step')), 'no numbered copy');
    // A save of a revision the page has not seen is refused, with the current one.
    const origin = new URL(shown.url).origin;
    const stale = await fetch(`${shown.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: origin }, body: JSON.stringify({ rev: 0, acceptDefaults: {} }) });
    assert.equal(stale.status, 409);
    assert.equal(typeof (await stale.json()).rev, 'number');
    assert.ok(!existsSync(join(dir, 'part.specified-2.step')), 'nothing written for a stale save');
    // Killing the MCP command drops SSE. The page briefly retries, then keeps the review and
    // identifies the last file actually saved rather than the next target.
    const beforeStop = problems.length;
    agent.stop();
    await check(page.locator('.app-offline')).toContainText('reconnecting', { timeout: 10_000 });
    const stopped = page.locator('.app-stopped');
    await check(stopped).toHaveText(`Specify has stopped. Run specify-core open … again (or ask your agent to show it). Your saved file is at ${byAgent.path}.`, { timeout: 30_000 });
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    await check(page.getByRole('button', { name: 'Save with defaults', exact: true })).toBeDisabled();
    await check(page.getByRole('button', { name: 'Answers (JSON)', exact: true })).toBeEnabled();
    await check(page.locator('.pmi-lab-panel')).toContainText('6061-T6');
    problems.splice(beforeStop, problems.length - beforeStop,
      ...problems.slice(beforeStop).filter(p => !/^console: .*net::ERR_CONNECTION_REFUSED$/.test(p)));
    onlyLocal(requests, shown.url);
    await context.close();
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
    const written = await first.call('save', { doc: docA, accept_defaults: {} });
    assert.equal(written.path, join(dir, 'asm.specified.step'));
    first.stop();
    const second = await mcp(cli, { cwd: dir, env }); agents.push(second);
    const docB2 = (await second.call('open', { path: written.path, part: b })).doc;
    const shown = await second.call('show', { doc: docB2, browser: false });
    // `show` names the part: the page starts on it (not on the first part with answers).
    assert.equal((await (await fetch(`${shown.url}config`)).json()).part, b);
    const session = await (await fetch(`${shown.url}answers`)).json();
    assert.equal(session.parts[String(a)]?.['part.material'], 'Steel 1.4301', `the other part's stored answers resume: ${JSON.stringify(session.parts).slice(0, 300)}`);
    assert.equal(session.parts[String(b)]?.['part.material'], 'Brass CW614N');
    // The page shows one part at a time and offers the others.
    const { context, page, requests } = await load(shown.url);
    await check(page.locator('.app-part select')).toBeVisible();
    await check(page.locator('.pmi-lab-panel')).toContainText(/1\.4301|CW614N/);
    await check(page.locator('.app-part select')).toHaveValue(String(b));
    const showing = Number(await page.locator('.app-part select').inputValue());
    const other = showing === a ? b : a;
    await page.locator('.app-part select').selectOption(String(other));
    await check(page.locator('.pmi-lab-panel')).toContainText(other === a ? '1.4301' : 'CW614N');
    // The agent answers the part not shown now: the page says so, by name, and offers to show it.
    const hidden = await second.call('open', { path: written.path, part: showing });
    await second.call('answer', { doc: hidden.doc, set: { 'part.general_tolerance': 'ISO 2768-f' } });
    const name = listed.parts.find(p => p.part === showing).name;
    await check(page.locator('.app-agent')).toContainText(`Updated by your agent: ${name}`);
    // Unsaved is the file's, not the part shown's: the agent's answer to the other part counts.
    await check(page.locator('.app-dirty')).toBeVisible();
    await page.getByRole('button', { name: 'Show it' }).click();
    await check(page.locator('.app-part select')).toHaveValue(String(showing));
    await check(page.locator('.pmi-lab-panel')).toContainText('2768-f');
    // Saved: and still saved, back and forth between the parts (not judged against the file as opened).
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText('asm.specified');
    const stillOpen = page.locator('.app-saved details');
    await stillOpen.locator('summary').click();
    const expected = [];
    for (const { part, name } of listed.parts.filter(({ part }) => part === a || part === b)) {
      const { doc } = await second.call('open', { path: written.path, part });
      const { questions } = await second.call('questions', { doc, state: 'open' });
      assert.ok(questions.length, `${name} has open questions`);
      // By the short label the command gives each question (#217), as MCP returns it.
      expected.push(...questions.map(q => `${name} — ${q.label}`));
    }
    await check(stillOpen.locator('li')).toHaveText(expected);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    await page.locator('.app-part select').selectOption(String(other));
    await check(page.locator('.pmi-lab-panel')).toContainText(other === a ? '1.4301' : 'CW614N');
    await page.locator('.app-part select').selectOption(String(showing));
    await check(page.locator('.pmi-lab-panel')).toContainText('2768-f');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    onlyLocal(requests, shown.url);
    await context.close();
    second.stop();
  }

  // The assembly Specify wrote above (parts a and b answered), with part b changed by an agent before
  // the page loaded. The page shows a: b's answers are unsaved though b is never shown, and Save with
  // defaults lists (and so writes) the defaults of every part the save writes, by part (ADR P9).
  {
    const asm = join(dir, 'asm.specified.step');
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    const [a, b] = listed.parts.map(p => p.part);
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    const docB = (await agent.call('open', { path: asm, part: b })).doc;
    await agent.call('answer', { doc: docB, set: { 'part.material': 'Bronze CC491K' } });
    const shown = await agent.call('show', { doc: docA, browser: false });
    const { context, page } = await load(shown.url);
    await check(page.locator('.app-part select')).toHaveValue(String(a));
    await check(page.locator('.pmi-lab-panel')).toContainText('1.4301');
    await check(page.locator('.app-dirty')).toBeVisible();
    const session = await (await fetch(`${shown.url}answers`)).json();
    const written = Object.keys(session.parts).map(Number).sort((x, y) => x - y);
    assert.deepEqual(written, [a, b].sort((x, y) => x - y), `the save writes parts a and b: ${written}`);
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list).toBeVisible();
    // Changed expectation (Paul's decision, 2026-11-19): every part of the file with a default has its own
    // list, answered or not (it was only the parts with answers, `written.length`), the one not shown too.
    // Exactly the parts of the fixture that have an open question with a default, as the agent sees them.
    let partsWithDefaults = 0;
    for (const { part } of listed.parts) {
      const doc = (await agent.call('open', { path: asm, part })).doc;
      const open = (await agent.call('questions', { doc, state: 'open' })).questions;
      if (open.some(q => q.default !== null && q.default !== undefined)) partsWithDefaults++;
    }
    assert.ok(partsWithDefaults > written.length, 'the fixture has parts with no answers that have defaults');
    await check(list.locator('[data-part]')).toHaveCount(partsWithDefaults);
    const nameB = listed.parts.find(p => p.part === b).name;
    await check(list.locator(`[data-part="${b}"] h4`)).toHaveText(nameB);
    await check(list.locator(`[data-part="${a}"] h4`)).toContainText('(shown)');
    const ofB = await list.locator(`[data-part="${b}"] li`).count();
    assert.ok(ofB > 0, 'part b\'s defaults are listed');
    await check(list.locator(`[data-part="${b}"] [data-default="part.material"]`)).toHaveCount(0);
    const total = await list.locator('li').count();
    await check(list.getByRole('button', { name: `Save with these ${total} defaults` })).toBeVisible();
    if (shots) await page.screenshot({ path: join(shots, 'assembly-defaults.png') });
    await list.getByRole('button', { name: `Save with these ${total} defaults` }).click();
    await check(page.locator('.app-saved')).toContainText('asm.specified.specified');
    const saved = (await page.locator('.app-saved code').textContent()).trim();
    // Part b, never shown, got the defaults listed for it: its PMI has more than its answers alone give.
    const withDefaults = (analyse(saved, b).characteristics ?? []).length;
    const without = (analyse(asm, b).characteristics ?? []).length;
    assert.ok(withDefaults > without, `part b's defaults written: ${withDefaults} > ${without}`);
    assert.equal(given(analyse(saved, b).resume?.answers ?? {})['part.material'], 'Bronze CC491K');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    await context.close();
    agent.stop();
  }

  // A part the agent opens while the page is open joins the session (`joined`): the page takes it
  // in (said, listed in the picker), a save asked for at the rev before it is stale (409), and the
  // defaults a save lists include it (every part of the file is listed, answered or not).
  {
    const asm = join(dir, 'asm-join.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    const [a, b] = listed.parts.map(p => p.part);
    const nameB = listed.parts.find(p => p.part === b).name;
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    await agent.call('answer', { doc: docA, set: { 'part.material': 'Steel 1.4301' } });
    const shown = await agent.call('show', { doc: docA, browser: false });
    const origin = new URL(shown.url).origin;
    const { context, page } = await load(shown.url);
    await check(page.locator('.app-part select')).toHaveValue(String(a));
    await check(page.locator(`.app-part option[value="${b}"]`)).toHaveText(nameB);
    const before = (await (await fetch(`${shown.url}answers`)).json());
    assert.equal(String(b) in before.parts, false, 'the session holds part a only');
    // The agent opens part b: a join, with no answers.
    const docB = (await agent.call('open', { path: asm, part: b })).doc;
    await check(page.locator('.app-agent')).toContainText(`Your agent opened ${nameB}`);
    const after = (await (await fetch(`${shown.url}answers`)).json());
    assert.ok(after.rev > before.rev, 'the join is a new rev');
    assert.deepEqual(after.parts[String(b)], {}, 'part b is in the session with no answers');
    // A save at the rev before the join is refused (409), and writes nothing.
    const stale = await fetch(`${shown.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: origin }, body: JSON.stringify({ rev: before.rev, acceptDefaults: {} }) });
    assert.equal(stale.status, 409);
    assert.ok(!existsSync(join(dir, 'asm-join.specified.step')), 'nothing written for the stale save');
    // Changed expectation (Paul's decision, 2026-11-19): Save with defaults lists the defaults of every part
    // of the file, so b is listed though it has no answers, and approving them is what writes it. It was a
    // only ("a save writes only parts with answers"), which left an untouched part out of Save with defaults.
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list.locator(`[data-part="${a}"]`)).toBeVisible();
    await check(list.locator(`[data-part="${b}"] h4`)).toHaveText(nameB);
    await check(list.locator('[data-written]')).toContainText(nameB);
    // The old wording is gone, and the written note names part b (it has defaults listed).
    await check(list.locator('[data-written]')).toContainText('a part with no answers and no default listed is left as it is');
    await list.getByRole('button', { name: 'Cancel' }).click();
    // Once b has an answer it is still listed.
    await agent.call('answer', { doc: docB, set: { 'part.material': 'Brass CW614N' } });
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    await check(list.locator(`[data-part="${b}"] h4`)).toHaveText(nameB);
    await check(list.locator('[data-written]')).toContainText(nameB);
    await context.close();
    agent.stop();
  }

  // Approvals name their part (`acceptDefaults: {"<part>": {"<id>": value}}`): a default approved for part a
  // only is written to a and part b is untouched. A refused one (400 with its part and question)
  // writes nothing and the page says which.
  {
    const asm = join(dir, 'asm-approve.step');
    writeFileSync(asm, readFileSync(join(dir, 'asm.specified.step')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    const [a, b] = listed.parts.map(p => p.part);
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    await agent.call('open', { path: asm, part: b });
    const shown = await agent.call('show', { doc: docA, browser: false });
    const origin = new URL(shown.url).origin;
    const { context, page } = await load(shown.url);
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list.locator(`[data-part="${b}"] li`).first()).toBeVisible();
    const ofA = await list.locator(`[data-part="${a}"] [data-default]`).evaluateAll(items => items.map(item => item.getAttribute('data-default')));
    const ofB = await list.locator(`[data-part="${b}"] [data-default]`).evaluateAll(items => items.map(item => item.getAttribute('data-default')));
    assert.ok(ofA.length > 0 && ofB.length > 0);
    const id = ofA.find(x => ofB.includes(x)) ?? ofA[0];
    // The page shows the command's refusal of a default when it sends one it refuses (here, forced).
    await page.route('**/save', route => route.fulfill({ status: 400, contentType: 'application/json', body: JSON.stringify({ error: 'not an open question with a default', part: b, question: id }) }));
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText(`the default for ${id}`);
    await check(page.getByRole('alert').first()).toContainText(listed.parts.find(p => p.part === b).name);
    await check(page.getByRole('alert').first()).toContainText('Nothing was written');
    await page.unroute('**/save');
    // A value that no longer matches the default (400 with expected and current): said plainly, what
    // the person confirmed and what it is now, nothing written, and the defaults listed again to confirm.
    await check(list.getByRole('button', { name: /^Save with these \d+ defaults$/ })).toBeVisible();
    await page.route('**/save', route => route.fulfill({ status: 400, contentType: 'application/json', body: JSON.stringify({ error: 'default changed', part: a, question: id, expected: 'old value', current: 'new value' }) }));
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText('changed. You confirmed');
    await check(page.getByRole('alert').first()).toContainText('it is now new value. Nothing was written; check the new list and confirm again.');
    await check(page.getByRole('alert').first()).toContainText('Nothing was written');
    await check(list).toBeVisible();
    await page.unroute('**/save');
    problems.splice(0, problems.length, ...problems.filter(p => !/400/.test(p)));
    assert.ok(!existsSync(join(dir, 'asm-approve.specified.step')), 'nothing written for a refused default');
    // The command's own refusal, as the page would meet it: a default that is no question.
    const { rev } = await (await fetch(`${shown.url}answers`)).json();
    const post = (acceptDefaults) => fetch(`${shown.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: origin }, body: JSON.stringify({ rev, acceptDefaults }) });
    const bad = await post({ [a]: { 'nope.unknown': 1 } });
    assert.equal(bad.status, 400);
    const badBody = await bad.json();
    assert.equal(badBody.part, a); assert.equal(badBody.question, 'nope.unknown');
    assert.equal((await post(true)).status, 400, 'a boolean is no longer a body');
    assert.equal((await post({ [a]: [id] })).status, 400, 'a list of ids is no longer a body: each id comes with its value');
    // A value that is no longer the default (here: never was): 400 with what was sent and what it is, nothing written.
    const wrong = await post({ [a]: { [id]: 'not-the-default' } });
    assert.equal(wrong.status, 400);
    const wrongBody = await wrong.json();
    assert.equal(wrongBody.part, a); assert.equal(wrongBody.question, id);
    assert.equal(wrongBody.expected, 'not-the-default');
    assert.ok('current' in wrongBody && wrongBody.current !== 'not-the-default', 'the body says what the default is now');
    assert.ok(!existsSync(join(dir, 'asm-approve.specified.step')), 'nothing written for a value that no longer matches');
    // Approved for part a only, with the value seen.
    const good = await post({ [a]: { [id]: wrongBody.current } });
    assert.equal(good.status, 200, await good.clone().text());
    const { path } = await good.json();
    const was = id => analyse(asm, id).characteristics?.length ?? 0;
    assert.ok((analyse(path, a).characteristics ?? []).length > was(a), 'part a has the default\'s PMI');
    assert.equal((analyse(path, b).characteristics ?? []).length, was(b), 'part b is untouched');
    assert.ok(id in given(stored2(path, a)), 'the approved default is an answer stored on part a');
    assert.ok(!(id in given(stored2(path, b))) || ofB.length === 0, 'part b has no default answer');
    const session = await (await fetch(`${shown.url}answers`)).json();
    assert.ok(id in session.parts[String(a)], 'the default is an answer on part a in the session');
    assert.ok(!(id in session.parts[String(b)]), 'and not on part b');
    await context.close();
    agent.stop();
  }

  // The set of parts changing during a save (409 stale): the page takes the session's answers and
  // parts, lists the defaults again (now with the part that joined) and asks the person to confirm.
  {
    const asm = join(dir, 'asm-stale.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    const [a, b] = listed.parts.map(p => p.part);
    const nameB = listed.parts.find(p => p.part === b).name;
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    await agent.call('answer', { doc: docA, set: { 'part.material': 'Steel 1.4301' } });
    const shown = await agent.call('show', { doc: docA, browser: false });
    const { context, page } = await load(shown.url);
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    // Changed expectation (Paul's decision, 2026-11-19): every part of the file is listed from the start,
    // b too (it was a alone, as only a had an answer). The agent's join below is still a stale save.
    await check(list.locator(`[data-part="${a}"]`)).toBeVisible();
    await check(list.locator(`[data-part="${b}"]`)).toBeVisible();
    // b has open questions with defaults. The agent answers one before the person confirms: what
    // would be written is no longer the list confirmed, so nothing is sent and the list is made again.
    const docB = (await agent.call('open', { path: asm, part: b })).doc;
    const [dq] = (await agent.call('questions', { doc: docB, state: 'open' })).questions.filter(q => q.default !== null && q.default !== undefined);
    await check(list.locator(`[data-part="${b}"] [data-default="${dq.id}"]`)).toHaveCount(1);
    await agent.call('answer', { doc: docB, set: { [dq.id]: dq.default } });
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText('not the ones you confirmed');
    await check(list.locator(`[data-part="${b}"] h4`)).toHaveText(nameB);
    await check(list.locator(`[data-part="${b}"] [data-default="${dq.id}"]`)).toHaveCount(0);
    assert.ok(!existsSync(join(dir, 'asm-stale.specified.step')), 'nothing written when the defaults changed');
    // While the save is on its way the agent changes b again: the command meets a save at an older
    // rev (409, with the session's rev), and the page takes the session's answers and lists again.
    await page.route('**/save', async route => {
      await agent.call('answer', { doc: docB, set: { 'part.material': 'Bronze CC491K' } });
      const { rev } = await (await fetch(`${shown.url}answers`)).json();
      await route.fulfill({ status: 409, contentType: 'application/json', body: JSON.stringify({ error: 'the answers have changed since that rev: show them and save again', rev }) });
    });
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText('Not saved');
    await page.unroute('**/save');
    await check(page.getByRole('alert').first()).toContainText('listed again');
    await check(list.locator(`[data-part="${b}"] h4`)).toHaveText(nameB);
    assert.ok(!existsSync(join(dir, 'asm-stale.specified.step')), 'nothing written for a stale save');
    problems.splice(0, problems.length, ...problems.filter(p => !/409/.test(p)));
    // Confirmed again, it saves, both parts.
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.locator('.app-saved')).toContainText('asm-stale.specified');
    assert.equal(given(stored2(join(dir, 'asm-stale.specified.step'), b))['part.material'], 'Bronze CC491K');
    await context.close();
    agent.stop();
  }

  // One rev carries an `answers` event for each part an approval touches: an agent's save approving
  // defaults on two parts reaches the page as one event per part at one rev, and the page shows both.
  {
    const asm = join(dir, 'asm-events.step');
    writeFileSync(asm, readFileSync(join(dir, 'asm.specified.step')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    const [a, b] = listed.parts.map(p => p.part);
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    const docB = (await agent.call('open', { path: asm, part: b })).doc;
    const defaultsOf = async doc => Object.fromEntries((await agent.call('questions', { doc, state: 'open' })).questions.filter(q => q.default !== null && q.default !== undefined).map(q => [q.id, q.default]));
    const accept = { [a]: await defaultsOf(docA), [b]: await defaultsOf(docB) };
    assert.ok(Object.keys(accept[a]).length > 0 && Object.keys(accept[b]).length > 0);
    const shown = await agent.call('show', { doc: docA, browser: false });
    const { context, page } = await load(shown.url);
    const before = await (await fetch(`${shown.url}answers`)).json();
    const saved = await agent.call('save', { doc: docA, accept_defaults: accept });
    const session = await (await fetch(`${shown.url}answers`)).json();
    assert.ok(session.rev > before.rev);
    for (const part of [a, b]) for (const id of Object.keys(accept[part])) assert.ok(id in session.parts[String(part)], `${id} is an answer on part ${part}`);
    await check(page.locator('.app-saved')).toContainText('Your agent saved');
    await check(page.locator('.app-saved')).toContainText(saved.path);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // Both parts show their defaults as answers, whichever is shown.
    const answered = async () => (await page.locator('.pmi-lab-panel').textContent()) ?? '';
    const onA = await answered();
    await page.locator('.app-part select').selectOption(String(b));
    await check(page.locator('.app-part select')).toHaveValue(String(b));
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.notEqual(await answered(), onA, 'part b is shown');
    await check(page.getByRole('alert')).toHaveCount(0);
    await context.close();
    agent.stop();
  }

  // A failed write (the folder is read-only) rolls its approvals back in the session: the page does
  // not show the approved defaults as applied, says Not saved, and nothing is written.
  if (process.getuid?.() !== 0) {
    const locked = join(dir, 'locked');
    mkdirSync(locked);
    const file = join(locked, 'shaft.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const agent = await mcp(cli, { cwd: locked, env }); agents.push(agent);
    const { doc } = await agent.call('open', { path: file });
    const shown = await agent.call('show', { doc, browser: false });
    const { context, page } = await load(shown.url);
    const answersBefore = (await (await fetch(`${shown.url}answers`)).json()).parts['0'] ?? {};
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list.locator('[data-default]').first()).toBeVisible();
    chmodSync(locked, 0o555);
    try {
      await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
      await check(page.getByRole('alert').first()).toContainText('Not saved');
    } finally { chmodSync(locked, 0o755); }
    assert.ok(!existsSync(join(locked, 'shaft.specified.step')), 'nothing written');
    await check(page.locator('.app-saved')).toHaveCount(0);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The session holds no approvals (rolled back), and the page shows what the session holds.
    const session = await (await fetch(`${shown.url}answers`)).json();
    assert.deepEqual(given(session.parts['0'] ?? {}), given(answersBefore), 'the approvals were rolled back');
    const open = await agent.call('questions', { doc, state: 'open' });
    assert.ok(open.questions.some(q => q.id === 'part.general_tolerance'), 'the approved default is still an open question');
    await check(page.locator('.pmi-lab-panel')).toContainText('Suggested: ISO 2768-m');
    problems.splice(0, problems.length, ...problems.filter(p => !/50\d/.test(p)));
    await context.close();
    agent.stop();
  }

  // Answers in the session the page cannot show (another window sent one Specify does not know):
  // the page says so and offers to show them again, Save says why it does not write, and the page
  // recovers once the session's answers can be shown.
  {
    const part = join(dir, 'unshown.step');
    writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const { doc } = await agent.call('open', { path: part });
    const shown = await agent.call('show', { doc, browser: false });
    const { context, page } = await load(shown.url);
    const origin = new URL(shown.url).origin;
    const put = async answers => {
      const { rev } = await (await fetch(`${shown.url}answers`)).json();
      const response = await fetch(`${shown.url}answers`, { method: 'PUT', headers: { 'Content-Type': 'application/json', Origin: origin }, body: JSON.stringify({ rev, part: 0, answers }) });
      assert.equal(response.status, 200, await response.text());
    };
    await put({ 'part.material': 'Steel 1.4301', 'nope.unknown': 1 });
    const unshown = page.locator('.app-unshown');
    await check(unshown).toContainText('could not be shown');
    await check(unshown).toContainText('nope.unknown');
    await check(unshown.getByRole('button', { name: 'Show the latest answers' })).toBeVisible();
    await unshown.getByRole('button', { name: 'Show the latest answers' }).click();
    await check(unshown).toContainText('could not be shown');
    // Save does not write answers the page has not shown, and says why rather than nothing.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-problem').first()).toContainText('not shown yet');
    await check(unshown).toBeVisible();
    assert.ok(!existsSync(join(dir, 'unshown.specified.step')), 'nothing written');
    // The session's answers become ones Specify knows: shown, and Save writes them.
    await put({ 'part.material': 'Steel 1.4301' });
    await check(unshown).toHaveCount(0);
    await check(page.locator('.pmi-lab-panel')).toContainText('1.4301');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(join(dir, 'unshown.specified.step'));
    assert.equal(given(stored(join(dir, 'unshown.specified.step')))['part.material'], 'Steel 1.4301');
    // The page's own errors (the failed review) are said in the page, not thrown.
    problems.splice(0, problems.length, ...problems.filter(p => !/nope\.unknown|unknown answer ID/.test(p)));
    await context.close();
    agent.stop();
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
  // A diff the agent opened (`open` with `old`): `show` serves diff mode beside the new file's own
  // session (ADR P11: nothing is carried; the comparison reads the two files' PMI).
  {
    const oldFile = join(dir, 'enclosure.step');
    const newFile = join(dir, 'enclosure-rev2.step');
    writeFileSync(oldFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_before.step.gz')));
    writeFileSync(newFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_after.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const opened = await agent.call('open', { path: newFile, old: oldFile });
    assert.ok(opened.verdict, 'open with old compares');
    const shown = await agent.call('show', { doc: opened.doc, browser: false });
    assert.equal(shown.mode, 'diff');
    const { context, page, requests } = await load(shown.url, '.diff-summary');
    await check(page.getByRole('tab', { name: 'What changed' })).toHaveAttribute('aria-selected', 'true');
    await check(page.locator('.app-path')).toHaveText(`${oldFile} → ${newFile}`);
    // What to check: the new file's requirements on the four holes resized Ø3 → Ø3.4. (Before
    // ADR P11 a third decision asked the new Ø8 entry hole's question; diff asks no questions.)
    assert.deepEqual(opened.report.check.map(c => c.requirement), ['⌀3.4 H11', '⌖ ⌀0.2Ⓜ A B C']);
    await check(page.locator('.app-decision')).toHaveText(['Check: ⌀3.4 H11', 'Check: ⌖ ⌀0.2Ⓜ A B C']);
    // The new Ø8 entry hole has ⌀8 and its locations stated in the new file, so no feature is
    // listed as new with nothing stated.
    assert.equal(opened.report.new_unstated, undefined, 'the Ø8 hole has requirements stated');
    await check(page.locator('.diff-report')).toContainText('Ø8 × 3 through hole');
    await check(page.locator('.diff-report')).not.toContainText('nothing stated');
    // Drawn from GET diff (the session's comparison): no engine diff call.
    const ops = await page.evaluate(() => window.__engineOps);
    assert.ok(ops.includes('open') && !ops.includes('diff'), `no engine diff call in diff mode: ${ops}`);
    assert.ok(requests.some(u => u === `${shown.url}diff`), 'the comparison came from GET diff');
    onlyLocal(requests, shown.url);
    await context.close();
    agent.stop();
  }
  assert.deepEqual(problems, [], 'no page errors');
  console.log('app-agent passed');
} finally {
  for (const agent of agents) agent.stop();
  await browser.close();
}
