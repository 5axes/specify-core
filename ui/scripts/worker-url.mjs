// Vite's library build wraps the worker's URL (new Worker(new URL(new URL("assets/…", import.meta.url).href, …))),
// which a host's bundler takes for a plain asset: it copies (or inlines) the worker without resolving the worker's
// import of @draftwright/specify-core-wasm. Rewritten to the standard form, the host's bundler (Vite, webpack 5)
// builds the worker as an entry of its own and resolves the engine and its .wasm, so a host need pass nothing.
import { readFileSync, writeFileSync } from 'node:fs';

const file = 'dist/specify-ui.js';
const text = readFileSync(file, 'utf8');
const wrapped = /new Worker\(\s*new URL\(\s*(?:\/\*\s*@vite-ignore\s*\*\/\s*)?new URL\("assets\/intent\.worker\.js", import\.meta\.url\)\.href,\s*"" \+ import\.meta\.url\s*\)/g;
const out = text.replace(wrapped, 'new Worker(new URL("./assets/intent.worker.js", import.meta.url)');
if (out.match(/new Worker\(/g)?.length !== 1 || !out.includes('new Worker(new URL("./assets/intent.worker.js", import.meta.url), { type: "module" })')) {
  throw new Error(`${file}: the intent worker's URL is not in the form a host's bundler builds as a worker; check the Vite output.`);
}
writeFileSync(file, out);
