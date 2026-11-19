// The PMI as a numbered list of characteristics, as an inspector balloons
// them: one balloon per requirement, and one row per feature it is on -- a
// pattern's four holes are 3.1 to 3.4 under balloon 3 -- with its nominal, its
// limits and what it is measured from.
// Pure, no DOM. Covers what is specified here, what the general tolerance
// covers (pmi-assist lists it, from the datums) and what the file states,
// ending with the part's notes -- material, general tolerance, finish -- which
// are inspected too. Numbers come from the STEP or the engine's planned allocation,
// so the display, inspection files and the STEP writer share the same IDs. Hosts without
// number metadata retain the original sequential preview balloons.
import type { PmiAnalysis, PmiIntent, PmiIntentItem } from '../types/pmi';

import { describeFaces } from './datums.ts';
import { CHARACTERISTIC, fmt } from './labels.ts';

export interface Characteristic {
  /** The numeric STEP/engine number, or a legacy balloon. Zero for a text ID or no ID. */
  readonly n: number;
  /** Which of the balloon's features it is, from 1; 0 when it has one. */
  readonly sub: number;
  /** Selection key, or numbered measurement ID ("3", "3.2"). Unnumbered keys are never exported. */
  readonly id: string;
  /** Exact ID_ATTRIBUTE string from the saved STEP (including leading zeros). */
  readonly identifier?: string;
  /** STEP numbering is explicit; absent for legacy hosts with sequential preview balloons. */
  readonly numbering?: 'step';
  /** The feature it is on: "6× Ø6.6 holes, hole 2", "top face". */
  readonly feature: string;
  /** What the whole balloon is on: "6× Ø6.6 holes". */
  readonly group: string;
  /** As a drawing writes it: "⌀8 H7", "⌖ ⌀0.6 Ⓜ", "M5×0.8-6H", "30 (basic)". */
  readonly text: string;
  readonly nominal: number | null;
  /** Upper and lower deviation, or for a tolerance zone its size as upper. */
  readonly upper: number | null;
  readonly lower: number | null;
  readonly datums: string;
  readonly basic: boolean;
  readonly faces: readonly number[];
  /** The question it answers, if any; and the label it is drawn as. */
  readonly question?: string;
  readonly source: string;
}

type Row = Omit<Characteristic, 'n' | 'sub' | 'id' | 'group'> & { group?: string; number?: number };

/** What a characteristic's text says when the file states it with no value. */
const NO_VALUE = 'no value stated';

/** The part's notes, in the order a drawing's notes block lists them. */
const PART_NOTES: readonly (readonly [string, string])[] = [
  ['material', 'Material'], ['general_tolerance', 'General tolerance'],
  ['surface_finish', 'Surface finish'], ['coating', 'Coating or treatment'],
  ['heat_treatment', 'Heat treatment'], ['edges', 'Edges'],
];

const num = (item: PmiIntentItem, key: string): number | null => {
  const v = item[key];
  return typeof v === 'number' ? v : null;
};
const str = (item: PmiIntentItem, key: string): string => {
  const v = item[key];
  return typeof v === 'string' ? v : '';
};
/** ``±0.05``, or ``+0.1/-0`` where the limits differ. */
const limits = (upper: number | null, lower: number | null): string => {
  if (upper === null) return '';
  if (lower === null || Math.abs(upper + lower) < 1e-9) return `±${fmt(upper)}`;
  return `${upper > 0 ? '+' : ''}${fmt(upper)}/${lower > 0 ? '+' : ''}${fmt(lower)}`;
};
const letters = (item: PmiIntentItem): string => {
  const v = item['datums'];
  return Array.isArray(v) ? v.join('|') : '';
};

/**
 * Every characteristic in ``intent`` and the file's own PMI, using stored or engine-planned
 * numbers when supplied. Legacy previews without metadata retain sequential balloons.
 * ``titles`` names features by id (a row's title).
 */
