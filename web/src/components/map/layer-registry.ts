import type { LayerSpecification, Map as MaplibreMap, SourceSpecification } from "maplibre-gl";

import type { BasemapMode } from "@/components/map/basemap";
import { POPULATION_LAYER_ID } from "@/components/map/population-layer";
import { pmtilesUrl } from "@/lib/tile-url";

/**
 * Groups drive both the layer panel's sections and iteration order —
 * see layer-panel.tsx's GROUP_ORDER.
 */
export type LayerGroup = "boundaries" | "electoral" | "infrastructure" | "environment" | "population";

export const LAYER_GROUP_LABELS: Record<LayerGroup, string> = {
  boundaries: "Boundaries",
  electoral: "Electoral",
  infrastructure: "Infrastructure",
  environment: "Environment",
  population: "Population",
};

/** Paint overrides keyed by style layer id, applied on top of that layer's static spec. */
type ThemedPaint = Record<string, Record<string, unknown>>;

/**
 * A MapLibre-rendered layer: a vector/geojson source plus the style layers
 * drawn from it. `layers`' non-color paint/layout props (widths, filters,
 * dasharrays, zoom ramps) are static; anything that needs to differ between
 * light/dark basemaps lives in `themedPaint` instead, applied at both
 * initial layer creation and on every theme change (see MapView, paired
 * with basemap.ts's applyBasemapMode).
 */
export interface MaplibreLayerDefinition {
  kind: "maplibre";
  /** Also used as the MapLibre source id — must be unique in the registry. */
  id: string;
  title: string;
  group: LayerGroup;
  source: SourceSpecification;
  layers: LayerSpecification[];
  defaultVisible: boolean;
  themedPaint?: (mode: BasemapMode) => ThemedPaint;
  /** Small legend swatch color for the current theme. */
  legendColor?: (mode: BasemapMode) => string;
}

/**
 * A deck.gl layer, built and owned imperatively by MapView (see
 * population-layer.ts) rather than via addSource/addLayer — deck.gl draws
 * into its own interleaved MapboxOverlay, not the MapLibre style. Still
 * lives in the registry so the layer panel and layer-store visibility
 * toggle stay generic over both kinds.
 */
export interface DeckLayerDefinition {
  kind: "deck";
  id: string;
  title: string;
  group: LayerGroup;
  defaultVisible: boolean;
}

export type LayerDefinition = MaplibreLayerDefinition | DeckLayerDefinition;

// ---------------------------------------------------------------------------
// Admin boundaries (admin.pmtiles: adm1..adm4, docs/architecture.md §3).
// One hue per level, lifted from the ceylon-hub reference's per-admin-level
// theming (provinces/districts/ds-divisions in that repo map roughly to our
// adm1/adm2/adm3; adm4 — GN divisions, the finest level — gets a fourth,
// unused-elsewhere hue). adm1 has no minzoom (always on once the layer is
// visible); adm2/3/4 gate progressively by zoom, matching the per-feature
// tippecanoe.minzoom stamps foundry/src/steps/tiles.ts already bakes into
// the tiles (6/8/10) — setting it again on the style layer is redundant
// with that but keeps the intent readable here and is harmless.
// ---------------------------------------------------------------------------

const ADMIN_LEVELS = [
  { sourceLayer: "adm1", minzoom: 0, light: "#0f766e", dark: "#2dd4bf" }, // teal
  { sourceLayer: "adm2", minzoom: 6, light: "#4338ca", dark: "#818cf8" }, // indigo
  { sourceLayer: "adm3", minzoom: 8, light: "#b45309", dark: "#fbbf24" }, // amber
  { sourceLayer: "adm4", minzoom: 10, light: "#7c3aed", dark: "#c4b5fd" }, // violet
] as const;

function adminFillId(level: string): string {
  return `admin-${level}-fill`;
}
function adminLineId(level: string): string {
  return `admin-${level}-line`;
}

/**
 * Builds the fill+line pair for each admin level. A plain loop rather than
 * `ADMIN_LEVELS.flatMap(...)` returning a two-element array literal: TS
 * infers a flatMap callback's return type as a single merged shape across
 * both branches (fill's `layout: {}` and line's `layout: {line-join,
 * line-cap}` collapse into one type with both sets of keys optional),
 * which then fails FillLayerSpecification's stricter layout check. Casting
 * each push individually sidesteps that inference entirely.
 */
