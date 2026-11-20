// What to write on the part: pmi-assist intent and existing PMI as label
// contents. Pure, no three.js or DOM.
//
// Ported from pmi-assist-viewer (picker/labels.js), whose label vocabulary is
// step-pmi-viewer's: a feature control frame is a row of cells, a datum a boxed
// letter, a size a value with a stacked or symmetric tolerance.
import type { PmiAnalysis, PmiIntent, PmiIntentItem, PmiVec } from '../types/pmi';

export type LabelGroup = 'answered' | 'default' | 'existing' | 'general';

export interface LabelTolerance {
  readonly style: 'symmetric' | 'pair';
  readonly upper: string;
  readonly lower?: string;
}

/** A length: from a reference plane, along its normal, to faces or a hole's axis. */
export interface LabelSpan {
  readonly reference: readonly number[];
  readonly distance: number;
  /** A point on the hole's axis it ends at; else it ends on its faces. */
  readonly axis?: PmiVec;
}

export interface PmiLabel {
  readonly id: string;
  readonly kind: 'frame' | 'datum' | 'size' | 'note' | 'dimension';
  readonly cells: readonly string[];
  readonly faces: readonly number[];
  readonly group: LabelGroup;
  readonly question?: string;
  readonly count?: number;
  readonly tolerance?: LabelTolerance | null;
  readonly span?: LabelSpan;
}

/** Geometric characteristic symbols, by pmi-assist kind and by OCCT type name. */
export const CHARACTERISTIC: Readonly<Record<string, string>> = {
  position: '⌖', flatness: '⏥', perpendicularity: '⟂', parallelism: '∥',
  profile: '⌓', runout: '↗', total_runout: '⌰',
  Position: '⌖', Flatness: '⏥', Straightness: '—', Circularity: '○', Cylindricity: '⌭',
  ProfileOfLine: '⌒', ProfileOfSurface: '⌓', Perpendicularity: '⟂', Parallelism: '∥',
  Angularity: '∠', Concentricity: '◎', Symmetry: '⌯', CircularRunout: '↗', TotalRunout: '↗',
};

const DIAMETER = '⌀';

/** A number as a drawing writes it: no trailing zeros, at most 4 decimals. */
export function fmt(value: number): string {
  return Number(value.toFixed(4)).toString();
}

function deviation(value: number): string {
  return value > 0 ? `+${fmt(value)}` : value < 0 ? `-${fmt(-value)}` : '0';
}

function tolerance(upper: unknown, lower: unknown): LabelTolerance | null {
  if (typeof upper !== 'number' || typeof lower !== 'number') return null;
  if (Math.abs(upper + lower) < 1e-12) return { style: 'symmetric', upper: `±${fmt(upper)}` };
  return { style: 'pair', upper: deviation(upper), lower: deviation(lower) };
}

const text = (item: PmiIntentItem, key: string): string => {
  const value = item[key];
  return typeof value === 'string' ? value : '';
};
const num = (item: PmiIntentItem, key: string): number => {
  const value = item[key];
  return typeof value === 'number' ? value : 0;
};

/**
 * Labels for ``intent`` (pmi-assist's preview) and the PMI the file already
 * has. Pattern requirements carry their member count. A default is drawn as a
 * draft only while its question still waits for a person (``pending``): one that
 * needs no one is accepted as it stands and written so, and looks it.
 */
