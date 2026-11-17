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
async function openPage(readOnly) {
  const page = await browser.newPage({ viewport: { width: 1300, height: 1100 } });
  page.on('pageerror', error => problems.push(String(error)));
  await page.goto(`${base}/?specify=1${readOnly ? '&readOnly=1' : ''}`);
  await page.locator('#specify-file').setInputFiles('test/fixtures/spindle_pmi_before.step.gz');
  await check(page.locator('#specify-panel .pmi-lab-row-head').first()).toBeVisible();
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
  // Read only.
  {
    const page = await openPage(true);
    await check(panel(page).getByText('Read only', { exact: true })).toBeVisible();
    await check(panel(page).getByRole('tab', { name: 'Review', exact: true })).toBeVisible();
    await check(panel(page).getByRole('tab', { name: 'Review/Edit' })).toHaveCount(0);
    await check(panel(page).getByText('change any of them below')).toHaveCount(0);
    await check(panel(page).getByText(/^\d+ suggestions?$/)).toHaveCount(0);
    for (const name of ['Keep all', 'Reset to default', 'Looks right']) await check(panel(page).getByRole('button', { name: new RegExp(name) })).toHaveCount(0);
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Added by you' }).click();
    await check(panel(page).getByRole('button', { name: '+ Add a requirement' })).toHaveCount(0);
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Datums' }).click();
    await check(panel(page).locator('.pmi-lab-datums .chip').first()).toBeVisible();
    await check(panel(page).getByRole('button', { name: '+ Add datum' })).toHaveCount(0);
    await check(panel(page).getByRole('button', { name: /^Remove / })).toHaveCount(0);
    // Opening a row works; its answers are shown and disabled.
    await openHoleRow(page);
    const open = panel(page).locator('.pmi-lab-item.on');
    await check(open.first()).toBeVisible();
    const buttons = open.locator('button, input, select');
    assert.ok(await buttons.count() > 0, 'the open question shows its controls');
    for (let i = 0; i < await buttons.count(); i++) assert.ok(await buttons.nth(i).isDisabled(), `control ${i} of the open question is disabled`);
    // Forcing clicks and typing at the disabled controls does nothing.
    await open.locator('button').first().click({ force: true });
    await open.locator('input[type=text]').first().fill('2x0.1', { force: true }).catch(() => {});
    await page.keyboard.press('Enter');
    // The part's fields: a folded one opens, its control disabled.
    await panel(page).locator('.pmi-lab-line', { hasText: 'General tolerance' }).first().click();
    const field = panel(page).locator('[data-question="part.general_tolerance"]');
    await check(field.locator('select')).toBeDisabled();
    // The table: no edit or call-out links; the CSV export is still there.
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    await check(panel(page).locator('table')).toBeVisible();
    await check(panel(page).getByRole('button', { name: /^(Edit|Call out)/ })).toHaveCount(0);
    await check(page.locator('#specify-exports').getByRole('button', { name: /Inspection list \(CSV\)/ })).toBeVisible();
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
