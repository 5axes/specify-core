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
  /**
   * The cell the text is set in, as haecceity lays it out (0.4.4): its corners [min, max] in the
   * plane's x and up, in mm from ``origin``, such as a feature control frame's compartment.
   */
  readonly cellBox?: readonly [readonly [number, number], readonly [number, number]];
  /** Whether haecceity also strokes this text into the graphic's polylines (0.4.4). */
  readonly stroked?: boolean;
}

/** What a polyline draws, as haecceity says (0.4.4): "text" (its text's strokes), "frame", "leader", "extension", "dimension" or "symbol". */
export type PolylineRole = string;

/** One drawn graphic: model-space millimetres. */
export interface DrawnGraphic {
  /** One graphic listed under several items (a datum's symbol under the datum and its feature) has one id. */
  readonly id?: string;
  readonly source: 'generated' | 'file';
  readonly name: string;
  readonly polylines: readonly (readonly DrawnPoint[])[];
  /** Each polyline's role, parallel to ``polylines``, when haecceity states them (0.4.4). */
  readonly polylineRoles?: readonly PolylineRole[];
  readonly triangles: readonly (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[];
  readonly text: readonly string[];
  readonly textPlacements?: readonly TextPlacement[];
  /** The annotation plane it is drawn on: an id in the presentation's ``planes``. */
  readonly planeId?: string;
}

/** An annotation plane as the file or haecceity states it: the in-plane y axis is normal × x axis. */
export interface DrawnPlane {
  readonly id: string;
  readonly name: string;
  readonly origin: DrawnPoint;
  readonly xAxis: DrawnPoint;
  readonly normal: DrawnPoint;
}

/** A saved view: its camera's direction (into the scene) and up when the file has a camera, and its graphics by id. */
export interface SavedView {
  readonly id: string;
  readonly name: string;
  readonly direction?: DrawnPoint;
  readonly up?: DrawnPoint;
  readonly graphics: readonly string[];
}

export interface Presentation {
  /** Graphics by the semantic item they present. */
  readonly items: Readonly<Record<string, readonly DrawnGraphic[]>>;
  /** Graphics linked to no semantic item: drawn, not selectable. */
  readonly unlinked: readonly DrawnGraphic[];
  readonly semanticIds: readonly string[];
  readonly planes: readonly DrawnPlane[];
  readonly views: readonly SavedView[];
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
  const planeId = v['plane_id'];
  if (planeId !== undefined && typeof planeId !== 'string') refuse(`${at}.plane_id is not a string`);
  const rawRoles = v['polyline_roles'];
  const roles = rawRoles === undefined ? undefined
    : list(rawRoles, `${at}.polyline_roles`).map((r, i) => (typeof r === 'string' ? r : refuse(`${at}.polyline_roles[${i}] is not a string`)));
  if (roles && roles.length !== polylines.length) refuse(`${at}.polyline_roles has ${roles.length} roles for ${polylines.length} polylines`);
  return {
    ...(typeof id === 'string' ? { id } : {}), source, name, polylines, ...(roles ? { polylineRoles: roles } : {}), triangles, text,
    ...(placements ? { textPlacements: placements } : {}), ...(typeof planeId === 'string' ? { planeId } : {}),
  };
}

function plane(v: unknown, at: string): DrawnPlane {
  if (!isObject(v)) return refuse(`${at} is not a plane`);
  const { id, name } = v;
  if (typeof id !== 'string') return refuse(`${at}.id is not a string`);
  return {
    id, name: typeof name === 'string' ? name : id,
    origin: point(v['origin'], `${at}.origin`), xAxis: point(v['x_axis'], `${at}.x_axis`), normal: point(v['normal'], `${at}.normal`),
  };
}

function savedView(v: unknown, at: string): SavedView {
  if (!isObject(v)) return refuse(`${at} is not a view`);
  const { id, name } = v;
  if (typeof id !== 'string') return refuse(`${at}.id is not a string`);
  const graphics = list(v['graphics'] ?? [], `${at}.graphics`).map((g, i) => (typeof g === 'string' ? g : refuse(`${at}.graphics[${i}] is not a string`)));
  return {
    id, name: typeof name === 'string' && name ? name : id, graphics,
    ...(v['direction'] == null ? {} : { direction: point(v['direction'], `${at}.direction`) }),
    ...(v['up'] == null ? {} : { up: point(v['up'], `${at}.up`) }),
  };
}

function placement(v: unknown, at: string): TextPlacement {
  if (!isObject(v)) return refuse(`${at} is not a text placement`);
  const { text_index: textIndex, height, alignment } = v;
  if (textIndex !== undefined && !(Number.isInteger(textIndex) && (textIndex as number) >= 0)) refuse(`${at}.text_index is not an index`);
  if (height !== undefined && !(typeof height === 'number' && Number.isFinite(height) && height >= 0)) refuse(`${at}.height is not a length`);
  if (alignment !== undefined && typeof alignment !== 'string') refuse(`${at}.alignment is not a string`);
  const { cell_box: box, stroked } = v;
  const pair = (c: unknown, where: string): [number, number] => {
    const xy = list(c, where);
    if (xy.length !== 2 || !xy.every((n) => typeof n === 'number' && Number.isFinite(n))) refuse(`${where} is not a point [x, y]`);
    return [xy[0] as number, xy[1] as number];
  };
  let cellBox: TextPlacement['cellBox'];
  if (box !== undefined && box !== null) {
    const corners = list(box, `${at}.cell_box`);
    if (corners.length !== 2) refuse(`${at}.cell_box does not have 2 corners`);
    cellBox = [pair(corners[0], `${at}.cell_box[0]`), pair(corners[1], `${at}.cell_box[1]`)];
  }
  if (stroked !== undefined && typeof stroked !== 'boolean') refuse(`${at}.stroked is not a boolean`);
  return {
    ...(textIndex === undefined ? {} : { textIndex: textIndex as number }),
    origin: point(v['origin'], `${at}.origin`), xAxis: point(v['x_axis'], `${at}.x_axis`), normal: point(v['normal'], `${at}.normal`),
    ...(height === undefined ? {} : { height: height as number }),
    ...(alignment === undefined ? {} : { alignment: alignment as string }),
    ...(cellBox ? { cellBox } : {}), ...(stroked === undefined ? {} : { stroked: stroked as boolean }),
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
    const planes = list(json['planes'] ?? [], 'planes').map((p, i) => plane(p, `planes[${i}]`));
    const views = list(json['views'] ?? [], 'views').map((p, i) => savedView(p, `views[${i}]`));
    return { ok: true, value: { items, unlinked, semanticIds, planes, views } };
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

/** Whether any graphic is the file's own (``source: "file"``), not one Specify generated. */
export const hasFileGraphics = (pmi: DrawnPmi): boolean =>
  [...pmi.presentation.unlinked, ...Object.values(pmi.presentation.items).flat()].some((g) => g.source === 'file');

/** Specify's general-notes block: in the 3D view a fixed panel shows the part's notes instead. */
export const NOTES_BLOCK = 'generated:general-notes';

/**
 * The drawn PMI without Specify's general-notes block, and without the view it generates for it:
 * notes on a plane in 3D turn away as the part turns, and the panel already says them.
 */
export function withoutNotesBlock(pmi: DrawnPmi): DrawnPmi {
  const keep = (g: DrawnGraphic) => g.id !== NOTES_BLOCK;
  const { presentation } = pmi;
  return {
    ...pmi,
    presentation: {
      ...presentation,
      items: Object.fromEntries(Object.entries(presentation.items).map(([item, gs]) => [item, gs.filter(keep)])),
      unlinked: presentation.unlinked.filter(keep),
      views: presentation.views.filter((v) => !(v.graphics.length && v.graphics.every((id) => id === NOTES_BLOCK))),
    },
  };
}

/** What the Show menu says of the drawn PMI, as it says it of the labels: which kinds are shown. */
export interface DrawnShow {
  /** "Defaults to check": items Specify defaulted. */
  readonly default: boolean;
  /** "PMI from the file": the file's own graphics and items Specify read from it. */
  readonly existing: boolean;
  /** "General dimensions": what the general tolerance covers. */
  readonly general: boolean;
}

/**
 * Whether a graphic is drawn under the Show menu: as a label is, by its item's status, and a
 * graphic the file draws counts as the file's PMI whatever its status. Answered items are
 * always drawn, as their labels are.
 */
export function drawnShown(source: DrawnGraphic['source'], status: DrawnStatus | undefined, show: DrawnShow): boolean {
  if (status === 'default' && !show.default) return false;
  if (status === 'general' && !show.general) return false;
  if ((status === 'existing' || source === 'file') && !show.existing) return false;
  return true;
}

type Vec = [number, number, number];
const sub = (a: DrawnPoint, b: DrawnPoint): Vec => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: DrawnPoint, b: DrawnPoint) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: DrawnPoint, b: DrawnPoint): Vec => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: DrawnPoint): Vec | null => {
  const n = Math.hypot(a[0], a[1], a[2]);
  return n > 1e-12 ? [a[0] / n, a[1] / n, a[2] / n] : null;
};

