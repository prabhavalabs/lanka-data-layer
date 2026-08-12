/**
 * Pixel -> canonical fine-cell distribution math for the `cells` step
 * (gridded population). Given a raster pixel's WGS84 bounds, returns the
 * ids of every canonical 0.001deg grid cell (src/grid.ts) whose CENTER
 * falls inside those bounds — half-open on both axes [west,east) x
 * [south,north), matching the half-open convention `cellId`/`cellCenter`
 * already use for the outer Sri Lanka bbox.
 *
 * Derived directly from the grid formula (no per-cell iteration/lookup
 * needed): a fine cell's center is at
 *   lon = lonMin + (col + 0.5) * step
 *   lat = latMax - (row + 0.5) * step
 * so the set of cols/rows whose center falls in [west,east) / [south,north)
 * is a closed-form integer range.
 */
import { GRID } from "../grid.ts";

/** [west, south, east, north] in WGS84 degrees. */
export type PixelBounds = readonly [number, number, number, number];

/**
 * Fine-cell ids (row*cols+col) whose center lies inside `bounds`, clipped to
 * the canonical grid's own coverage (0 <= row < GRID.rows, 0 <= col < GRID.cols).
 * A pixel that lies entirely outside the canonical grid's bbox yields [].
 */
export function fineCellsForBounds(bounds: PixelBounds): number[] {
  const [west, south, east, north] = bounds;
  const { step, rows, cols, latMax, lonMin } = GRID;

  // col: lonMin + (col+0.5)*step in [west, east)
  //   >= west  <=>  col >= (west-lonMin)/step - 0.5
  //   <  east  <=>  col <  (east-lonMin)/step - 0.5
  const colStartRaw = Math.ceil((west - lonMin) / step - 0.5);
  const colEndRaw = Math.ceil((east - lonMin) / step - 0.5) - 1;

  // row: latMax - (row+0.5)*step in [south, north)
  //   >= south <=>  row <= (latMax-south)/step - 0.5
  //   <  north <=>  row >  (latMax-north)/step - 0.5
  const rowEndRaw = Math.floor((latMax - south) / step - 0.5);
  const rowStartRaw = Math.floor((latMax - north) / step - 0.5) + 1;

  const colStart = Math.max(0, colStartRaw);
  const colEnd = Math.min(cols - 1, colEndRaw);
  const rowStart = Math.max(0, rowStartRaw);
  const rowEnd = Math.min(rows - 1, rowEndRaw);

  const ids: number[] = [];
  if (colEnd < colStart || rowEnd < rowStart) return ids;
  for (let row = rowStart; row <= rowEnd; row++) {
    const base = row * cols;
    for (let col = colStart; col <= colEnd; col++) {
      ids.push(base + col);
    }
  }
  return ids;
}
