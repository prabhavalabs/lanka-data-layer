import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

const API_TARGET = process.env.LANKA_API_TARGET ?? 'http://127.0.0.1:8601'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  // the engine's API paths go to the local Rust server, so the app also works when served on another host
  server: { port: 5183, strictPort: true, host: '127.0.0.1', proxy: Object.fromEntries(['/v2/', '/app/', '/dl/', '/mcp'].map((p) => [p, { target: API_TARGET, changeOrigin: false }])) },
  preview: { proxy: Object.fromEntries(['/v2/', '/app/', '/dl/', '/mcp'].map((p) => [p, { target: API_TARGET, changeOrigin: false }])) },
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 1100,
    rollupOptions: {
      output: {
        manualChunks(id) {
          // React and the bundler's own helpers get a chunk of their own. Left alone they are folded into `three` (the
          // first manual chunk that needs them), and the entry then imports that chunk for the sake of React and the
          // dynamic-import helper: the whole renderer on the first paint, which is what lazy-loading the scene is meant to avoid.
          if (/node_modules\/(react|react-dom|scheduler)\//.test(id) || id.includes('vite/preload-helper') || id.includes('commonjsHelpers')) return 'react'
          if (id.includes('node_modules/three/') || id.includes('@react-three') || id.includes('three-stdlib') || id.includes('node_modules/maath') || id.includes('troika') || id.includes('node_modules/meshline') || id.includes('camera-controls')) return 'three'
          if (id.includes('maplibre-gl')) return 'maplibre'
        },
      },
    },
  },
})
