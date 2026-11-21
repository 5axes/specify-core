import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import { gunzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
import init, { diff, IntentEngine } from '../public/wasm/intent/specify_core_wasm.js';
// <StepDiff> in a minimal host page (test/harness) on three-cad-viewer: two picked files
// compared, the report and the part linked both ways, through src/specify/cad-view.ts. An upgrade
// of the package that changes what it relies on fails here. Run `npm run fetch:wasm` first;
// `npm run test:browser` builds the harness and runs this.
const check = expect.configure({ timeout: 90000 });
const base = process.env.BASE_URL ?? 'http://127.0.0.1:4291';
// With WASM_URL, the page passes it as wasmUrl: the engine the host serves itself (public/wasm/intent),
// not the dependency's, which the host's bundler builds in.
const url = process.env.WASM_URL ? `${base}/?wasmUrl=${encodeURIComponent(process.env.WASM_URL)}` : base;
const server = process.env.BASE_URL ? null : spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--config', 'vite.harness.config.ts', '--host', '127.0.0.1', '--port', '4291', '--strictPort'], { stdio: 'inherit' });
for (let n = 0; n < 100; n++) { try { if ((await fetch(url)).ok) break; } catch {} await new Promise(resolve => setTimeout(resolve, 200)); }
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 1300, height: 900 } });
  // Every operation the page asks of its engine workers, in order, to count them.
  await page.addInitScript(() => {
    window.__ops = [];
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (data, ...rest) {
      if (data && typeof data.operation === 'string') window.__ops.push(data.operation);
      return post.call(this, data, ...rest);
    };
  });
  const ops = () => page.evaluate(() => window.__ops.splice(0));
  const problems = [];
  page.on('pageerror', error => problems.push(String(error)));
  page.on('console', message => { if (/Content Security Policy|Uncaught/i.test(message.text())) problems.push(message.text()); });
  // Which engine's .wasm the page fetches.
  const wasmFetched = new Set();
  page.on('request', request => { const path = new URL(request.url()).pathname; if (/specify_core_wasm.*\.wasm$/.test(path)) wasmFetched.add(path); });
  await page.goto(url);
  await page.locator('.diff-slot input[type=file]').nth(0).setInputFiles('test/fixtures/cast_bearing_unit_before.step.gz');
  await page.locator('.diff-slot input[type=file]').nth(1).setInputFiles('test/fixtures/cast_bearing_unit_after.step.gz');
  await check(page.locator('[data-change]')).toHaveCount(3);
  await check(page.locator('.diff-summary')).toContainText('2 holes changed');
  const fetched = [...wasmFetched];
  if (process.env.WASM_URL) assert.deepEqual(fetched, [new URL(process.env.WASM_URL.replace(/\.js$/, '_bg.wasm'), base).pathname], 'the engine is the one at wasmUrl');
  else assert.ok(fetched.length === 1 && !fetched[0].startsWith('/wasm/intent/'), `the engine is the dependency's, built in by the host (${fetched})`);
  // The tree is the report: a node per line, named by its feature.
  for (const name of ['4× R1 fillets', '2× Ø10 × 11 through holes', '4× freeform surfaces', 'Unchanged']) await check(page.locator('.diff-stage .tv-node-label', { hasText: new RegExp(`^${name}`) }).first()).toBeVisible();

  const canvas = page.locator('.diff-stage canvas').first();
  // Where the part shows a colour, near enough, right of the viewer's tree (and without the
  // orientation cube, which is not the part), in page coordinates; and how many pixels.
  const where = async ([r, g, b]) => {
    const area = await canvas.boundingBox();
    await page.locator('.diff-stage .cad-view-cube').evaluate(cube => { cube.style.visibility = 'hidden'; });
    const png = await page.screenshot({ clip: { x: area.x + 240, y: area.y, width: area.width - 240, height: area.height } });
    await page.locator('.diff-stage .cad-view-cube').evaluate(cube => { cube.style.visibility = ''; });
    const at = await page.evaluate(async ([data, r, g, b]) => {
      const image = await createImageBitmap(new Blob([new Uint8Array(data)], { type: 'image/png' }));
      const c = new OffscreenCanvas(image.width, image.height); const x = c.getContext('2d'); x.drawImage(image, 0, 0);
      const d = x.getImageData(0, 0, image.width, image.height).data; const at = [];
      for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - r) + Math.abs(d[i + 1] - g) + Math.abs(d[i + 2] - b) < 90) at.push([(i / 4) % image.width, Math.floor(i / 4 / image.width)]);
      return at;
    }, [[...png], r, g, b]);
    return at.map(([x, y]) => [area.x + 240 + x, area.y + y]);
  };
  const count = async colour => (await where(colour)).length;
  const lit = [0x89, 0xb9, 0xe3]; // three-cad-viewer's highlight
  assert.ok(await count(lit) < 200, 'nothing is lit before a line is picked');
  // A line lights its faces and turns the part to them.
  await page.click('[data-change="1"]');
  await check(page.locator('[data-change="1"]')).toHaveClass(/is-picked/);
  await page.waitForTimeout(800);
  // Well clear of the unlit count: how many depends on how big the part is drawn.
  assert.ok(await count(lit) > 800, 'the picked line lights its faces');
  // Pointing at a changed face finds its line; clicking it picks it. The faces lit by the line
  // are changed faces, so they are pointed at (not a grid swept over the whole view, one real
  // move at a time on a software GPU).
  const litAt = await where(lit);
  await page.click('[data-change="1"]');
  const box = await canvas.boundingBox();
  let found = null;
  for (let k = 0; k < 40 && !found; k++) {
    const [x, y] = litAt[Math.floor((k * 0.618034 % 1) * litAt.length)];
    await page.mouse.move(x, y); await page.waitForTimeout(25);
    // The page re-renders after the pointer: a hit is one that holds once it has.
    if (await page.locator('[data-change].is-current').count()) {
      await page.waitForTimeout(250);
      if (await page.locator('[data-change].is-current').count()) found = [x, y];
    }
  }
  assert.ok(found, 'pointing at a changed face lights its line');
  await page.mouse.click(...found);
  await check(page.locator('[data-change].is-picked')).toHaveCount(1);
  // Picking a feature in the viewer's tree picks its line, and it stays lit off the tree.
  await page.locator('.diff-stage .tv-node-label', { hasText: /^4× freeform surfaces/ }).click();
  await check(page.locator('[data-change="2"]')).toHaveClass(/is-picked/);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); await page.mouse.move(box.x + 2, box.y + box.height - 2); await page.waitForTimeout(500);
  assert.ok(await count(lit) > 300, 'the line picked in the tree stays lit');

  // Every Before can be shown: the part looks different with them.
  await page.mouse.move(box.x + 2, box.y + box.height - 2);
  await page.click('[data-change].is-picked');
  await page.waitForTimeout(500);
  const shown = await canvas.screenshot();
  await page.locator('.diff-legend input[type=checkbox]').check(); await page.waitForTimeout(500);
  const hidden = await canvas.screenshot();
  const changed = await page.evaluate(async ([a, b]) => {
    const pixels = async data => {
      const image = await createImageBitmap(new Blob([new Uint8Array(data)], { type: 'image/png' }));
      const c = new OffscreenCanvas(image.width, image.height); const x = c.getContext('2d'); x.drawImage(image, 0, 0);
      return x.getImageData(0, 0, image.width, image.height).data;
    };
    const [p, q] = [await pixels(a), await pixels(b)]; let n = 0;
    for (let i = 0; i < p.length; i += 4) if (Math.abs(p[i] - q[i]) + Math.abs(p[i + 1] - q[i + 1]) + Math.abs(p[i + 2] - q[i + 2]) > 30) n++;
    return n;
  }, [[...shown], [...hidden]]);
  assert.ok(changed > 300, `showing every Before changes the picture (${changed} pixels)`);
  // The page still scrolls: three-cad-viewer's stylesheet would stop it.
  assert.notEqual(await page.evaluate(() => getComputedStyle(document.body).overflow), 'hidden');

  // PMI: two revisions Specify wrote, a tolerance changed, one removed, three added and a note
  // reworded (test/fixtures/README.md). The list is the engine's report, item for item.
  await init({ module_or_path: readFileSync('public/wasm/intent/specify_core_wasm_bg.wasm') });
  const spindle = which => gunzipSync(readFileSync(`test/fixtures/spindle_pmi_${which}.step.gz`));
  const spindlePmi = JSON.parse(diff('spindle_pmi_before.step', spindle('before'), 'spindle_pmi_after.step', spindle('after'), '{}', 'spindle', true)).report.pmi;
  const reported = spindlePmi.items;
  // Square to an item's callout: looking into its plane (along -normal), its up the plane's
  // normal × x axis, so its text reads along its x axis, left to right; as data-camera writes it.
  const squareTo = (side, id) => {
    const p = spindlePmi.scenes[side].presentation;
    const { normal: n, x_axis: x } = p.planes.find(q => q.id === p.items[id][0].plane_id);
    const up = [n[1] * x[2] - n[2] * x[1], n[2] * x[0] - n[0] * x[2], n[0] * x[1] - n[1] * x[0]];
    const fixed = u => u.map(v => (Math.abs(v) < 5e-4 ? 0 : v).toFixed(3)).join(',');
    return `${fixed(n.map(v => -v))} ${fixed(up)}`;
  };
  assert.equal(reported.length, 6);
  await ops();
  await page.locator('.diff-slot input[type=file]').nth(0).setInputFiles('test/fixtures/spindle_pmi_before.step.gz');
  await page.locator('.diff-slot input[type=file]').nth(1).setInputFiles('test/fixtures/spindle_pmi_after.step.gz');
  const rows = page.locator('.diff-pmi li');
  await check(rows).toHaveCount(reported.length);
  await check(page.locator('.diff-pmi-legend')).toBeVisible();
  for (const [i, c] of reported.entries()) {
    const row = rows.nth(i);
    await check(row).toHaveAttribute('data-pmi', c.id);
    await check(row).toHaveClass(new RegExp(`is-${c.class}`));
    await check(row.locator('.diff-pmi-number')).toHaveText(c.number === undefined ? '' : `#${c.number}`);
    if (c.old !== undefined) await check(row.locator('.diff-pmi-old')).toHaveText(c.old);
    if (c.new !== undefined && c.new !== c.old) await check(row.locator('.diff-pmi-new')).toHaveText(c.new);
    await check(row.locator('.diff-pmi-on')).toContainText(c.on);
  }
  // Drawn: the new revision's changed callouts orange, added green, the rest grey; the removed
  // one from the old revision, a red ghost.
  const now = page.locator('.diff-pmi-now');
  const was = page.locator('.diff-pmi-was');
  const items = async layer => new Map((await layer.getAttribute('data-drawn-items')).split('\n').filter(Boolean).map(l => l.split(/ (?=#)/)));
  const colour = { changed: '#c26a00', added: '#0a8f3d', removed: '#d62929', unchanged: '#5d646c' };
  const drawnNow = await items(now);
  for (const c of reported.filter(c => c.class !== 'removed')) assert.equal(drawnNow.get(c.id), colour[c.class], `${c.id} is drawn ${c.class}`);
  assert.equal(drawnNow.get('datum:A'), colour.unchanged, 'an unchanged callout is grey');
  assert.deepEqual([...await items(was)], [['position:holes:15.16', colour.removed]], 'the removed callout is the only ghost');
  // Only diffs, which give the two scenes they compared: no scene is read again. (Replacing
  // Before, then After, compares twice; the linked comparisons below count exactly one.)
  assert.deepEqual([...new Set(await ops())], ['diff'], 'each comparison is a diff, with its scenes');
  // A changed item's old callout: no ghost until its row is pointed at, then a red one.
  const before = page.locator('.diff-pmi-before');
  await check(before).toHaveAttribute('data-drawn-items', '');
  // On the canvas, in those colours (the ghost see-through over the page's grey).
  const near = async (test) => {
    const area = await canvas.boundingBox();
    const png = await page.screenshot({ clip: { x: area.x + 240, y: area.y, width: area.width - 240, height: area.height } });
    return page.evaluate(async ([data, test]) => {
      const pass = new Function('r', 'g', 'b', `return ${test}`);
      const image = await createImageBitmap(new Blob([new Uint8Array(data)], { type: 'image/png' }));
      const c = new OffscreenCanvas(image.width, image.height); const x = c.getContext('2d'); x.drawImage(image, 0, 0);
      const d = x.getImageData(0, 0, image.width, image.height).data; let n = 0;
      for (let i = 0; i < d.length; i += 4) if (pass(d[i], d[i + 1], d[i + 2])) n++;
      return n;
    }, [[...png], test]);
  };
  await page.waitForTimeout(800);
  assert.ok(await near('g > 110 && g > r + 50 && g > b + 30') > 40, 'added callouts are drawn green');
  assert.ok(await near('r > 170 && g > 80 && g < 140 && b < 60') > 40, 'changed callouts are drawn orange');
  assert.ok(await near('r > 200 && g > 110 && g < 200 && b > 110 && b < 200 && r - g > 40') > 10, 'the removed callout is a translucent red ghost');
  await mkdir('.cache', { recursive: true });
  await page.screenshot({ path: '.cache/diff-pmi.png' });
  // The Show menu: unchanged general dimensions only when chosen; PMI off and on.
  const graphics = async () => Number(await now.getAttribute('data-drawn-graphics'));
  const shownBefore = await graphics();
  await page.locator('.diff-pmi-show summary').click();
  await page.getByLabel('General dimensions').check();
  await check.poll(graphics).toBeGreaterThan(shownBefore);
  await page.getByLabel('General dimensions').uncheck();
  await page.locator('.diff-pmi-show summary').click();
  await page.locator('.diff-pmi-legend').getByLabel('PMI', { exact: true }).uncheck();
  await check(now).toHaveAttribute('data-drawn-graphics', '0');
  await check(was).toHaveAttribute('data-drawn-graphics', '0');
  await page.locator('.diff-pmi-legend').getByLabel('PMI', { exact: true }).check();
  await check.poll(graphics).toBe(shownBefore);
  // A row lights its callout and turns square to its plane.
  await rows.filter({ hasText: '#13' }).click();
  await check(rows.filter({ hasText: '#13' })).toHaveClass(/is-picked/);
  await check.poll(async () => (await items(now)).get('position:holes:14')).toBe('#0b62c4');
  // data-camera is the look direction, then the way up.
  await check(now).toHaveAttribute('data-camera', squareTo('new', 'position:holes:14'));
  // Its old callout (⌖ ⌀0.3Ⓜ A) is ghosted red beside its new one, lit, while it is picked.
  { const a = await canvas.boundingBox(); await page.mouse.move(a.x + 2, a.y + a.height - 2); }
  await check(before).toHaveAttribute('data-drawn-items', `position:holes:14 ${colour.removed}`);
  await page.waitForTimeout(300);
  const ghostRed = 'r > 200 && g > 110 && g < 200 && b > 110 && b < 200 && r - g > 40';
  const withGhost = await near(ghostRed);
  await page.screenshot({ path: '.cache/diff-pmi-changed-ghost.png' });
  // Pointing at another row (the view stays put) takes it away: the red was that ghost's.
  await rows.filter({ hasText: '#16' }).hover();
  await check(before).toHaveAttribute('data-drawn-items', '');
  await page.waitForTimeout(300);
  const withoutGhost = await near(ghostRed);
  assert.ok(withGhost > withoutGhost + 40, `the old callout is a translucent red ghost (${withoutGhost} → ${withGhost} pixels)`);
  // The removed one's row turns to it in the old revision's place, lit in the ghost.
  await rows.filter({ hasText: '#12' }).click();
  await check.poll(async () => (await items(was)).get('position:holes:15.16')).toBe('#0b62c4');
  await check(before).toHaveAttribute('data-drawn-items', '');
  await check(now).toHaveAttribute('data-camera', squareTo('old', 'position:holes:15.16'));
  // #17's plane reads with -Z up: an orbit control, holding +Z up, would turn it over (#143).
  assert.match(squareTo('new', '17'), / [-0.]+,[-0.]+,-1\.000$/);
  await rows.filter({ hasText: '#17' }).click();
  await check(now).toHaveAttribute('data-camera', squareTo('new', '17'));
  await rows.filter({ hasText: '#12' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: '.cache/diff-pmi-removed-focus.png' });
  // A callout clicked on the part selects its row: back at #13, let go, and found by pointing.
  await rows.filter({ hasText: '#13' }).click();
  await rows.filter({ hasText: '#13' }).click();
  await check(page.locator('.diff-pmi li.is-picked')).toHaveCount(0);
  // Where the changed callouts are drawn: orange pixels, pointed at until one lights its row.
  const area = await canvas.boundingBox();
  await page.mouse.move(area.x + 2, area.y + area.height - 2);
  await page.waitForTimeout(300);
  const png = await page.screenshot({ clip: { x: area.x + 240, y: area.y, width: area.width - 240, height: area.height } });
  const orangeAt = await page.evaluate(async data => {
    const image = await createImageBitmap(new Blob([new Uint8Array(data)], { type: 'image/png' }));
    const c = new OffscreenCanvas(image.width, image.height); const x = c.getContext('2d'); x.drawImage(image, 0, 0);
    const d = x.getImageData(0, 0, image.width, image.height).data; const at = [];
    for (let i = 0; i < d.length; i += 4) if (d[i] > 170 && d[i + 1] > 80 && d[i + 1] < 140 && d[i + 2] < 60) at.push([(i / 4) % image.width, Math.floor(i / 4 / image.width)]);
    return at;
  }, [...png]);
  assert.ok(orangeAt.length > 0, 'the changed callout is drawn where it can be pointed at');
  let callout = null;
  for (let k = 0; k < 30 && !callout; k++) {
    const [px, py] = orangeAt[Math.floor((k + 0.5) * orangeAt.length / 30)];
    const point = [area.x + 240 + px, area.y + py];
    await page.mouse.move(...point); await page.waitForTimeout(40);
    if (await page.locator('.diff-pmi li.is-current').count()) callout = point;
  }
  assert.ok(callout, 'pointing at a callout lights its row');
  const pointedAt = await page.locator('.diff-pmi li.is-current').first().getAttribute('data-pmi');
  await page.mouse.click(...callout);
  await check(page.locator('.diff-pmi li.is-picked')).toHaveAttribute('data-pmi', pointedAt);
  await page.screenshot({ path: '.cache/diff-pmi-clicked.png' });
  // The report's richer items (specify-core 0.1.7): gramil's shaft, an end chamfer added, each
  // revision written with Specify's defaults (ADR P11: diff compares the annotated files, and
  // reads no answers). Since specify-core #106 its location dimensions pair by geometry, each
  // with its own value: the overall 37.6 only renumbered (#5 → #6), with an old id that differs.
  // (The new file's defaults locate the shoulder from the chamfer, 1.5, not 36.1 as before: that
  // is now one removed and another added, never a value changed into another.)
  const shaft = which => gunzipSync(readFileSync(`test/fixtures/gramil_shaft_${which}.step.gz`));
  const written = bytes => {
    const e = new IntentEngine('part.step', bytes);
    try { return Buffer.from(JSON.parse(e.writePmi(e.acceptDefaults('{}'))).bytes); } finally { e.free(); }
  };
  const [shaftBefore, shaftAfter] = [written(shaft('before')), written(shaft('after'))];
  const shaftReport = JSON.parse(diff('gramil_shaft_before.step', shaftBefore, 'gramil_shaft_after.step', shaftAfter, '{}', 'gramil_shaft_after.step', true)).report.pmi;
  assert.deepEqual(shaftReport.items.filter(c => c.class === 'renumbered').map(c => [c.class, c.number, c.old, c.new, c.reason]), [
    ['renumbered', 6, '37.6', '37.6', 'characteristic number #5 → #6']], 'the shaft\'s pairing (specify-core #106)');
  assert.ok(!shaftReport.items.some(c => c.class === 'changed' && c.old !== c.new), 'each dimension keeps its own value');
  assert.ok(shaftReport.items.some(c => c.class === 'removed' && c.old === '36.1'), 'the 36.1 the new file no longer states');
  const renamed = shaftReport.items.find(c => c.old_id && c.old_id !== c.id && shaftReport.scenes.old.presentation.items[c.old_id]?.length);
  // The spindle pair, its old side written with Specify's defaults: the default for its ⌀3.3
  // bore is tapped M4, but the new file states ⌀3.3 H11, and the file's PMI wins. So since
  // specify-core 0.1.8 (#110) the thread is reported removed, with its wording (#106), and the
  // ⌀3.3 H11 added; 0.1.7 paired the thread with an unrelated dimension and kept an M4 the new
  // file does not hold.
  const spindleBefore = written(spindle('before'));
  const spindleReport = JSON.parse(diff('spindle_pmi_before.step', spindleBefore, 'spindle_pmi_after.step', spindle('after'), '{}', 'spindle_pmi_after.step', true)).report.pmi;
  const thread = spindleReport.items.find(c => c.class === 'removed' && c.kind === 'thread' && /^M4×0\.7-6H THRU$/.test(c.old ?? ''));
  assert.ok(renamed && thread, 'the shaft has a renumbered item with an old id; the spindle its thread removed');
  assert.ok(spindleReport.items.some(c => c.class === 'added' && c.new === '⌀3.3 H11'), 'the file\'s ⌀3.3 H11 added');
  // Reported once (specify-core 0.1.9): the thread's manufacturing note is no longer a second
  // removed item, with no callout, beside the thread.
  assert.equal(spindleReport.items.filter(c => c.class === 'removed' && c.old === 'M4×0.7-6H THRU').length, 1, 'the removed thread reported once');
  assert.ok(spindleReport.scenes.old.presentation.items[thread.id]?.length, 'the removed thread has its old callout');
  // The host hands the component the two files it compares (written, here, as above).
  const file = (name, buffer) => ({ name, mimeType: 'application/step', buffer });
  const compareWith = async (a, b) => {
    await page.goto(url);
    await ops();
    await page.locator('.diff-slot input[type=file]').nth(0).setInputFiles(a);
    await page.locator('.diff-slot input[type=file]').nth(1).setInputFiles(b);
  };
  await compareWith(file('gramil_shaft_before.step', shaftBefore), file('gramil_shaft_after.step', shaftAfter));
  const shaftRows = page.locator('.diff-pmi li');
  await check(shaftRows).toHaveCount(shaftReport.items.length);
  for (const [i, c] of shaftReport.items.entries()) await check(shaftRows.nth(i)).toHaveAttribute('data-pmi', c.id);
  await check(page.locator('.diff-pmi-now')).not.toHaveAttribute('data-drawn-items', '');
  assert.deepEqual(await ops(), ['diff'], 'one diff, no scene read');
  // The renumbered item's old callout, by its old id, ghosted while its row is picked.
  await page.locator(`.diff-pmi li[data-pmi="${renamed.id}"]`).click();
  await check(page.locator('.diff-pmi-before')).toHaveAttribute('data-drawn-items', `${renamed.old_id} ${colour.removed}`);
  await page.waitForTimeout(500);
  await page.screenshot({ path: '.cache/diff-pmi-old-id-ghost.png' });
  await compareWith(file('spindle_pmi_before.step', spindleBefore), 'test/fixtures/spindle_pmi_after.step.gz');
  await check(page.locator('.diff-pmi li')).toHaveCount(spindleReport.items.length);
  await check(page.locator('.diff-pmi-now')).not.toHaveAttribute('data-drawn-items', '');
  // Since specify-core 0.1.8 (#110) the old scene holds no stale ids: nothing fails to draw.
  const threadRow = page.locator(`.diff-pmi li[data-pmi="${thread.id}"]`).first();
  await check(threadRow).toContainText('M4×0.7-6H THRU');
  await check(page.getByRole('alert')).toHaveCount(0);
  // A removed item with no callout, shown on its faces, is no longer in the spindle pair: the
  // thread's note it was is reported with the thread (specify-core 0.1.9).
  await check(page.locator('.diff-pmi-on', { hasText: 'no callout' })).toHaveCount(0);
  assert.deepEqual(await page.evaluate(() => window.__events.includes('diff_failed')), false);

  // A part's comparison, apart from an assembly's whole view.
  await page.goto(url);
  await page.evaluate(() => { window.__events.length = 0; window.__parts.length = 0; });
  const PART = '.diff-body:not(.diff-body-assembly)';
  // An assembly, two picked revisions of gramil's: its parts listed, the two changed ones
  // opened to their own comparison, and back.
  await page.locator('.diff-slot input[type=file]').nth(0).setInputFiles('test/fixtures/gramil_assembly_before.step.gz');
  await page.locator('.diff-slot input[type=file]').nth(1).setInputFiles('test/fixtures/gramil_assembly_after.step.gz');
  await check(page.locator('table.diff-parts:visible tbody tr')).toHaveCount(12);
  await check(page.locator('.diff-body-assembly .diff-summary')).toContainText('2 parts changed (GRM-02 shaft, GRM-04 drive plate), 10 unchanged.');
  // Whole, in 3D: every part in the viewer's tree, the two changed ones orange, and one picked
  // there opened to its own comparison.
  await check(page.locator('.diff-assembly-reading')).toHaveCount(0);
  for (const name of ['GRM-02 shaft', 'GRM-04 drive plate', 'GRM-01 shank']) await check(page.locator('.diff-body-assembly .tv-node-label', { hasText: new RegExp(`^${name}`) }).first()).toBeVisible();
  const orange = async () => {
    const area = await page.locator('.diff-body-assembly .diff-stage canvas').first().boundingBox();
    const png = await page.screenshot({ clip: { x: area.x + 240, y: area.y, width: area.width - 240, height: area.height } });
    return page.evaluate(async data => {
      const image = await createImageBitmap(new Blob([new Uint8Array(data)], { type: 'image/png' }));
      const c = new OffscreenCanvas(image.width, image.height); const x = c.getContext('2d'); x.drawImage(image, 0, 0);
      const d = x.getImageData(0, 0, image.width, image.height).data; let n = 0;
      for (let i = 0; i < d.length; i += 4) if (d[i] > 170 && d[i + 1] > 70 && d[i + 1] < 150 && d[i + 2] < 60) n++;
      return n;
    }, [...png]);
  };
  await page.waitForTimeout(800);
  assert.ok(await orange() > 100, 'the changed parts are orange on the assembly');
  await page.locator('.diff-body-assembly .tv-node-label', { hasText: /^GRM-02 shaft/ }).first().click();
  await check(page.locator('.diff-part')).toContainText('GRM-02 shaft');
  await page.getByRole('button', { name: 'All parts' }).click();
  await page.locator('.diff-body-assembly .tv-node-label', { hasText: /^GRM-01 shank/ }).first().click();
  await check(page.locator('.diff-part')).toContainText('GRM-01 shank');
  await check(page.locator(`${PART} .diff-summary`)).toContainText('Unchanged.');
  await page.getByRole('button', { name: 'All parts' }).click();
  // Clicked on the assembly itself (the viewer's own pick is not used: it draws a box): some
  // point on the tall shank opens it, whatever the window's size.
  {
    const area = await page.locator('.diff-body-assembly .diff-stage canvas').first().boundingBox();
    let opened = false;
    for (const [fx, fy] of [[0.5, 0.32], [0.5, 0.45], [0.52, 0.6], [0.48, 0.55], [0.5, 0.7]]) {
      await page.mouse.move(area.x + area.width * fx, area.y + area.height * fy); await page.waitForTimeout(150);
      await page.mouse.down(); await page.mouse.up(); await page.waitForTimeout(800);
      if (await page.locator('.diff-part').count()) { opened = true; break; }
    }
    assert.ok(opened, 'a click on a part of the assembly opens it');
    await check(page.locator(`${PART} .diff-summary`)).toContainText('Unchanged.');
    await check(page.locator(`${PART} .diff-hint`)).toHaveCount(0);
  }
  await check(page.locator(`${PART} .diff-summary`)).toBeVisible();
  await page.getByRole('button', { name: 'All parts' }).click();
  await check(page.locator('.diff-body-assembly')).toBeVisible();
  await check(page.locator('table.diff-parts:visible').getByRole('button', { name: 'Open' })).toHaveCount(12);
  await page.locator('.diff-parts tr', { hasText: 'GRM-04 drive plate' }).getByRole('button', { name: 'Open' }).click();
  await check(page.locator(`${PART} .diff-summary`)).toContainText('1 hole changed');
  assert.equal(await page.evaluate(() => window.__parts.at(-1)), 'GRM-04 drive plate', 'onPart hears the part opened');
  assert.ok(await page.locator('[data-change]').count() >= 1, 'the open part has its What changed lines');
  await check(page.locator(`${PART} .diff-stage canvas`).first()).toBeVisible();
  await page.getByRole('button', { name: 'All parts' }).click();
  await check(page.locator('table.diff-parts:visible tbody tr')).toHaveCount(12);
  await check(page.locator(PART)).toBeHidden();
  assert.equal(await page.evaluate(() => window.__parts.at(-1)), null, 'onPart hears All parts');
  assert.ok(await page.evaluate(() => window.__events.includes('diff_viewed')), 'onEvent hears diff_viewed');
  // The action's links: the whole assembly, and one part by name. GitHub is stood in for.
  // The host's part, set before the files: the component opens it at once, or says the assembly
  // has none by that name. (The editor's action links carry it as &part=.)
  const assembly = await browser.newPage({ viewport: { width: 1300, height: 900 } });
  assembly.on('pageerror', error => problems.push(String(error)));
  assembly.on('console', message => { if (/Uncaught/i.test(message.text())) problems.push(message.text()); });
  const compareAssembly = async part => {
    await assembly.goto(url);
    await assembly.evaluate(part => window.__setPart(part), part);
    await assembly.locator('.diff-slot input[type=file]').nth(0).setInputFiles('test/fixtures/gramil_assembly_before.step.gz');
    await assembly.locator('.diff-slot input[type=file]').nth(1).setInputFiles('test/fixtures/gramil_assembly_after.step.gz');
  };
  await compareAssembly('GRM-02 shaft');
  await check(assembly.locator('.diff-part')).toContainText('GRM-02 shaft');
  assert.ok(await assembly.locator('[data-change]').count() >= 1, 'the part the host named opens at once');
  await assembly.getByRole('button', { name: 'All parts' }).click();
  await check(assembly.locator('table.diff-parts:visible tbody tr')).toHaveCount(12);
  assert.equal(await assembly.evaluate(() => window.__parts.at(-1)), null, 'All parts is told to the host');
  // Opening a part tells the host, to pass on.
  await assembly.locator('.diff-parts tr', { hasText: 'GRM-04 drive plate' }).getByRole('button', { name: 'Open' }).click();
  await check(assembly.locator('[data-change]').first()).toBeVisible();
  assert.equal(await assembly.evaluate(() => window.__parts.at(-1)), 'GRM-04 drive plate');
  // A part the assembly did not change opens too, on its own, plain.
  await compareAssembly('GRM-01 shank');
  await check(assembly.locator('.diff-part')).toContainText('GRM-01 shank');
  await check(assembly.locator(`${PART} .diff-summary`)).toContainText('Unchanged.');
  await check(assembly.locator(`${PART} .diff-stage canvas`).first()).toBeVisible();
  // One it does not have: the list, saying so.
  await compareAssembly('GRM-99');
  await check(assembly.getByText('The assembly has no part named "GRM-99".')).toBeVisible();
  await check(assembly.locator('table.diff-parts:visible tbody tr')).toHaveCount(12);
  assert.deepEqual(problems, []);
  console.log('<StepDiff> on three-cad-viewer: compared, linked both ways, every Before shown, PMI changes listed as the engine reports them and drawn by change from the scenes the diff compared (one diff, no scene read again; a removed callout ghosted, the old callout of a changed one beside it, by its old id too), a thread the new file no longer states reported removed once, a row turning to its callout and a callout finding its row, an assembly listed and its parts opened, onPart and onEvent heard.');
} finally {
  await browser.close();
  server?.kill();
}
