// Install the checksum-pinned specify-core-wasm release into public/wasm/intent, for tests and development.
// Hosts serve it themselves and pass its URL as wasmUrl.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export async function installEngine(name, pin, { token, transport = fetch, root = '.' } = {}) {
  const cache = join(root, '.cache/wasm-releases');
  const output = join(root, 'public/wasm');
  mkdirSync(cache, { recursive: true }); mkdirSync(output, { recursive: true });
  const archivePath = join(cache, `${pin.sha256}.tgz`);
  let archive = existsSync(archivePath) ? readFileSync(archivePath) : null;
  const digest = bytes => createHash('sha256').update(bytes).digest('hex');
  if (!archive || digest(archive) !== pin.sha256) {
    const headers = { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28', ...(token ? { Authorization: `Bearer ${token}` } : {}) };
    const request = async (url, accept) => {
      const response = await transport(url, { headers: { ...headers, ...(accept ? { Accept: accept } : {}) }, signal: AbortSignal.timeout(120000) });
      if (!response.ok) throw new Error(`Cannot download ${name} ${pin.tag}: HTTP ${response.status}. Set DEPENDENCY_GITHUB_TOKEN with Contents: read on ${pin.repository}.`);
      return response;
    };
    const release = await (await request(`https://api.github.com/repos/${pin.repository}/releases/tags/${pin.tag}`)).json();
    const asset = release.assets?.find(item => item.name === pin.asset);
    if (!asset) throw new Error(`${pin.repository} ${pin.tag} has no ${pin.asset}.`);
    archive = Buffer.from(await (await request(`https://api.github.com/repos/${pin.repository}/releases/assets/${asset.id}`, 'application/octet-stream')).arrayBuffer());
    if (digest(archive) !== pin.sha256) throw new Error(`Checksum mismatch for ${pin.asset}; refusing to install.`);
    writeFileSync(archivePath, archive);
  }
  const staging = mkdtempSync(join(output, `.${name}-`));
  try {
    execFileSync('tar', ['-xz', '--strip-components=1', '-C', staging], { input: archive });
    for (const file of [`${pin.module}.js`, `${pin.module}_bg.wasm`, `${pin.module}.d.ts`, 'package.json']) {
      if (!existsSync(join(staging, file))) throw new Error(`${pin.asset} is missing ${file}.`);
    }
    // Replace glue, WASM, declarations and notices together only after verification.
    rmSync(join(output, name), { recursive: true, force: true });
    renameSync(staging, join(output, name));
  } finally { rmSync(staging, { recursive: true, force: true }); }
  console.log(`Installed ${name}: ${pin.repository} ${pin.tag} (${pin.sha256.slice(0, 12)}).`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  let token = process.env.DEPENDENCY_GITHUB_TOKEN || process.env.GITHUB_TOKEN;
  if (!token) { try { token = execFileSync('gh', ['auth', 'token'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {} }
  const { intent } = JSON.parse(readFileSync('engines.lock.json', 'utf8'));
  await installEngine('intent', intent, { token });
}
