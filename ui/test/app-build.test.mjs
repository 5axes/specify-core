import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { SESSION_API } from '../app/session-api.ts';

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));

test('the app build has index.html and the versioned session API manifest at its root', async (t) => {
  const temp = mkdtempSync(join(tmpdir(), 'specify-app-manifest-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const app = join(temp, 'app');
  await build({ configFile: fileURLToPath(new URL('../vite.app.config.ts', import.meta.url)),
    logLevel: 'silent', build: { outDir: app } });
  const expected = { name: 'specify-ui-app', version: pkg.version, api: SESSION_API };
  // specify-core's build.rs builds in a dist-app with exactly these at its root, of its own version.
  assert.deepEqual(JSON.parse(readFileSync(join(app, 'specify-ui-app.json'), 'utf8')), expected);
  assert.ok(existsSync(join(app, 'index.html')));
});
