import { WorkerClient } from './rpc.ts';

/** The Specify intent engine in a worker, on the dependency @draftwright/specify-core-wasm. `wasmUrl`, for a host that
 * serves the engine itself, is the URL of its `specify_core_wasm.js` module, resolved against the page; a cross-origin
 * URL needs the host's CSP `script-src` (and `connect-src` for the .wasm) to allow it.
 * The legacy `write` operation WRITES EVERY DEFAULT. A host choosing it must first obtain
 * approval for every default it will write. `review-given` accepts no defaults and supplies
 * the writer's characteristic numbers from the STEP geometry and history. */
export function createIntentEngine({ wasmUrl }: { wasmUrl?: string } = {}): WorkerClient {
  return new WorkerClient(() => {
    const worker = new Worker(new URL('./intent.worker.ts', import.meta.url), { type: 'module' });
    if (wasmUrl) worker.postMessage({ wasmUrl: new URL(wasmUrl, location.href).href });
    return worker;
  });
}
