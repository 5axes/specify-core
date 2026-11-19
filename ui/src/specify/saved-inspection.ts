import type { PmiScene, PmiSceneItem } from '@draftwright/specify-core-wasm';
import type { PmiAnalysis } from '../types/pmi';
import type { Characteristic } from './characteristics';
import { describeFaces } from './datums.ts';
import { CHARACTERISTIC, fmt } from './labels.ts';

export interface StoredInspection {
  chars: Characteristic[];
  notes: { key: string; text: string }[];
}

type ObjectValue = Record<string, unknown>;
const object = (value: unknown): ObjectValue => value && typeof value === 'object' ? value as ObjectValue : {};
const list = (value: unknown): unknown[] => Array.isArray(value) ? value : [];
const text = (value: unknown): string => typeof value === 'string' ? value : '';

/** Read the stored semantic scene. No answer review, allocation or default acceptance occurs. */
export function inspectionFromStep(analysis: PmiAnalysis, scene: PmiScene): StoredInspection {
  const pmi = scene.pmi.pmi;
  const entries = Object.values(pmi).flatMap((value) => Array.isArray(value) ? value : [])
    .filter((value): value is PmiSceneItem => typeof object(value)['id'] === 'string' && 'value' in object(value));
  const byId = new Map(entries.map((entry) => [entry.id, entry.value]));
  const features = new Map(pmi.features.map((entry) => [entry.id, entry.value]));
  const facesOf = (value: unknown, seen: ReadonlySet<string> = new Set()): number[] => {
    if (typeof value === 'string') {
      if (seen.has(value)) return [];
      return facesOf(features.get(value), new Set([...seen, value]));
    }
    const face = object(value)['face'];
    if (typeof face === 'number') return [face];
    // A semantic feature may be an items list or a group of other features.
    return [...new Set((Array.isArray(value) ? value : Object.values(object(value)))
      .flatMap((item) => facesOf(item, seen)))].sort((a, b) => a - b);
  };
  const unitFactors = new Map<string, number>();
  const factors: Record<string, number> = {
    millimetre: 1, metre: 1000, centimetre: 10, micrometre: 0.001,
    inch: 25.4, foot: 304.8, degree: 1, radian: 180 / Math.PI,
  };
  for (const entry of [...scene.pmi.units.length, ...scene.pmi.units.angle]) {
    const factor = factors[text(entry.value)];
    if (factor !== undefined) unitFactors.set(entry.id, factor);
  }
  const quantity = (value: unknown): number | null => {
    if (value == null) return null;
    const row = object(value);
    if ('quantity' in row) {
      const q = object(row['quantity']);
      return quantity(q['length'] ?? q['angle']);
    }
    if ('value' in row) {
      const number = quantity(row['value']);
      const unit = text(row['unit']);
      if (number === null || (unit && !unitFactors.has(unit))) return null;
      return number * (unitFactors.get(unit) ?? 1);
    }
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
  };
  const chars: Characteristic[] = [];
  const notes: StoredInspection['notes'] = Object.entries(scene.specify).flatMap(([id, meta]) => {
    if (!id.startsWith('part.') || !meta.notation) return [];
    return [{ key: id.replace(/^part\./, 'part:'), text: meta.notation }];
  });
  if (!notes.some((note) => note.key === 'part:material')) {
    const material = object(pmi.material);
    const name = text(material['name']) || text(material['id']);
    if (name) notes.push({ key: 'part:material', text: `MATERIAL ${name}` });
  }
  const add = (entry: PmiSceneItem, collection: string) => {
    const value = object(entry.value);
    const meta = scene.specify[entry.id];
    // identifier is the ID_ATTRIBUTE actually read from the STEP, including foreign IDs.
    const identifier = entry.identifier ?? (meta?.number === undefined ? '' : String(meta.number));
    // The writer emits canonical positive u64 IDs, but the semantic reader also preserves
    // imported decimal strings such as 009. Keep that exact string for both exports.
    const numeric = /^\+?\d+$/.test(identifier) ? BigInt(identifier) : 0n;
    const number = numeric > 0n && numeric <= 18446744073709551615n ? Number(numeric) : 0;
    // A location measures the target; its "from" feature is a separate reference, not
    // another measured instance. This matches the preview and stored characteristic support.
    const location = object(object(value['kind'])['location']);
    const on = facesOf(collection === 'dimensions' && 'to' in location ? location['to'] : value);
    let nominal: number | null = null;
    let upper: number | null = null;
    let lower: number | null = null;
    let basic = false;
    let datums = '';
    let wording = meta?.notation ?? '';
    if (collection === 'datums') wording ||= `Datum ${text(value['label'])}`;
    if (collection === 'dimensions') {
      nominal = quantity(value['nominal']);
      const tolerance = object(value['tolerance']);
      const deviations = object(tolerance['deviations']);
      const limits = object(tolerance['limits']);
      upper = quantity(deviations['upper']);
      lower = quantity(deviations['lower']);
      if ('limits' in tolerance && nominal !== null) {
        const high = quantity(limits['upper']);
        const low = quantity(limits['lower']);
        upper = high === null ? null : high - nominal;
        lower = low === null ? null : low - nominal;
      }
      basic = value['tolerance'] === 'basic' || meta?.basic === true;
      const kind = object(value['kind']);
      const size = object(kind['size']);
      const prefix = size['kind'] === 'diameter' ? '⌀' : '';
      wording ||= nominal === null ? 'no value stated' : `${prefix}${fmt(nominal)}`;
      if (upper !== null && !meta?.notation) wording += ` +${fmt(upper)}/${fmt(lower ?? -upper)}`;
      if (basic && !wording.includes('basic')) wording += ' (basic)';
      const reference = facesOf(object(kind['location'])['from']);
      if (reference.length) wording += ` from ${describeFaces(reference, analysis)}`;
    }
    if (collection === 'tolerances') {
      upper = quantity(value['magnitude']);
      datums = list(value['datums']).flatMap((system) => list(object(system)['references']).map((reference) => {
        const id = text(object(reference)['datum']);
        return text(object(byId.get(id))['label']) || id;
      })).join('|');
      const kind = text(value['kind']).replace(/_tolerance$/, '');
      const symbol = CHARACTERISTIC[kind] ?? CHARACTERISTIC[kind[0]?.toUpperCase() + kind.slice(1)] ?? kind;
      const zone = object(value['zone']);
      const diameter = zone['form'] === 'cylindrical or circular' ? '⌀' : '';
      const mmc = list(value['modifiers']).includes('maximum_material_requirement') ? ' Ⓜ' : '';
      wording ||= `${symbol} ${diameter}${upper === null ? 'no value stated' : fmt(upper)}${mmc}`;
    }
    if (collection === 'threads' || collection === 'knurls') {
      const manufacturing = Object.entries(scene.specify).find(([id, item]) =>
        id.startsWith(`attribute:${entry.id}:`) && item.notation);
      wording ||= manufacturing?.[1].notation || text(value['qualifier']) || (collection === 'threads' ? 'Thread' : 'Knurl');
    }
    if (collection === 'surface_textures') {
      wording ||= list(value['parameters']).map((parameter) => {
        const row = object(parameter);
        const number = quantity(row['value']);
        return `${text(row['characteristic'])} ${number === null ? 'no value stated' : fmt(number)} mm`;
      }).join(', ');
    }
    const feature = on.length ? describeFaces(on, analysis) : 'Part';
    chars.push({
      n: number, sub: 0, numbering: 'step', identifier: number ? identifier : '', id: number ? identifier : `unnumbered:${entry.id}`,
      source: entry.id, feature, group: feature, faces: on, text: wording,
      nominal, upper, lower, datums, basic,
    });
  };
  for (const collection of ['datums', 'dimensions', 'tolerances', 'surface_textures', 'threads', 'knurls'] as const) {
    for (const entry of pmi[collection]) add(entry, collection);
  }
  chars.sort((a, b) => (a.n || Infinity) - (b.n || Infinity));
  return { chars, notes };
}
