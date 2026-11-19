import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { SESSION_API } from '../app/session-api.ts';

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));

test('the app build and release archive contain the versioned session API manifest at their root', async (t) => {
  const temp = mkdtempSync(join(tmpdir(), 'specify-app-manifest-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const app = join(temp, 'app');
  await build({ configFile: fileURLToPath(new URL('../vite.app.config.ts', import.meta.url)),
    logLevel: 'silent', build: { outDir: app } });
  const expected = { name: 'specify-ui-app', version: pkg.version, api: SESSION_API };
  assert.deepEqual(JSON.parse(readFileSync(join(app, 'specify-ui-app.json'), 'utf8')), expected);
  // The release workflow packs the contents of dist-app with precisely this directory layout.
  const archive = join(temp, `specify-ui-app-${pkg.version}.tgz`);
  execFileSync('tar', ['-czf', archive, '-C', app, '.']);
  const entries = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' }).split('\n');
  assert.ok(entries.includes('./index.html'));
  assert.ok(entries.includes('./specify-ui-app.json'));
  const packed = execFileSync('tar', ['-xOzf', archive, './specify-ui-app.json'], { encoding: 'utf8' });
  assert.deepEqual(JSON.parse(packed), expected);
});
