import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { parsePostalTsv, dedupeByCode } from "../lib/postal-parser.ts";

export const name = "postal";

// GeoNames LK dump district spelling -> our admin_units level-2 spelling,
// both after normalizeDistrictName(). The dump's admin2Name column (the
// district) is otherwise a clean match against admin_units level 2 names —
// this is the one known exception. Add entries here as spelling drift is
// discovered; anything else that doesn't match exactly is logged loudly and
// left admin_pcode = NULL rather than guessed at.
const DISTRICT_ALIASES: Record<string, string> = {
  moneragala: "monaragala",
};

/** lowercase, trim, drop a trailing " District" (ours says "Kurunegala District", the dump says "Kurunegala"). */
function normalizeDistrictName(raw: string): string {
  return raw.trim().toLowerCase().replace(/\s+district$/, "").trim();
}

/**
 * Exact-match key for postal place name <-> admin unit name_en: lowercase,
 * collapse whitespace, then strip everything that isn't alphanumeric (which
 * also disposes of the just-collapsed whitespace). Deliberately strict, no
 * fuzzy/edit-distance matching — e.g. postal "Dummalasuriya" must NOT match
 * GND "Dummalasooriya"; a near-miss match would silently overwrite a
 * coordinate that might already be fine with the wrong unit's centroid.
 */
function normalizePlaceKey(raw: string): string {
  return raw.toLowerCase().trim().replace(/\s+/g, " ").replace(/[^a-z0-9]/g, "");
}

interface AdminUnitRow {
  pcode: string;
  level: number;
  name_en: string;
  parent_pcode: string | null;
  centroid_lat: number | null;
  centroid_lon: number | null;
}

/**
 * postal_codes from the GeoNames LK postal dump fetched by `fetch-postal`.
 *
 * admin_pcode: resolved from the dump's own admin2Name (district) column by
 * normalized name match against admin_units level 2 — see
 * docs/architecture.md §2. This is independent of the dump's lat/lon, so
 * it's trustworthy even for the ~1/3 of rows whose coordinates are wrong
 * (GeoNames accuracy <= 3; some are 10+ km off, occasionally landing on top
 * of a *different* postal code's true location — see cell-lookup.ts).
 *
 * lat/lon: GeoNames' own coordinate is kept only when nothing better is
 * available. Where an admin unit inside the resolved district has a
 * name_en that exactly matches the postal place name (normalized), that
 * unit's centroid replaces the dump's coordinate — admin unit centroids
 * come from official COD-AB/Survey Dept figures (admin.ts) and are far more
 * trustworthy than GeoNames' community-sourced points.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const tsvText = await readFile(rawPath("postal", "LK.txt"), "utf8");
  const parsed = parsePostalTsv(tsvText);
  const rows = dedupeByCode(parsed);

  // --- 1. Resolve each row's district name to our level-2 admin_pcode. ---
  const districts = db.prepare(`SELECT pcode, name_en FROM admin_units WHERE level = 2`).all() as {
    pcode: string;
    name_en: string;
  }[];
  const districtPcodeByName = new Map<string, string>();
  for (const d of districts) districtPcodeByName.set(normalizeDistrictName(d.name_en), d.pcode);

  const districtPcodeByCode = new Map<string, string>();
  const unmatchedDistrictNames = new Map<string, number>();
  for (const row of rows) {
    const normalized = normalizeDistrictName(row.admin2Name);
    const canonical = DISTRICT_ALIASES[normalized] ?? normalized;
    const pcode = districtPcodeByName.get(canonical);
    if (pcode) {
      districtPcodeByCode.set(row.postalCode, pcode);
    } else {
      unmatchedDistrictNames.set(row.admin2Name, (unmatchedDistrictNames.get(row.admin2Name) ?? 0) + 1);
    }
  }
  for (const [rawName, count] of unmatchedDistrictNames) {
    log(`postal: WARNING unmatched district name "${rawName}" (${count} postal codes) — admin_pcode left NULL`);
  }

  // --- 2. Re-geocode: index level 3/4 admin units per district, by normalized name. ---
  const units = db
    .prepare(
      `SELECT pcode, level, name_en, parent_pcode, centroid_lat, centroid_lon FROM admin_units WHERE level IN (3, 4)`,
    )
    .all() as AdminUnitRow[];
  const unitByPcode = new Map(units.map((u) => [u.pcode, u]));

  // Ground truth for "which district is this unit in": walk the parent
  // chain rather than assume anything about pcode structure. Level 3's
  // parent is already the district (admin.ts); level 4's parent is a level
  // 3 DS division, so one more hop via unitByPcode gets to the district.
  function districtOf(unit: AdminUnitRow): string | null {
    if (unit.level === 3) return unit.parent_pcode;
    const dsDivision = unit.parent_pcode ? unitByPcode.get(unit.parent_pcode) : undefined;
    return dsDivision?.parent_pcode ?? null;
  }

  const level4ByDistrict = new Map<string, Map<string, AdminUnitRow>>();
  const level3ByDistrict = new Map<string, Map<string, AdminUnitRow>>();
  for (const u of units) {
    const district = districtOf(u);
    if (!district) continue;
    const target = u.level === 4 ? level4ByDistrict : level3ByDistrict;
    let byName = target.get(district);
    if (!byName) {
      byName = new Map();
      target.set(district, byName);
    }
    const key = normalizePlaceKey(u.name_en);
    // First occurrence wins on a rare in-district name collision — stable,
    // and consistent with dedupeByCode's own "first in source order" rule.
    if (!byName.has(key)) byName.set(key, u);
  }

  let regeocodedLevel4 = 0;
  let regeocodedLevel3 = 0;
  let keptOriginal = 0;
  let unmatchedDistrictCoords = 0;

  const finalRows = rows.map((row) => {
    const districtPcode = districtPcodeByCode.get(row.postalCode) ?? null;
    let lat = row.lat;
    let lon = row.lon;

    if (!districtPcode) {
      unmatchedDistrictCoords++;
    } else {
      const key = normalizePlaceKey(row.placeName);
      const l4Match = level4ByDistrict.get(districtPcode)?.get(key);
      const l3Match = l4Match ? undefined : level3ByDistrict.get(districtPcode)?.get(key);
      const match = l4Match ?? l3Match;
      if (match && match.centroid_lat != null && match.centroid_lon != null) {
        lat = match.centroid_lat;
        lon = match.centroid_lon;
        if (l4Match) regeocodedLevel4++;
        else regeocodedLevel3++;
      } else {
        keptOriginal++;
      }
    }

    return { code: row.postalCode, name: row.placeName, admin_pcode: districtPcode, lat, lon };
  });

  const insert = db.prepare(`
    INSERT INTO postal_codes (code, name, admin_pcode, lat, lon) VALUES (@code, @name, @admin_pcode, @lat, @lon)
    ON CONFLICT(code) DO UPDATE SET name=excluded.name, admin_pcode=excluded.admin_pcode, lat=excluded.lat, lon=excluded.lon
  `);

  const tx = db.transaction(() => {
    for (const row of finalRows) insert.run(row);
  });
  tx();

  log(`postal: upserted ${finalRows.length} postal_codes (deduped from ${parsed.length} source rows)`);
  log(
    `postal: re-geocoded ${regeocodedLevel4} to a GND centroid, ${regeocodedLevel3} to a DS centroid, ` +
      `${keptOriginal} kept their original GeoNames coordinate (district resolved but no exact name match), ` +
      `${unmatchedDistrictCoords} kept original coordinate (district unresolved)`,
  );
}
