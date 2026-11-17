// Packs the real tarball and uses it in a minimal Vite host app (test/package-host), so the
// post-build steps run on every PR: scripts/worker-url.mjs (the `new Worker(new URL(...))` form
// host bundlers build as a worker), the worker's external import of @draftwright/specify-core-wasm,
// and scripts/rewrite-dts-imports.mjs (the shipped .d.ts must typecheck with skipLibCheck: false).
// The host is a copy in .cache/package-host with a package.json written here. It must typecheck,
// build (worker and .wasm emitted), and pass test/step-diff-browser.mjs served by `vite preview`.
// Run from the repo root after `npm run build` and `npm run fetch:wasm`.
import { execFileSync, spawn } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = process.cwd();
const packDir = resolve(root, '.cache/package-host-pack');
const host = resolve(root, '.cache/package-host');
const port = 4292;
const url = `http://127.0.0.1:${port}`;
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';

function fail(message) {
  console.error(`test:package FAILED: ${message}`);
  process.exit(1);
}
const run = (cmd, args, cwd, env) => execFileSync(cmd, args, { cwd, stdio: 'inherit', env: { ...process.env, ...env } });

if (!existsSync(resolve(root, 'dist'))) fail('dist/ is missing; run `npm run build` first');
const rootPkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json'), 'utf8'));

const dev = rootPkg.devDependencies;
const version = name => dev[name] ?? fail(`${name} is not in the root devDependencies`);

// Pack.
rmSync(packDir, { recursive: true, force: true });
mkdirSync(packDir, { recursive: true });
const packed = JSON.parse(execFileSync(npm, ['pack', '--ignore-scripts', '--json', '--pack-destination', packDir], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }));
const tarball = resolve(packDir, packed[0].filename);
if (!existsSync(tarball)) fail(`npm pack did not produce ${tarball}`);

// The peer range spans three 0.184 to 0.186. 0.184.0 is what three-cad-viewer pins and the repo
// tests against; 0.186.1 is what the draftwright editor runs.
async function testHost(threeVersion) {
  console.log(`\n== host on three ${threeVersion} ==`);
  // Fresh host.
  rmSync(host, { recursive: true, force: true });
  cpSync(resolve(root, 'test/package-host'), host, { recursive: true });
  let harness = readFileSync(resolve(root, 'test/harness/main.tsx'), 'utf8');
  for (const from of ["'../../src/index'", "'../../src/model/diff'"]) {
    if (!harness.includes(from)) fail(`test/harness/main.tsx no longer imports ${from}`);
    harness = harness.replace(from, "'@draftwright/specify-ui'");
  }
  if (harness.includes('../../src')) fail('test/harness/main.tsx still imports from ../../src');
  writeFileSync(resolve(host, 'src/harness.tsx'), harness);

  const core = lock.packages?.['node_modules/@draftwright/specify-core-wasm']?.resolved
    ?? fail('package-lock.json has no resolved URL for @draftwright/specify-core-wasm');
  const pick = names => Object.fromEntries(names.map(name => [name, version(name)]));
  writeFileSync(resolve(host, 'package.json'), JSON.stringify({
    name: 'package-host',
    private: true,
    type: 'module',
    dependencies: {
      '@draftwright/specify-ui': `file:${tarball}`,
      ...pick(['react', 'react-dom', 'three-cad-viewer']),
      three: threeVersion,
    },
    devDependencies: pick(['vite', '@vitejs/plugin-react', 'typescript', '@types/react', '@types/react-dom']),
    // Not on npm yet; the lockfile's resolved URL stays valid once it is.
    overrides: { '@draftwright/specify-core-wasm': core },
  }, null, 2));

  run(npm, ['install', '--no-audit', '--no-fund'], host);

  const installed = JSON.parse(readFileSync(resolve(host, 'node_modules/three/package.json'), 'utf8')).version;
  if (installed !== threeVersion) fail(`host three is ${installed}, expected ${threeVersion}`);
  const nested = resolve(host, 'node_modules/three-cad-viewer/node_modules/three');
  if (threeVersion === version('three')) {
    // The version three-cad-viewer pins: one three only.
    if (existsSync(nested)) fail('a second copy of three is installed under three-cad-viewer');
    console.log(`three ${installed} (single copy)`);
  } else {
    // A newer three in the peer range (the draftwright editor's): three-cad-viewer depends on its
    // own exact three, so npm nests that one under it. Record what is installed.
    const own = existsSync(nested) ? JSON.parse(readFileSync(resolve(nested, 'package.json'), 'utf8')).version : 'none';
    console.log(`three ${installed} for the host, three-cad-viewer's own three: ${own}`);
  }

  run(npx, ['tsc', '-p', '.'], host);
  run(npx, ['vite', 'build'], host);
  const assets = readdirSync(resolve(host, 'dist/assets'));
  if (!assets.some(name => /^intent\.worker.*\.js$/.test(name))) fail(`no intent.worker JS in the build: ${assets.join(', ')}`);
  if (!assets.some(name => name.endsWith('.wasm'))) fail(`no .wasm in the build: ${assets.join(', ')}`);
  console.log(`built assets: ${assets.filter(name => /intent\.worker|\.wasm$/.test(name)).join(', ')}`);

  // Serve and run the browser test against it.
  const server = spawn(npx, ['vite', 'preview', '--host', '127.0.0.1', '--port', String(port), '--strictPort'], { cwd: host, stdio: 'inherit' });
  let serverFailed = false;
  let stopping = false;
  server.on('exit', code => {
    if (!stopping) { serverFailed = true; console.error(`vite preview exited early (${code})`); }
  });
  const stop = () => { stopping = true; server.kill(); };
  process.on('exit', stop);
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { stop(); process.exit(1); });

  try {
    let up = false;
    for (let i = 0; i < 60 && !up && !serverFailed; i++) {
      up = await fetch(url).then(response => response.ok, () => false);
      if (!up) await new Promise(done => setTimeout(done, 500));
    }
    if (!up) fail('vite preview did not respond');
    run(process.execPath, ['test/step-diff-browser.mjs'], root, { BASE_URL: url });
    if (serverFailed) fail('vite preview died during the browser test');
  } catch (error) {
    stop();
    fail(error.message);
  }
  stop();
}

for (const threeVersion of [version('three'), '0.186.1']) await testHost(threeVersion);
console.log('test:package passed');
