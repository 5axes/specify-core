import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
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
async function openPage(readOnly, file = 'test/fixtures/spindle_pmi_before.step.gz', ready = '#specify-panel .pmi-lab-row-head', query = '') {
  const page = await browser.newPage({ viewport: { width: 1300, height: 1100 } });
  page.on('pageerror', error => problems.push(String(error)));
  await page.goto(`${base}/?specify=1${readOnly ? '&readOnly=1' : ''}${query}`);
  await page.locator('#specify-file').setInputFiles(file);
  await check(page.locator(ready).first()).toBeVisible();
  // Wait for the host's display path: optional inspection or the original preview method.
  await check.poll(() => page.evaluate(() => window.__calls.includes(location.search.includes('legacy=1') ? 'preview' : 'inspection'))).toBe(true);
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
// Diagnose disabled/covered host controls before starting a download-event timeout. A trial
// click preserves Playwright's hit testing and never runs an immediate or deferred export.
async function downloadFrom(page, button) {
  await check(button).toBeEnabled();
  await button.click({ trial: true });
  const [file] = await Promise.all([page.waitForEvent('download'), button.click()]);
  return file.path();
}
const inspectionDownload = (page, name) => downloadFrom(page,
  page.locator('#specify-exports').getByRole('button', { name, exact: true }));
try {
  // A host implementing only the original three methods keeps origin/main's shaft preview,
  // including all 15 sequential balloons and its complete Form 3 (Material is balloon 12).
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&legacy=1');
    const input = panel(page).locator('[data-question="part.material"] input');
    await input.fill('Steel');
    await input.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    const material = panel(page).locator('.pmi-lab-table tr').filter({ hasText: 'Material: Steel' });
    await check(material.locator('td').nth(1)).toHaveText('12');
    const download = (name) => inspectionDownload(page, name);
    const csv = readFileSync(await download('Inspection list (CSV)'), 'utf8');
    const expected = [
      'M2×0.4-6H', '⌖ ⌀0.2', 'M4×0.7-6H', '⌖ ⌀0.25',
      '36.1 from datum B', '10.4 from datum B', '4 from datum B', '3 from bottom face',
      '4.9 from bottom face', '4.75 from back face', '37.6 from 2 left faces',
      'Material: Steel', 'General tolerance: ISO 2768-m', 'Surface finish: Ra 3.2', 'Edges: Break sharp edges 0.2 max',
    ];
    assert.deepEqual(csv.trimEnd().split('\n').slice(1).map((line) => [line.split(',')[0], line.split(',')[3]]), expected.map((text, i) => [String(i + 1), text]));
    const form3 = execFileSync('unzip', ['-p', await download('AS9102 FAI (Excel)'), 'xl/worksheets/sheet3.xml'], { encoding: 'utf8' });
    for (const [i, text] of expected.entries()) {
      assert.match(form3, new RegExp(`<c r="A${i + 5}"[^>]*><is><t [^>]*>${i + 1}</t>`));
      assert.ok(form3.includes(text), `Form 3 retains ${text}`);
    }
    assert.ok(!(await calls(page)).includes('inspection'), 'the host exposes only three methods');
    await page.close();
  }

  // A legacy export gated on sign-in must read rows, skips and notes at execution time,
  // just as origin/main does. Queue both formats, then edit and skip before resuming.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&legacy=1&defer=1');
    const input = panel(page).locator('[data-question="part.material"] input');
    await input.fill('Steel');
    await input.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    await check(panel(page).locator('.pmi-lab-table')).toContainText('Material: Steel');
    const exports = page.locator('#specify-exports');
    await exports.getByRole('button', { name: 'Inspection list (CSV)', exact: true }).click();
    await exports.getByRole('button', { name: 'AS9102 FAI (Excel)', exact: true }).click();
    await panel(page).getByRole('tab', { name: 'Review/Edit', exact: true }).click();
    await panel(page).getByRole('button', { name: /^Material/ }).click();
    await input.fill('Brass CW614N');
    await input.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    await check(panel(page).locator('.pmi-lab-table')).toContainText('Material: Brass CW614N');
    await panel(page).getByRole('checkbox', { name: 'Inspect 1', exact: true }).uncheck();
    const resume = () => downloadFrom(page, page.getByRole('button', { name: 'Resume download', exact: true }));
    const csv = readFileSync(await resume(), 'utf8');
    assert.match(csv, /12,,Part,Material: Brass CW614N/);
    assert.doesNotMatch(csv, /Steel|M2×0.4-6H/);
    const book = await resume();
    const form2 = execFileSync('unzip', ['-p', book, 'xl/worksheets/sheet2.xml'], { encoding: 'utf8' });
    const form3 = execFileSync('unzip', ['-p', book, 'xl/worksheets/sheet3.xml'], { encoding: 'utf8' });
    assert.match(form2, /MATERIAL Brass CW614N/);
    assert.match(form3, /Material: Brass CW614N/);
    assert.doesNotMatch(form2 + form3, /Steel|M2×0.4-6H/);
    await page.close();
  }

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
  // A suggestion not kept is a suggestion: no tick, counted as open. Keeping it makes it an answer, ticked.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz');
    const rows = panel(page).locator('.pmi-lab-rows');
    const cards = panel(page).locator('.pmi-lab-suggestion');
    const count = async () => Number((await panel(page).getByText(/^\d+ suggestions?$/).innerText()).split(' ')[0]);
    const before = await count();
    assert.ok(before > 1, 'the shaft has suggestions to keep');
    // (The open row shows its own suggestions as its questions, not as cards.)
    assert.ok(await cards.count() > 0 && await cards.count() <= before, 'the suggestions are cards, each counted');
    const cardsBefore = await cards.count();
    await check(rows).not.toContainText('✓');
    await check(panel(page).locator('.pmi-lab-review')).not.toContainText('✓');
    const tolerance = panel(page).locator('.pmi-lab-suggestion:has([data-suggestion="part.general_tolerance"])');
    await check(tolerance.locator('.suggested')).toContainText(/^Suggested: ISO 2768-m/);
    await check(tolerance.getByRole('button', { name: 'Keep', exact: true })).toBeVisible();
    await check(panel(page).locator('.pmi-lab-line', { hasText: 'General tolerance' })).toHaveCount(0);
    await tolerance.getByRole('button', { name: 'Keep', exact: true }).click();
    // Kept: it is an answer, shown folded with a tick, and no longer counted.
    await check(panel(page).locator('.pmi-lab-line', { hasText: 'General tolerance' })).toContainText('✓');
    await check(tolerance).toHaveCount(0);
    await check.poll(count).toBe(before - 1);
    assert.equal(await cards.count(), cardsBefore - 1);
    // Only the one kept has a tick: the suggestions left still have none.
    await check(cards.locator('text=✓')).toHaveCount(0);
    await check(rows.locator('.pmi-lab-tick')).toHaveCount(1);
    await page.close();
  }
  // Keep all lists what it will keep first (ADR P9): cancelled, nothing is kept; confirmed, all are.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz');
    const count = async () => Number((await panel(page).getByText(/^\d+ suggestions?$/).innerText()).split(' ')[0]);
    const before = await count();
    assert.ok(before > 1, 'more than one suggestion, so Keep all is offered');
    const asked = (await calls(page)).filter(c => c === 'questions').length;
    await panel(page).getByRole('button', { name: 'Keep all', exact: true }).click();
    const list = panel(page).getByRole('group', { name: 'Keep all suggestions' });
    await check(list.locator('li')).toHaveCount(before);
    await check(list).toContainText('ISO 2768-m');
    await list.getByRole('button', { name: 'Cancel' }).click();
    await check(list).toHaveCount(0);
    assert.equal(await count(), before, 'cancelled: nothing kept');
    assert.equal((await calls(page)).filter(c => c === 'questions').length, asked, 'cancelled: nothing committed');
    await panel(page).getByRole('button', { name: 'Keep all', exact: true }).click();
    await list.getByRole('button', { name: `Keep these ${before}` }).click();
    await check(panel(page).getByText(/^\d+ suggestions?$/)).toHaveCount(0);
    await check(panel(page).locator('.pmi-lab-suggestion')).toHaveCount(0);
    await page.close();
  }
  // No export override: the shared component uses the host engine inspection for every given
  // requirement. Material has no numbered STEP entity, so its displayed and exported ID is empty.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz');
    const material = panel(page).locator('[data-question="part.material"] input');
    await material.fill('Aluminium 6082-T6');
    await material.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    const row = panel(page).locator('.pmi-lab-table tr').filter({ hasText: 'Material: Aluminium 6082-T6' });
    await check(row.getByRole('checkbox', { name: 'Inspect Material: Aluminium 6082-T6', exact: true })).toBeVisible();
    const download = (name) => inspectionDownload(page, name);
    const csv = readFileSync(await download('Inspection list (CSV)'), 'utf8');
    assert.deepEqual(csv.trimEnd().split('\n').slice(1), [',,Part,Material: Aluminium 6082-T6,,,,,,']);
    const path = await download('AS9102 FAI (Excel)');
    const form3 = execFileSync('unzip', ['-p', path, 'xl/worksheets/sheet3.xml'], { encoding: 'utf8' });
    assert.doesNotMatch(form3, /<c r="A5"/, 'unnumbered Material has an empty number cell');
    assert.doesNotMatch(form3, /Material: Aluminium 6082-T6/, 'Material belongs only on Form 2');
    assert.match(execFileSync('unzip', ['-p', path, 'xl/worksheets/sheet2.xml'], { encoding: 'utf8' }), /MATERIAL Aluminium 6082-T6/);
    assert.doesNotMatch(form3, /ISO 2768|Ra 3.2|M4×|M2×/, 'no unapproved part notes or threads');
    await page.close();
  }
  // A host that keeps suggestions itself (the editor: onKeep, and kept for the ticks): Keep goes to
  // the host and nothing is answered; the question the host lists as kept is ticked in its row.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&keep=host');
    const id = 'hole.function:holes:13';
    const card = panel(page).locator(`.pmi-lab-suggestion:has([data-suggestion="${id}"])`);
    const before = (await calls(page)).length;
    await card.getByRole('button', { name: 'Keep', exact: true }).click();
    await check.poll(async () => (await calls(page)).slice(before)).toEqual([`onKeep:${id}`]);
    await check(panel(page).locator('.pmi-lab-tick')).toHaveCount(0);
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Ø3.3' }).click();
    await check(panel(page).locator('.pmi-lab-tick')).toHaveCount(1);
    // On the question kept: what the Ø3.3 hole is for.
    assert.match(await panel(page).locator('.pmi-lab-tick').evaluate(el => el.closest('.pmi-lab-item')?.textContent ?? ''), /What is the Ø3\.3 hole for\?/);
    assert.ok(!(await calls(page)).slice(before).includes('questions'), 'the host keeps it: no answer is committed');
    const csv = readFileSync(await inspectionDownload(page, 'Inspection list (CSV)'), 'utf8');
    assert.equal(csv.trimEnd().split('\n').length, 2, 'only the one explicitly kept thread is exported');
    assert.match(csv, /M4×0.7-6H/);
    assert.doesNotMatch(csv, /ISO 2768|Ra 3.2|M2×/);
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
    const position = table.locator('tbody tr').filter({ hasText: /⌖.*0.2/ }).first();
    await page.locator('#specify-view .pmi-in').filter({ hasText: '0.2' }).first().click();
    await check(position).toHaveClass(/\bon\b/);
    await panel(page).getByRole('button', { name: 'Datum B' }).click();
    await check(panel(page).locator('.pmi-lab-datums .chip.on')).toHaveCount(1);
    await check(table.locator('tbody tr.on')).toHaveCount(0);
    await page.waitForTimeout(500);
    const made = await calls(page);
    // Changed API expectation: inspection replaces preview to obtain writer-assigned IDs.
    assert.ok(!made.some(call => call !== 'inspection'), `only inspections reach the engine: ${made}`);
    assert.ok(made.includes('inspection'), 'the answers given are labelled with engine IDs');
    await page.close();
  }
  assert.deepEqual(problems, [], 'no page errors');
  console.log('specify-browser passed');
} finally {
  await browser.close();
  server?.kill();
}
