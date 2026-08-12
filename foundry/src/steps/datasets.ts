import type { StepContext } from "../step.ts";

export const name = "datasets";

interface CatalogEntry {
  id: string;
  title: string;
  category: string;
  description: string;
  source_name: string;
  source_url: string;
  license: string;
  /** SQL returning a single integer column for the live feature count. */
  countSql: string;
}

/**
 * The public dataset catalog served by /v1/datasets. Attribution here flows
 * into every API response that touches the dataset (meta.source), so
 * source_name/license must stay accurate. download_path stays null until the
 * bulk-download emitter exists.
 */
const CATALOG: CatalogEntry[] = [
  {
    id: "admin-boundaries",
    title: "Administrative boundaries",
    category: "boundaries",
    description:
      "Country, 9 provinces, 25 districts and 330 divisional secretariat divisions with trilingual names, areas and parent links.",
    source_name: "geoBoundaries",
    source_url: "https://www.geoboundaries.org",
    license: "CC BY 3.0 IGO",
    countSql: "SELECT COUNT(*) FROM admin_units",
  },
  {
    id: "population-2023",
    title: "Population projections 2023",
    category: "demographics",
    description:
      "Population by province and district: totals and 17 age buckets by sex, p-code keyed. OCHA common operational dataset.",
    source_name: "OCHA / HDX cod-ps-lka",
    source_url: "https://data.humdata.org/dataset/cod-ps-lka",
    license: "CC BY-IGO",
    countSql: "SELECT COUNT(*) FROM admin_population",
  },
  {
    id: "ethnicity-religion-2012",
    title: "Ethnicity and religion, census 2012",
    category: "demographics",
    description:
      "District-level ethnicity and religion counts from the 2012 census. Religion figures are rounded in the source.",
    source_name: "Department of Census and Statistics",
    source_url: "http://www.statistics.gov.lk",
    license: "open",
    countSql: "SELECT COUNT(*) FROM admin_stats",
  },
  {
    id: "elections",
    title: "Election results 2015-2024",
    category: "elections",
    description:
      "Presidential 2015, 2019, 2024 and parliamentary 2024 results for 22 electoral districts, 160 polling divisions, 22 postal-vote entities and the national tally.",
    source_name: "Election Commission of Sri Lanka (via nuuuwan/lk_elections)",
    source_url: "https://github.com/nuuuwan/lk_elections",
    license: "open",
    countSql: "SELECT COUNT(*) FROM election_results",
  },
  {
    id: "places",
    title: "Cities and towns",
    category: "places",
    description: "Populated places with Sinhala and Tamil names, place kind and population.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    countSql: "SELECT COUNT(*) FROM places",
  },
  {
    id: "postal-codes",
    title: "Postal codes",
    category: "places",
    description: "Sri Lankan postal codes with place name and representative coordinate.",
    source_name: "GeoNames postal",
    source_url: "https://download.geonames.org/export/zip/",
    license: "CC BY 4.0",
    countSql: "SELECT COUNT(*) FROM postal_codes",
  },
  {
    id: "pois",
    title: "Points of interest",
    category: "infrastructure",
    description: "Hospitals, schools and universities, and airports with trilingual names.",
    source_name: "OpenStreetMap",
    source_url: "https://www.openstreetmap.org",
    license: "ODbL",
    countSql: "SELECT COUNT(*) FROM pois",
  },
];

export async function run({ db, log }: StepContext): Promise<void> {
  const insert = db.prepare(`
    INSERT INTO datasets (id, title, category, description, source_name, source_url, license, feature_count, download_path)
    VALUES (@id, @title, @category, @description, @source_name, @source_url, @license, @feature_count, NULL)
    ON CONFLICT(id) DO UPDATE SET
      title=excluded.title, category=excluded.category, description=excluded.description,
      source_name=excluded.source_name, source_url=excluded.source_url, license=excluded.license,
      feature_count=excluded.feature_count
  `);

  const tx = db.transaction(() => {
    for (const entry of CATALOG) {
      const count = db.prepare(entry.countSql).pluck().get() as number;
      const { countSql: _countSql, ...row } = entry;
      insert.run({ ...row, feature_count: count });
    }
  });
  tx();

  log(`datasets: cataloged ${CATALOG.length} datasets`);
}
