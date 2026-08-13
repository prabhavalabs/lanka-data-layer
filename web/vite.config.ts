import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  optimizeDeps: {
    // maplibre-gl spins up its tile-parsing work in a Web Worker built from
    // a separate `maplibre-gl-worker.mjs` entry; Vite's esbuild dep
    // pre-bundler flattens the package into one chunk and loses that
    // file, so the worker 404s at runtime ("Cannot read properties of
    // undefined (reading 'height')" from the resulting empty worker).
    // Excluding it from pre-bundling serves it as real ESM instead, where
    // the worker's relative URL resolves correctly.
    exclude: ["maplibre-gl"],
  },
  worker: {
    // The maplibre worker entry (see pmtiles-protocol.ts) is loaded with
    // `new Worker(url, { type: "module" })`; build it as ESM to match.
    format: "es",
  },
  server: {
    port: process.env.PORT ? Number(process.env.PORT) : 5173,
    proxy: {
      "/v1": {
        target: "http://localhost:8600",
        changeOrigin: true,
      },
    },
  },
});
