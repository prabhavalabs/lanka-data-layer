import { defineConfig } from "tsup";

// Forces .mjs / .cjs extensions for both output formats regardless of the
// package's own "type" field, matching the exports map in package.json.
export default defineConfig({
  outExtension({ format }) {
    return { js: format === "cjs" ? ".cjs" : ".mjs" };
  },
});
