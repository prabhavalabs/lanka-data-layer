import { ColumnLayer } from "@deck.gl/layers";
import type { PickingInfo } from "@deck.gl/core";

export const POPULATION_LAYER_ID = "population-3d";
const POPULATION_CAP_LAYER_ID = `${POPULATION_LAYER_ID}-caps`;

/** One bucket from `GET /v1/population/grid?res=0.02`: `[lat, lon, pop]` (see api/src/routes/population.ts). */
export type PopulationCell = [number, number, number];

// res=0.02deg buckets are ~2.2km square; half that (center to edge) is
// ~1.1km. The Explore design calls for "slim columns (radius ~55% of
// cell)" — read as 55% of that half-width, not the full cell — so columns
// sit well clear of their neighbors and read as discrete spikes rather
// than a slab.
const CELL_HALF_WIDTH_M = 1100;
const CELL_RADIUS_M = Math.round(CELL_HALF_WIDTH_M * 0.55); // ~605m

// Color and elevation both key off the same log-normalized value (t, see
// normalizedValue below) rather than raw population, so a cell's height and
// hue always agree. The distribution at res=0.02 (14.1k populated cells) is
// heavily right-skewed — p50 ~527, p90 ~3.5k, p99 ~15.6k, max ~106.7k
// (central Colombo) — so a linear domain would blow the scale out on one
// cell; log1p spreads the p50-p90 band (where most of the country's
// population actually lives) across visibly different values. Domain
// ceiling is the observed p99.9 (~38.4k); higher outliers just clip to 1
// instead of compressing everything else. Raising that normalized value to
// a >1 exponent for elevation (per the Explore design's "elevation ∝
// pow(normalized, 1.7)") then pushes ordinary cells down relative to the
// hottest ones — the skyline reads as gentle countryside cresting into a
// handful of dramatic peaks, not a field of same-height blocks.
const COLOR_DOMAIN_MAX = 38_000;
const ELEVATION_EXPONENT = 1.7;
const MAX_ELEVATION_M = 1600;
const MIN_ELEVATION_M = 12;

// Cells at/above this percentile of the *current* dataset get a second,
// thinner, brighter "cap" layer stacked near their tip — deck.gl's
// ColumnLayer can't gradient a single column's fill top-to-bottom, so this
// is the approximation the design brief calls for: two flat-shaded layers
// standing in for one gradiented one.
const CAP_PERCENTILE = 0.85;
const CAP_RADIUS_RATIO = 0.5;
const CAP_HEIGHT_RATIO = 0.22;
const CAP_BASE_RATIO = 1 - CAP_HEIGHT_RATIO;

// Explore design's population ramp (DESIGN-NOTES.md "Design tokens":
// #41102382 -> #8D153A -> #C73E3E -> #EF8A2C -> #FFD166) — one ramp for
// both themes, unlike the basemap/glass surfaces, which do split light/
// dark: the columns sit on top of whichever basemap is showing and need to
// read the same way regardless.
const RAMP: readonly [number, number, number][] = [
  [65, 16, 35], // #411023 (the legend's #41102382 stop, alpha dropped — the column fill's alpha is applied uniformly below instead)
  [141, 21, 58], // #8D153A
  [199, 62, 62], // #C73E3E
  [239, 138, 44], // #EF8A2C
  [255, 209, 102], // #FFD166
];

/** The legend's CSS gradient — same stops as RAMP, keeping the faded-in alpha on the coolest stop that the (opaque) deck.gl fill doesn't use. */
export const POPULATION_RAMP_CSS =
  "linear-gradient(to right, #41102382 0%, #8D153A 25%, #C73E3E 50%, #EF8A2C 75%, #FFD166 100%)";

function rampColor(t: number): [number, number, number] {
  const clamped = Math.min(1, Math.max(0, t));
  const scaled = clamped * (RAMP.length - 1);
  const i = Math.min(RAMP.length - 2, Math.floor(scaled));
  const f = scaled - i;
  const [r0, g0, b0] = RAMP[i];
  const [r1, g1, b1] = RAMP[i + 1];
  return [r0 + (r1 - r0) * f, g0 + (g1 - g0) * f, b0 + (b1 - b0) * f];
}