export function characteristics(
  intent: PmiIntent | null, analysis: PmiAnalysis, titles: ReadonlyMap<string, string>,
): Characteristic[] {
  const features = new Map(analysis.features.map((f) => [f.id, f]));
  const parentOf = new Map<string, { id: string; index: number; count: number }>();
  for (const f of analysis.features) {
    (f.members ?? []).forEach((m, i) => {
      parentOf.set(m, { id: f.id, index: i + 1, count: f.members?.length ?? 1 });
    });
  }
  // A feature with no question's title is named by its faces, never by Specify's id: a pattern,
  // or several features joined by commas, as "4× ⌀35 holes".
  const described = (id: string, faces: readonly number[]): string => {
    if (faces.length) return describeFaces(faces, analysis);
    const own = features.get(id);
    const parts = own?.members ?? (own ? [id] : id.split(','));
    const found = parts.flatMap((p) => {
      const f = features.get(p);
      return f?.faces.length ? [f.faces] : [];
    });
    if (!found.length) return own?.faces.length ? describeFaces(own.faces, analysis) : id;
    const names = [...new Set(found.map((f) => describeFaces(f, analysis)))];
    const [one] = names;
    if (names.length > 1 || !one) return names.join(', ');
    return found.length > 1 && /(hole|shaft|cylinder|face)$/.test(one) ? `${found.length}× ${one}s` : one;
  };
  const title = (id: string) => titles.get(id) ?? described(id, []);
  const name = (id: string, faces: readonly number[]): string => {
    const parent = parentOf.get(id);
    if (parent) return `${title(parent.id)}, hole ${parent.index}`;
    return titles.get(id) ?? described(id, faces);
  };
  // One requirement on a pattern is a characteristic of each of its holes.
  const instances = (item: PmiIntentItem): { feature: string; faces: readonly number[] }[] => {
    const id = str(item, 'feature');
    const members = features.get(id)?.members;
    if (!members?.length) return [{ feature: name(id, item.faces), faces: item.faces }];
    return members.flatMap((m) => {
      const own = item.faces.filter((f) => features.get(m)?.faces.includes(f));
      return own.length ? [{ feature: name(m, own), faces: own }] : [];
    });
  };

  const stepNumbers = intent?.partNumbers !== undefined
    || [...(intent?.datums ?? []), ...(intent?.requirements ?? []), ...(intent?.general ?? []), ...analysis.existing]
      .some((item) => item.number !== undefined);
  const rows: Row[] = [];
  for (const datum of intent?.datums ?? []) {
    if (!stepNumbers || datum.existing) continue;
    rows.push({
      source: `datum:${datum.letter}`, number: datum.number, question: datum.question,
      feature: describeFaces(datum.faces, analysis), faces: datum.faces, text: `Datum ${datum.letter}`,
      nominal: null, upper: null, lower: null, datums: '', basic: false,
    });
  }
  (intent?.requirements ?? []).forEach((r, i) => {
    // One Specify did not write (an answered requirement covers it) is not on the part.
    if (r['not_written_reason']) return;
    const source = `req:${i}`;
    const base = { source, number: r.number, ...(r.question ? { question: r.question } : {}) };
    const zone = (symbol: string) => {
      const t = num(r, 'tolerance');
      return {
        text: `${symbol} ${r['diametral'] ? '⌀' : ''}${t === null ? '' : fmt(t)}${r['mmc'] ? ' Ⓜ' : ''}`,
        nominal: null, upper: t, lower: null, datums: letters(r), basic: false,
      };
    };
    let what: Omit<Row, 'feature' | 'faces' | 'source' | 'question'> | null = null;
    if (r.kind === 'size') {
      const d = num(r, 'nominal');
      // A called-out size has a fit, or its limits.
      const fit = str(r, 'fit') || (str(r, 'feature') === 'added' ? limits(num(r, 'upper'), num(r, 'lower')) : '');
      what = {
        text: `⌀${d === null ? '' : fmt(d)} ${fit}`.trim(),
        nominal: d, upper: num(r, 'upper'), lower: num(r, 'lower'), datums: '', basic: false,
      };
    } else if (r.kind === 'location') {
      const d = num(r, 'distance');
      const basic = Boolean(r['basic']);
      what = {
        text: basic ? `${d === null ? '' : fmt(d)} (basic)` : `${d === null ? '' : fmt(d)} ${limits(num(r, 'upper'), num(r, 'lower'))}`,
        nominal: d, upper: basic ? null : num(r, 'upper'), lower: basic ? null : num(r, 'lower'),
        datums: '', basic,
      };
    } else if (r.kind === 'thread') {
      const spec = str(r, 'designation') ? `${str(r, 'designation')}×${fmt(num(r, 'pitch') ?? 0)}` : str(r, 'spec');
      what = { text: `${spec}-${str(r, 'class')}`, nominal: null, upper: null, lower: null, datums: '', basic: false };
    } else if (r.kind === 'knurl') {
      what = { text: `${str(r, 'pattern')} knurl`, nominal: null, upper: null, lower: null, datums: '', basic: false };
    } else if (r.kind === 'finish') {
      const ra = Number(str(r, 'value').replace(/^Ra\s*/i, ''));
      what = { text: `√ ${str(r, 'value')}`, nominal: null, upper: Number.isFinite(ra) ? ra : null, lower: null, datums: '', basic: false };
    } else if (r.kind && CHARACTERISTIC[r.kind]) {
      what = zone(CHARACTERISTIC[r.kind] as string);
    }
    if (!what) return;
    // A hole's basic locations are balloned with its pattern's.
    const id = str(r, 'feature');
    const parent = r.kind === 'location' ? parentOf.get(id) : undefined;
    const group = parent ? title(parent.id) : name(id, r.faces);
    for (const at of instances(r)) rows.push({ ...base, ...what, ...at, group });
  });

  // Basic dimensions after what is toleranced: they say where, not how closely.
  rows.sort((a, b) => Number(a.basic) - Number(b.basic));

  // Then what the general tolerance covers, with the ± it allows. A length
  // is from a datum, named by its letter, or else from a face.
  const referenceName = (faces: readonly number[]) => {
    const key = [...faces].sort((a, b) => a - b).join(',');
    const datum = intent?.datums.find((x) => [...x.faces].sort((a, b) => a - b).join(',') === key);
    return datum ? `datum ${datum.letter}` : describeFaces(faces, analysis);
  };
  (intent?.general ?? []).forEach((g, i) => {
    const source = `general:${i}`;
    const limits = { upper: num(g, 'upper'), lower: num(g, 'lower'), datums: '', basic: false };
    if (g.kind === 'size') {
      const d = num(g, 'nominal');
      for (const at of instances(g)) {
        rows.push({
          source, number: g.number, ...limits, ...at, text: `⌀${d === null ? '' : fmt(d)}`, nominal: d,
          group: name(str(g, 'feature'), g.faces),
        });
      }
    } else if (g.kind === 'location') {
      const d = num(g, 'distance');
      const from = Array.isArray(g['reference']) ? referenceName(g['reference'] as number[]) : '';
      rows.push({
        source, number: g.number, ...limits, feature: name(str(g, 'feature'), g.faces), faces: g.faces,
        text: `${d === null ? '' : fmt(d)}${from ? ` from ${from}` : ''}`, nominal: d,
      });
    }
  });

  // What the file already states, after what is specified here.
  const existingDatums = new Set<string>();
  analysis.existing.forEach((e, i) => {
    const faces = e.faces ?? [];
    if (!faces.length) return;
    const feature = describeFaces(faces, analysis);
    const source = `existing:${i}`;
    // A characteristic the file states without a value (CTC-01's presentation-only
    // dimensions) is listed as such, never as 0.
    if (e.kind === 'datum') {
      if (!stepNumbers || existingDatums.has(e.type)) return;
      existingDatums.add(e.type);
      rows.push({
        feature, faces, source, number: e.number, text: `Datum ${e.type}`,
        nominal: null, upper: null, lower: null, datums: '', basic: false,
      });
    } else if (e.kind === 'dimension') {
      const prefix = e.type.includes('Diameter') ? '⌀' : '';
      rows.push({
        feature, faces, source, number: e.number, text: e.value == null ? NO_VALUE : `${prefix}${fmt(e.value)}`, nominal: e.value ?? null,
        upper: e.upper ?? null, lower: e.lower ?? null, datums: '', basic: false,
      });
    } else if (e.kind === 'geometric_tolerance') {
      rows.push({
        feature, faces, source, number: e.number, text: `${CHARACTERISTIC[e.type] ?? e.type} ${e.value == null ? NO_VALUE : fmt(e.value)}`,
        nominal: null, upper: e.value ?? null, lower: null, datums: (e.datums ?? []).join('|'), basic: false,
      });
    } else {
      rows.push({ feature, faces, source, number: e.number, text: e.type, nominal: null, upper: null, lower: null, datums: '', basic: false });
    }
  });

  // The part's notes, as given here or as the file states them.
  const part = { ...analysis.part, ...intent?.part };
  for (const [key, name] of PART_NOTES) {
    const value = part[key];
    if (!value) continue;
    const ra = key === 'surface_finish' ? Number(value.replace(/^Ra\s*/i, '')) : Number.NaN;
    rows.push({
      feature: 'Part', faces: [], source: `part:${key}`, number: intent?.partNumbers?.[key], text: `${name}: ${value}`, nominal: null,
      upper: Number.isFinite(ra) ? ra : null, lower: null, datums: '', basic: false,
    });
  }
  if (!stepNumbers) {
    // Preserve origin/main's grouping, ordering and numbering for three-method hosts.
    const key = (r: Row | undefined) => (r?.basic ? `basic:${r.group}` : r?.source);
    const size = new Map<string | undefined, number>();
    for (const r of rows) size.set(key(r), (size.get(key(r)) ?? 0) + 1);
    const out: Characteristic[] = [];
    let n = 0;
    rows.forEach((r, i) => {
      const first = key(r) !== key(rows[i - 1]);
      if (first) n++;
      const sub = (size.get(key(r)) ?? 1) > 1 ? (first ? 1 : (out[out.length - 1]?.sub ?? 0) + 1) : 0;
      out.push({ ...r, n, sub, id: sub ? `${n}.${sub}` : `${n}`, group: r.group ?? r.feature });
    });
    return out;
  }
  // The engine assigns one stable number per semantic requirement. A pattern may have several
  // measured instances of that requirement. Unnumbered settings get internal selection keys,
  // while the display and files leave their number cell empty.
  const key = (r: Row) => r.number ? `number:${r.number}` : r.source;
  const size = new Map<string, number>();
  for (const row of rows) size.set(key(row), (size.get(key(row)) ?? 0) + 1);
  const instance = new Map<string, number>();
  const out: Characteristic[] = rows.map((row) => {
    const number = row.number ?? 0;
    const n = Number.isSafeInteger(number) && number > 0 ? number : 0;
    const count = (instance.get(key(row)) ?? 0) + 1;
    instance.set(key(row), count);
    const sub = (size.get(key(row)) ?? 1) > 1 ? count : 0;
    const id = n ? `${n}${sub ? `.${sub}` : ''}` : `unnumbered:${row.source}${sub ? `.${sub}` : ''}`;
    return { ...row, n, sub, id, numbering: 'step', group: row.group ?? row.feature };
  });
  out.sort((a, b) => (a.n || Infinity) - (b.n || Infinity));
  return out;
}

