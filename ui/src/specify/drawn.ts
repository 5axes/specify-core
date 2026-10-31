// A STEP's graphical PMI as haecceity draws it: `haecceity.presentation/1`, read and checked in
// the parts the Specify view uses, and refused with a reason otherwise. Pure, no three.js or DOM.
//
// What is drawn comes from haecceity as it is: polylines, filled triangles and text, never
// notation formatted here. Which faces (and Specify question) each semantic item is about comes
// with it, from Specify's pmiScene; until that exists the only source is a fixture opened through
// the dev path below.

export type DrawnPoint = readonly [number, number, number];

/** One drawn graphic: model-space millimetres. */
export interface DrawnGraphic {
  readonly source: 'generated' | 'file';
  readonly name: string;
  readonly polylines: readonly (readonly DrawnPoint[])[];
  readonly triangles: readonly (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[];
  readonly text: readonly string[];
}

export interface Presentation {
  /** Graphics by the semantic item they present. */
  readonly items: Readonly<Record<string, readonly DrawnGraphic[]>>;
  /** Graphics linked to no semantic item: drawn, not selectable. */
  readonly unlinked: readonly DrawnGraphic[];
  readonly semanticIds: readonly string[];
}

/** What a semantic item is on, in the session's face numbering, and its question if Specify asks one. */
export interface DrawnLink {
  readonly faces: readonly number[];
  readonly question?: string;
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
  return { source, name, polylines, triangles, text };
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
 * Links for a fixture, from its `haecceity.pmi/1` document: each item's faces are its features'
 * (`features[].value.items[].face`, the session's numbering for a file Specify reads). pmiScene
 * will give these itself, with each item's question. Refused unless the document's ids are the
 * presentation's `semantic_ids`.
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

/** A same-origin path's JSON, gunzipped if it is gzip. */
async function fetchJson(path: string): Promise<unknown> {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  const gzip = bytes[0] === 0x1f && bytes[1] === 0x8b;
  const text = gzip
    ? await new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'))).text()
    : new TextDecoder().decode(bytes);
  return JSON.parse(text);
}

/**
 * The dev path, until pmiScene: `?presentation=<path>&pmi=<path>` on the page, both same-origin
 * paths to JSON (gzipped or not). Without `presentation` there is nothing, and nothing changes.
 * Without `pmi`, graphics are drawn but none is linked to faces.
 */
export async function devDrawnPmi(search: string): Promise<Read<DrawnPmi> | null> {
  const query = new URLSearchParams(search);
  const path = query.get('presentation');
  if (!path) return null;
  const local = (p: string) => p.startsWith('/') && !p.startsWith('//');
  if (!local(path)) return { ok: false, reason: 'presentation must be a same-origin path' };
  try {
    const read = readPresentation(await fetchJson(path));
    if (!read.ok) return read;
    const pmiPath = query.get('pmi');
    if (!pmiPath) return { ok: true, value: { presentation: read.value, links: {} } };
    if (!local(pmiPath)) return { ok: false, reason: 'pmi must be a same-origin path' };
    const links = linksFromPmi(await fetchJson(pmiPath), read.value);
    return links.ok ? { ok: true, value: { presentation: read.value, links: links.value } } : links;
  } catch (error) {
    return { ok: false, reason: error instanceof Error ? error.message : String(error) };
  }
}
