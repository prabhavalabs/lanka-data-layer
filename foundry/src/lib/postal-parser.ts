/**
 * Parser for the GeoNames postal code dump (download.geonames.org/export/zip/LK.zip).
 * Format: https://download.geonames.org/export/zip/readme.txt
 *
 * Tab-separated, no header, 12 columns:
 *   0 country code       2-letter ISO
 *   1 postal code
 *   2 place name
 *   3 admin name1        1st order subdivision (province)
 *   4 admin code1
 *   5 admin name2        2nd order subdivision (district)
 *   6 admin code2
 *   7 admin name3        3rd order subdivision (community)
 *   8 admin code3
 *   9 latitude            WGS84
 *  10 longitude           WGS84
 *  11 accuracy            1 (estimated) .. 6 (centroid)
 */
export interface PostalRow {
  country: string;
  postalCode: string;
  placeName: string;
  admin1Name: string;
  admin1Code: string;
  admin2Name: string;
  admin2Code: string;
  admin3Name: string;
  admin3Code: string;
  lat: number;
  lon: number;
  accuracy: number | null;
}

const EXPECTED_COLUMNS = 12;

/** Parses the raw LK.txt TSV body into rows. Skips blank lines. */
export function parsePostalTsv(text: string): PostalRow[] {
  const rows: PostalRow[] = [];
  const lines = text.split(/\r?\n/);
  for (const line of lines) {
    if (line.trim() === "") continue;
    const cols = line.split("\t");
    if (cols.length < EXPECTED_COLUMNS) {
      throw new Error(`Malformed postal row (expected ${EXPECTED_COLUMNS} columns, got ${cols.length}): ${line}`);
    }
    const lat = Number(cols[9]);
    const lon = Number(cols[10]);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) {
      throw new Error(`Malformed postal row (bad lat/lon): ${line}`);
    }
    const accuracyRaw = cols[11]?.trim();
    rows.push({
      country: cols[0]!,
      postalCode: cols[1]!,
      placeName: cols[2]!,
      admin1Name: cols[3]!,
      admin1Code: cols[4]!,
      admin2Name: cols[5]!,
      admin2Code: cols[6]!,
      admin3Name: cols[7]!,
      admin3Code: cols[8]!,
      lat,
      lon,
      accuracy: accuracyRaw ? Number(accuracyRaw) : null,
    });
  }
  return rows;
}

/** Dedupes by postal code, keeping the first occurrence (source order). */
export function dedupeByCode(rows: PostalRow[]): PostalRow[] {
  const seen = new Set<string>();
  const out: PostalRow[] = [];
  for (const row of rows) {
    if (seen.has(row.postalCode)) continue;
    seen.add(row.postalCode);
    out.push(row);
  }
  return out;
}