/** The rows under each balloon, in order. */
/**
 * The part's notes as a drawing's notes block states them, in PART_NOTES' order: the wording in
 * capitals, values (a material, ISO 2768-mK, Ra 3.2) as written, since their case is notation;
 * a setting that is none left out. ``question`` is the Specify question each answers.
 */
export function noteStatements(
  intent: PmiIntent | null, analysis: PmiAnalysis,
): { key: string; text: string; question: string }[] {
  const part = { ...analysis.part, ...intent?.part };
  const out: { key: string; text: string; question: string }[] = [];
  for (const [key] of PART_NOTES) {
    const value = part[key]?.trim();
    if (!value || /^(none|no edge note|no coating|no treatment|as machined|bare)\b/i.test(value)) continue;
    const label = STATEMENT[key];
    out.push({
      key: `part:${key}`, question: `part.${key}`,
      // Edges and treatments are said in words; the rest are a label and a value as written.
      text: label ? `${label} ${value}` : capitals(value),
    });
  }
  return out;
}

/**
 * Words in capitals, but notation as written: a word with a figure in it (10µm, H7/g6, 0.5mm),
 * and units and roughness parameters standing alone.
 */
const capitals = (text: string) => text.replace(/\S+/g, (w) =>
  (/\d/.test(w) || /^\(?(mm|µm|um|h|min|s|Ra|Rz)[,.;)]?$/.test(w) ? w : w.toUpperCase()));

