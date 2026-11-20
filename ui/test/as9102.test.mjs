import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { toCsv } from '../src/specify/characteristics.ts';
import { as9102 } from '../src/specify/as9102.ts';
import { xlsx, workbookFiles } from '../src/model/xlsx.ts';

const row = (n, text, extra = {}) => ({ n, sub: 0, id: String(n), feature: 'hole 1', group: 'hole 1', text, nominal: null, upper: null, lower: null, datums: '', basic: false, faces: [], source: `req:${n}`, ...extra });
const rows = [
  row(1, '⌀10 +0.1/0 THRU', { nominal: 10, upper: 0.1, lower: 0 }),
  row(2, '⌖ ⌀0.1', { upper: 0.1, datums: 'A|B|C' }),
  row(3, '30 (basic)', { nominal: 30, basic: true }),
  row(5, '20 ±0.1', { nominal: 20, upper: 0.1 }),
  row(4, 'skipped one'),
];
const notes = [{ key: 'part:material', text: 'MATERIAL Alu_G6061_T6' }, { key: 'part:general_tolerance', text: 'GENERAL TOLERANCES ISO 2768-mK' },
  { key: 'part:surface_finish', text: 'SURFACE FINISH Ra 3.2' }, { key: 'part:coating', text: 'ANODISE CLEAR (MIL-A-8625 TYPE II)' }];

test('Forms 1–3: the part, its materials, each characteristic with its requirement and no result', () => {
  const [f1, f2, f3] = as9102({ name: 'pmi_plate' }, rows, new Set(['hole 1|hole 1|skipped one']), notes);
  assert.deepEqual(f1.rows.find(r => r[0] === 'Part Name'), ['Part Name', 'pmi_plate']);
  assert.deepEqual(f1.rows.find(r => r[0] === 'Serial Number'), ['Serial Number', ''], 'what the part does not say is left blank');
  for (const f of [f2, f3]) assert.deepEqual(f.rows[1].slice(0, 4), ['Part Number', '', 'Part Name', 'pmi_plate'], `${f.name} repeats the part`);
  assert.deepEqual(f2.rows.slice(4, 6).map(r => r[0]), ['MATERIAL Alu_G6061_T6', 'ANODISE CLEAR (MIL-A-8625 TYPE II)'], 'material and special processes; finish and the general tolerance are requirements');
  assert.deepEqual(f3.rows.slice(4).map(r => [r[0], r[3], r[4], r[7]]), [
    ['1', '⌀10 +0.1/0 THRU', '', '10–10.1'],
    ['2', '⌖ ⌀0.1', '', 'datums A B C'],
    ['3', '30 (basic)', '', 'basic'],
    ['5', '20 ±0.1', '', '19.9–20.1'],
  ], 'skipped rows are left out; results are the inspector\'s');
});

// Explicit STEP metadata selects saved-file rules; legacy rows keep origin/main's contents.
test('Form 3 has numbered characteristics only, while Material appears on Form 2', () => {
  const material = row(0, 'Material: Steel', { source: 'part:material', numbering: 'step' });
  const note = row(0, 'An unnumbered note', { numbering: 'step' });
  const [, form2, form3] = as9102({ name: 'part' }, [material, note, row(9, 'Stored dimension')], new Set(), [{ key: 'part:material', text: 'MATERIAL Steel' }]);
  assert.deepEqual(form2.rows[4], ['MATERIAL Steel', '', '', '', '', '']);
  assert.deepEqual(form3.rows.slice(4).map((row) => [row[0], row[3]]), [['9', 'Stored dimension']]);
});
test('the workbook is a valid .xlsx: a zip every part of which is well-formed XML', () => {
  const dir = mkdtempSync(join(tmpdir(), 'as9102-'));
  const path = join(dir, 'fai.xlsx');
  writeFileSync(path, xlsx(as9102({ name: 'a <b> & "c"' }, rows, new Set(), notes)));
  const out = execFileSync('python3', ['-I', '-c', `
import sys, zipfile, xml.dom.minidom
z = zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None
for n in z.namelist(): xml.dom.minidom.parseString(z.read(n))
print(sorted(z.namelist()))`, path]).toString();
  for (const part of ['[Content_Types].xml', 'xl/workbook.xml', 'xl/worksheets/sheet3.xml', 'xl/styles.xml']) assert.ok(out.includes(part), part);
  assert.ok(workbookFiles([{ name: 'S', rows: [['a <b> & "c"', 1.5]] }])[5][1].includes('a &lt;b&gt; &amp; &quot;c&quot;'));
  assert.ok(!workbookFiles([{ name: 'S', rows: [['x\uffffy\u0001']] }])[5][1].match(/[\uffff\u0001]/), 'characters XML cannot hold are dropped');
});

test('stored IDs round-trip CR, LF, CRLF, tabs, commas and quotes through CSV and XLSX', () => {
  const ids = ['A\r9', 'A\n9', 'A\r\n9', 'A\t9', 'A,9', 'A"9', 'A\r\n\t,"9"'];
  const chars = ids.map((id, i) => row(0, 'Stored requirement', { id, identifier: id, numbering: 'step', source: `stored:${i}` }));
  const csv = toCsv(chars);
  assert.ok(csv.includes('"A\r9",'), 'RFC 4180 quotes CR even without LF, comma or quote');
  const sheets = as9102({ name: 'part' }, chars, new Set(), []);
  const xml = workbookFiles(sheets).find(([name]) => name === 'xl/worksheets/sheet3.xml')[1];
  assert.ok(xml.includes('A&#13;9'));
  assert.ok(xml.includes('A&#13;\n9'), 'CRLF retains both characters');
  assert.ok(xml.includes('A\t9'), 'tabs remain in cell text');
  const dir = mkdtempSync(join(tmpdir(), 'inspection-controls-'));
  const csvPath = join(dir, 'inspection.csv');
  const xlsxPath = join(dir, 'inspection.xlsx');
  writeFileSync(csvPath, csv);
  writeFileSync(xlsxPath, xlsx(sheets));
  // Independent parsers catch CSV record splitting and XML's normalization of bare CR.
  const parsed = JSON.parse(execFileSync('python3', ['-I', '-c', `
import csv, json, sys, zipfile, xml.etree.ElementTree as ET
with open(sys.argv[1], encoding='utf-8', newline='') as f:
    records = list(csv.reader(f, strict=True))
assert all(len(record) == 10 for record in records)
with zipfile.ZipFile(sys.argv[2]) as z:
    root = ET.fromstring(z.read('xl/worksheets/sheet3.xml'))
ns = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
ids = [cell.find('s:is/s:t', ns).text for cell in root.findall('.//s:c', ns)
       if cell.attrib['r'].startswith('A') and int(cell.attrib['r'][1:]) >= 5]
print(json.dumps({'csv': [row[0] for row in records[1:]], 'xlsx': ids}))`, csvPath, xlsxPath], { encoding: 'utf8' }));
  assert.deepEqual(parsed, { csv: ids, xlsx: ids });
});
