import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Static build for Cloudflare Pages. There is no dev proxy and no API target:
// the parser is meant to run in the browser as a WebAssembly module, so the
// upload never leaves the machine and the site is deployable as plain assets.
export default defineConfig({
  root: '.',
  base: './',
  plugins: [vue()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
  },
  server: {
    // Preferred, not required. The upstream `statute-web` dev server also binds 5173,
    // and `strictPort: true` would make this one fail to start while it is running
    // rather than moving to 5174.
    port: 5173,
    strictPort: false,
  },
})
