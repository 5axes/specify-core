import { WorkerClient } from './rpc.ts';

/** The Specify intent engine in a worker. `wasmUrl` is where its WASM glue is served from, if not the editor's own `/wasm/intent/`. */
export function createIntentEngine({ wasmUrl }: { wasmUrl?: string } = {}): WorkerClient {
  return new WorkerClient(() => {
    const worker = new Worker(new URL('./intent.worker.ts', import.meta.url), { type: 'module' });
    if (wasmUrl) worker.postMessage({ wasmUrl: new URL(wasmUrl, location.href).href });
    return worker;
  });
}
