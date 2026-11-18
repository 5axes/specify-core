import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
// The local app (dist-app, `npm run build:app`) served by the real `specify-core open`, the manual
// way in (no agent; test/app-agent.mjs is the agent's): on a part it loads, is titled for the file,
// takes an answer, has the command save part.specified.step beside it (whose PMI `specify-core
// analyse` reads back) even with questions open, saying which, saves with the defaults when asked,
// resumes from that file with the open questions still open, and has no Save when read-only. With
// `open --diff OLD NEW` it shows what changed, lists what needs a decision, opens NEW's
// requirements in Graphical, and Save writes NEW with the carried answers. Nothing is requested
// from anywhere but 127.0.0.1. `npm run test:app`.
//
//   SPECIFY_CORE  the specify-core binary (default: specify-core on PATH); it needs the session
//                 contract (`GET answers`, `POST save {rev, acceptDefaults}`, `open --diff`).
//   DIFF_OLD, DIFF_NEW  the revisions for diff mode (default: the electronics enclosure pair in
//                 ~/Downloads/enclosure-demo, else the spindle fixtures).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent, from `npm run fetch:wasm`).
//   SHOTS         a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_CORE ?? 'specify-core';
const app = resolve('dist-app');
const wasm = resolve(process.env.SPECIFY_WASM_DIR ?? 'public/wasm/intent');
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
assert.ok(existsSync(join(app, 'index.html')), 'dist-app is built (npm run build:app)');
assert.ok(existsSync(join(wasm, 'specify_core_wasm.js')), 'the engine is fetched (npm run fetch:wasm)');

const dir = mkdtempSync(join(tmpdir(), 'specify-app-'));
const part = join(dir, 'part.step');
// A part with no answers stored in it: gramil's shaft.
writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
const specified = join(dir, 'part.specified.step');
// The files the saves write, in order: nothing answered, the defaults taken, one answer given.
const withDefaults = join(dir, 'part.specified-2.step');
const second = join(dir, 'part.specified-3.step');

/** `specify-core open` on ``files`` (one, or `--diff` and two): its address once it prints it, and a way to stop it. */
async function open(file, ...flags) {
  const child = spawn(cli, ['open', '--app', app, '--wasm', wasm, '--no-browser', ...flags, ...[file].flat()], { stdio: ['ignore', 'pipe', 'pipe'] });
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
  const page = await context.newPage();
  page.on('pageerror', error => problems.push(String(error)));
  page.on('console', message => { if (message.type() === 'error') problems.push(`console: ${message.text()}`); });
  await page.goto(server.url);
  await check(page.locator(ready).first()).toBeVisible();
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
/** `specify-core analyse` of ``file``: what Specify reads from it (its PMI as characteristics, the answers stored). */
/** The answers a person gave: Specify's own `_` records left out. */
const given = answers => Object.fromEntries(Object.entries(answers ?? {}).filter(([id]) => !id.startsWith('_')));
const analyse = file => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));

