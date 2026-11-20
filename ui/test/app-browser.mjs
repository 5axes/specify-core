import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { appendFileSync, chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
import { characteristicHistory, validateOpening, wasmDirectory } from './engine-fixture.mjs';
// The local app (dist-app, `npm run build:app`) served by the real `specify-core open`, the manual
// way in (no agent; test/app-agent.mjs is the agent's): on a part it loads, is titled for the file,
// takes an answer, has the command save part.specified.step beside it (whose PMI `specify-core
// analyse` reads back) even with questions open, saying which, saves with the defaults when asked,
// resumes from that file with the open questions still open, and has no Save when read-only. With
// `open --diff OLD NEW` it shows what changed, lists what needs a decision, opens NEW's
// requirements in Graphical, and Save writes NEW with the carried answers. With `--in-place`, a
// save over a file another program changed is refused as that, not as answers that moved on. With
// the command's event streams all taken, the page says it gets no live updates, and catches up
// when it gets one. Nothing is requested from anywhere but 127.0.0.1. `npm run test:app`.
//
//   SPECIFY_CORE  the specify-core binary (default: specify-core on PATH); it needs the session
//                 contract (`GET answers`, `POST save {rev, acceptDefaults}`, `open --diff`).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent, from `npm run fetch:wasm`).
//   SPECIFY_APP_BUILT_IN=1  serve the app and engine built into SPECIFY_CORE (ADR P12) instead
//                 of passing --app dist-app --wasm SPECIFY_WASM_DIR (the release's check).
//   SHOTS         a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_CORE ?? 'specify-core';
const app = resolve('dist-app');
const wasm = wasmDirectory;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const builtIn = process.env.SPECIFY_APP_BUILT_IN === '1';
if (!builtIn) assert.ok(existsSync(join(app, 'index.html')), 'dist-app is built (npm run build:app)');
assert.ok(existsSync(join(wasm, 'specify_core_wasm.js')), 'the engine is fetched (npm run fetch:wasm)');
/** The command's app and engine: its own, or dist-app and the engine directory. */
const assetArgs = builtIn ? [] : ['--app', app, '--wasm', wasm];
const assetEnv = builtIn ? { ...process.env, SPECIFY_APP_DIR: '', SPECIFY_WASM_DIR: '' } : process.env;

const dir = mkdtempSync(join(tmpdir(), "specify app's-"));
const part = join(dir, 'part.step');
// A part with no answers stored in it: gramil's shaft.
writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
// The copy the session's saves write: the first save creates it and later ones update it (#217).
const specified = join(dir, 'part.specified.step');
// What each save wrote there, kept by the test for the checks below: nothing answered, the
// defaults taken, one answer given.
const saves = join(dir, 'saves');
mkdirSync(saves);
const plainSave = join(saves, 'part.specified.step');
const withDefaults = join(saves, 'part.specified-2.step');
const second = join(saves, 'part.specified-3.step');

/** `specify-core open` on ``files`` (one, or `--diff` and two): its address once it prints it, and a way to stop it. */
async function open(file, ...flags) {
  const child = spawn(cli, ['open', ...assetArgs, '--no-browser', ...flags, ...[file].flat()], { env: assetEnv, stdio: ['ignore', 'pipe', 'pipe'] });
  let out = '';
  child.stdout.on('data', chunk => { out += chunk; });
  child.stderr.on('data', chunk => { out += chunk; });
  for (let n = 0; n < 200 && !/http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//.test(out); n++) {
    if (child.exitCode !== null) break;
    await new Promise(r => setTimeout(r, 50));
  }
  const url = out.match(/http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//)?.[0];
  assert.ok(url, `specify-core open printed its address: ${out}`);
  return { url, origin: new URL(url).origin, output: () => out, stop: () => child.kill() };
}

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
/** A page on the served app, every request it (and its worker) makes recorded. */
async function load(server, ready = '.pmi-lab-panel') {
  const context = await browser.newContext({ viewport: { width: 1400, height: 950 }, acceptDownloads: true });
  const requests = [];
  context.on('request', request => requests.push({ url: request.url(), method: request.method() }));
  // Every message the page posts to a worker (the engine), by operation: what the engine was asked.
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
  await page.goto(server.url);
  await check(page.locator(`${ready}, .app-open-failed`).first()).toBeVisible();
  if (ready !== '.app-open-failed' && await page.locator('.app-open-failed').isVisible()) {
    throw new Error(`The app could not open the part: ${await page.locator('.app-open-failed').textContent()}`);
  }
  return { context, page, requests };
}
/** Nothing but the page's own origin (and its blob:/data: URLs) was asked for. */
const onlyLocal = (requests, origin) => {
  const elsewhere = requests.filter(({ url }) => !url.startsWith(`${origin}/`) && !/^(blob|data):/.test(url));
  assert.deepEqual(elsewhere, [], 'no request leaves 127.0.0.1');
  assert.ok(requests.length > 0);
};
const material = page => page.locator('[data-question="part.material"]');
let first = null;
/** The question ids the defaults dialog listed and the person confirmed. */
let listed = [];
/** `specify-core analyse` of ``file``: what Specify reads from it (its PMI as characteristics, the answers stored). */
/** The answers a person gave: Specify's own `_` records left out. */
const given = answers => Object.fromEntries(Object.entries(answers ?? {}).filter(([id]) => !id.startsWith('_')));
const analyse = file => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));

