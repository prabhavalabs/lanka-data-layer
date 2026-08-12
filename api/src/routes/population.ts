import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { cellId, GRID } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotInCoverageError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { latDegreesToMeters, metersToLatDegrees, metersToLonDegrees } from "../lib/geo.ts";

const MAX_RADIUS_KM = 10;

const QuerySchema = z.object({
  lat: z.coerce.number().finite(),
  lon: z.coerce.number().finite(),
  radius: z.coerce.number().min(0).max(MAX_RADIUS_KM).optional(),
});

/** True if `cells` has any rows at all — gridded population may not be built yet this phase. */
function cellsTableHasData(db: Database.Database): boolean {
  const row = prepared<{ present: number }>(db, "SELECT EXISTS(SELECT 1 FROM cells) as present").get();
  return row?.present === 1;
}

/**
 * Sums `cells.pop` within `radiusKm` of (lat, lon) using the same
 * row-band technique as the canonical grid: for each grid row within
 * range, work out how many columns on either side fall inside the circle
 * (via haversine-consistent meters↔degrees conversion) and sum that
 * row's slice with one indexed BETWEEN range query.
 */
function sumRadiusPopulation(
  db: Database.Database,
  lat: number,
  lon: number,
  radiusKm: number,
): { population: number; cell_count: number } {
  const radiusM = radiusKm * 1000;
  const centerRow = Math.floor((GRID.latMax - lat) * 1000);
  const centerCol = Math.floor((lon - GRID.lonMin) * 1000);
  const rowSpan = Math.ceil(metersToLatDegrees(radiusM) / GRID.step);

  const rangeStmt = prepared<{ pop: number; n: number }>(
    db,
    "SELECT COALESCE(SUM(pop), 0) as pop, COUNT(*) as n FROM cells WHERE cell_id BETWEEN ? AND ?",
  );

  let population = 0;
  let cellCount = 0;
  for (let row = centerRow - rowSpan; row <= centerRow + rowSpan; row++) {
    if (row < 0 || row >= GRID.rows) continue;
    const rowLat = GRID.latMax - (row + 0.5) * GRID.step;
    const verticalM = latDegreesToMeters(Math.abs(rowLat - lat));
    if (verticalM > radiusM) continue;
    const remainingM = Math.sqrt(radiusM * radiusM - verticalM * verticalM);
    const lonSpanDeg = metersToLonDegrees(remainingM, rowLat);
    const colSpan = Math.ceil(lonSpanDeg / GRID.step);
    const colMin = Math.max(0, centerCol - colSpan);
    const colMax = Math.min(GRID.cols - 1, centerCol + colSpan);
    if (colMin > colMax) continue;
    const result = rangeStmt.get(row * GRID.cols + colMin, row * GRID.cols + colMax);
    population += result?.pop ?? 0;
    cellCount += result?.n ?? 0;
  }
  return { population, cell_count: cellCount };
}

/**
 * GET /v1/population?lat&lon&radius — point lookup in `cells`, optionally
 * summed over a radius (≤10 km). Gracefully degrades when the gridded
 * population artifact hasn't been built yet (`cells` empty this phase).
 */
export function mountPopulationRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/population", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { lat, lon, radius } = parsed.data;

    const id = cellId(lat, lon);
    if (id === null) throw new NotInCoverageError();

    const populated = cellsTableHasData(db);
    const pointRow = prepared<{ pop: number }>(db, "SELECT pop FROM cells WHERE cell_id = ?").get(id);

    const payload: Record<string, unknown> = {
      cell_id: id,
      populated,
      point: {
        population: pointRow?.pop ?? 0,
        in_coverage: pointRow !== undefined,
      },
      radius: null,
    };

    if (radius !== undefined && radius > 0) {
      const sum = sumRadiusPopulation(db, lat, lon, radius);
      payload.radius = { radius_km: radius, population: sum.population, cell_count: sum.cell_count };
    }

    const meta = buildMeta(db, ["gridded_population"]);
    return c.json(ok(payload, meta));
  });
}
