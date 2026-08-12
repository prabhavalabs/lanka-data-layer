import type { StepContext } from "../step.ts";
import { downloadIfMissing } from "../lib/fetch.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "fetch-worldpop";

/**
 * WorldPop "Global 2000-2020, 1km UNadj" Sri Lanka raster: ~30 arc-sec
 * (~0.0083deg, ~1km) gridded population estimate, UN-adjusted to match
 * national population totals. This is the direct per-country-clipped GeoTIFF
 * — no mosaic/clip step needed. Confirmed live (200, ~373KB) at time of
 * writing; if WorldPop reorganizes the 2020/LKA path, the unadjusted (non
 * "_UNadj") variant lives one directory up as
 * .../2020/LKA/lka_ppp_2020_1km_Aggregated.tif — we deliberately want the
 * UN-adjusted one (matches docs/architecture.md §2's `cells` comment) since
 * it's rescaled to UN population division totals rather than raw
 * census-extrapolation, which is the more defensible number to distribute
 * over the grid.
 */
const TIF_URL =
  "https://data.worldpop.org/GIS/Population/Global_2000_2020_1km_UNadj/2020/LKA/lka_ppp_2020_1km_Aggregated_UNadj.tif";

export async function run({ log }: StepContext): Promise<void> {
  const destPath = rawPath("worldpop", "lka_ppp_2020_1km_Aggregated_UNadj.tif");
  const downloaded = await downloadIfMissing(TIF_URL, destPath);
  log(downloaded ? `fetch-worldpop: downloaded ${TIF_URL}` : "fetch-worldpop: raster already cached");
}
