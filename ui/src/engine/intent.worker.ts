import { readCarried } from './carried';
import { withCharacteristicNumbers, type NumberScene } from './characteristic-numbers';
import type { IntentReview } from '../types/pmi';
import { inspectionFromStep } from '../specify/saved-inspection';

interface IntentModule {
  default(): Promise<unknown>;
  review(analysis: string, answers: string): string;
  pmiScene(bytes: Uint8Array, part?: number): string;
  interpret(analysis: string, answers: string, question: string, text: string): string;
  answerEdit(state: string, answers: string): string;
  tolerancingEdit(state: string, mode: string | undefined, answers: string): string;
  openedTolerancing(state: string, resume?: string): string;
  diff(oldName: string, old: Uint8Array, newName: string, next: Uint8Array, answers: string, title: string, scenes?: boolean): string;
  IntentEngine: {
    new (name: string, bytes: Uint8Array): IntentInstance;
    openPart(name: string, bytes: Uint8Array, part: number): IntentInstance;
    parts(name: string, bytes: Uint8Array): string;
  };
}
interface IntentInstance { free(): void; snapshot(): string; review(answers: string): string; settle(state: string): string; writePmi(answers: string, project?: string, payload?: string, bom?: boolean): string; acceptDefaults(answers: string): string;
    evidence(): string; rematchAnswers(answers: string, evidence?: string | null, automatic?: string | null): string;
    payload(name: string): string | undefined; original(): Uint8Array | undefined; pmiScene(state: string): string }

let engine: IntentInstance | undefined;
let sourceId = '';
// The part of an assembly the engine is of, if its file is one.
let part: number | undefined;
// The open file has parts but no assembly to write a parts list to (Specify refused once).
const noAssemblyFiles = new Set<string>();

// Legacy 'review' previews defaults as suggestions, marking unanswered intent items as defaults
// and keeping their questions open. Legacy 'write' WRITES EVERY DEFAULT without checking approvals.
// 'review-given' and 'review-analysis-given' review only the answers supplied.
// Specify's refusal to write a BOM into a file with no assembly structure: its code from specify-core
// #156 (bom_no_assembly), and the wording of 0.1.12 and 0.1.13 before it.
const noAssemblyRefusal = (error: unknown) => /bom_no_assembly|requires an assembly product definition|has no assembly structure/.test(String(error));
const accepted = (answers: unknown) => engine!.acceptDefaults(JSON.stringify(answers));
function reviewed(answers: Record<string, unknown>) {
  const review = markDefaults(JSON.parse(engine!.review(accepted(answers))), answers);
  return { ...review, open: JSON.parse(engine!.review(JSON.stringify(answers))).open };
}
function inspectionReview(answers: Record<string, unknown>): IntentReview {
  const review = JSON.parse(engine!.review(JSON.stringify(answers))) as IntentReview;
  const scene = JSON.parse(engine!.pmiScene(JSON.stringify({ answers }))) as NumberScene;
  return { ...review, intent: withCharacteristicNumbers(review.intent, scene) };
}
function markDefaults<T>(review: T, answers: Record<string, unknown>): T {
  const mark = (value: unknown): void => {
    if (Array.isArray(value)) value.forEach(mark);
    else if (value && typeof value === 'object') {
      const item = value as Record<string, unknown>;
      if (typeof item.question === 'string' && 'default' in item) item.default = !(item.question in answers);
      Object.values(item).forEach(mark);
    }
  };
  mark((review as { intent?: unknown }).intent);
  return review;
}

let modulePromise: Promise<IntentModule> | undefined;
// Where the Specify WASM glue is, when the host serves its own (a `{ wasmUrl }` message before any
// work). Otherwise the dependency @draftwright/specify-core-wasm, which the host's bundler resolves;
// its glue finds its .wasm beside it (new URL(..., import.meta.url)).
let configuredUrl: string | undefined;
function load() {
  modulePromise ??= (async () => {
    const module: IntentModule = configuredUrl
      ? await import(/* @vite-ignore */ configuredUrl)
      : await import('@draftwright/specify-core-wasm') as unknown as IntentModule;
    await module.default();
    return module;
  })();
  return modulePromise;
}

