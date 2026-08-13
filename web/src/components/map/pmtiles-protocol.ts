import { addProtocol, setWorkerUrl } from "maplibre-gl";
import { Protocol } from "pmtiles";
import maplibreWorkerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";

/**
 * Registers the `pmtiles://` custom protocol with MapLibre so vector
 * sources can point straight at the API's static PMTiles passthrough
 * (docs/architecture.md §4) via `pmtilesUrl()` (see lib/tile-url.ts).
 *
 * Side-effecting module, imported once for its effect (by map-view.tsx).
 * ES modules only evaluate once no matter how many places import it, so
 * this is safe to import from more than one place without double-registering.
 *
 * A single shared `Protocol` instance also de-dupes PMTiles readers by URL
 * internally — LAYER_REGISTRY entries that reference the same archive (e.g.
 * admin.pmtiles backing all four adm1..adm4 layers) share one reader instead
 * of opening the archive once per style layer.
 */
// MapLibre resolves its tile-parsing worker as
// `new URL("./maplibre-gl-worker.mjs", import.meta.url)`. Once Rollup
// bundles the library that base URL becomes our own /assets/index-*.js, so
// production requested /assets/maplibre-gl-worker.mjs — a file the build
// never emits — and every vector/GeoJSON source died with it (black map).
// Dev never showed it because the module is served straight from
// node_modules there. `?worker&url` (not plain `?url`: the worker script
// statically imports its sibling maplibre-gl-shared.mjs, which a bare asset
// copy leaves behind) makes Vite bundle the worker entry with its deps into
// one self-contained chunk and hands back its final URL.
setWorkerUrl(maplibreWorkerUrl);

const protocol = new Protocol();
addProtocol("pmtiles", protocol.tile);
