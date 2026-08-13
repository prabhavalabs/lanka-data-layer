import type { StepContext } from "../step.ts";
import { downloadIfMissing } from "../lib/fetch.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "fetch-census";

/**
 * 2024 Census of Population and Housing, final report tables (Department of
 * Census and Statistics, statistics.gov.lk) at DS-division resolution:
 *
 *   A5 - population by sex and coarse age groups (<15, 15-59, 60-64, 65+)
 *   A6 - population by ethnicity
 *   A7 - population by religion
 *
 * Each URL serves a small (~50 KB) .xlsx directly (confirmed live at time of
 * writing; no license terms are stated on the source page — see the
 * top-level README's data-sources table and known-limitations notes). The
 * tables carry country, district, and DS-division rows; `census` parses and
 * loads them.
 */
const BASE_URL = "http://www.statistics.gov.lk/Population/StaticalInformation/CPH2024/PopulationA";

export const CENSUS_FILES = [
  { table: "A5", file: "a5-sex-age.xlsx" },
  { table: "A6", file: "a6-ethnicity.xlsx" },
  { table: "A7", file: "a7-religion.xlsx" },
] as const;

export async function run({ log }: StepContext): Promise<void> {
  for (const { table, file } of CENSUS_FILES) {
    const url = `${BASE_URL}/${table}`;
    const downloaded = await downloadIfMissing(url, rawPath("census-2024", file));
    log(downloaded ? `fetch-census: downloaded ${url} -> ${file}` : `fetch-census: ${file} already cached`);
  }
}
