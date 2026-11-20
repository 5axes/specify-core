// Install the in-repo specify-core-wasm build (wasm-pack's crates/specify-core-wasm/pkg, built from the
// same commit) into public/wasm/intent, for tests and development. Hosts serve it themselves and pass its
// URL as wasmUrl. Build it first, from the repository root:
//   wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const engineBuild = fileURLToPath(new URL('../../crates/specify-core-wasm/pkg', import.meta.url));
const MODULE = 'specify_core_wasm';

export function installEngine(name, { source = engineBuild, root = '.', version } = {}) {
  const missing = [`${MODULE}.js`, `${MODULE}_bg.wasm`, `${MODULE}.d.ts`, 'package.json']
    .filter(file => !existsSync(join(source, file)));
  if (missing.length) {
    throw new Error(`The Specify WASM build at ${source} is missing ${missing.join(', ')}. Build it from the repository root: wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked`);
  }
  const built = JSON.parse(readFileSync(join(source, 'package.json'), 'utf8')).version;
  // Lockstep versions: the UI and the engine are released together, at one version.
  if (version && built !== version) throw new Error(`The Specify WASM build at ${source} is ${built}, not ${version}: rebuild it.`);
  const output = join(root, 'public/wasm');
  mkdirSync(output, { recursive: true });
  const staging = mkdtempSync(join(output, `.${name}-`));
  try {
    cpSync(source, staging, { recursive: true });
    // Replace glue, WASM, declarations and notices together.
    rmSync(join(output, name), { recursive: true, force: true });
    renameSync(staging, join(output, name));
  } finally { rmSync(staging, { recursive: true, force: true }); }
  console.log(`Installed ${name}: specify-core-wasm ${built} from ${source}.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
  try { installEngine('intent', { version }); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