/** The capitals a notes block puts before a value (none: the value is the note, in capitals). */
const STATEMENT: Readonly<Record<string, string>> = {
  material: 'MATERIAL', general_tolerance: 'GENERAL TOLERANCES', surface_finish: 'SURFACE FINISH',
};

export function byBalloon(rows: readonly Characteristic[]): Characteristic[][] {
  const out: Characteristic[][] = [];
  for (const r of rows) {
    const last = out[out.length - 1];
    if (r.n && last && last[0]?.n === r.n) last.push(r);
    else out.push([r]);
  }
  return out;
}

/** Numbers as a person reads a run: "3–8", "2, 5". */
export function ranges(numbers: readonly number[]): string {
  // A pattern's members share a balloon: each number once.
  const sorted = [...new Set(numbers)].sort((a, b) => a - b);
  const out: string[] = [];
  for (let i = 0; i < sorted.length; i++) {
    let j = i;
    while (j + 1 < sorted.length && (sorted[j + 1] ?? 0) === (sorted[j] ?? 0) + 1) j++;
    out.push(j > i ? `${sorted[i]}–${sorted[j]}` : `${sorted[i]}`);
    i = j;
  }
  return out.join(', ');
}

/** Balloon numbers by what they are drawn as: a label's source. */
export function balloonsBySource(rows: readonly Characteristic[]): Map<string, number[]> {
  const out = new Map<string, number[]>();
  for (const row of rows) {
    if (!row.n) continue;
    out.set(row.source, [...new Set([...(out.get(row.source) ?? []), row.n])]);
  }
  return out;
}

/**
 * The rows as CSV, one per feature to measure, a blank column to record what
 * was measured. The balloon and the feature under it are separate columns: a
 * spreadsheet would read "3.10" as 3.1.
 */
/**
 * A row as it is known from one preview to the next, whose numbering and
 * sources move as the PMI changes: what it is on and what it says.
 */
export const inspectKey = (r: Characteristic): string => `${r.group}|${r.feature}|${r.text}`;

export function toCsv(rows: readonly Characteristic[], skipped: ReadonlySet<string> = new Set()): string {
  const cell = (v: string | number | null) => {
    const text = v === null ? '' : String(v);
    return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
  };
  const head = ['Balloon', 'Instance', 'Feature', 'Characteristic', 'Nominal', 'Upper', 'Lower', 'Datums', 'Basic', 'Measured'];
  return [head, ...rows.filter((r) => !skipped.has(inspectKey(r))).map((r) => [
    r.identifier ?? (r.numbering === 'step' ? (r.n || '') : r.n), r.sub || '', r.feature, r.text, r.nominal, r.upper, r.lower, r.datums, r.basic ? 'yes' : '', '',
  ])].map((line) => line.map(cell).join(',')).join('\n') + '\n';
}
