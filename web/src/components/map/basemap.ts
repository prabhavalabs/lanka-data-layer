import type { Map as MaplibreMap, StyleSpecification } from "maplibre-gl";

export type BasemapMode = "light" | "dark";

export const BASEMAP_SOURCE_ID = "openfreemap-basemap";
export const BASEMAP_LAYER_PREFIX = "basemap-";

const OSM_ATTRIBUTION =
  '&copy; <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noreferrer">OpenStreetMap</a> contributors';
const BASEMAP_ATTRIBUTION =
  '<a href="https://openfreemap.org" target="_blank" rel="noreferrer">OpenFreeMap</a> ' +
  '&copy; <a href="https://openmaptiles.org" target="_blank" rel="noreferrer">OpenMapTiles</a> ' +
  OSM_ATTRIBUTION;

const PALETTE = {
  light: { land: "#F5F4F0", water: "#EAEFF3", park: "#DFE8DD", building: "#DEDCD5", road: "#FFFFFF", majorRoad: "#D7C9AE", label: "#53616D" },
  dark: { land: "#141A21", water: "#07090C", park: "#182720", building: "#232D37", road: "#303B47", majorRoad: "#46505C", label: "#94A1AE" },
} as const;

/** Resolves "system" against the OS color-scheme media query. */
export function resolveBasemapMode(theme: "light" | "dark" | "system"): BasemapMode {
  if (theme === "light" || theme === "dark") return theme;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Key-free OpenFreeMap vector tiles with a muted, theme-aware palette. */
export function basemapStyle(mode: BasemapMode): StyleSpecification {
  const colors = PALETTE[mode];
  return {
    version: 8,
    glyphs: "https://tiles.openfreemap.org/fonts/{fontstack}/{range}.pbf",
    sources: {
      [BASEMAP_SOURCE_ID]: {
        type: "vector",
        url: "https://tiles.openfreemap.org/planet",
        attribution: BASEMAP_ATTRIBUTION,
      },
    },
    layers: [
      {
        id: `${BASEMAP_LAYER_PREFIX}background`,
        type: "background",
        paint: { "background-color": colors.land },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}park`,
        type: "fill",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "landcover",
        filter: ["match", ["get", "class"], ["wood", "grass"], true, false],
        paint: { "fill-color": colors.park, "fill-opacity": 0.55 },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}water`,
        type: "fill",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "water",
        filter: ["!=", ["get", "brunnel"], "tunnel"],
        paint: { "fill-color": colors.water },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}waterway`,
        type: "line",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "waterway",
        minzoom: 9,
        paint: { "line-color": colors.water, "line-width": ["interpolate", ["linear"], ["zoom"], 9, 0.5, 16, 3] },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}building`,
        type: "fill",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "building",
        minzoom: 13,
        paint: { "fill-color": colors.building },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}road-minor`,
        type: "line",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "transportation",
        minzoom: 11,
        filter: ["match", ["get", "class"], ["minor", "service", "track", "tertiary"], true, false],
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": colors.road, "line-width": ["interpolate", ["linear"], ["zoom"], 11, 0.5, 18, 5] },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}road-major`,
        type: "line",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "transportation",
        filter: ["match", ["get", "class"], ["motorway", "trunk", "primary", "secondary"], true, false],
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": colors.majorRoad, "line-width": ["interpolate", ["linear"], ["zoom"], 6, 0.4, 12, 1.5, 18, 7] },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}road-label`,
        type: "symbol",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "transportation_name",
        minzoom: 13,
        layout: {
          "symbol-placement": "line",
          "text-field": ["coalesce", ["get", "name_en"], ["get", "name"]],
          "text-font": ["Noto Sans Regular"],
          "text-size": 11,
        },
        paint: { "text-color": colors.label, "text-halo-color": colors.land, "text-halo-width": 1.5 },
      },
      {
        id: `${BASEMAP_LAYER_PREFIX}place-label`,
        type: "symbol",
        source: BASEMAP_SOURCE_ID,
        "source-layer": "place",
        minzoom: 5,
        filter: ["step", ["zoom"], ["==", ["get", "class"], "city"], 8, ["match", ["get", "class"], ["city", "town"], true, false], 11, ["match", ["get", "class"], ["city", "town", "village", "suburb", "neighbourhood"], true, false]],
        layout: {
          "text-field": ["coalesce", ["get", "name_en"], ["get", "name"]],
          "text-font": ["Noto Sans Regular"],
          "text-size": ["interpolate", ["linear"], ["zoom"], 5, 10, 12, 14],
          "symbol-sort-key": ["coalesce", ["get", "rank"], 99],
        },
        paint: { "text-color": colors.label, "text-halo-color": colors.land, "text-halo-width": 1.5 },
      },
    ],
  };
}

/** Recolors the existing basemap, preserving overlays, selection and camera. */
export function applyBasemapMode(map: MaplibreMap, mode: BasemapMode): void {
  for (const layer of basemapStyle(mode).layers) {
    if (!map.getLayer(layer.id)) continue;
    for (const [property, value] of Object.entries(layer.paint ?? {})) {
      // Object.entries widens keys; these come from a typed style's paint.
      map.setPaintProperty(layer.id, property as Parameters<MaplibreMap["setPaintProperty"]>[1], value);
    }
  }
}
