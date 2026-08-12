import { ColumnLayer } from "@deck.gl/layers";
import type { PickingInfo } from "@deck.gl/core";

import type { BasemapMode } from "@/components/map/basemap";

export const POPULATION_LAYER_ID = "population-3d";

/** One bucket from `GET /v1/population/grid?res=0.02`: `[lat, lon, pop]` (see api/src/routes/population.ts). */
export type PopulationCell = [number, number, number];

// res=0.02deg buckets are ~2.2km square; a radius a bit under half that
// leaves a hairline gap between neighbors so the grid reads as discrete
// columns rather than a solid slab once extruded.
const CELL_RADIUS_M = 950;

// Elevation = sqrt(pop) * scale, not linear pop * scale. Tried linear first
// (per the task brief's "scale ~0.05-0.15" framing) and it breaks visually:
// the distribution at res=0.02 (14.1k populated cells) is p50 ~527, p90
// ~3.5k, p99 ~15.6k, max ~106.7k (central Colombo) — a single cell 200x the
// median. Any linear scale that gives typical cells a legible height (tens
// of meters) turns that one Colombo cell into a multi-kilometer monolith;
// at 0.1 it's 10.7km tall, which at any zoom close enough to actually see
// Colombo renders as a giant near-black wall filling the screen (confirmed
// visually — verified via deck.gl's picking API that the geometry/color
// pipeline was otherwise correct: right coordinates, valid RGBA, no WebGL
// errors, just an absurd height). sqrt compresses that outlier — max
// becomes ~1.1km (a dramatic peak, not an architectural impossibility)
// while typical cells still separate visibly from each other.
const ELEVATION_SCALE = 3.5;

// Color is mapped through log1p(pop) rather than linear pop: the
// distribution above is heavily right-skewed, so a linear color domain
// would render nearly every cell in the coolest/palest bucket and waste
// the ramp on Colombo alone. log1p spreads the mid-range (the p50-p90 band
// where most of the country's population actually lives) across visibly
// different colors. Domain ceiling is the observed p99.9 (~38.4k); higher
// outliers just clip to the ramp's hottest stop instead of compressing
// everything else.
const COLOR_DOMAIN_MAX = 38_000;

// Warm ramp (light mode): pale straw -> amber -> vermillion -> deep maroon.
const LIGHT_RAMP: [number, number, number][] = [
  [255, 247, 188],
  [254, 196, 79],
  [240, 59, 32],
  [128, 0, 38],
];

// Cool-to-hot ramp (dark mode): deep blue -> cyan -> amber -> deep red.
const DARK_RAMP: [number, number, number][] = [
  [8, 29, 88],
  [65, 182, 196],
  [254, 178, 76],
  [189, 0, 38],
];

function rampColor(t: number, ramp: readonly [number, number, number][]): [number, number, number] {
  const clamped = Math.min(1, Math.max(0, t));
  const scaled = clamped * (ramp.length - 1);
  const i = Math.min(ramp.length - 2, Math.floor(scaled));
  const f = scaled - i;
  const [r0, g0, b0] = ramp[i];
  const [r1, g1, b1] = ramp[i + 1];
  return [r0 + (r1 - r0) * f, g0 + (g1 - g0) * f, b0 + (b1 - b0) * f];
}

function rampForMode(mode: BasemapMode): readonly [number, number, number][] {
  return mode === "dark" ? DARK_RAMP : LIGHT_RAMP;
}

function colorForPop(pop: number, mode: BasemapMode): [number, number, number, number] {
  const t = Math.log1p(Math.max(0, pop)) / Math.log1p(COLOR_DOMAIN_MAX);
  const [r, g, b] = rampColor(t, rampForMode(mode));
  return [r, g, b, 225];
}

export interface BuildPopulationLayerOptions {
  cells: PopulationCell[];
  mode: BasemapMode;
  visible: boolean;
}

/** Builds (or rebuilds) the population-3d ColumnLayer. Cheap enough to call on every visibility/theme/data change — deck.gl diffs props internally and only re-uploads what changed. */
export function buildPopulationLayer(options: BuildPopulationLayerOptions): ColumnLayer<PopulationCell> {
  const { cells, mode, visible } = options;
  return new ColumnLayer<PopulationCell>({
    id: POPULATION_LAYER_ID,
    data: cells,
    visible,
    pickable: true,
    extruded: true,
    diskResolution: 6,
    radius: CELL_RADIUS_M,
    getPosition: (d) => [d[1], d[0]],
    getElevation: (d) => Math.sqrt(Math.max(0, d[2])) * ELEVATION_SCALE,
    getFillColor: (d) => colorForPop(d[2], mode),
    material: { ambient: 0.4, diffuse: 0.7, shininess: 24, specularColor: [255, 255, 255] },
    // getFillColor is deliberately not transitioned (only getElevation is):
    // a theme switch should recolor immediately, and animating through the
    // intermediate ramp colors on every data refresh reads as a glitch,
    // not a feature.
    transitions: { getElevation: 300 },
    updateTriggers: { getFillColor: [mode] },
  });
}

/** Tooltip text for a picked population column, or null when nothing is under the pointer. Passed as `getTooltip` to the MapboxOverlay. */
export function populationTooltip(info: PickingInfo): { text: string } | null {
  const cell = info.object as PopulationCell | undefined;
  if (!cell) return null;
  return { text: `${Math.round(cell[2]).toLocaleString()} people` };
}

/** CSS gradient for the legend's ramp bar, matching the deck.gl color stops for the current theme. */
export function populationRampCss(mode: BasemapMode): string {
  const ramp = rampForMode(mode);
  const stops = ramp.map(([r, g, b], i) => `rgb(${r}, ${g}, ${b}) ${(i / (ramp.length - 1)) * 100}%`);
  return `linear-gradient(to right, ${stops.join(", ")})`;
}
