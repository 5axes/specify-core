// One engine directory for Node fixtures and the app served by specify-core open.
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const wasmDirectory = resolve(process.env.SPECIFY_WASM_DIR ?? 'public/wasm/intent');

export function requireEngine() {
  const modulePath = join(wasmDirectory, 'specify_core_wasm.js');
  const wasmPath = join(wasmDirectory, 'specify_core_wasm_bg.wasm');
  const missing = [modulePath, wasmPath].filter((path) => !existsSync(path));
  if (missing.length) throw new Error(`Specify WASM engine is missing: ${missing.join(', ')}. Please run npm run fetch:wasm, or set SPECIFY_WASM_DIR to an installed engine.`);
  return { modulePath, wasmPath };
}

// npm test checks this before starting the suite, without importing absent WASM glue.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { requireEngine(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
