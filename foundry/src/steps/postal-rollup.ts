import type { StepContext } from "../step.ts";

export const name = "postal-rollup";

// Codes covering less than this share of a unit's cells are noise from the
// nearest-centroid assignment (a code just across the boundary "leaking" a
// few edge cells) — dropped rather than shown as if they served the area.
const MIN_SHARE = 0.05;

/**
 * postal-rollup: derives which postal codes serve each DS/GN division from
 * the reverse-geocode index. cell_lookup assigns every land cell its nearest
 * postal code, so grouping those assignments per GND (and summing GNDs per
 * DS division) gives each unit's code list with a coverage share.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const tx = db.transaction(() => {
    db.prepare("DELETE FROM admin_postal").run();

    const gnd = db
      .prepare(
        `INSERT INTO admin_postal (pcode, code, share)
         SELECT gnd_pcode, postal_code,
                ROUND(CAST(COUNT(*) AS REAL) / SUM(COUNT(*)) OVER (PARTITION BY gnd_pcode), 4)
         FROM cell_lookup
         WHERE postal_code IS NOT NULL
         GROUP BY gnd_pcode, postal_code`,
      )
      .run();

    const ds = db
      .prepare(
        `INSERT INTO admin_postal (pcode, code, share)
         SELECT u.parent_pcode, ap.code,
                ROUND(SUM(ap.share) / COUNT(DISTINCT ap.pcode), 4)
         FROM admin_postal ap
         JOIN admin_units u ON u.pcode = ap.pcode AND u.level = 4
         WHERE u.parent_pcode IS NOT NULL
         GROUP BY u.parent_pcode, ap.code`,
      )
      .run();

    const pruned = db.prepare("DELETE FROM admin_postal WHERE share < ?").run(MIN_SHARE);
    return { gnd: gnd.changes, ds: ds.changes, pruned: pruned.changes };
  });

  const counts = tx();
  log(
    `postal-rollup: ${counts.gnd} GND + ${counts.ds} DS code assignments, ${counts.pruned} sub-${MIN_SHARE * 100}% slivers pruned`,
  );
}
