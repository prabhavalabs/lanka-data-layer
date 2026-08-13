import type { StepContext } from "../step.ts";
import { downloadIfMissing } from "../lib/fetch.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "fetch-worldpop";

/**
 * WorldPop "Global 2015-2030, R2024B" Sri Lanka raster: ~30 arc-sec
 * (~0.0083deg, ~1km) gridded population estimate for 2025, unconstrained and
 * UN-adjusted to match national population totals ("UC" = unconstrained
 * settlement model, "UA" = UN-adjusted — the file naming convention for this
 * release; the earlier 2020 product used "_UNadj" instead). This is the
 * direct per-country-clipped GeoTIFF — no mosaic/clip step needed. Confirmed
 * live (200, ~403KB) at time of writing. The directory layout for this
 * release is .../R2024B/<year>/LKA/v1/1km_ua/{constrained,unconstrained}/ —
 * we deliberately want the unconstrained one (matches docs/architecture.md
 * §2's `cells` comment and the prior 2020 product's choice) since it doesn't
 * mask population out of cells a constrained settlement layer excludes,
 * which is the more defensible number to distribute over the whole grid.
 * Note the 2025 raster's pixel grid isn't identical to the retired 2020
 * product's (different width — this release's bbox extends slightly further
 * west — though same ~1km resolution); `cells.ts` recomputes fine-cell
 * coverage from each raster's own affine transform, so this is expected and
 * handled without any code change.
 */
const TIF_URL =
  "https://data.worldpop.org/GIS/Population/Global_2015_2030/R2024B/2025/LKA/v1/1km_ua/unconstrained/lka_pop_2025_UC_1km_R2024B_UA_v1.tif";

export async function run({ log }: StepContext): Promise<void> {
  const destPath = rawPath("worldpop", "lka_pop_2025_UC_1km_R2024B_UA_v1.tif");
  const downloaded = await downloadIfMissing(TIF_URL, destPath);
  log(downloaded ? `fetch-worldpop: downloaded ${TIF_URL}` : "fetch-worldpop: raster already cached");
}
