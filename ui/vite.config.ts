import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Library build: ES output, with the host's React and three left to the host. The intent worker
// is emitted as its own chunk, referenced from the bundle by new URL(..., import.meta.url). The
// Specify WASM is not bundled: the host serves it and passes wasmUrl.
const external = [/^react($|\/)/, /^react-dom($|\/)/, /^three($|\/)/, /^three-cad-viewer($|\/)/];

export default defineConfig({
  plugins: [react()],
  base: './',
  publicDir: false, // public/ holds the dev WASM; hosts serve their own
  worker: { format: 'es' },
  build: {
    target: 'es2023',
    lib: { entry: 'src/index.ts', formats: ['es'], fileName: () => 'specify-ui.js' },
    rollupOptions: { external },
  },
});
