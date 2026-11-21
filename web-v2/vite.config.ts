import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  server: { port: 5183, strictPort: true, host: '127.0.0.1' },
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
