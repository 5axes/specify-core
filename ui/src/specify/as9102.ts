// An AS9102 first article inspection workbook from the characteristics Specify numbers: Form 1
// (the part), Form 2 (materials, specifications and processes) and Form 3 (each characteristic,
// with its requirement and an empty result). Every requirement comes from the part's PMI, not
// read off a drawing; what the part doesn't say (serial number, report number, supplier, the
// results) is left for the inspector.
import type { Cell, Sheet } from '../model/xlsx.ts';
import { type Characteristic, inspectKey } from './characteristics.ts';

const round = (v: number) => Number(v.toFixed(6));

/** A characteristic's limits, where a nominal and its deviations give them: "7.95–8.05". */
function limits(c: Characteristic): string {
  if (c.basic || c.nominal === null || c.upper === null) return '';
  // A lower deviation not given is the upper's, below: ±upper (characteristics.ts's limits).
  const lower = c.lower ?? -c.upper;
  return `${round(c.nominal + lower)}–${round(c.nominal + c.upper)}`;
}

export interface FaiPart { name: string; number?: string; revision?: string; drawing?: string }

/** Forms 1–3, for ``rows`` (the inspection list, less those skipped) and the part's notes. */
export function as9102(part: FaiPart, rows: readonly Characteristic[], skipped: ReadonlySet<string>,
  notes: readonly { key: string; text: string }[]): Sheet[] {
  // STEP-aware rows use only stored/planned IDs; Material belongs on Form 2. Legacy hosts
  // retain origin/main's complete preview list, including sequentially ballooned part notes.
  const listed = rows.filter((r) => {
    const hasId = r.identifier !== undefined ? r.identifier !== '' : r.n > 0;
    return !skipped.has(inspectKey(r)) && (r.numbering !== 'step' || (hasId && r.source !== 'part:material'));
  });
  // Each form repeats the part it is about, as AS9102's do.
  const heading = (width: number): Cell[][] => [
    ['Part Number', part.number ?? '', 'Part Name', part.name, ...Array(Math.max(0, width - 4)).fill('')],
    ['Serial Number', '', 'FAI Report Number', '', ...Array(Math.max(0, width - 4)).fill('')],
  ];
  const form1: Sheet = {
    name: 'Form 1', header: 1, widths: [42, 48],
    rows: [
      ['Form 1: Part Number Accountability', ''],
      ['Part Number', part.number ?? ''], ['Part Name', part.name], ['Serial Number', ''], ['FAI Report Number', ''],
      ['Part Revision Level', part.revision ?? ''], ['Drawing Number', part.drawing ?? ''], ['Drawing Revision Level', ''],
      ['Additional Changes', ''], ['Manufacturing Process Reference', ''], ['Organization Name', ''], ['Supplier Code', ''],
      ['Purchase Order Number', ''], ['Detail / Assembly', 'Detail'], ['Full FAI / Partial FAI', 'Full FAI'],
      ['Reason for Partial FAI', ''], ['FAI Complete / Not Complete', ''], ['Signature / Date', ''], ['Reviewed By / Date', ''],
      ['Customer Approval / Date', ''], ['Comments', `Characteristics from the part's semantic PMI (STEP AP242), ${listed.length} listed.`],
    ],
  };
  const form2: Sheet = {
    name: 'Form 2', header: 4, widths: [44, 26, 22, 22, 22, 22],
    rows: [
      ['Form 2: Product Accountability — Raw Material, Specifications and Special Processes, Functional Testing', '', '', '', '', ''],
      ...heading(6),
      ['Material or Process Name', 'Specification Number', 'Code', 'Special Process Supplier Code', 'Customer Approval Verification', 'Certificate of Conformance Number'],
      // The part's material, finish and treatments as its notes state them; their specifications,
      // where a note names one, are in the name for the inspector to place.
      // Material and special processes (coating, heat treatment); finish, edges and the general
      // tolerance are requirements, on Form 3.
      ...notes.filter((n) => ['part:material', 'part:coating', 'part:heat_treatment'].includes(n.key))
        .map((n) => [n.text, '', '', '', '', '']),
      ['', '', '', '', '', ''],
      ['Functional Test Procedure Number', 'Acceptance Report Number', '', '', '', ''],
    ],
  };
  const form3: Sheet = {
    name: 'Form 3', header: 4, widths: [16, 30, 18, 34, 16, 16, 16, 30],
    rows: [
      ['Form 3: Characteristic Accountability, Verification and Compatibility Evaluation', '', '', '', '', '', '', ''],
      ...heading(8),
      ['Char. No.', 'Reference Location', 'Characteristic Designator', 'Requirement', 'Results', 'Designed / Qualified Tooling', 'Nonconformance Number', 'Limits / Comments'],
      ...listed.map((c) => [c.id, c.feature, '', c.text, '', '', '', [limits(c), c.datums ? `datums ${c.datums.split('|').join(' ')}` : '', c.basic ? 'basic' : ''].filter(Boolean).join('; ')]),
    ],
  };
  return [form1, form2, form3];
}
