export class IntentEngine {
  [Symbol.dispose](): void;
  acceptDefaults(answers: string): string;
  constructor(name: string, bytes: Uint8Array);
  evidence(): string;
  free(): void;
  original(): Uint8Array | undefined;
  payload(name: string): string | undefined;
  pmiScene(answers: string): string;
  rematchAnswers(answers: string, evidence?: string | null, automatic?: string | null): string;
  review(answers: string): string;
  settle(state: string): string;
  snapshot(): string;
  static openPart(name: string, bytes: Uint8Array, part: number): IntentEngine;
  static parts(name: string, bytes: Uint8Array): string;
  writePmi(answers: string, project?: string | null, payload?: string | null): string;
}

export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

export function acceptDefaults(analysis: string, answers: string): string;

export function answerEdit(state: string, answers: string): string;

export function diff(old_name: string, old: Uint8Array, new_name: string, _new: Uint8Array, answers: string, title: string): string;

export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

export function interpret(analysis: string, answers: string, question_id: string, text: string): string;

export function openedTolerancing(state: string, resume?: string | null): string;

export function pmiScene(bytes: Uint8Array, part?: number | null): string;

export function review(analysis: string, answers: string): string;

export function start(): void;

export function tolerancingEdit(state: string, mode: string | null | undefined, answers: string): string;

export interface AnalysisFace {
  area: number;
  centroid: [number, number, number];
  contact?: unknown;
  direction?: [number, number, number];
  id: number;
  kind: string;
  name?: string;
  origin?: [number, number, number];
  radius?: number;
}

export interface AnswerEvidence {
  face_count: number;
  face_names?: (string | null)[];
  faces: string;
  geometry?: unknown;
  schema: 1 | 2 | 3;
}

export interface DiffPmi {
  answers_from: "given" | "stored" | "none";
  existing: unknown[];
  existing_count: [number, number];
  items: PmiItemChange[];
  new_stored: boolean;
  stored: unknown[];
}

export interface InitOutput {
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbg_intentengine_free: (a: number, b: number) => void;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_start: () => void;
  readonly acceptDefaults: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly answerEdit: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly diff: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number) => [number, number, number, number];
  readonly intentengine_acceptDefaults: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_evidence: (a: number) => [number, number, number, number];
  readonly intentengine_open: (a: number, b: number, c: number, d: number) => [number, number, number];
  readonly intentengine_openPart: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
  readonly intentengine_original: (a: number) => [number, number];
  readonly intentengine_parts: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly intentengine_payload: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_pmiScene: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_rematchAnswers: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
  readonly intentengine_review: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_settle: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_snapshot: (a: number) => [number, number];
  readonly intentengine_writePmi: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
  readonly interpret: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number, number];
  readonly memory: WebAssembly.Memory;
  readonly openedTolerancing: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly pmiScene: (a: number, b: number, c: number) => [number, number, number, number];
  readonly review: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly start: () => void;
  readonly tolerancingEdit: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
}

export interface PmiItemChange {
  class: "added" | "removed" | "changed" | "renumbered";
  id: string;
  kind: string;
  new?: string;
  number?: number;
  old?: string;
  on: string;
  reason?: string;
  status: "answered" | "default" | "general" | "existing";
}

export interface PmiScene {
  pmi: { schema: "haecceity.pmi/1"; units: {length: PmiSceneItem[]; angle: PmiSceneItem[]; density: PmiSceneItem[]; other: PmiSceneItem[]}; pmi: PmiSceneItems; };
  presentation: { schema: "haecceity.presentation/1"; length_unit: string; semantic_ids: PmiSceneId[]; items: Record<PmiSceneId, PmiSceneGraphic[]>; unlinked: PmiSceneGraphic[]; planes?: PmiScenePlane[]; views?: PmiSceneView[]; };
  specify: Record<PmiSceneId, PmiSceneMetadata>;
}

export interface PmiSceneGraphic {
  id?: PmiSceneId;
  name: string;
  plane_id?: PmiSceneId;
  polylines: PmiSceneVector[][];
  source: "generated" | "file";
  step_ids: number[];
  text: string[];
  text_placements?: PmiSceneTextPlacement[];
  triangles: [PmiSceneVector, PmiSceneVector, PmiSceneVector][];
  unsupported: string[];
}

export interface PmiSceneItem {
  id: PmiSceneId;
  identifier?: string;
  value: unknown;
}

export interface PmiSceneItems {
  attributes: PmiSceneItem[];
  datum_targets: PmiSceneItem[];
  datums: PmiSceneItem[];
  decimal_places: number | null;
  derived_geometry: PmiSceneItem[];
  dimensions: PmiSceneItem[];
  features: PmiSceneItem[];
  general: PmiSceneItem[];
  geometry: PmiSceneItem[];
  holes: unknown[];
  knurls: PmiSceneItem[];
  material: unknown;
  notes: PmiSceneItem[];
  standards: unknown[];
  surface_textures: PmiSceneItem[];
  threads: PmiSceneItem[];
  tolerance_relations: PmiSceneItem[];
  tolerances: PmiSceneItem[];
}

export interface PmiSceneMetadata {
  notation?: string;
  number?: number;
  origin?: "requirement" | "general_tolerance" | "imported";
  question?: string;
  source?: {schema: number; faces: string; support: number[]; parameter: string};
  status: PmiSceneStatus;
}

export interface PmiScenePlane {
  id: PmiSceneId;
  name: string;
  normal: PmiSceneVector;
  origin: PmiSceneVector;
  x_axis: PmiSceneVector;
}

export interface PmiSceneTextPlacement {
  alignment?: string;
  height?: number;
  line_spacing?: number;
  normal: PmiSceneVector;
  origin: PmiSceneVector;
  stroked?: boolean;
  text_index?: number;
  x_axis: PmiSceneVector;
}

export interface PmiSceneView {
  direction?: PmiSceneVector;
  graphics: PmiSceneId[];
  id: PmiSceneId;
  name: string;
  up?: PmiSceneVector;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export type PmiSceneId = string;

export type PmiSceneStatus = "answered" | "default" | "general" | "existing";

export type PmiSceneVector = [number, number, number];

export type SyncInitInput = BufferSource | WebAssembly.Module;