function buildAdminLayers(): LayerSpecification[] {
  const layers: LayerSpecification[] = [];
  for (const { sourceLayer, minzoom } of ADMIN_LEVELS) {
    layers.push({
      id: adminFillId(sourceLayer),
      type: "fill",
      source: "admin",
      "source-layer": sourceLayer,
      minzoom,
      layout: {},
      paint: {
        "fill-color": "#000000",
        "fill-opacity": ["case", ["boolean", ["feature-state", "hover"], false], 0.18, 0.02],
      },
    } as LayerSpecification);
    layers.push({
      id: adminLineId(sourceLayer),
      type: "line",
      source: "admin",
      "source-layer": sourceLayer,
      minzoom,
      layout: { "line-join": "round", "line-cap": "round" },
      paint: {
        "line-color": "#000000",
        "line-width": ["case", ["boolean", ["feature-state", "hover"], false], 2, 1],
        "line-opacity": 0.8,
      },
    } as LayerSpecification);
  }
  return layers;
}

const adminLayer: MaplibreLayerDefinition = {
  kind: "maplibre",
  id: "admin",
  title: "Admin boundaries",
  group: "boundaries",
  defaultVisible: true,
  source: {
    type: "vector",
    url: pmtilesUrl("admin"),
    // All four source-layers use the same property name for their unique
    // id, so the string form applies across the board (per the MapLibre
    // style spec: "specified as a string ... used across all its source
    // layers"). Needed for feature-state hover highlighting below — the
    // tiles carry no numeric tippecanoe id.
    promoteId: "pcode",
  },
  layers: buildAdminLayers(),
  themedPaint: (mode) => {
    const themed: ThemedPaint = {};
    for (const level of ADMIN_LEVELS) {
      const color = mode === "dark" ? level.dark : level.light;
      themed[adminFillId(level.sourceLayer)] = { "fill-color": color };
      themed[adminLineId(level.sourceLayer)] = { "line-color": color };
    }
    return themed;
  },
  legendColor: (mode) => (mode === "dark" ? ADMIN_LEVELS[1].dark : ADMIN_LEVELS[1].light),
};

/** Style layer ids MapView hit-tests for the admin hover card. */
export const ADMIN_HOVER_LAYER_IDS = ADMIN_LEVELS.map((l) => adminFillId(l.sourceLayer));
export const ADMIN_SOURCE_ID = "admin";

// ---------------------------------------------------------------------------
// Electoral (electoral.pmtiles: ed, pd). Off by default — this is a
// specialist overlay, not base map furniture.
// ---------------------------------------------------------------------------

const electoralLayer: MaplibreLayerDefinition = {
  kind: "maplibre",
  id: "electoral",
  title: "Electoral divisions",
  group: "electoral",
  defaultVisible: false,
  source: { type: "vector", url: pmtilesUrl("electoral") },
  layers: [
    {
      id: "electoral-ed-line",
      type: "line",
      source: "electoral",
      "source-layer": "ed",
      layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#be185d", "line-width": 1.3, "line-opacity": 0.85 },
    },
    {
      id: "electoral-pd-line",
      type: "line",
      source: "electoral",
      "source-layer": "pd",
      minzoom: 7,
      layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#c2410c", "line-width": 0.7, "line-opacity": 0.75 },
    },
  ],
  themedPaint: (mode) => ({
    "electoral-ed-line": { "line-color": mode === "dark" ? "#f472b6" : "#be185d" },
    "electoral-pd-line": { "line-color": mode === "dark" ? "#fb923c" : "#c2410c" },
  }),
  legendColor: (mode) => (mode === "dark" ? "#f472b6" : "#be185d"),
};

// ---------------------------------------------------------------------------
// Transport (transport.pmtiles: roads, railways). Off by default.
// Width/color ramps lifted from the ceylon-hub reference's roads-layer.ts
// (per this file's previous revision's comment pointing at it): highway
// class graduated by zoom, casing-free here since we only need one line per
// class rather than the predecessor's selected-segment highlight state.
// Railways include both LineString track (kind="rail") and Point
// stations/halts in the same source-layer — split by geometry-type.
// ---------------------------------------------------------------------------

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- MapLibre's expression types don't narrow well through nested match/interpolate; the ceylon-hub reference this is lifted from casts the same way.
type Expr = any;

function roadWidthExpr(): Expr {
  return [
    "interpolate",
    ["linear"],
    ["zoom"],
    6,
    ["match", ["get", "highway"], "motorway", 1.2, "trunk", 0.9, "primary", 0.6, "secondary", 0, 0],
    9,
    ["match", ["get", "highway"], "motorway", 2.2, "trunk", 1.6, "primary", 1.1, "secondary", 0.7, 0.4],
    13,
    ["match", ["get", "highway"], "motorway", 5, "trunk", 3.8, "primary", 2.8, "secondary", 2, 1.4],
  ];
}