/** A graphic's points: its strokes, its fills and where its text is set. */
export const graphicPoints = (g: DrawnGraphic): DrawnPoint[] => [
  ...g.polylines.flat(), ...g.triangles.flat(), ...(g.textPlacements ?? []).map((t) => t.origin),
];

/**
 * The normal of the plane that best fits ``points`` (least squares), with no particular sign;
 * null when they do not make a plane (too few, on a line, or spread in depth nearly as much as
 * across).
 */
export function bestFitNormal(points: readonly DrawnPoint[]): Vec | null {
  if (points.length < 3) return null;
  const c: Vec = [0, 0, 0];
  for (const p of points) for (let k = 0; k < 3; k++) c[k] += p[k]! / points.length;
  // The covariance, and its eigenvectors by Jacobi rotations: the normal is the least spread.
  const a: Vec[] = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  for (const p of points) {
    const d = sub(p, c);
    for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) a[i]![j] += d[i]! * d[j]!;
  }
  const v: Vec[] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
  for (let sweep = 0; sweep < 50; sweep++) {
    let off = 0;
    for (const [p, q] of [[0, 1], [0, 2], [1, 2]] as const) {
      const apq = a[p]![q]!;
      off += Math.abs(apq);
      if (Math.abs(apq) < 1e-300) continue;
      const theta = (a[q]![q]! - a[p]![p]!) / (2 * apq);
      const t = Math.sign(theta || 1) / (Math.abs(theta) + Math.sqrt(theta * theta + 1));
      const cos = 1 / Math.sqrt(t * t + 1);
      const sin = t * cos;
      for (const m of [a, v]) {
        for (let k = 0; k < 3; k++) {
          const [kp, kq] = [m[k]![p]!, m[k]![q]!];
          m[k]![p] = cos * kp - sin * kq;
          m[k]![q] = sin * kp + cos * kq;
        }
      }
      for (let k = 0; k < 3; k++) {
        const [pk, qk] = [a[p]![k]!, a[q]![k]!];
        a[p]![k] = cos * pk - sin * qk;
        a[q]![k] = sin * pk + cos * qk;
      }
    }
    if (off < 1e-12) break;
  }
  const values = [0, 1, 2].map((i) => a[i]![i]!);
  const order = [0, 1, 2].sort((i, j) => values[i]! - values[j]!);
  const [least, middle] = [values[order[0]!]!, values[order[1]!]!];
  // On a line (two spreads nil), or not flat (the least spread over a hundredth of the next).
  if (middle <= 1e-12 || least > 1e-2 * middle) return null;
  const k = order[0]!;
  return unit([v[0]![k]!, v[1]![k]!, v[2]![k]!]);
}

