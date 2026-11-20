import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// The component test's host page (test/harness), with the repo's public/ (the dev WASM) beside it.
export default defineConfig({
  root: 'test/harness',
  publicDir: '../../public',
  plugins: [react()],
  worker: { format: 'es' },
  build: { target: 'es2023', outDir: '../../.cache/harness', emptyOutDir: true },
});