let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  if (data.id === undefined && data.operation === undefined && typeof data.wasmUrl === 'string') { configuredUrl = data.wasmUrl; return; }
  const { id, operation, args } = data;
  queue = queue.then(async () => {
    try {
      const module = await load();
      let result: unknown = true;
      if (operation === 'file-scene') {
        // A saved file's own PMI as drawn (the free pmiScene): the cached engine is left as it is.
        result = JSON.parse(module.pmiScene(args.bytes, args.part));
      } else if (operation === 'file-inspection') {
        const file = args.part === undefined ? new module.IntentEngine(args.name, args.bytes)
          : module.IntentEngine.openPart(args.name, args.bytes, args.part);
        try {
          const { analysis } = JSON.parse(file.snapshot());
          result = inspectionFromStep(analysis, JSON.parse(module.pmiScene(args.bytes, args.part)));
        } finally { file.free(); }
      } else if (operation === 'open' || operation === 'review' || operation === 'review-given' || operation === 'settle' || operation === 'write' || operation === 'rematch' || operation === 'scene') {
        if (!engine || sourceId !== args.sourceId || part !== args.part || operation === 'open') {
          engine?.free(); engine = undefined;
          engine = args.part === undefined ? new module.IntentEngine(args.name, args.bytes)
            : module.IntentEngine.openPart(args.name, args.bytes, args.part);
          sourceId = args.sourceId; part = args.part;
        }
        if (operation === 'write') {
          // Legacy 'write' API: WRITES EVERY DEFAULT, including defaults absent from args.answers.
          // Hosts must obtain approval for every default before choosing this operation.
          // The local Specify app saves through the command's approved-defaults session API.
          // A part of an assembly is written with its parts-list row: Specify's suggestions for what
          // was not answered, and every part numbered. A file of several parts not placed in an
          // assembly (no assembly product definition) has no parts list to write them to: Specify
          // refuses the BOM there, so the part is written without it and without any bom.* answers.
          const write = (answers: Record<string, unknown>, bom: boolean) =>
            JSON.parse(engine!.writePmi(accepted(answers), args.project, undefined, bom));
          const withoutBom = () => write(Object.fromEntries(Object.entries(args.answers ?? {}).filter(([id]) => !id.startsWith('bom.'))), false);
          let document;
          const fileId = args.fileId ?? args.sourceId;
          if (noAssemblyFiles.has(fileId)) document = withoutBom();
          else try { document = write(args.answers ?? {}, args.part !== undefined); }
          catch (error) {
            if (args.part === undefined || !noAssemblyRefusal(error)) throw error;
            noAssemblyFiles.add(fileId);
            document = withoutBom();
          }
          result = { ...document, bytes: new Uint8Array(document.bytes) };
        } else if (operation === 'review-given') {
          // Inspection files use the given answers and the STEP's PMI directly. Filling defaults
          // even for preview can add part notes, general tolerances and dependent dimensions.
          result = inspectionReview(args.answers);
        } else if (operation === 'settle') {
          const { state } = JSON.parse(engine.settle(JSON.stringify(args.state)));
          result = { review: reviewed(state.answers), state };
        } else if (operation === 'scene') {
          // The PMI as drawn for the review state (specify-core 0.1.5 pmiScene). The defaults the
          // editor accepts are written, so they are in the scene, as unreviewed (`auto`) unless answered.
          const answers = JSON.parse(accepted(args.state.answers)) as Record<string, unknown>;
          const auto = [...new Set([...args.state.auto ?? [], ...Object.keys(answers).filter(id => !(id in args.state.answers))])];
          result = JSON.parse(engine.pmiScene(JSON.stringify({ ...args.state, answers, auto })));
        } else if (operation === 'rematch') {
          result = JSON.parse(engine.rematchAnswers(JSON.stringify(args.answers), args.evidence ? JSON.stringify(args.evidence) : null, args.automatic ? JSON.stringify(args.automatic) : null));
        } else result = operation === 'open' ? { ...JSON.parse(engine.snapshot()), evidence: JSON.parse(engine.evidence()) } : reviewed(args.answers);
      } else if (operation === 'review-analysis-given') {
        // Pure analysis-only review, without allocation geometry. Inspection hosts use
        // review-given with STEP bytes to obtain the writer's stable characteristic numbers.
        result = JSON.parse(module.review(JSON.stringify(args.analysis), JSON.stringify(args.answers)));
      } else if (operation === 'parts') {
        result = JSON.parse(module.IntentEngine.parts(args.name, args.bytes));
      } else if (operation === 'part-mesh') {
        // One part's mesh for the assembly's view, on an engine of its own: the open part's is kept.
        const one = module.IntentEngine.openPart(args.name, args.bytes, args.part);
        try { result = JSON.parse(one.snapshot()).mesh; } finally { one.free(); }
      } else if (operation === 'carried') {
        // What a STEP carries, read on an engine of its own: the open part's is kept.
        result = readCarried(module, args.name, args.bytes, args.payload, args.part);
      } else if (operation === 'answer-edit') {
        result = JSON.parse(module.answerEdit(JSON.stringify(args.state), JSON.stringify(args.answers)));
      } else if (operation === 'tolerancing-edit') {
        result = JSON.parse(module.tolerancingEdit(JSON.stringify(args.state), args.mode, JSON.stringify(args.answers)));
      } else if (operation === 'opened') {
        result = JSON.parse(module.openedTolerancing(JSON.stringify(args.state), args.resume && JSON.stringify(args.resume)));
      } else if (operation === 'diff') {
        // Two revisions of a part compared, as `specify-core diff` compares them; with ``scenes``,
        // the two PMI scenes it compared too (`report.pmi.scenes`, specify-core 0.1.7).
        result = JSON.parse(module.diff(args.oldName, args.old, args.newName, args.new, JSON.stringify(args.answers), args.title, args.scenes === true));
      } else if (operation === 'interpret') {
        result = JSON.parse(module.interpret(
          JSON.stringify(args.analysis), JSON.stringify(args.answers), args.question, args.text,
        ));
      }
      self.postMessage({ id, ok: true, result });
    } catch (error) {
      const fatal = error instanceof WebAssembly.RuntimeError;
      self.postMessage({ id, ok: false, error: error instanceof Error ? error.message : String(error), code: fatal ? 'panic' : undefined, fatal });
    }
  });
};
