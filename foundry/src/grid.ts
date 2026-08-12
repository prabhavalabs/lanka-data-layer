/**
 * Canonical Sri Lanka grid — the single spatial convention of the platform.
 * Contract: docs/architecture.md §1. The same constants and formulas exist in
 * shared/src/grid.ts; both are pinned by the same test vectors. Foundry
 * duplicates rather than imports shared so the offline pipeline never
 * depends on the runtime packages (see docs/architecture.md §5).
 */

export const GRID = {
  latMax: 10.0,
  latMin: 5.8,
  lonMin: 79.4,
  lonMax: 82.1,
  step: 0.001,
  rows: 4200,
  cols: 2700,
} as const;

export const MAX_CELL_ID = GRID.rows * GRID.cols - 1;

/** Returns the cell id for a WGS84 coordinate, or null when outside coverage. */
export function cellId(lat: number, lon: number): number | null {
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) return null;
  if (lat < GRID.latMin || lat >= GRID.latMax) return null;
  if (lon < GRID.lonMin || lon >= GRID.lonMax) return null;
  const row = Math.floor((GRID.latMax - lat) * 1000);
  const col = Math.floor((lon - GRID.lonMin) * 1000);
  // Guard float artifacts at the bbox edges (e.g. (10.0-5.8)*1000 === 4200.0000000000005)
  if (row < 0 || row >= GRID.rows || col < 0 || col >= GRID.cols) return null;
  return row * GRID.cols + col;
}

/** Center coordinate of a cell. Inverse of cellId up to cell resolution. */
export function cellCenter(id: number): { lat: number; lon: number } | null {
  if (!Number.isInteger(id) || id < 0 || id > MAX_CELL_ID) return null;
  const row = Math.floor(id / GRID.cols);
  const col = id % GRID.cols;
  return {
    lat: GRID.latMax - (row + 0.5) * GRID.step,
    lon: GRID.lonMin + (col + 0.5) * GRID.step,
  };
}

/** Bounding box [west, south, east, north] of a cell. */
export function cellBounds(id: number): [number, number, number, number] | null {
  if (!Number.isInteger(id) || id < 0 || id > MAX_CELL_ID) return null;
  const row = Math.floor(id / GRID.cols);
  const col = id % GRID.cols;
  const north = GRID.latMax - row * GRID.step;
  const west = GRID.lonMin + col * GRID.step;
  return [west, north - GRID.step, west + GRID.step, north];
}
