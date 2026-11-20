// A small .xlsx writer, in the browser, with no dependency: sheets of text and numbers, a bold
// header row, set column widths. Office Open XML (ECMA-376) in an uncompressed (stored) zip.

export type Cell = string | number | null;
export interface Sheet { name: string; rows: readonly (readonly Cell[])[]; widths?: readonly number[]; header?: number }

// XML normalizes literal CR to LF; a character reference preserves the stored text.
const xml = (text: string) => text.replace(/[<>&"\r]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;', '\r': '&#13;' })[c]!)
  // Characters XML 1.0 can't hold.
  .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\ufffe\uffff]/g, '');
const column = (i: number): string => (i >= 26 ? column(Math.floor(i / 26) - 1) : '') + String.fromCharCode(65 + (i % 26));

function sheetXml(sheet: Sheet): string {
  const cols = sheet.widths?.length
    ? `<cols>${sheet.widths.map((w, i) => `<col min="${i + 1}" max="${i + 1}" width="${w}" customWidth="1"/>`).join('')}</cols>` : '';
  const rows = sheet.rows.map((row, r) => `<row r="${r + 1}">${row.map((cell, c) => {
    if (cell === null || cell === '') return '';
    const at = `${column(c)}${r + 1}`, style = r < (sheet.header ?? 0) ? ' s="1"' : ' s="2"';
    return typeof cell === 'number' && Number.isFinite(cell)
      ? `<c r="${at}"${style}><v>${cell}</v></c>`
      : `<c r="${at}"${style} t="inlineStr"><is><t xml:space="preserve">${xml(String(cell))}</t></is></c>`;
  }).join('')}</row>`).join('');
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">${cols}<sheetData>${rows}</sheetData></worksheet>`;
}

const STYLES = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"><alignment wrapText="1" vertical="top"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment wrapText="1" vertical="top"/></xf></cellXfs></styleSheet>`;

/** The workbook's files, by path in the zip. */
export function workbookFiles(sheets: readonly Sheet[]): [string, string][] {
  const names = sheets.map((s) => xml(s.name.slice(0, 31)));
  return [
    ['[Content_Types].xml', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>${sheets.map((_, i) => `<Override PartName="/xl/worksheets/sheet${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`).join('')}</Types>`],
    ['_rels/.rels', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>`],
    ['xl/workbook.xml', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>${names.map((n, i) => `<sheet name="${n}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('')}</sheets></workbook>`],
    ['xl/_rels/workbook.xml.rels', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${sheets.map((_, i) => `<Relationship Id="rId${i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`).join('')}<Relationship Id="rId${sheets.length + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>`],
    ['xl/styles.xml', STYLES],
    ...sheets.map((s, i): [string, string] => [`xl/worksheets/sheet${i + 1}.xml`, sheetXml(s)]),
  ];
}

const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
  return t;
})();
const crc32 = (bytes: Uint8Array) => { let c = 0xffffffff; for (const b of bytes) c = CRC[(c ^ b) & 0xff]! ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };

/** A zip of ``files``, stored (not compressed): what .xlsx readers accept. */
export function zip(files: readonly [string, string | Uint8Array][]): Uint8Array {
  const enc = new TextEncoder(), parts: Uint8Array[] = [], central: Uint8Array[] = [];
  let offset = 0;
  for (const [path, text] of files) {
    const name = enc.encode(path), data = typeof text === 'string' ? enc.encode(text) : text, crc = crc32(data);
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true); local.setUint16(4, 20, true); local.setUint16(6, 0x0800, true);
    local.setUint32(14, crc, true); local.setUint32(18, data.length, true); local.setUint32(22, data.length, true);
    local.setUint16(26, name.length, true);
    const entry = new DataView(new ArrayBuffer(46));
    entry.setUint32(0, 0x02014b50, true); entry.setUint16(4, 20, true); entry.setUint16(6, 20, true); entry.setUint16(8, 0x0800, true);
    entry.setUint32(16, crc, true); entry.setUint32(20, data.length, true); entry.setUint32(24, data.length, true);
    entry.setUint16(28, name.length, true); entry.setUint32(42, offset, true);
    parts.push(new Uint8Array(local.buffer), name, data);
    central.push(new Uint8Array(entry.buffer), name);
    offset += 30 + name.length + data.length;
  }
  const size = central.reduce((n, b) => n + b.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true); end.setUint16(8, files.length, true); end.setUint16(10, files.length, true);
  end.setUint32(12, size, true); end.setUint32(16, offset, true);
  const all = [...parts, ...central, new Uint8Array(end.buffer)];
  const out = new Uint8Array(all.reduce((n, b) => n + b.length, 0));
  let at = 0; for (const b of all) { out.set(b, at); at += b.length; }
  return out;
}

/** An .xlsx workbook of ``sheets``. */
export const xlsx = (sheets: readonly Sheet[]): Uint8Array => zip(workbookFiles(sheets));
