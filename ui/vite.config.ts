import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Library build: ES output, with the host's React and three left to the host. The intent worker
// is emitted as its own chunk, referenced from the bundle by new URL(..., import.meta.url). The
// Specify WASM is not bundled: the worker imports the dependency @draftwright/specify-core-wasm,
// which the host's bundler resolves (or loads the host's own from wasmUrl).
const external = [/^react($|\/)/, /^react-dom($|\/)/, /^three($|\/)/, /^three-cad-viewer($|\/)/, /^@draftwright\/specify-core-wasm($|\/)/];

export default defineConfig({
  plugins: [react()],
  base: './',
  publicDir: false, // public/ holds the dev WASM; hosts serve their own
  worker: { format: 'es', rollupOptions: { external, output: { entryFileNames: 'assets/[name].js' } } },
  build: {
    target: 'es2023',
    lib: { entry: 'src/index.ts', formats: ['es'], fileName: () => 'specify-ui.js' },
    rollupOptions: { external },
  },
});