export function toLabels(
  intent: PmiIntent | null, analysis: PmiAnalysis, pending?: ReadonlySet<string>,
): PmiLabel[] {
  const members = new Map(
    analysis.features.filter((f) => f.members).map((f) => [f.id, f.members?.length ?? 1]),
  );
  const out: PmiLabel[] = [];
  const group = (item: PmiIntentItem): LabelGroup => (
    item.default && (!pending || (item.question !== undefined && pending.has(item.question)))
      ? 'default' : 'answered');

  for (const d of intent?.datums ?? []) {
    // The file's own datums are drawn with what the file states, below.
    if (d.existing) continue;
    out.push({
      id: `datum:${d.letter}`, kind: 'datum', cells: [d.letter], faces: d.faces,
      group: group(d), ...(d.question ? { question: d.question } : {}),
    });
  }
  (intent?.requirements ?? []).forEach((r, i) => {
    // One Specify did not write (an answered requirement covers it) is not on the part.
    if (r['not_written_reason']) return;
    const base = {
      id: `req:${i}:${r.question ?? ''}`, faces: r.faces, group: group(r),
      ...(r.question ? { question: r.question } : {}),
      count: members.get(text(r, 'feature')) ?? 1,
    };
    if (r.kind === 'size') {
      const fit = text(r, 'fit');
      out.push({
        ...base, kind: 'size',
        cells: [`${DIAMETER}${fmt(num(r, 'nominal'))}`, ...(fit ? [fit] : [])],
        tolerance: tolerance(r['upper'], r['lower']),
      });
    } else if (r.kind === 'location' && text(r, 'feature') === 'added' && Array.isArray(r['reference'])) {
      // A distance called out: drawn as the general one it replaces, to a hole's axis if it
      // runs to one -- at the hole's own location, as a general one is.
      const first = r.faces[0];
      const at = analysis.features.find((f) => f.family === 'holes' && first !== undefined && f.faces.includes(first))
        ?.record['location'];
      const axis = Array.isArray(at) && at.length === 3 ? at as unknown as PmiVec : undefined;
      out.push({
        ...base, kind: 'dimension', cells: [fmt(num(r, 'distance'))],
        tolerance: tolerance(r['upper'], r['lower']),
        span: {
          reference: r['reference'] as number[], distance: num(r, 'distance'),
          ...(axis ? { axis } : {}),
        },
      });
    } else if (r.kind === 'thread') {
      // M5×0.8-6H, as a drawing calls it.
      const designation = text(r, 'designation');
      const spec = designation ? `${designation}×${fmt(num(r, 'pitch'))}` : text(r, 'spec').replace('x', '×');
      const cls = text(r, 'class');
      out.push({ ...base, kind: 'note', cells: [cls ? `${spec}-${cls}` : spec] });
    } else if (r.kind === 'knurl') {
      const pattern = text(r, 'pattern');
      out.push({
        ...base, kind: 'note',
        cells: [`${pattern.charAt(0).toUpperCase()}${pattern.slice(1)} knurl P${fmt(num(r, 'pitch'))}`],
      });
    } else if (r.kind === 'finish') {
      out.push({ ...base, kind: 'note', cells: [`√ ${text(r, 'value')}`] });
    } else if (r.kind && CHARACTERISTIC[r.kind]) {
      const datums = Array.isArray(r['datums']) ? (r['datums'] as string[]) : [];
      out.push({
        ...base, kind: 'frame',
        cells: [
          CHARACTERISTIC[r.kind] ?? r.kind,
          `${r['diametral'] ? DIAMETER : ''}${fmt(num(r, 'tolerance'))}${r['mmc'] ? ' Ⓜ' : ''}`,
          ...datums,
        ],
      });
    }
  });

  // What the general tolerance covers: diameters as sizes, lengths and hole
  // locations as dimensions from their reference plane.
  const features = new Map(analysis.features.map((f) => [f.id, f]));
  (intent?.general ?? []).forEach((g, i) => {
    const base = {
      id: `general:${i}`, faces: g.faces, group: 'general' as const,
      tolerance: tolerance(g['upper'], g['lower']),
    };
    if (g.kind === 'size') {
      out.push({
        ...base, kind: 'size', cells: [`${DIAMETER}${fmt(num(g, 'nominal'))}`],
        count: members.get(text(g, 'feature')) ?? 1,
      });
    } else if (g.kind === 'location' && Array.isArray(g['reference'])) {
      const record = features.get(text(g, 'feature'))?.record;
      const at = record?.['location'];
      const axis = Array.isArray(at) && at.length === 3 ? at as unknown as PmiVec : undefined;
      out.push({
        ...base, kind: 'dimension', cells: [fmt(num(g, 'distance'))],
        span: {
          reference: g['reference'] as number[], distance: num(g, 'distance'),
          ...(axis ? { axis } : {}),
        },
      });
    }
  });

  // What the file already states. A datum is read back once per tolerance
  // that refers to it, so letters are shown once.
  const seen = new Set<string>();
  analysis.existing.forEach((e, i) => {
    const faces = e.faces ?? [];
    if (!faces.length) return;
    const base = { id: `existing:${i}`, faces, group: 'existing' as const };
    if (e.kind === 'datum') {
      if (seen.has(e.type)) return;
      seen.add(e.type);
      out.push({ ...base, kind: 'datum', cells: [e.type] });
    } else if (e.kind === 'geometric_tolerance') {
      out.push({
        ...base, kind: 'frame',
        // A tolerance the file states with no value is drawn without one, never as 0.
        cells: [CHARACTERISTIC[e.type] ?? e.type, ...(e.value == null ? [] : [fmt(e.value)]), ...(e.datums ?? [])],
      });
    } else if (e.kind === 'note') {
      out.push({ ...base, kind: 'note', cells: [e.type] });
    } else if (e.kind === 'dimension' && e.value != null) {
      const prefix = e.type.includes('Diameter') ? DIAMETER : '';
      out.push({
        ...base, kind: 'size', cells: [`${prefix}${fmt(e.value)}`],
        tolerance: tolerance(e.upper, e.lower),
      });
    }
  });
  return out;
}