async function downloadFile(page, name, filename) {
  const [download] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name }).click()]);
  assert.equal(download.suggestedFilename(), filename);
  return download.path();
}

const exportAnswers = async (page, filename = 'part.answers.json') => JSON.parse(readFileSync(await downloadFile(page, 'Answers (JSON)', filename), 'utf8'));
const workbookSheet = (path, number) => execFileSync('unzip', ['-p', path, `xl/worksheets/sheet${number}.xml`], { encoding: 'utf8' });

/** The exports downloaded from the page, named for ``path``, against ``file`` (what the save wrote there) read back. */
async function inspectSaved(page, path, file = path) {
  const name = basename(path).replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');
  await check(page.locator('.app-inspection-source')).toContainText(`from ${basename(path)}`);
  const csv = readFileSync(await downloadFile(page, 'Inspection list (CSV)', `${name}-characteristics.csv`), 'utf8');
  const book = await downloadFile(page, 'AS9102 FAI (Excel)', `${name}-AS9102.xlsx`);
  const form2 = workbookSheet(book, 2);
  const form3 = workbookSheet(book, 3);
  const numbers = (analyse(file).characteristics ?? []).map((item) => item.number).sort((a, b) => a - b);
  const csvNumbers = csv.trimEnd().split('\n').slice(1).map((line) => Number(line.split(',')[0])).filter((n) => n > 0);
  const faiNumbers = [...form3.matchAll(/<c r="A(\d+)"[^>]*><is><t [^>]*>([^<]+)<\/t>/g)]
    .filter((match) => Number(match[1]) >= 5).map((match) => Number(match[2]));
  assert.deepEqual(csvNumbers, numbers, 'CSV numbers are exactly those read back from the saved STEP');
  assert.deepEqual(faiNumbers, numbers, 'AS9102 numbers are exactly those read back from the saved STEP');
  assert.doesNotMatch(form3, /Material:/, 'Material belongs on Form 2 only');
  return { csv, form2, form3 };
}
const assertSaveFirst = async (page) => {
  for (const name of ['Inspection list (CSV)', 'AS9102 FAI (Excel)']) {
    const button = page.getByRole('button', { name, exact: true });
    await check(button).toBeDisabled();
    await check(button).toHaveAttribute('title', 'Save first: the inspection files describe the saved STEP');
  }
};

/** After a save to a copy, the exports are enabled from the file as saved (`file?rev=saved`,
 * specify-ui#14), and the 3D view draws that file's PMI. ``before``: the save line shown before it. */
