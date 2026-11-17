import { WorkerClient } from './rpc.ts';

/** The Specify intent engine in a worker. `wasmUrl` is the URL of specify-core-wasm's `specify_core_wasm.js` module (default `/wasm/intent/specify_core_wasm.js`),
 * resolved against the page. A cross-origin URL needs the host's CSP `script-src` (and `connect-src` for the .wasm) to allow it. */
export function createIntentEngine({ wasmUrl }: { wasmUrl?: string } = {}): WorkerClient {
  return new WorkerClient(() => {
    const worker = new Worker(new URL('./intent.worker.ts', import.meta.url), { type: 'module' });
    if (wasmUrl) worker.postMessage({ wasmUrl: new URL(wasmUrl, location.href).href });
    return worker;
  });
}
