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
          if (id.includes('node_modules/three/') || id.includes('@react-three') || id.includes('three-stdlib') || id.includes('node_modules/maath') || id.includes('troika') || id.includes('node_modules/meshline') || id.includes('camera-controls')) return 'three'
          if (id.includes('maplibre-gl')) return 'maplibre'
        },
      },
    },
  },
})