try {
  // Specify: answer, save, read the PMI back.
  {
    const server = await open(part);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.step — Specify');
    await check(page.locator('.app-brand')).toHaveText('Specify');
    await check(page.getByText('Draftwright')).toHaveCount(0);
    await check(page.locator('.app-target')).toHaveText(specified);
    // Save with the material, which is required, still open is not refused: it writes what is answered
    // (nothing, here), and says what is still open. Many STEP files have no complete answer to everything.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(specified);
    await check(page.locator('.app-saved')).toContainText('still open');
    await check(page.locator('.app-saved')).toContainText('part.material');
    await check(page.getByRole('alert')).toHaveCount(0);
    assert.ok(existsSync(specified), 'part.specified.step written with a required question open');
    assert.deepEqual(given(analyse(specified).resume?.answers), {}, 'a plain Save with nothing answered stores no answers');
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    // The person chooses the defaults, which is its own button, always there: Save writes beside the file and says where.
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    await check(page.locator('.app-saved')).toContainText(withDefaults);
    await check(page.getByRole('alert')).toHaveCount(0);
    assert.ok(existsSync(withDefaults), 'the defaults save written');
    assert.match(server.output(), /Saved .*part\.specified-2\.step/);
    // The next save takes the next free name, and the header says so.
    await check(page.locator('.app-target')).toHaveText(second);
    // An answer: the material. Unsaved until Save.
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await check(page.locator('.app-dirty')).toBeVisible();
    if (shots) await page.screenshot({ path: join(shots, 'specify.png') });
    // The exports that are files.
    for (const [name, file] of [['Answers (JSON)', 'part.answers.json'], ['Inspection list (CSV)', 'part-characteristics.csv'], ['AS9102 FAI (Excel)', 'part-AS9102.xlsx']]) {
      const [download] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name }).click()]);
      assert.equal(download.suggestedFilename(), file);
      if (file.endsWith('.json')) {
        const answers = JSON.parse(readFileSync(await download.path(), 'utf8'));
        assert.equal(answers['part.material'], 'Aluminium 6082-T6', 'the answers file has the answer given');
      }
    }
    // A plain Save writes the answers given, and assumes no defaults.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(second);
    await check(page.locator('.app-saved')).toContainText('still open');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(given(analyse(second).resume?.answers), { 'part.material': 'Aluminium 6082-T6' }, 'Save stores only the answers given');
    onlyLocal(requests, server.origin);
    // Leaving the page (pagehide, not into the back/forward cache) sends `closed`: the command then
    // stops 90 s later (without it, after 30 minutes), checked at the end.
    await page.goto('about:blank');
    first = server;
    await context.close();
  }
  // The PMI Specify wrote reads back.
  {
    const analysis = analyse(withDefaults);
    assert.ok(!('part.material' in (analysis.resume?.answers ?? {})), 'the defaults save had no material');
    // The command writes as `specify-core write --accept-defaults` does: the defaults become PMI, and the
    // answers stored are the session's (none here), not the defaults (a contract change from the page's
    // own writer, which stored them; reported to specify-core).
    assert.deepEqual(given(analysis.resume?.answers), {}, 'the answers stored are the session\'s');
    const plain = (analyse(specified).characteristics ?? []).length;
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
  // The partial save reopens with what was answered, and the questions left open are still open.
  {
    const server = await open(specified);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified.step — Specify');
    await check(material(page)).toHaveClass(/required/);
    await check(page.locator('.pmi-lab-panel')).toContainText('required');
    onlyLocal(requests, server.origin);
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
    const refused = await fetch(`${server.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify({ rev, acceptDefaults: false }) });
    assert.equal(refused.status, 409, 'a save is refused when read-only');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // Diff: what changed from OLD to NEW, what needs a decision, and Save writes NEW with the carried answers.
  {
    const demo = join(homedir(), 'Downloads', 'enclosure-demo');
    const [oldSource, newSource] = process.env.DIFF_OLD && process.env.DIFF_NEW ? [process.env.DIFF_OLD, process.env.DIFF_NEW]
      : existsSync(join(demo, 'electronics-enclosure-base-rev2-pmi.step'))
        ? [join(demo, 'electronics-enclosure-base-pmi.step'), join(demo, 'electronics-enclosure-base-rev2-pmi.step')]
        : ['test/fixtures/spindle_pmi_before.step.gz', 'test/fixtures/spindle_pmi_after.step.gz'];
    const oldFile = join(dir, basename(oldSource));
    const newFile = join(dir, basename(newSource));
    copyFileSync(oldSource, oldFile); copyFileSync(newSource, newFile);
    const server = await open([oldFile, newFile], '--diff');
    const config = await (await fetch(`${server.url}config`)).json();
    assert.equal(config.mode, 'diff');
    const report = await (await fetch(`${server.url}diff`)).json();
    const { parts } = await (await fetch(`${server.url}answers`)).json();
    const carried = given(parts['0']);
    // It opens on what changed; the requirements are behind their tab.
    const { context, page, requests } = await load(server, '.diff-summary');
    await check(page.locator('.pmi-lab-panel')).toBeAttached();
    await check(page).toHaveTitle(`${basename(newFile)} — Specify`);
    await check(page.locator('.app-path')).toHaveText(`${oldFile} → ${newFile}`);
    // What changed, first: the comparison drawn, its summary.
    await check(page.getByRole('tab', { name: 'What changed' })).toHaveAttribute('aria-selected', 'true');
    await check(page.locator('.diff-summary').first()).toBeVisible();
    // What needs a decision, from the command's report: each one a button that opens its question.
    const decide = [...report.unresolved ?? [], ...report.check ?? [], ...report.questions ?? []];
    await check(page.locator('.app-decision')).toHaveCount(decide.length);
    if (shots) await page.screenshot({ path: join(shots, 'diff.png') });
    if (decide.length) {
      await page.locator('.app-decision').last().click();
      await check(page.getByRole('tab', { name: /^Requirements for/ })).toHaveAttribute('aria-selected', 'true');
      await check(page.locator(`[data-question="${decide.at(-1).id}"]`).first()).toBeVisible();
    } else await page.getByRole('tab', { name: /^Requirements for/ }).click();
    // NEW carries PMI (its own, or Specify's): View PMI opens in Graphical.
    await check(page.getByRole('radio', { name: 'Graphical' })).toBeChecked();
    if (shots) await page.screenshot({ path: join(shots, 'diff-decide.png') });
    // NEW is not specified yet: the carried answers are unsaved until Save.
    if (Object.keys(carried).length) await check(page.locator('.app-dirty')).toBeVisible();
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    const target = join(dir, `${basename(newFile).replace(/\.gz$/, '').replace(/\.(step|stp)$/, '')}.specified.step`);
    await check(page.locator('.app-saved')).toContainText(target);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(given(analyse(target).resume?.answers), carried, 'Save writes NEW with the session\'s (carried) answers');
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
