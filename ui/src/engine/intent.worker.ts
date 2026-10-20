interface IntentModule {
  default(): Promise<unknown>;
  review(analysis: string, answers: string): string;
  interpret(analysis: string, answers: string, question: string, text: string): string;
  answerEdit(state: string, answers: string): string;
  tolerancingEdit(state: string, mode: string | undefined, answers: string): string;
  openedTolerancing(state: string, resume?: string): string;
  IntentEngine: new (name: string, bytes: Uint8Array) => { free(): void; snapshot(): string; review(answers: string): string; settle(state: string): string; writePmi(answers: string, project?: string): string; acceptDefaults(answers: string): string };
}

let engine: InstanceType<IntentModule['IntentEngine']> | undefined;
let sourceId = '';

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
      if (operation === 'open' || operation === 'review' || operation === 'settle' || operation === 'write') {
        if (!engine || sourceId !== args.sourceId || operation === 'open') {
          engine?.free(); engine = undefined;
          engine = new module.IntentEngine(args.name, args.bytes);
          sourceId = args.sourceId;
        }
        if (operation === 'write') {
          const document = JSON.parse(engine.writePmi(accepted(args.answers), args.project));
          result = { ...document, bytes: new Uint8Array(document.bytes) };
        } else if (operation === 'settle') {
          const { state } = JSON.parse(engine.settle(JSON.stringify(args.state)));
          result = { review: reviewed(state.answers), state };
        } else result = operation === 'open' ? JSON.parse(engine.snapshot()) : reviewed(args.answers);
      } else if (operation === 'answer-edit') {
        result = JSON.parse(module.answerEdit(JSON.stringify(args.state), JSON.stringify(args.answers)));
      } else if (operation === 'tolerancing-edit') {
        result = JSON.parse(module.tolerancingEdit(JSON.stringify(args.state), args.mode, JSON.stringify(args.answers)));
      } else if (operation === 'opened') {
        result = JSON.parse(module.openedTolerancing(JSON.stringify(args.state), args.resume && JSON.stringify(args.resume)));
      } else if (operation === 'interpret') {
        result = JSON.parse(module.interpret(
          JSON.stringify(args.analysis), JSON.stringify(args.answers), args.question, args.text,
        ));
      }
      self.postMessage({ id, ok: true, result });
    } catch (error) {
      self.postMessage({ id, ok: false, error: error instanceof Error ? error.message : String(error), fatal: error instanceof WebAssembly.RuntimeError });
    }
  });
};
