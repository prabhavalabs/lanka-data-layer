import { fromFile } from "geotiff";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { fineCellsForBounds } from "../lib/raster.ts";

export const name = "cells";

const TIF_PATH = ["worldpop", "lka_ppp_2020_1km_Aggregated_UNadj.tif"];

// WorldPop's nodata sentinel for this product (confirmed via the raster's
// own pixel values — geotiff.js doesn't surface GDAL_NODATA as a typed
// field for this file). Also doubles as "not finite" / "<= 0" guard: any of
// nodata, exact zero, or a negative value means "no population here".
const NODATA = -99999;

/**
 * Gridded population (docs/architecture.md §2 `cells`): resamples the
 * WorldPop ~1km UN-adjusted raster onto the canonical 0.001deg grid. For
 * every pixel with population > 0, its value is split evenly across every
 * fine cell whose center the pixel's bounds contain (lib/raster.ts, driven
 * by the raster's own affine transform) — point reads and radius sums both
 * stay correct at raster granularity; this is not a resampling that
 * invents sub-pixel detail.
 *
 * Every fine cell belongs to exactly one raster pixel (pixels tile the
 * plane via floor-division, same as the grid itself), so no cell is ever
 * written by two different pixels — but the insert still accumulates
 * on conflict as a defensive measure rather than assuming that invariant
 * holds for every future raster.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const tiff = await fromFile(rawPath(...TIF_PATH));
  const image = await tiff.getImage();
  const width = image.getWidth();
  const height = image.getHeight();
  const [xres, yres] = image.getResolution();
  const [originX, originY] = image.getOrigin();
  if (xres === undefined || yres === undefined || originX === undefined || originY === undefined) {
    throw new Error("cells: raster is missing its affine transform (resolution/origin)");
  }

  const rasters = await image.readRasters();
  const band = rasters[0] as ArrayLike<number>;
  if (!band || band.length !== width * height) {
    throw new Error(`cells: unexpected raster band shape (expected ${width * height} pixels)`);
  }

  const insert = db.prepare(`
    INSERT INTO cells (cell_id, pop) VALUES (?, ?)
    ON CONFLICT(cell_id) DO UPDATE SET pop = pop + excluded.pop
  `);

  let rasterTotalPop = 0;
  let positivePixels = 0;
  let insertedCellRows = 0;
  let pixelsWithNoOverlap = 0;

  const tx = db.transaction(() => {
    db.prepare(`DELETE FROM cells`).run();

    for (let row = 0; row < height; row++) {
      const rowOffset = row * width;
      const north = originY + row * yres;
      const south = north + yres;
      for (let col = 0; col < width; col++) {
        const value = band[rowOffset + col] as number;
        if (!Number.isFinite(value) || value <= 0 || value === NODATA) continue;

        positivePixels++;
        rasterTotalPop += value;

        const west = originX + col * xres;
        const east = west + xres;
        const ids = fineCellsForBounds([west, south, east, north]);
        if (ids.length === 0) {
          pixelsWithNoOverlap++;
          continue;
        }
        const perCell = value / ids.length;
        for (const id of ids) {
          insert.run(id, perCell);
          insertedCellRows++;
        }
      }
    }
  });
  tx();

  const totals = db.prepare(`SELECT COUNT(*) AS n, COALESCE(SUM(pop), 0) AS total FROM cells`).get() as {
    n: number;
    total: number;
  };
  const insertedTotalPop = totals.total;
  const deviation = rasterTotalPop === 0 ? 0 : Math.abs(insertedTotalPop - rasterTotalPop) / rasterTotalPop;

  log(
    `cells: raster total pop = ${rasterTotalPop.toFixed(2)}, distributed total pop = ${insertedTotalPop.toFixed(2)} ` +
      `(deviation ${(deviation * 100).toFixed(4)}%)`,
  );
  log(
    `cells: ${positivePixels} positive pixels -> ${totals.n} cells (${insertedCellRows} pixel-to-cell writes` +
      (pixelsWithNoOverlap > 0 ? `, ${pixelsWithNoOverlap} pixels had no overlapping fine cell)` : ")"),
  );

  if (deviation > 0.005) {
    throw new Error(
      `cells: distributed population deviates from raster total by ${(deviation * 100).toFixed(3)}% ` +
        `(raster=${rasterTotalPop.toFixed(2)}, distributed=${insertedTotalPop.toFixed(2)}) — exceeds the 0.5% guard`,
    );
  }
}
