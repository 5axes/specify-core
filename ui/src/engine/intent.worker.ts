import { readCarried } from './carried';

interface IntentModule {
  default(): Promise<unknown>;
  review(analysis: string, answers: string): string;
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

// The editor draws and writes Specify's defaults, listed for review (agreed 2026-10-16). From
// specify-core alpha.6 an unanswered default is not written unless accepted, so every review and
// write accepts them; an intent item is a default again when the person has not answered it, and
// its question stays open, as the engine leaves it.
const accepted = (answers: unknown) => engine!.acceptDefaults(JSON.stringify(answers));
function reviewed(answers: Record<string, unknown>) {
  const review = markDefaults(JSON.parse(engine!.review(accepted(answers))), answers);
  return { ...review, open: JSON.parse(engine!.review(JSON.stringify(answers))).open };
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
function load() {
  modulePromise ??= (async () => {
    const modulePath = new URL('/wasm/intent/specify_core_wasm.js', self.location.origin).href;
    const module: IntentModule = await import(/* @vite-ignore */ modulePath);
    await module.default();
    return module;
  })();
  return modulePromise;
}

let queue = Promise.resolve();
self.onmessage = ({ data: { id, operation, args } }) => {
  queue = queue.then(async () => {
    try {
      const module = await load();
      let result: unknown = true;
      if (operation === 'open' || operation === 'review' || operation === 'settle' || operation === 'write' || operation === 'rematch' || operation === 'scene' || operation === 'answers') {
        if (!engine || sourceId !== args.sourceId || part !== args.part || operation === 'open') {
          engine?.free(); engine = undefined;
          engine = args.part === undefined ? new module.IntentEngine(args.name, args.bytes)
            : module.IntentEngine.openPart(args.name, args.bytes, args.part);
          sourceId = args.sourceId; part = args.part;
        }
        if (operation === 'write') {
          // A part of an assembly is written with its parts-list row: Specify's suggestions for what
          // was not answered, and every part numbered. A file of several parts not placed in an
          // assembly (no assembly product definition) has no parts list to write them to: Specify
          // 0.1.12 still asks them, so they are left out and the part written without.
          const write = (answers: Record<string, unknown>, bom: boolean) =>
            JSON.parse(engine!.writePmi(accepted(answers), args.project, undefined, bom));
          const withoutBom = () => write(Object.fromEntries(Object.entries(args.answers ?? {}).filter(([id]) => !id.startsWith('bom.'))), false);
          let document;
          const fileId = args.fileId ?? args.sourceId;
          if (noAssemblyFiles.has(fileId)) document = withoutBom();
          else try { document = write(args.answers ?? {}, args.part !== undefined); }
          catch (error) {
            if (args.part === undefined || !String(error).includes('requires an assembly product definition')) throw error;
            noAssemblyFiles.add(fileId);
            document = withoutBom();
          }
          result = { ...document, bytes: new Uint8Array(document.bytes) };
        } else if (operation === 'answers') {
          // Every answer the STEP would be written with, the defaults made explicit: a part's
          // `<part>.answers.json` for a repository, which `specify-core write` takes as it is.
          result = JSON.parse(accepted(args.answers));
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
