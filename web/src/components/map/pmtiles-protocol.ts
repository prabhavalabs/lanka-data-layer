import { addProtocol } from "maplibre-gl";
import { Protocol } from "pmtiles";

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
const protocol = new Protocol();
addProtocol("pmtiles", protocol.tile);