const assertExportsFromSaved = async (page, file, before) => {
  if (before !== undefined) await check(page.locator('.app-saved')).not.toHaveText(before);
  for (const name of ['Inspection list (CSV)', 'AS9102 FAI (Excel)']) {
    await check(page.getByRole('button', { name, exact: true })).toBeEnabled();
  }
  const exported = await inspectSaved(page, specified, file);
  const ops = await page.evaluate(() => window.__engineOps);
  assert.ok(!ops.includes('saved-copy'), 'a separate copy is never reconstructed');
  await check.poll(async () => (await page.evaluate(() => window.__engineOps)).includes('file-scene')).toBe(true);
  return exported;
};
/** The text of the save line, or null before one. */
const savedLine = page => page.locator('.app-saved').textContent({ timeout: 1000 }).catch(() => null);

try {
  // Specify: answer, save, read the PMI back.
  {
    const server = await open(part);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.step — Specify');
    await check(page.locator('.app-brand')).toHaveText('Specify');
    await check(page.getByText('Draftwright')).toHaveCount(0);
    // A file of one part: no part named by the command.
    assert.equal((await (await fetch(`${server.url}config`)).json()).part, null);
    await check(page.locator('.app-target')).toHaveText(specified);
    await assertSaveFirst(page);
    // Save with the material, which is required, still open is not refused: it writes what is answered
    // (nothing, here), and says what is still open. Many STEP files have no complete answer to everything.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(specified);
    await assertExportsFromSaved(page, specified);
    await check(page.locator('.app-saved')).toContainText('still open');
    // Open questions by their short labels: not their ids, nor the questions with their hints (#217).
    await check(page.locator('.app-saved')).toContainText('Material');
    await check(page.locator('.app-saved')).not.toContainText('What material is the part made from?');
    await check(page.locator('.app-saved')).not.toContainText('part.material');
    await check(page.getByRole('alert')).toHaveCount(0);
    assert.ok(existsSync(specified), 'part.specified.step written with a required question open');
    assert.deepEqual(given(analyse(specified).resume?.answers), {}, 'a plain Save with nothing answered stores no answers');
    copyFileSync(specified, plainSave);
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    // The person chooses the defaults, which is its own button, always there. It first lists every
    // default it would write (ADR P9: written only when approved): cancelled, nothing is written.
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list).toContainText('Which general tolerance applies');
    await check(list).toContainText('2768-m');
    await check(list.locator('[data-default="part.material"]')).toHaveCount(0);
    assert.ok(await list.locator('li').count() > 3, 'the full list of defaults');
    await list.getByRole('button', { name: 'Cancel' }).click();
    await check(list).toHaveCount(0);
    await page.waitForTimeout(500);
    assert.deepEqual(readFileSync(specified), readFileSync(plainSave), 'nothing written when the list is cancelled');
    assert.deepEqual(await exportAnswers(page), {}, 'JSON after cancellation contains no unapproved defaults');
    // Confirmed: Save writes beside the file and says where.
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    await check(list.locator('[data-default]').first()).toBeVisible();
    listed = await list.locator('[data-default]').evaluateAll(items => items.map(item => item.getAttribute('data-default')).sort());
    const shownTolerance = (await list.locator('[data-default="part.general_tolerance"] strong').textContent()).trim();
    assert.equal(shownTolerance, 'ISO 2768-m', 'the dialog lists the default the person sees');
    assert.deepEqual(await exportAnswers(page), {}, 'listing defaults does not make them exportable');
    const beforeDefaults = await savedLine(page);
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    // The same copy, updated: the session wrote it (#217).
    await check(page.locator('.app-saved')).not.toHaveText(beforeDefaults);
    await check(page.locator('.app-saved')).toContainText(specified);
    await check.poll(() => (server.output().match(/Saved /g) ?? []).length).toBe(2);
    copyFileSync(specified, withDefaults);
    // The value the person confirmed is exactly what lands in the file.
    assert.equal(given(analyse(withDefaults).resume?.answers)['part.general_tolerance'], shownTolerance, 'the stored answer is the value listed');
    // Inspect the file itself: filtering internal keys here would conceal an export leak.
    const approved = await exportAnswers(page);
    assert.deepEqual(Object.keys(approved).sort(), listed, 'JSON contains exactly the confirmed defaults');
    assert.equal(approved['part.general_tolerance'], shownTolerance, 'JSON contains the value confirmed');
    await check(page.getByRole('alert')).toHaveCount(0);
    assert.ok(existsSync(withDefaults), 'the defaults save written');
    assert.match(server.output(), /Saved .*part\.specified\.step[^\n]*\n[^]*Saved .*part\.specified\.step/);
    assert.ok(!existsSync(join(dir, 'part.specified-2.step')), 'no numbered copy');
    // The next save updates the same copy, and the header says so.
    await check(page.locator('.app-target')).toHaveText(specified);
    await assertExportsFromSaved(page, withDefaults);
    // An answer: the material. Unsaved until Save.
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await check(page.locator('.app-dirty')).toBeVisible();
    if (shots) await page.screenshot({ path: join(shots, 'specify.png') });
    // The exports stay those of the file as saved, not the unsaved answer.
    await inspectSaved(page, specified, withDefaults);
    for (const [name, file] of [['Answers (JSON)', 'part.answers.json']]) {
      const path = await downloadFile(page, name, file);
      if (file.endsWith('.json')) {
        const answers = JSON.parse(readFileSync(path, 'utf8'));
        assert.equal(answers['part.material'], 'Aluminium 6082-T6', 'the answers file has the answer given');
      }
    }
    // A plain Save writes the answers given and assumes no new defaults. The defaults the person
    // confirmed earlier are answers in the session now (the command made them so), so with the
    // material given nothing is left open (before the per-part contract they were never answers, so
    // the page said 'still open' and stored the material alone).
    const beforeAnswer = await savedLine(page);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(specified);
    await check(page.locator('.app-saved')).toContainText('complete');
    await check.poll(() => (server.output().match(/Saved /g) ?? []).length).toBe(3);
    copyFileSync(specified, second);
    await assertExportsFromSaved(page, second, beforeAnswer);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(Object.keys(given(analyse(second).resume?.answers)).sort(), [...listed, 'part.material'].sort(), 'Save stores the answers given and the defaults confirmed, nothing else');
    assert.equal(given(analyse(second).resume?.answers)['part.material'], 'Aluminium 6082-T6');
    onlyLocal(requests, server.origin);
    // Leaving the page (pagehide, not into the back/forward cache) sends `closed`: the command then
    // stops 90 s later (without it, after 30 minutes), checked at the end.
    await page.goto('about:blank');
    first = server;
    await context.close();
  }
  // A scene can refuse private metadata even when review accepts it. Opening must finish
  // with the actual engine error and disabled Save, rather than an endless loading state.
  {
    const server = await open(part);
    const { rev } = await (await fetch(`${server.url}answers`)).json();
    const answers = { 'part.material': 'Steel', _specify: { keep: [], private_state: 'not an answer' },
      _specify_characteristic_numbers: { schema: 2, high_water: 0, entries: [] } };
    const put = await fetch(`${server.url}answers`, { method: 'PUT', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify({ rev, part: 0, answers }) });
    assert.equal(put.status, 200);
    const { context, page } = await load(server, '.app-open-failed');
    await check(page.getByRole('alert')).toContainText('_specify_characteristic_numbers: missing field `faces`');
    await check(page.getByRole('alert')).toContainText('Correct the session answers, then reload this page');
    await check(page.locator('.app-loading')).toHaveCount(0);
    await check(page.locator('.pmi-lab-panel')).toHaveCount(0);
    for (const name of ['Save', 'Save with defaults']) {
      const button = page.getByRole('button', { name, exact: true });
      await check(button).toBeDisabled();
      await check(button).toHaveAttribute('title', /Save is disabled until the file and its answers can be loaded/);
    }
    await context.close();
    server.stop();
  }
  // The sidecar keeps documented metadata while removing a real writer record. The old
  // invented history was invalid (no faces fingerprint), so it tested an opening refusal.
  {
    const server = await open(part);
    const { rev } = await (await fetch(`${server.url}answers`)).json();
    const history = characteristicHistory(new Uint8Array(readFileSync(withDefaults)));
    assert.ok(history.faces && history.entries.length, 'use the history written by the earlier successful Save');
    const answers = { 'part.material': 'Steel', _specify: { keep: [] }, _specify_characteristic_numbers: history };
    assert.doesNotThrow(() => validateOpening('part.step', new Uint8Array(readFileSync(part)), answers), 'the fixture passes review AND scene in the served 0.2.0 engine');
    const put = await fetch(`${server.url}answers`, { method: 'PUT', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify({ rev, part: 0, answers }) });
    assert.equal(put.status, 200);
    const { context, page } = await load(server);
    assert.deepEqual(await exportAnswers(page), { 'part.material': 'Steel', _specify: { keep: [] } });
    await context.close();
    server.stop();
  }
  // The PMI Specify wrote reads back.
  {
    const analysis = analyse(withDefaults);
    assert.ok(!('part.material' in (analysis.resume?.answers ?? {})), 'the defaults save had no material');
    // The page sends the ids it listed and the person confirmed, each with the value seen (`acceptDefaults: {"0": {id: value}}`): the
    // command makes exactly those answers (a new rev) and stores them, so the answers stored are the
    // confirmed defaults and nothing else (before the per-part contract: the session's, none here).
    assert.deepEqual(Object.keys(given(analysis.resume?.answers)).sort(), listed, 'the answers stored are the defaults confirmed');
    const plain = (analyse(plainSave).characteristics ?? []).length;
    assert.ok((analysis.characteristics ?? []).length > plain, 'the defaults save has the defaults\' PMI');
    const kinds = new Set((analysis.characteristics ?? []).map(c => c.kind));
    assert.ok(kinds.has('datum') && kinds.has('dimension'), `the PMI written reads back: ${[...kinds]}`);
  }
  // Resume: the file Specify wrote opens with its answers.
  {
    const server = await open(second);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified-3.step — Specify');
    await check(page.locator('.pmi-lab-resumed')).toBeVisible();
    await check(page.locator('.pmi-lab-panel')).toContainText('6082');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The heartbeat: `GET config` again within 30 s of the first.
    const configs = () => requests.filter(({ url, method }) => method === 'GET' && url === `${server.url}config`).length;
    assert.equal(configs(), 1);
    await check.poll(configs, { timeout: 40000 }).toBe(2);
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // An assembly saved with its defaults: the 3D view draws the saved file's PMI for the part
  // shown, for another part shown next, and again on coming back (specify-ui#14).
  {
    const asmDir = join(dir, 'drawn');
    mkdirSync(asmDir);
    const asm = join(asmDir, 'asm.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const server = await open(asm);
    const { context, page, requests } = await load(server);
    const viewPmi = page.getByRole('radiogroup', { name: 'View PMI' });
    const select = page.locator('.app-part select');
    await check(select).toBeVisible();
    /** The PMI drawn for the part shown: its graphics, by the planes they are looked square at. */
    const drawn = async () => {
      await check(viewPmi).toBeVisible();
      await viewPmi.getByRole('radio', { name: 'Graphical' }).check();
      const squares = page.locator('.pmi-lab-squares button');
      await check(squares.first()).toBeAttached();
      return squares.allTextContents();
    };
    // Before the save: the scene built on opening, from the answers then.
    const opening = await drawn();
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.locator('.app-saved')).toContainText(join(asmDir, 'asm.specified.step'));
    // After it: the saved file's PMI, as the file states it (its datum features and callouts).
    await check.poll(async () => JSON.stringify(await drawn())).not.toBe(JSON.stringify(opening));
    const afterSave = await drawn();
    assert.ok(afterSave.length > 0, 'the saved PMI is drawn');
    const shownPart = await select.inputValue();
    const values = await select.locator('option').evaluateAll(options => options.map(o => o.value));
    const other = values.find(v => v !== shownPart);
    await select.selectOption(other);
    await check(select).toHaveValue(other);
    assert.ok((await drawn()).length > 0, 'another part shows its saved PMI');
    await select.selectOption(shownPart);
    await check(select).toHaveValue(shownPart);
    assert.deepEqual(await drawn(), afterSave, 'coming back shows the saved PMI again');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The partial save reopens with what was answered, and the questions left open are still open.
  {
    const server = await open(plainSave);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified.step — Specify');
    await check(material(page)).toHaveClass(/required/);
    await check(page.locator('.pmi-lab-panel')).toContainText('required');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // A default whose value no longer matches (the command's 400 with expected and current) is said
  // plainly, nothing is written and the defaults are listed again; a failed write rolls its approvals
  // back, so the page does not show them as applied.
  {
    const copy = join(dir, 'fresh');
    mkdirSync(copy);
    const file = join(copy, 'shaft.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const server = await open(file);
    const { context, page } = await load(server);
    const { rev, parts } = await (await fetch(`${server.url}answers`)).json();
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list.locator('[data-default]').first()).toBeVisible();
    const id = await list.locator('[data-default]').first().getAttribute('data-default');
    // The command's own refusal of a stale value: what was sent and what the default is.
    const wrong = await fetch(`${server.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify({ rev, acceptDefaults: { 0: { [id]: 'not-the-default' } } }) });
    assert.equal(wrong.status, 400);
    const body = await wrong.json();
    assert.deepEqual([body.part, body.question, body.expected], [0, id, 'not-the-default']);
    assert.ok('current' in body);
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    // The page, meeting it: says what changed, and lists the defaults again.
    await page.route('**/save', route => route.fulfill({ status: 400, contentType: 'application/json', body: JSON.stringify({ ...body, current: 'a new default' }) }));
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText(`the default for ${id}`);
    await check(page.getByRole('alert').first()).toContainText('changed. You confirmed');
    await check(page.getByRole('alert').first()).toContainText('it is now a new default');
    await check(page.getByRole('alert').first()).toContainText('Nothing was written');
    await check(list).toBeVisible();
    await page.unroute('**/save');
    problems.splice(0, problems.length, ...problems.filter(p => !/400/.test(p)));
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    // The write fails (the folder is read-only): the approvals are rolled back, the page shows none of them.
    if (process.getuid?.() !== 0) {
      chmodSync(copy, 0o555);
      try {
        await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
        await check(page.getByRole('alert').first()).toContainText('Not saved');
      } finally { chmodSync(copy, 0o755); }
      await check(page.locator('.app-saved')).toHaveCount(0);
      await check(page.locator('.app-dirty')).toHaveCount(0);
      const now = await (await fetch(`${server.url}answers`)).json();
      assert.deepEqual(given(now.parts['0'] ?? {}), given(parts['0'] ?? {}), 'the session holds no approval after the failed write');
      await check(page.locator('.pmi-lab-panel')).toContainText('Suggested: ISO 2768-m');
      problems.splice(0, problems.length, ...problems.filter(p => !/50\d/.test(p)));
      // The folder writable again, the same confirmation saves.
      await page.getByRole('button', { name: 'Save with defaults' }).click();
      await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
      await check(page.locator('.app-saved')).toContainText('shaft.specified');
    }
    await context.close();
    server.stop();
  }
  // Read only: a viewer, no Save, and the command refuses one.
  {
    const server = await open(withDefaults, '--read-only');
    const { context, page, requests } = await load(server);
    await check(page.locator('.pmi-lab-panel')).toContainText('PMI (view only)');
    await check(page.getByRole('button', { name: 'Save', exact: true })).toHaveCount(0);
    await check(page.locator('.app-target')).toHaveCount(0);
    await check(page.getByRole('button', { name: 'Inspection list (CSV)' })).toBeEnabled();
    if (shots) await page.screenshot({ path: join(shots, 'read-only.png') });
    // The command refuses a save as the page would send it (its Origin, the session's rev).
    const { rev } = await (await fetch(`${server.url}answers`)).json();
    const refused = await fetch(`${server.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify({ rev, acceptDefaults: {} }) });
    assert.equal(refused.status, 409, 'a save is refused when read-only');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // Decision 2026-11-19: local inspection downloads describe the last saved STEP, never live answers.
  {
    const saved = join(dir, "inspection's in-place.step");
    writeFileSync(saved, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const server = await open(saved, '--in-place');
    const { context, page } = await load(server);
    await assertSaveFirst(page);
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await page.locator('.pmi-lab-suggestion:has([data-suggestion="part.general_tolerance"])').getByRole('button', { name: 'Keep', exact: true }).click();
    await check(page.locator('.app-dirty')).toBeVisible();
    await assertSaveFirst(page);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(saved);
    const original = await inspectSaved(page, saved);
    assert.match(original.form2, /MATERIAL:? Aluminium 6082-T6/);
    assert.ok(existsSync(`${saved}.bak`), 'successful in-place save retained the original backup');
    await page.locator('.pmi-lab-panel').getByRole('button', { name: /^Material/ }).click();
    await box.fill('Brass CW614N');
    await box.press('Enter');
    await check(page.locator('.app-dirty')).toBeVisible();
    assert.deepEqual(await inspectSaved(page, saved), original, 'further edits leave both saved inspection downloads unchanged');
    const json = await exportAnswers(page, "inspection's in-place.answers.json");
    assert.equal(json['part.material'], 'Brass CW614N', 'Answers JSON still exports the current answers');
    await context.close();
    server.stop();
    const readOnly = await open(saved, '--read-only');
    const viewer = await load(readOnly);
    assert.deepEqual(await inspectSaved(viewer.page, saved), original, 'read-only exports the actual opened file');
    await viewer.context.close();
    readOnly.stop();
  }
  // Stored characteristics absent from an interview reconstruction must remain in the exports.
  {
    const spindle = join(dir, 'stored-spindle.step');
    writeFileSync(spindle, gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz')));
    const server = await open(spindle, '--read-only');
    const { context, page } = await load(server);
    const stored = await inspectSaved(page, spindle);
    const numbers = stored.csv.trimEnd().split('\n').slice(1).map((row) => Number(row.split(',')[0]));
    for (const number of [9, 16, 17]) assert.ok(numbers.includes(number), `stored spindle characteristic #${number}`);
    await context.close();
    server.stop();
  }
  // Diff: what changed from OLD to NEW, what needs checking, and Save writes NEW with its own answers.
  {
    // The enclosure: NEW's requirements on the resized holes to check (test/fixtures/README.md).
    const oldFile = join(dir, 'enclosure.step');
    const newFile = join(dir, 'enclosure-rev2.step');
    writeFileSync(oldFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_before.step.gz')));
    writeFileSync(newFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_after.step.gz')));
    const server = await open([oldFile, newFile], '--diff');
    const config = await (await fetch(`${server.url}config`)).json();
    assert.equal(config.mode, 'diff');
    // GET diff: exactly WASM diff()'s {report, markdown, view}, with the PMI scenes in report.pmi.scenes.
    const body = await (await fetch(`${server.url}diff`)).json();
    assert.deepEqual(Object.keys(body).sort(), ['markdown', 'report', 'view']);
    assert.ok(body.report.pmi?.scenes && 'old' in body.report.pmi.scenes && 'new' in body.report.pmi.scenes, 'GET diff carries the PMI scenes');
    const report = body.report;
    // Diff reads the two files' PMI only (ADR P11): nothing is carried; the session is NEW's own.
    assert.equal(report.answers, undefined);
    const { parts } = await (await fetch(`${server.url}answers`)).json();
    const own = given(parts['0'] ?? {});
    assert.deepEqual(own, given(analyse(newFile).resume?.answers ?? {}), 'the session starts from NEW\'s own answers');
    // It opens on what changed; the requirements are behind their tab.
    const { context, page, requests } = await load(server, '.diff-summary');
    await check(page.locator('.pmi-lab-panel')).toBeAttached();
    await check(page).toHaveTitle(`${basename(newFile)} — Specify`);
    await check(page.locator('.app-path')).toHaveText(`${oldFile} → ${newFile}`);
    // What changed, first: the comparison drawn, its summary.
    await check(page.getByRole('tab', { name: 'What changed' })).toHaveAttribute('aria-selected', 'true');
    await check(page.locator('.diff-summary').first()).toBeVisible();
    // The page drew it from GET diff and compared nothing itself: no `diff` request reached its engine
    // (it still asks the engine to open NEW and for the parts' meshes).
    const ops = await page.evaluate(() => window.__engineOps);
    assert.ok(ops.length > 0 && ops.includes('open'), `the engine was used: ${ops}`);
    assert.ok(!ops.includes('diff'), `no engine diff call in diff mode: ${ops}`);
    assert.ok(requests.some(({ url, method }) => method === 'GET' && url === `${server.url}diff`), 'the comparison came from GET diff');
    // What needs checking, from the command's report: NEW's requirements on the resized holes.
    assert.equal(report.unresolved, undefined);
    assert.equal(report.questions, undefined);
    assert.ok((report.check ?? []).length > 0, 'requirements to check');
    await check(page.locator('.app-decision')).toHaveCount(report.check.length);
    if (shots) await page.screenshot({ path: join(shots, 'diff.png') });
    await page.getByRole('tab', { name: /^Requirements for/ }).click();
    // NEW carries PMI (its own, or Specify's): View PMI opens in Graphical.
    await check(page.getByRole('radio', { name: 'Graphical' })).toBeChecked();
    if (shots) await page.screenshot({ path: join(shots, 'diff-decide.png') });
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    const target = join(dir, `${basename(newFile).replace(/\.gz$/, '').replace(/\.(step|stp)$/, '')}.specified.step`);
    await check(page.locator('.app-saved')).toContainText(target);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(given(analyse(target).resume?.answers ?? {}), own, 'Save writes NEW with the session\'s answers');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // --in-place: another program changes the file after it was opened. Save is refused as that,
  // its own error, with no "Save again" (it cannot succeed), and nothing is written over the file.
  {
    const file = join(dir, 'in-place.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const server = await open(file, '--in-place');
    const { context, page, requests } = await load(server);
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await check(page.locator('.app-dirty')).toBeVisible();
    appendFileSync(file, '/* written by another program */\n');
    const changed = readFileSync(file);
    const before = problems.length;
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.getByRole('alert')).toContainText('the file changed on disk since it was opened');
    await check(page.getByRole('alert')).not.toContainText('Save again');
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    assert.deepEqual(readFileSync(file), changed, 'nothing written over the changed file');
    assert.ok(!existsSync(`${file}.bak`), 'no backup made for a save that did not happen');
    // The refused save's 409 is the browser's console error, expected here.
    problems.splice(before, problems.length - before, ...problems.slice(before).filter(p => !/409/.test(p)));
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The event stream refused (the command serves at most 4 at once: here all taken): the page says
  // it gets no live updates and keeps trying; once it gets a stream it shows what changed meanwhile.
  {
    const server = await open(part);
    const taken = [];
    for (let n = 0; n < 4; n++) {
      const stop = new AbortController();
      const response = await fetch(`${server.url}events`, { signal: stop.signal });
      assert.equal(response.status, 200);
      // Read (and so held) until aborted: an unread response the collector frees closes its stream.
      response.body.pipeTo(new WritableStream()).catch(() => {});
      taken.push({ stop, response });
    }
    const before = problems.length;
    const { context, page, requests } = await load(server);
    await check(page.locator('.app-offline')).toContainText('Not receiving live updates');
    // Another tab answers meanwhile: this page cannot hear it yet.
    const { rev } = await (await fetch(`${server.url}answers`)).json();
    const put = await fetch(`${server.url}answers`, { method: 'PUT', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify({ rev, part: 0, answers: { 'part.material': 'Brass CW614N' } }) });
    assert.equal(put.status, 200);
    for (const { stop } of taken) stop.abort();
    await check(page.locator('.app-offline')).toHaveCount(0);
    await check(page.locator('.pmi-lab-panel')).toContainText('CW614N');
    // The refused stream's 503s are the browser's console errors, expected here.
    problems.splice(before, problems.length - before, ...problems.slice(before).filter(p => !/503|EventSource|events/.test(p)));
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The first page's `closed` arrived: the command said so and stopped.
  await check.poll(() => first.output().includes('Closed.'), { timeout: 150000 }).toBe(true);
  assert.deepEqual(problems, [], 'no page errors');
  console.log('app-browser passed');
} finally {
  first?.stop();
  await browser.close();
}
