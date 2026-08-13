import type { StepContext } from "../step.ts";
import { CENSUS_YEAR } from "../lib/census.ts";

export const name = "population-rollup";

// The WorldPop raster's reference year (see fetch-worldpop) — kept distinct
// from the 2023 HDX projections so consumers can tell the two sources apart.
const WORLDPOP_YEAR = 2025;

/**
 * population-rollup: fills the levels no official source covers with
 * WorldPop-derived totals, by rolling the gridded population up through the
 * reverse-geocode index:
 *
 *   GND total  = SUM(cells.pop) over the cells cell_lookup assigns to it
 *   DS total   = SUM of its GND children
 *
 * Rows are written as (year 2025, sex 't', age_bucket 'total') only — a
 * modeled estimate, not a census count. The API serves whatever the latest
 * year per unit is, so a DS division with a census row must NOT also get a
 * WorldPop row (the raster year postdates the census year and would win) —
 * the DS insert below skips units the `census` step already covered, leaving
 * the rollup only for GN divisions plus any DS division the census name
 * matching missed. Levels 0-2 keep their 2023 HDX / 2024 census rows.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const tx = db.transaction(() => {
    db.prepare("DELETE FROM admin_population WHERE year = ?").run(WORLDPOP_YEAR);

    const gnd = db
      .prepare(
        `INSERT INTO admin_population (pcode, year, sex, age_bucket, count)
         SELECT cl.gnd_pcode, ?, 't', 'total', CAST(ROUND(SUM(c.pop)) AS INTEGER)
         FROM cell_lookup cl JOIN cells c ON c.cell_id = cl.cell_id
         GROUP BY cl.gnd_pcode`,
      )
      .run(WORLDPOP_YEAR);

    const ds = db
      .prepare(
        `INSERT INTO admin_population (pcode, year, sex, age_bucket, count)
         SELECT u.parent_pcode, ?, 't', 'total', SUM(ap.count)
         FROM admin_population ap
         JOIN admin_units u ON u.pcode = ap.pcode AND u.level = 4
         WHERE ap.year = ? AND u.parent_pcode IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM admin_population census
             WHERE census.pcode = u.parent_pcode AND census.year = ?
           )
         GROUP BY u.parent_pcode`,
      )
      .run(WORLDPOP_YEAR, WORLDPOP_YEAR, CENSUS_YEAR);

    return { gnd: gnd.changes, ds: ds.changes };
  });

  const counts = tx();
  log(`population-rollup: ${counts.gnd} GND + ${counts.ds} DS division totals (worldpop ${WORLDPOP_YEAR})`);
}
