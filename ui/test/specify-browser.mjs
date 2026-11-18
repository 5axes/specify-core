import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chromium, expect } from '@playwright/test';
// <SpecifyReview> in the harness page (test/harness, ?specify=1) on the spindle: editable, then readOnly.
// The harness's engine records each call the component makes of it on window.__calls. Run by
// `npm run test:browser` (it builds the harness); `npm run fetch:wasm` first.
const check = expect.configure({ timeout: 90000 });
const base = process.env.BASE_URL ?? 'http://127.0.0.1:4291';
const server = process.env.BASE_URL ? null : spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--config', 'vite.harness.config.ts', '--host', '127.0.0.1', '--port', '4291', '--strictPort'], { stdio: 'inherit' });
for (let n = 0; n < 100; n++) { try { if ((await fetch(base)).ok) break; } catch {} await new Promise(resolve => setTimeout(resolve, 200)); }
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
async function openPage(readOnly, file = 'test/fixtures/spindle_pmi_before.step.gz', ready = '#specify-panel .pmi-lab-row-head') {
  const page = await browser.newPage({ viewport: { width: 1300, height: 1100 } });
  page.on('pageerror', error => problems.push(String(error)));
  await page.goto(`${base}/?specify=1${readOnly ? '&readOnly=1' : ''}`);
  await page.locator('#specify-file').setInputFiles(file);
  await check(page.locator(ready).first()).toBeVisible();
  // The start effect's preview has been asked for.
  await check.poll(() => page.evaluate(() => window.__calls.includes('preview'))).toBe(true);
  return page;
}
const calls = page => page.evaluate(() => [...window.__calls]);
// The first hole row, opened (it may already be: the first thing needing a person opens at the start).
const openHoleRow = async page => {
  const head = page.locator('#specify-panel .pmi-lab-row-head', { hasText: /hole/i }).first();
  if (await head.getAttribute('aria-expanded') !== 'true') await head.click();
  await check(head).toHaveAttribute('aria-expanded', 'true');
};
const panel = page => page.locator('#specify-panel');
try {
  // Editable: the controls the read-only checks look for are there.
  {
    const page = await openPage(false);
    await check(panel(page).getByText('Read only')).toHaveCount(0);
    await check(panel(page).getByRole('tab', { name: 'Review/Edit' })).toBeVisible();
    // The spindle has one suggestion (no cards or Keep all to see: its row is open, and Keep all needs two).
    await check(panel(page).getByText(/^\d+ suggestions?$/)).toBeVisible();
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Added by you' }).click();
    await check(panel(page).getByRole('button', { name: '+ Add a requirement' })).toBeVisible();
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Datums' }).click();
    await check(panel(page).getByRole('button', { name: '+ Add datum' })).toBeVisible();
    await check(panel(page).getByRole('button', { name: /^Remove datum / }).first()).toBeVisible();
    await openHoleRow(page);
    await check(panel(page).locator('.pmi-lab-item.on')).toBeVisible();
    assert.ok(await panel(page).locator('.pmi-lab-item.on button:not([disabled])').count() > 0, 'an open question has live answer buttons');
    await page.close();
  }
  // Read only: a PMI viewer. The spindle Specify wrote (semantic PMI linked to graphical PMI) shows what is
  // set; nothing to answer, tick, export or change.
  {
    const page = await openPage(true, 'test/fixtures/19-spindle-semantic-links-only.step.gz', '#specify-panel .pmi-lab-table tbody tr');
    const exportsBox = page.locator('#specify-exports');
    await check(panel(page).getByText('PMI (view only)', { exact: true })).toBeVisible();
    await check(panel(page).getByText('Read only')).toHaveCount(0);
    await check(panel(page).getByRole('tab')).toHaveCount(0);
    for (const selector of ['input[type=checkbox]', 'input[type=radio]', 'input[type=text]', 'select', '.pmi-lab-item', '.pmi-lab-row-head']) {
      await check(panel(page).locator(selector), selector).toHaveCount(0);
    }
    for (const name of ['Keep all', 'Reset to default', 'Looks right', '\\+ Add', 'Remove', 'Edit', 'Call out', 'Download CSV']) {
      await check(panel(page).getByRole('button', { name: new RegExp(name) }), name).toHaveCount(0);
    }
    await check(panel(page).getByText(/\d+ suggestions?/)).toHaveCount(0);
    await check(panel(page).getByText('left out of the CSV')).toHaveCount(0);
    await check(exportsBox.getByRole('button')).toHaveCount(0);
    // What is set: the datums, and the characteristics the file states.
    await check(panel(page).getByRole('button', { name: 'Datum A' })).toBeVisible();
    await check(panel(page).getByRole('button', { name: 'Datum B' })).toBeVisible();
    await check(panel(page).getByText('characteristics set on this part.')).toBeVisible();
    const table = panel(page).locator('.pmi-lab-table');
    await check(table.getByText('H11').first()).toBeVisible();
    await check(table.getByText(/⌖/).first()).toBeVisible();
    // What the general tolerance covers, folded under it in one row (no tick box: checked above).
    await check(table.locator('tr.general-summary', { hasText: /dimensions? under ISO 2768/ })).toBeVisible();
    // View PMI: the three views, each chosen in turn.
    const viewPmi = page.getByRole('radiogroup', { name: 'View PMI' });
    await check(viewPmi.getByRole('radio')).toHaveCount(3);
    for (const name of ['Semantic', 'Both', 'Graphical']) {
      await viewPmi.getByRole('radio', { name }).check();
      await check(viewPmi.getByRole('radio', { name })).toBeChecked();
    }
    await viewPmi.getByRole('radio', { name: 'Semantic' }).check();
    await check(page.locator('#specify-view .pmi:not(.hidden)').first()).toBeVisible();
    // Notation, not boxes: no label is carded or bordered (a boxed value reads as a basic dimension);
    // only a frame's cells and a datum's square carry lines.
    await check(page.locator('#specify-view .pmi:not(.hidden) .datum').first()).toBeAttached();
    const lines = await page.evaluate(() => {
      const edge = e => ['Top', 'Right', 'Bottom', 'Left'].map(s => parseFloat(getComputedStyle(e)[`border${s}Width`]) || 0);
      const own = [...document.querySelectorAll('.pmi-lab .pmi:not(.hidden) .pmi-in')];
      return {
        plain: own.filter(e => !e.querySelector('.fcf, .datum')).map(e => ({ text: e.textContent, edges: edge(e), backing: getComputedStyle(e).backgroundColor, shadow: getComputedStyle(e.parentElement).textShadow })),
        wrappers: own.map(e => edge(e).reduce((a, b) => a + b, 0)),
        frames: [...document.querySelectorAll('.pmi-lab .pmi .fcf')].map(f => ({ outer: edge(f), dividers: [...f.children].slice(0, -1).map(c => Math.max(edge(c)[1], edge(c)[3])) })),
        datums: [...document.querySelectorAll('.pmi-lab .pmi .datum')].map(edge),
      };
    });
    assert.ok(lines.plain.length > 0 && lines.frames.length > 0 && lines.datums.length > 0, 'the part has a plain label, a frame and a datum');
    for (const label of lines.plain) {
      assert.deepEqual(label.edges, [0, 0, 0, 0], `"${label.text}" has no border`);
      assert.equal(label.backing, 'rgba(0, 0, 0, 0)', `"${label.text}" has no card (its backing is the text-shadow halo)`);
      assert.notEqual(label.shadow, 'none', `"${label.text}" is haloed`);
    }
    assert.ok(lines.wrappers.every(w => w === 0), 'no label wrapper is bordered');
    for (const f of lines.frames) {
      assert.ok(f.outer.every(w => w > 0), 'a frame keeps its outline');
      assert.ok(f.dividers.every(w => w > 0), 'a frame keeps the lines between its cells');
    }
    for (const d of lines.datums) assert.ok(d.every(w => w > 0), 'a datum keeps its square');
    // A characteristic chosen is marked.
    const row = table.locator('tbody tr').first();
    await row.click();
    await check(row).toHaveClass(/\bon\b/);
    // A label on the part selects its characteristic's row; a datum chosen is marked.
    const second = table.locator('tbody tr').nth(1);
    await page.locator('#specify-view .pmi-in').filter({ hasText: '0.2' }).first().click();
    await check(second).toHaveClass(/\bon\b/);
    await panel(page).getByRole('button', { name: 'Datum B' }).click();
    await check(panel(page).locator('.pmi-lab-datums .chip.on')).toHaveCount(1);
    await check(table.locator('tbody tr.on')).toHaveCount(0);
    await page.waitForTimeout(500);
    const made = await calls(page);
    assert.ok(!made.some(call => call !== 'preview'), `only previews reach the engine: ${made}`);
    assert.ok(made.includes('preview'), 'the answers given are labelled');
    await page.close();
  }
  assert.deepEqual(problems, [], 'no page errors');
  console.log('specify-browser passed');
} finally {
  await browser.close();
  server?.kill();
}
