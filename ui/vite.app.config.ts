import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';
import { SESSION_API } from './app/session-api';

// The local app `specify-core open` serves (app/): a static directory, dist-app/, with relative URLs
// only and nothing from another origin. The engine is not in it: the command serves its own at
// config.wasmUrl, so the dependency's copy is aliased to a stub.
const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };
const fontLicence = fileURLToPath(new URL('./node_modules/@fontsource/ibm-plex-mono/LICENSE', import.meta.url));

/** The bundled font's licence (SIL OFL 1.1) and this package's, beside the app. */
const licences = (): Plugin => ({
  name: 'specify-app-licences',
  apply: 'build',
  generateBundle() {
    this.emitFile({ type: 'asset', fileName: 'licenses/IBM-Plex-Mono-OFL.txt', source: readFileSync(fontLicence, 'utf8') });
    for (const name of ['LICENSE', 'NOTICE']) {
      this.emitFile({ type: 'asset', fileName: name, source: readFileSync(new URL(`./${name}`, import.meta.url), 'utf8') });
    }
  },
});

/** specify-core verifies this manifest before serving a downloaded app. */
const manifest = (): Plugin => ({
  name: 'specify-app-manifest',
  apply: 'build',
  generateBundle() {
    this.emitFile({ type: 'asset', fileName: 'specify-ui-app.json',
      source: `${JSON.stringify({ name: 'specify-ui-app', version: pkg.version, api: SESSION_API })}\n` });
  },
});

export default defineConfig({
  root: 'app',
  base: './',
  publicDir: false,
  plugins: [react(), licences(), manifest()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  resolve: { alias: { '@draftwright/specify-core-wasm': fileURLToPath(new URL('./app/no-bundled-engine.ts', import.meta.url)) } },
  worker: { format: 'es' },
  build: {
    target: 'es2023', outDir: '../dist-app', emptyOutDir: true,
    // Fonts as files: the CSP allows data: fonts, but files keep the CSS small.
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
  },
});