/** Log-normalizes `pop` against COLOR_DOMAIN_MAX, clamped to [0, 1]. Drives both color and elevation. */
function normalizedValue(pop: number): number {
  const t = Math.log1p(Math.max(0, pop)) / Math.log1p(COLOR_DOMAIN_MAX);
  return Math.min(1, Math.max(0, t));
}

function colorForValue(t: number): [number, number, number, number] {
  const [r, g, b] = rampColor(t);
  return [r, g, b, 225];
}

/** Lightens an RGB triple toward white — stands in for the hot-cell "cap" layer's brighter tint. */
function brighten([r, g, b]: readonly [number, number, number], amount: number): [number, number, number, number] {
  return [r + (255 - r) * amount, g + (255 - g) * amount, b + (255 - b) * amount, 255];
}

function elevationForValue(t: number): number {
  return MIN_ELEVATION_M + Math.pow(t, ELEVATION_EXPONENT) * (MAX_ELEVATION_M - MIN_ELEVATION_M);
}

/** The value at the given percentile (0-1) of `sortedAscending`. Returns Infinity for an empty array so a percentile filter over it naturally yields no cells. */
function percentile(sortedAscending: number[], p: number): number {
  if (sortedAscending.length === 0) return Infinity;
  const idx = Math.min(sortedAscending.length - 1, Math.max(0, Math.floor(p * sortedAscending.length)));
  return sortedAscending[idx];
}

export interface BuildPopulationLayerOptions {
  cells: PopulationCell[];
  visible: boolean;
}

/**
 * Builds (or rebuilds) the population-3d layers: the main ColumnLayer for
 * every cell, plus — for cells at/above the 85th percentile of the current
 * dataset — a second, thinner, brighter "cap" layer floating near the top
 * of the main column (via a z-offset getPosition) to approximate a glow
 * deck.gl can't render as a true per-column gradient. Cheap enough to call
 * on every visibility/data change — deck.gl diffs props internally and only
 * re-uploads what changed.
 */
export function buildPopulationLayer(options: BuildPopulationLayerOptions): ColumnLayer<PopulationCell>[] {
  const { cells, visible } = options;

  const main = new ColumnLayer<PopulationCell>({
    id: POPULATION_LAYER_ID,
    data: cells,
    visible,
    pickable: true,
    extruded: true,
    diskResolution: 6,
    radius: CELL_RADIUS_M,
    getPosition: (d) => [d[1], d[0]],
    getElevation: (d) => elevationForValue(normalizedValue(d[2])),
    getFillColor: (d) => colorForValue(normalizedValue(d[2])),
    material: { ambient: 0.55, diffuse: 0.65, shininess: 28, specularColor: [255, 255, 255] },
    // getFillColor is deliberately not transitioned (only getElevation is):
    // a data refresh should recolor immediately, not glitch through
    // intermediate ramp colors.
    transitions: { getElevation: 300 },
  });

  const sortedPops = cells.map((c) => c[2]).sort((a, b) => a - b);
  const hotThreshold = percentile(sortedPops, CAP_PERCENTILE);
  const hotCells = cells.filter((c) => c[2] >= hotThreshold);

  const caps = new ColumnLayer<PopulationCell>({
    id: POPULATION_CAP_LAYER_ID,
    data: hotCells,
    visible: visible && hotCells.length > 0,
    pickable: false,
    extruded: true,
    diskResolution: 6,
    radius: CELL_RADIUS_M * CAP_RADIUS_RATIO,
    // LNGLAT coordinates take a meters altitude as their third component —
    // floating the cap's base near the top of the matching main column is
    // what makes it read as a bright tip rather than a second full column.
    getPosition: (d) => [d[1], d[0], elevationForValue(normalizedValue(d[2])) * CAP_BASE_RATIO],
    getElevation: (d) => elevationForValue(normalizedValue(d[2])) * CAP_HEIGHT_RATIO,
    getFillColor: (d) => brighten(rampColor(normalizedValue(d[2])), 0.4),
    material: { ambient: 0.85, diffuse: 0.5, shininess: 40, specularColor: [255, 255, 255] },
    transitions: { getElevation: 300 },
  });

  return [main, caps];
}

/** Tooltip text for a picked population column, or null when nothing is under the pointer. Passed as `getTooltip` to the MapboxOverlay. */
export function populationTooltip(info: PickingInfo): { text: string } | null {
  const cell = info.object as PopulationCell | undefined;
  if (!cell) return null;
  return { text: `${Math.round(cell[2]).toLocaleString()} people` };
}