/**
 * The side a graphic is read from: its annotation plane's normal, else its placed text's, else
 * the plane its strokes lie in, turned away from ``centre`` (the part's), since a callout is read
 * from outside the part; null when that cannot be told.
 */
export function graphicNormal(g: DrawnGraphic, planes: readonly DrawnPlane[], centre?: DrawnPoint): Vec | null {
  const stated = planes.find((p) => p.id === g.planeId)?.normal ?? g.textPlacements?.[0]?.normal;
  if (stated) return unit(stated);
  const points = graphicPoints(g);
  const n = bestFitNormal(points);
  if (!n || !centre) return null;
  const mid = points.reduce<Vec>((m, p) => [m[0] + p[0] / points.length, m[1] + p[1] / points.length, m[2] + p[2] / points.length], [0, 0, 0]);
  const out = sub(mid, centre);
  const side = dot(out, n);
  // A plane through the centre has no outside.
  if (Math.abs(side) <= 1e-6 * Math.max(1, Math.hypot(...out))) return null;
  return side > 0 ? n : [-n[0], -n[1], -n[2]];
}

/** The middle of the box around a mesh's flat ``positions`` (x, y, z, x, …): the part's centre. */
export function partCentre(positions: readonly number[]): DrawnPoint | undefined {
  if (positions.length < 3) return undefined;
  const min: Vec = [Infinity, Infinity, Infinity];
  const max: Vec = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i + 2 < positions.length; i += 3) {
    for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k]!, positions[i + k]!); max[k] = Math.max(max[k]!, positions[i + k]!); }
  }
  return [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2];
}