function roadColorExpr(mode: BasemapMode): Expr {
  const ramp =
    mode === "dark"
      ? { motorway: "#fb7185", trunk: "#fdba74", primary: "#fbbf24", secondary: "#fef3c7" }
      : { motorway: "#e11d48", trunk: "#ea580c", primary: "#f59e0b", secondary: "#fde68a" };
  return ["match", ["get", "highway"], "motorway", ramp.motorway, "trunk", ramp.trunk, "primary", ramp.primary, ramp.secondary, ramp.secondary, ramp.secondary];
}

const transportLayer: MaplibreLayerDefinition = {
  kind: "maplibre",
  id: "transport",
  title: "Roads & railways",
  group: "infrastructure",
  defaultVisible: false,
  source: { type: "vector", url: pmtilesUrl("transport") },
  layers: [
    {
      id: "transport-roads-line",
      type: "line",
      source: "transport",
      "source-layer": "roads",
      layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
      paint: {
        "line-color": "#f59e0b",
        "line-width": roadWidthExpr(),
        "line-opacity": 0.9,
      },
    } as LayerSpecification,
    {
      id: "transport-rail-line",
      type: "line",
      source: "transport",
      "source-layer": "railways",
      filter: ["==", ["geometry-type"], "LineString"],
      layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
      paint: {
        "line-color": "#475569",
        "line-width": 1,
        "line-dasharray": [2, 1.5],
        "line-opacity": 0.85,
      },
    },
    {
      id: "transport-rail-station",
      type: "circle",
      source: "transport",
      "source-layer": "railways",
      filter: ["==", ["geometry-type"], "Point"],
      minzoom: 9,
      layout: { visibility: "none" },
      paint: {
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 9, 1.5, 13, 3.5],
        "circle-color": "#475569",
        "circle-stroke-color": "#ffffff",
        "circle-stroke-width": 1,
      },
    },
  ],
  themedPaint: (mode) => ({
    "transport-roads-line": { "line-color": roadColorExpr(mode) },
    "transport-rail-line": { "line-color": mode === "dark" ? "#cbd5e1" : "#475569" },
    "transport-rail-station": {
      "circle-color": mode === "dark" ? "#cbd5e1" : "#475569",
      "circle-stroke-color": mode === "dark" ? "#0f172a" : "#ffffff",
    },
  }),
  legendColor: (mode) => (mode === "dark" ? "#fdba74" : "#ea580c"),
};

// ---------------------------------------------------------------------------
// Water (water.pmtiles: waterways — mixed geometry, rivers as LineString
// and reservoirs/lakes as Polygon under the same source-layer). Off by
// default.
// ---------------------------------------------------------------------------

const waterLayer: MaplibreLayerDefinition = {
  kind: "maplibre",
  id: "water",
  title: "Waterways",
  group: "environment",
  defaultVisible: false,
  source: { type: "vector", url: pmtilesUrl("water") },
  layers: [
    {
      id: "water-fill",
      type: "fill",
      source: "water",
      "source-layer": "waterways",
      filter: ["==", ["geometry-type"], "Polygon"],
      layout: { visibility: "none" },
      paint: { "fill-color": "#38bdf8", "fill-opacity": 0.35 },
    },
    {
      id: "water-line",
      type: "line",
      source: "water",
      "source-layer": "waterways",
      filter: ["==", ["geometry-type"], "LineString"],
      layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#0284c7", "line-width": 1, "line-opacity": 0.8 },
    },
  ],
  themedPaint: (mode) => ({
    "water-fill": { "fill-color": mode === "dark" ? "#0ea5e9" : "#38bdf8" },
    "water-line": { "line-color": mode === "dark" ? "#38bdf8" : "#0284c7" },
  }),
  legendColor: (mode) => (mode === "dark" ? "#38bdf8" : "#0284c7"),
};

// ---------------------------------------------------------------------------
// Protected areas (protected.pmtiles: protected — parks/reserves, all
// Polygon). Off by default.
// ---------------------------------------------------------------------------

const protectedLayer: MaplibreLayerDefinition = {
  kind: "maplibre",
  id: "protected",
  title: "Protected areas",
  group: "environment",
  defaultVisible: false,
  source: { type: "vector", url: pmtilesUrl("protected") },
  layers: [
    {
      id: "protected-fill",
      type: "fill",
      source: "protected",
      "source-layer": "protected",
      layout: { visibility: "none" },
      paint: { "fill-color": "#16a34a", "fill-opacity": 0.14 },
    },
    {
      id: "protected-line",
      type: "line",
      source: "protected",
      "source-layer": "protected",
      layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#15803d", "line-width": 1, "line-opacity": 0.8 },
    },
  ],
  themedPaint: (mode) => ({
    "protected-fill": { "fill-color": mode === "dark" ? "#4ade80" : "#16a34a" },
    "protected-line": { "line-color": mode === "dark" ? "#4ade80" : "#15803d" },
  }),
  legendColor: (mode) => (mode === "dark" ? "#4ade80" : "#15803d"),
};

