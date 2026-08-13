import { HeatmapLayer } from "@deck.gl/aggregation-layers";
import type { Color, PickingInfo } from "@deck.gl/core";

export const POPULATION_LAYER_ID = "population-3d";

/** One bucket from `GET /v1/population/grid?res=0.02`: `[lat, lon, pop]` (see api/src/routes/population.ts). */
export type PopulationCell = [number, number, number];

// res=0.02deg buckets are ~2.2km apart center-to-center. HeatmapLayer's
// radiusPixels is a *screen-space* blur radius, not a world-space one, so
// it isn't derived from that spacing directly — it just needs to be large
// enough that neighboring cells overlap into one continuous blob at every
// zoom the layer is ever shown at, and small enough that city-core
// clusters still read as distinct peaks rather than one island-wide wash.
//
// Worked at the layer's own ceiling (POPULATION_MAX_ZOOM = 11.5 in
// map-view.tsx, past which the layer hides itself): at ~7.9°N, meters/pixel
// there is ~156543*cos(7.9°)/2^11.5 ≈ 53.5, so the ~2.2km cell spacing is
// ~41 screen px at that zoom — almost exactly one radius, i.e. the
// tightest the grid ever packs on screen while this layer is visible. A
// radius that already blends neighbors there guarantees full overlap at
// every zoom below it too (island-wide views pack far more cells per
// pixel, yielding smoother, broader district blobs).
const HEATMAP_RADIUS_PIXELS = 40;

// HeatmapLayer sums getWeight over every cell within radiusPixels of a
// given screen pixel, then maps that sum linearly onto colorRange across
// [threshold * max, max] — max being whatever the single hottest
// aggregated screen pixel actually is, auto-computed per frame (colorDomain
// is intentionally left unset below so it keeps auto-fitting rather than
// clipping to a fixed ceiling). Feeding it raw population counts would make
// that max Colombo's own core and nothing else: the distribution is
// heavily right-skewed (p50 ~527, p90 ~3.5k, p99 ~15.6k, max ~106.7k in a
// single res=0.02 cell), so every other city's aggregate would land at a
// tiny fraction of that peak and mostly fall under the threshold cutoff.
// sqrt splits the difference: strong enough compression that secondary
// cities register against Colombo's peak, but — unlike log1p, which left
// barely 2x between the median cell and the max and made the whole island
// saturate uniformly hot — it preserves a ~14x median-to-max spread, so
// rural interior stays in the ramp's cool end while city cores climb to
// the top.
function heatmapWeight(pop: number): number {
  return Math.sqrt(Math.max(0, pop));
}

// Neutral intensity — with sqrt weights the ramp's dynamic range does the
// separating on its own; boosting past 1 was what washed the whole island
// to the ramp's top under the flatter log1p weights.
const HEATMAP_INTENSITY = 1;

// Ratio of the fading weight to the max weight (deck.gl default: 0.05).
// Lowered slightly so the smooth halo around inhabited areas extends a bit
// further into lower-density fringe before fading out — reads as
// continuous district-level coverage rather than a few isolated islands of
// color with hard edges.
const HEATMAP_THRESHOLD = 0.035;

// Explore design's population ramp (DESIGN-NOTES.md "Design tokens":
// #41102382 -> #8D153A -> #C73E3E -> #EF8A2C -> #FFD166) — one ramp for
// both themes, unlike the basemap/glass surfaces, which do split light/
// dark: the heatmap sits on top of whichever basemap is showing and needs
// to read the same way regardless.
const RAMP: readonly [number, number, number][] = [
  [65, 16, 35], // #411023
  [141, 21, 58], // #8D153A
  [199, 62, 62], // #C73E3E
  [239, 138, 44], // #EF8A2C
  [255, 209, 102], // #FFD166
];

// Alpha per RAMP stop, ascending — the lowest ramp stop is the most
// transparent so sparse/low-density areas fade toward the basemap instead
// of painting a flat wash over them. 130 (0x82) is the same coolest-stop
// alpha the legend's CSS gradient already carries (POPULATION_RAMP_CSS's
// "#41102382"): the old column layer's flat-alpha fill never actually used
// that channel, so the heatmap's colorRange is the first place it does.
const RAMP_ALPHA: readonly number[] = [130, 170, 200, 230, 255];

const HEATMAP_COLOR_RANGE: Color[] = RAMP.map(([r, g, b], i) => [r, g, b, RAMP_ALPHA[i]]);

/** The legend's CSS gradient — same stops as RAMP, keeping the faded-in alpha on the coolest stop that HEATMAP_COLOR_RANGE also now uses. */
export const POPULATION_RAMP_CSS =
  "linear-gradient(to right, #41102382 0%, #8D153A 25%, #C73E3E 50%, #EF8A2C 75%, #FFD166 100%)";

export interface BuildPopulationLayerOptions {
  cells: PopulationCell[];
  visible: boolean;
}

/**
 * Builds (or rebuilds) the population-3d layer as a flat, top-down
 * HeatmapLayer over the same `GET /v1/population/grid` cells the previous
 * ColumnLayer used — no elevation, no camera pitch coupling (see
 * map-view.tsx, which no longer tilts the camera when this layer toggles
 * on). Cheap enough to call on every visibility/data change — deck.gl
 * diffs props internally and only re-uploads what changed.
 */
export function buildPopulationLayer(options: BuildPopulationLayerOptions): HeatmapLayer<PopulationCell>[] {
  const { cells, visible } = options;

  const heatmap = new HeatmapLayer<PopulationCell>({
    id: POPULATION_LAYER_ID,
    data: cells,
    visible,
    // HeatmapLayer aggregates into a screen-space density texture rather
    // than picking discrete instances, so it doesn't support picking at
    // all (@deck.gl/aggregation-layers' heatmap-layer.js has no
    // encodePickingColor / pickable handling anywhere in it). Explicit
    // false rather than an implicit default, so that's documented instead
    // of silently relied upon.
    pickable: false,
    getPosition: (d) => [d[1], d[0]],
    getWeight: (d) => heatmapWeight(d[2]),
    radiusPixels: HEATMAP_RADIUS_PIXELS,
    intensity: HEATMAP_INTENSITY,
    threshold: HEATMAP_THRESHOLD,
    colorRange: HEATMAP_COLOR_RANGE,
    aggregation: "SUM",
  });

  return [heatmap];
}

/**
 * Tooltip text for a picked population cell, or null when nothing is under
 * the pointer. Still wired as the MapboxOverlay's global `getTooltip` in
 * map-view.tsx, but HeatmapLayer never actually picks anything (see
 * buildPopulationLayer's `pickable: false` above) — `info.object` is always
 * undefined for this layer, so this now always returns null in practice.
 * Left in place rather than removed: it's harmless, and still exported in
 * case a future pickable layer (e.g. a per-cell hover layer) wants it.
 */
export function populationTooltip(info: PickingInfo): { text: string } | null {
  const cell = info.object as PopulationCell | undefined;
  if (!cell) return null;
  return { text: `${Math.round(cell[2]).toLocaleString()} people` };
}
