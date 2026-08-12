import type { Map as MaplibreMap, RasterTileSource, StyleSpecification } from "maplibre-gl";

export type BasemapMode = "light" | "dark";

export const BASEMAP_SOURCE_ID = "carto-basemap";
export const BASEMAP_LAYER_ID = "carto-basemap";

const OSM_ATTRIBUTION =
  '&copy; <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noreferrer">OpenStreetMap</a> contributors';
const CARTO_ATTRIBUTION =
  '&copy; <a href="https://carto.com/attributions" target="_blank" rel="noreferrer">CARTO</a>';

const TILE_SUBDOMAINS = ["a", "b", "c", "d"] as const;

// CARTO's raster tile URL scheme differs between styles: voyager lives
// under /rastertiles/, dark_all does not.
const TILE_URLS: Record<BasemapMode, string[]> = {
  light: TILE_SUBDOMAINS.map(
    (sub) => `https://${sub}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}@2x.png`
  ),
  dark: TILE_SUBDOMAINS.map(
    (sub) => `https://${sub}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}@2x.png`
  ),
};

/** Resolves "system" against the OS color-scheme media query. */
export function resolveBasemapMode(theme: "light" | "dark" | "system"): BasemapMode {
  if (theme === "light" || theme === "dark") return theme;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Initial MapLibre style: just the CARTO raster basemap for `mode`. */
export function basemapStyle(mode: BasemapMode): StyleSpecification {
  return {
    version: 8,
    sources: {
      [BASEMAP_SOURCE_ID]: {
        type: "raster",
        tiles: TILE_URLS[mode],
        tileSize: 256,
        attribution: `${CARTO_ATTRIBUTION} ${OSM_ATTRIBUTION}`,
      },
    },
    layers: [
      {
        id: BASEMAP_LAYER_ID,
        type: "raster",
        source: BASEMAP_SOURCE_ID,
      },
    ],
  };
}

/** Swaps the basemap's tile URLs in place (no full style reload). */
export function applyBasemapMode(map: MaplibreMap, mode: BasemapMode): void {
  const source = map.getSource(BASEMAP_SOURCE_ID) as RasterTileSource | undefined;
  source?.setTiles(TILE_URLS[mode]);
}
