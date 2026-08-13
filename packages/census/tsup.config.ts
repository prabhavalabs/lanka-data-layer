import { defineConfig } from "tsup";

// tsup defaults the ESM output extension to .js when package.json sets
// "type": "module" (and .mjs only for the non-default type). This package's
// exports map is fixed to index.mjs/index.cjs regardless of "type", so pin
// the extensions explicitly rather than relying on that default.
export default defineConfig({
  outExtension({ format }) {
    return { js: format === "cjs" ? ".cjs" : ".mjs" };
  },
});
