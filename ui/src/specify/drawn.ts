// A STEP's graphical PMI as haecceity draws it: `haecceity.presentation/1`, read and checked in
// the parts the Specify view uses, and refused with a reason otherwise. Pure, no three.js or DOM.
//
// What is drawn comes from haecceity as it is: polylines, filled triangles and text, never
// notation formatted here. It comes with Specify's pmiScene (specify-core 0.1.5): the presentation,
// the haecceity.pmi/1 document whose features give each item's faces, and Specify's overlay, which
// gives each item's status and question.

export type DrawnPoint = readonly [number, number, number];

/**
 * Where a text is set, as the file or haecceity states it: its alignment point, its plane's x
 * axis and normal (the text's up is normal × x axis), and its box height in mm when stated.
 */
export interface TextPlacement {
  readonly textIndex?: number;
  readonly origin: DrawnPoint;
  readonly xAxis: DrawnPoint;
  readonly normal: DrawnPoint;
  readonly height?: number;
  /** As stated, such as "baseline left" or "centre". */
  readonly alignment?: string;
}

/** One drawn graphic: model-space millimetres. */
export interface DrawnGraphic {
  /** One graphic listed under several items (a datum's symbol under the datum and its feature) has one id. */
  readonly id?: string;
  readonly source: 'generated' | 'file';
  readonly name: string;
  readonly polylines: readonly (readonly DrawnPoint[])[];
  readonly triangles: readonly (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[];
  readonly text: readonly string[];
  readonly textPlacements?: readonly TextPlacement[];
}

export interface Presentation {
  /** Graphics by the semantic item they present. */
  readonly items: Readonly<Record<string, readonly DrawnGraphic[]>>;
  /** Graphics linked to no semantic item: drawn, not selectable. */
  readonly unlinked: readonly DrawnGraphic[];
  readonly semanticIds: readonly string[];
}

export type DrawnStatus = 'answered' | 'default' | 'general' | 'existing';

/**
 * What a semantic item is on, in the session's face numbering (`snapshot().mesh.faces`), and,
 * from Specify's overlay, its status and its question if Specify asks one.
 */
export interface DrawnLink {
  readonly faces: readonly number[];
  readonly question?: string;
  readonly status?: DrawnStatus;
}

/** What the Specify view draws as the file's own graphics: pmiScene's presentation and links. */
export interface DrawnPmi {
  readonly presentation: Presentation;
  readonly links: Readonly<Record<string, DrawnLink>>;
}

export type Read<T> = { ok: true; value: T } | { ok: false; reason: string };

const SCHEMA = 'haecceity.presentation/1';
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

class Refused extends Error {}
const refuse = (reason: string): never => { throw new Refused(reason); };

function point(v: unknown, at: string): DrawnPoint {
  if (!Array.isArray(v) || v.length !== 3 || !v.every((n) => typeof n === 'number' && Number.isFinite(n))) {
    return refuse(`${at} is not a point [x, y, z]`);
  }
  return [v[0], v[1], v[2]];
}

function list(v: unknown, at: string): unknown[] {
  return Array.isArray(v) ? v : refuse(`${at} is not an array`);
}

function graphic(v: unknown, at: string): DrawnGraphic {
  if (!isObject(v)) return refuse(`${at} is not a graphic`);
  const { source, name } = v;
  if (source !== 'file' && source !== 'generated') return refuse(`${at}.source is ${JSON.stringify(source)}, not "file" or "generated"`);
  if (typeof name !== 'string') return refuse(`${at}.name is not a string`);
  const polylines = list(v['polylines'], `${at}.polylines`)
    .map((line, i) => list(line, `${at}.polylines[${i}]`).map((p, j) => point(p, `${at}.polylines[${i}][${j}]`)));
  const triangles = list(v['triangles'], `${at}.triangles`).map((t, i) => {
    const corners = list(t, `${at}.triangles[${i}]`);
    if (corners.length !== 3) refuse(`${at}.triangles[${i}] does not have 3 corners`);
    return [point(corners[0], `${at}.triangles[${i}][0]`), point(corners[1], `${at}.triangles[${i}][1]`), point(corners[2], `${at}.triangles[${i}][2]`)] as const;
  });
  const text = list(v['text'], `${at}.text`).map((s, i) => (typeof s === 'string' ? s : refuse(`${at}.text[${i}] is not a string`)));
  const id = v['id'];
  if (id !== undefined && typeof id !== 'string') refuse(`${at}.id is not a string`);
  const placements = v['text_placements'] === undefined ? undefined
    : list(v['text_placements'], `${at}.text_placements`).map((t, i) => placement(t, `${at}.text_placements[${i}]`));
  return { ...(typeof id === 'string' ? { id } : {}), source, name, polylines, triangles, text, ...(placements ? { textPlacements: placements } : {}) };
}

function placement(v: unknown, at: string): TextPlacement {
  if (!isObject(v)) return refuse(`${at} is not a text placement`);
  const { text_index: textIndex, height, alignment } = v;
  if (textIndex !== undefined && !(Number.isInteger(textIndex) && (textIndex as number) >= 0)) refuse(`${at}.text_index is not an index`);
  if (height !== undefined && !(typeof height === 'number' && Number.isFinite(height) && height >= 0)) refuse(`${at}.height is not a length`);
  if (alignment !== undefined && typeof alignment !== 'string') refuse(`${at}.alignment is not a string`);
  return {
    ...(textIndex === undefined ? {} : { textIndex: textIndex as number }),
    origin: point(v['origin'], `${at}.origin`), xAxis: point(v['x_axis'], `${at}.x_axis`), normal: point(v['normal'], `${at}.normal`),
    ...(height === undefined ? {} : { height: height as number }),
    ...(alignment === undefined ? {} : { alignment: alignment as string }),
  };
}

/** `haecceity.presentation/1`, checked where the view relies on it; anything else is refused with why. */
export function readPresentation(json: unknown): Read<Presentation> {
  try {
    if (!isObject(json)) return refuse('not a JSON object');
    if (json['schema'] !== SCHEMA) return refuse(`schema is ${JSON.stringify(json['schema'] ?? null)}, not "${SCHEMA}"`);
    if (json['length_unit'] !== 'millimetre') return refuse(`length_unit is ${JSON.stringify(json['length_unit'] ?? null)}, not "millimetre"`);
    const rawItems = json['items'];
    if (!isObject(rawItems)) return refuse('items is not an object');
    const items: Record<string, readonly DrawnGraphic[]> = {};
    for (const [id, graphics] of Object.entries(rawItems)) {
      items[id] = list(graphics, `items[${JSON.stringify(id)}]`).map((g, i) => graphic(g, `items[${JSON.stringify(id)}][${i}]`));
    }
    const unlinked = list(json['unlinked'], 'unlinked').map((g, i) => graphic(g, `unlinked[${i}]`));
    const semanticIds = list(json['semantic_ids'], 'semantic_ids')
      .map((s, i) => (typeof s === 'string' ? s : refuse(`semantic_ids[${i}] is not a string`)));
    return { ok: true, value: { items, unlinked, semanticIds } };
  } catch (error) {
    if (error instanceof Refused) return { ok: false, reason: error.message };
    throw error;
  }
}

/**
 * Each presented item's faces, from the `haecceity.pmi/1` document the presentation was made with:
 * its features' (`features[].value.items[].face`, the session's numbering). Refused unless the
 * document's ids are the presentation's `semantic_ids`.
 */
export function linksFromPmi(json: unknown, presentation: Presentation): Read<Record<string, DrawnLink>> {
  if (!isObject(json) || json['schema'] !== 'haecceity.pmi/1' || !isObject(json['pmi'])) {
    return { ok: false, reason: 'not a haecceity.pmi/1 document' };
  }
  // Every identified item: the document's collections and its units.
  const collections = [...Object.values(json['pmi']), ...(isObject(json['units']) ? Object.values(json['units']) : [])];
  const entries = collections.flatMap((v) => (Array.isArray(v) ? v : []))
    .filter((e): e is { id: string; value: unknown } => isObject(e) && typeof e['id'] === 'string');
  const faces = new Map<string, number[]>();
  for (const e of entries) {
    if (!e.id.startsWith('features:') || !isObject(e.value)) continue;
    const on = Array.isArray(e.value['items']) ? e.value['items'] : [];
    faces.set(e.id, on.flatMap((x) => (isObject(x) && typeof x['face'] === 'number' ? [x['face']] : [])));
  }
  // The presentation's links hold only for the document it was made with: the same ids.
  const ids = [...new Set(entries.map((e) => e.id))].sort();
  const expected = [...presentation.semanticIds].sort();
  if (ids.join('\n') !== expected.join('\n')) return { ok: false, reason: 'the PMI document is not the one the presentation was made with (semantic_ids differ)' };
  // An item's faces: those of every feature it names, however deep.
  const named = (v: unknown, into: Set<number>): Set<number> => {
    if (typeof v === 'string') for (const f of faces.get(v) ?? []) into.add(f);
    else if (Array.isArray(v)) for (const x of v) named(x, into);
    else if (isObject(v)) for (const x of Object.values(v)) named(x, into);
    return into;
  };
  const links: Record<string, DrawnLink> = {};
  for (const e of entries) {
    if (!(e.id in presentation.items)) continue;
    links[e.id] = { faces: [...named(e.id.startsWith('features:') ? e.id : e.value, new Set())].sort((a, b) => a - b) };
  }
  return { ok: true, value: links };
}

const STATUSES: readonly DrawnStatus[] = ['answered', 'default', 'general', 'existing'];

/**
 * Specify's pmiScene (`{pmi, presentation, specify}`, specify-core 0.1.5) as the view draws it:
 * the presentation, and each presented item's faces with its status and question from `specify`.
 */
export function drawnFromScene(scene: unknown): Read<DrawnPmi> {
  if (!isObject(scene)) return { ok: false, reason: 'the PMI scene is not a JSON object' };
  const read = readPresentation(scene['presentation']);
  if (!read.ok) return read;
  const links = linksFromPmi(scene['pmi'], read.value);
  if (!links.ok) return links;
  const overlay = isObject(scene['specify']) ? scene['specify'] : {};
  const linked: Record<string, DrawnLink> = {};
  for (const [id, link] of Object.entries(links.value)) {
    const entry = overlay[id];
    const status = isObject(entry) && STATUSES.includes(entry['status'] as DrawnStatus) ? entry['status'] as DrawnStatus : undefined;
    const question = isObject(entry) && typeof entry['question'] === 'string' ? entry['question'] : undefined;
    linked[id] = { ...link, ...(question ? { question } : {}), ...(status ? { status } : {}) };
  }
  return { ok: true, value: { presentation: read.value, links: linked } };
}

/** Whether there is anything to draw. */
export const hasGraphics = (pmi: DrawnPmi): boolean =>
  pmi.presentation.unlinked.length > 0 || Object.values(pmi.presentation.items).some((graphics) => graphics.length > 0);
