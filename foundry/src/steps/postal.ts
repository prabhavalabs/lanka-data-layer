import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { parsePostalTsv, dedupeByCode } from "../lib/postal-parser.ts";

export const name = "postal";

/**
 * postal_codes from the GeoNames LK postal dump fetched by `fetch-postal`.
 * admin_pcode is left null for now — see docs/architecture.md §2, filled
 * in once cell_lookup (and the boundaries it needs) exist.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const tsvText = await readFile(rawPath("postal", "LK.txt"), "utf8");
  const parsed = parsePostalTsv(tsvText);
  const rows = dedupeByCode(parsed);

  const insert = db.prepare(`
    INSERT INTO postal_codes (code, name, admin_pcode, lat, lon) VALUES (@code, @name, NULL, @lat, @lon)
    ON CONFLICT(code) DO UPDATE SET name=excluded.name, lat=excluded.lat, lon=excluded.lon
  `);

  const tx = db.transaction(() => {
    for (const row of rows) {
      insert.run({ code: row.postalCode, name: row.placeName, lat: row.lat, lon: row.lon });
    }
  });
  tx();

  log(`postal: upserted ${rows.length} postal_codes (deduped from ${parsed.length} source rows)`);
}