/** Whether a plane with ``normal`` is seen from behind by a camera looking along ``direction``. */
export const facesAway = (normal: DrawnPoint, direction: DrawnPoint): boolean => dot(normal, direction) > 1e-9;

/**
 * A way to look at the drawn PMI square on: a saved view's camera, or an annotation plane's
 * front. The camera stands at ``eye`` (a unit vector) from what it looks at, ``up`` its way up
 * when known, framing the box of the graphics it shows.
 */
export interface DrawnView {
  readonly key: string;
  readonly name: string;
  /** The graphics it shows, by name, one a line. */
  readonly title: string;
  readonly eye: DrawnPoint;
  readonly up?: DrawnPoint;
  readonly min: DrawnPoint;
  readonly max: DrawnPoint;
}

const rounded = (v: DrawnPoint) => v.map((x) => (Math.abs(x) < 5e-4 ? 0 : x).toFixed(3)).join(',');

/**
 * The views of the drawn PMI: the file's saved views that have a camera (not the views Specify
 * generates for its own graphics), then one for each way
 * the shown graphics' annotation planes face (planes facing alike, with the same up, are one
 * view), named by the graphics on them.
 */
export function drawnViews(
  pmi: DrawnPmi, shown: (g: DrawnGraphic, item: string | null) => boolean, centre?: DrawnPoint,
): DrawnView[] {
  const { presentation } = pmi;
  const all = [
    ...Object.entries(presentation.items).flatMap(([item, gs]) => gs.map((g) => ({ g, item: item as string | null }))),
    ...presentation.unlinked.map((g) => ({ g, item: null })),
  ];
  // One graphic under several items is one graphic, shown if shown under any of them.
  const byId = new Map<string, DrawnGraphic>();
  const graphics: DrawnGraphic[] = [];
  for (const { g, item } of all) {
    if (!shown(g, item)) continue;
    if (g.id === undefined) graphics.push(g);
    else if (!byId.has(g.id)) { byId.set(g.id, g); graphics.push(g); }
  }
  const box = (gs: readonly DrawnGraphic[]) => {
    const points = gs.flatMap(graphicPoints);
    if (!points.length) return null;
    const min: Vec = [Infinity, Infinity, Infinity];
    const max: Vec = [-Infinity, -Infinity, -Infinity];
    for (const p of points) {
      for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k]!, p[k]!); max[k] = Math.max(max[k]!, p[k]!); }
    }
    return { min, max };
  };
  const out: DrawnView[] = [];
  for (const view of presentation.views) {
    const eye = view.direction ? unit([-view.direction[0], -view.direction[1], -view.direction[2]]) : null;
    if (!eye) continue;
    const members = view.graphics.flatMap((id) => { const g = byId.get(id); return g ? [g] : []; });
    // The file's saved views only: Specify generates a view for each graphic it draws (0.1.6),
    // which the planes below already offer, grouped. A view whose graphics are all hidden (by
    // the Show menu, or facing away) has nothing to look square at, so it is not offered either.
    if (view.graphics.length && !members.length) continue;
    if (members.length && members.every((g) => g.source === 'generated')) continue;
    const b = box(members.length ? members : graphics);
    if (!b) continue;
    const up = view.up ? unit(view.up) : null;
    out.push({ key: `view:${view.id}`, name: view.name, title: members.map((g) => g.name).join('\n'), eye, ...(up ? { up } : {}), ...b });
  }
  const groups = new Map<string, { eye: Vec; up: Vec | null; graphics: DrawnGraphic[] }>();
  for (const g of graphics) {
    const eye = graphicNormal(g, presentation.planes, centre);
    if (!eye) continue;
    const x = presentation.planes.find((p) => p.id === g.planeId)?.xAxis ?? g.textPlacements?.[0]?.xAxis;
    const up = x ? unit(cross(eye, x)) : null;
    const key = `${rounded(eye)}|${up ? rounded(up) : ''}`;
    const group = groups.get(key) ?? { eye, up, graphics: [] };
    group.graphics.push(g);
    groups.set(key, group);
  }
  let n = 0;
  for (const [key, { eye, up, graphics: gs }] of groups) {
    const b = box(gs);
    if (!b) continue;
    const names = [...new Set(gs.map((g) => g.name))];
    const listed = names.slice(0, 3).join(' · ') + (names.length > 3 ? ` +${names.length - 3}` : '');
    out.push({ key: `plane:${key}`, name: `Plane ${++n}: ${listed}`, title: names.join('\n'), eye, ...(up ? { up } : {}), ...b });
  }
  return out;
}