// ---------------------------------------------------------------------------
// Places (places.pmtiles: places — settlements; postal is in the same
// archive but out of scope here). On by default: low visual noise once
// zoom-gated, and useful as base map furniture rather than a specialist
// overlay. Circle + label zoom/kind ramps lifted from the ceylon-hub
// reference's cities-layer.ts.
// ---------------------------------------------------------------------------

const placesLayer: MaplibreLayerDefinition = {
  kind: "maplibre",
  id: "places",
  title: "Places",
  group: "infrastructure",
  defaultVisible: true,
  source: { type: "vector", url: pmtilesUrl("places") },
  layers: [
    {
      id: "places-circle",
      type: "circle",
      source: "places",
      "source-layer": "places",
      minzoom: 5,
      layout: {},
      paint: {
        "circle-color": "#1e293b",
        "circle-stroke-color": "#ffffff",
        "circle-stroke-width": 1,
        "circle-opacity": ["match", ["get", "kind"], "city", 0.95, "town", 0.85, 0.6],
        "circle-radius": [
          "interpolate",
          ["linear"],
          ["zoom"],
          6,
          ["match", ["get", "kind"], "city", 4, "town", 2, 0],
          9,
          ["match", ["get", "kind"], "city", 5, "town", 3, 1.5],
          12,
          ["match", ["get", "kind"], "city", 6, "town", 4, 2.5],
        ] as unknown as LayerSpecification["paint"],
      },
    } as LayerSpecification,
    {
      id: "places-label",
      type: "symbol",
      source: "places",
      "source-layer": "places",
      minzoom: 6,
      layout: {
        "text-field": ["coalesce", ["get", "name_en"], ""],
        "text-size": [
          "interpolate",
          ["linear"],
          ["zoom"],
          6,
          ["match", ["get", "kind"], "city", 11, 0],
          9,
          ["match", ["get", "kind"], "city", 13, "town", 10, 0],
          12,
          ["match", ["get", "kind"], "city", 14, "town", 12, 10],
        ] as unknown as LayerSpecification["layout"],
        "text-font": ["Open Sans Regular"],
        "text-anchor": "top",
        "text-offset": [0, 0.6],
        "text-optional": true,
        "text-allow-overlap": false,
        "symbol-sort-key": ["match", ["get", "kind"], "city", 1, "town", 2, 3] as unknown as LayerSpecification["layout"],
      },
      paint: {
        "text-color": "#0f172a",
        "text-halo-color": "#ffffff",
        "text-halo-width": 1.4,
        "text-halo-blur": 0.5,
      },
    } as LayerSpecification,
  ],
  themedPaint: (mode) => ({
    "places-circle": {
      "circle-color": mode === "dark" ? "#f8fafc" : "#1e293b",
      "circle-stroke-color": mode === "dark" ? "#0f172a" : "#ffffff",
    },
    "places-label": {
      "text-color": mode === "dark" ? "#f1f5f9" : "#0f172a",
      "text-halo-color": mode === "dark" ? "#0b1220" : "#ffffff",
    },
  }),
  legendColor: (mode) => (mode === "dark" ? "#f8fafc" : "#1e293b"),
};

// ---------------------------------------------------------------------------
// Population-3d: deck.gl ColumnLayer, built imperatively by MapView from
// population-layer.ts — see that file for the elevation/color tuning.
// ---------------------------------------------------------------------------

const populationLayer: DeckLayerDefinition = {
  kind: "deck",
  id: POPULATION_LAYER_ID,
  title: "Population (3D)",
  group: "population",
  defaultVisible: false,
};

export const LAYER_REGISTRY: LayerDefinition[] = [
  adminLayer,
  electoralLayer,
  transportLayer,
  waterLayer,
  protectedLayer,
  placesLayer,
  populationLayer,
];

/**
 * Re-applies every registry entry's theme-dependent paint props. Paired
 * with basemap.ts's applyBasemapMode — call both when the resolved theme
 * changes. `setPaintProperty`'s real signature is generic over a specific
 * paint-property key per layer type, which our dynamically-keyed
 * ThemedPaint records can't satisfy statically; the cast below is the one
 * place that gets bypassed; the values themselves still come from this
 * file's own typed layer specs.
 */
export function applyThemedPaint(map: MaplibreMap, mode: BasemapMode): void {
  for (const layer of LAYER_REGISTRY) {
    if (layer.kind !== "maplibre" || !layer.themedPaint) continue;
    const themed = layer.themedPaint(mode);
    for (const [layerId, paint] of Object.entries(themed)) {
      if (!map.getLayer(layerId)) continue;
      for (const [prop, value] of Object.entries(paint)) {
        map.setPaintProperty(layerId, prop as never, value as never);
      }
    }
  }
}
