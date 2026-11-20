// `npm pack` for publishing. In this repository the engine dependency is the in-repo WASM build
// (file:../crates/specify-core-wasm/pkg); the published package depends on
// @draftwright/specify-core-wasm at its own version (lockstep: one release publishes both). This
// writes that exact version into package.json for the pack only, restores package.json, and checks
// the tarball's package.json. Run after `npm run build` (the pack runs no scripts).
//   node scripts/pack.mjs <destination dir>   prints the tarball's path
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ENGINE = '@draftwright/specify-core-wasm';
const LOCAL = 'file:../crates/specify-core-wasm/pkg';
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';

export function pack(destination, root = process.cwd()) {
  const path = resolve(root, 'package.json');
  const original = readFileSync(path, 'utf8');
  const pkg = JSON.parse(original);
  const spec = pkg.dependencies?.[ENGINE];
  if (spec !== LOCAL) throw new Error(`package.json depends on ${ENGINE} ${spec}, expected ${LOCAL}`);
  const built = JSON.parse(readFileSync(resolve(root, '../crates/specify-core-wasm/pkg/package.json'), 'utf8')).version;
  if (built !== pkg.version) throw new Error(`The in-repo WASM build is ${built}, the package ${pkg.version}: versions are lockstep.`);
  mkdirSync(destination, { recursive: true });
  let packed;
  try {
    writeFileSync(path, `${JSON.stringify({ ...pkg, dependencies: { ...pkg.dependencies, [ENGINE]: pkg.version } }, null, 2)}\n`);
    packed = JSON.parse(execFileSync(npm, ['pack', '--ignore-scripts', '--json', '--pack-destination', resolve(destination)],
      { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }));
  } finally {
    writeFileSync(path, original);
  }
  const tarball = join(resolve(destination), packed[0].filename);
  const shipped = JSON.parse(execFileSync('tar', ['-xOzf', tarball, 'package/package.json'], { encoding: 'utf8' }));
  if (shipped.version !== pkg.version || shipped.dependencies?.[ENGINE] !== pkg.version) {
    throw new Error(`${tarball} is ${shipped.version} on ${ENGINE} ${shipped.dependencies?.[ENGINE]}, expected ${pkg.version} on ${pkg.version}`);
  }
  return tarball;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const destination = process.argv[2] ?? fail('usage: node scripts/pack.mjs <destination dir>');
  console.log(pack(destination));
}

function fail(message) { console.error(message); process.exit(1); }
