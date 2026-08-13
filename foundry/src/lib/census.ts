/**
 * Parsing + name-matching helpers for the 2024 Census of Population and
 * Housing tables (statistics.gov.lk CPH2024, tables A5-A7). Pure functions
 * over already-extracted cell matrices — the XLSX reading itself lives in
 * lib/xlsx.ts so this module stays dependency-free and unit-testable.
 *
 * Sheet layout (identical across A5/A6/A7): column B carries a trilingual
 * name cell ("si\nta\nen"), columns C.. carry the numeric values. Rows come
 * in document order: the country row ("... Sri Lanka", single line), then per
 * district a "<Name> District" row followed by that district's DS division
 * rows. Everything else (titles, headers, blank spacers) has a non-numeric
 * column C and is skipped.
 */

export const CENSUS_YEAR = 2024;

export type CellValue = string | number | null;

export interface CensusRow {
  kind: "country" | "district" | "ds";
  nameEn: string;
  values: number[];
}

/**
 * Extracts the data rows of one census sheet. `valueCount` pins how many
 * value columns (from column C on) the sheet is expected to carry — a row
 * that starts numeric but has a non-numeric cell inside that range means the
 * upstream layout changed, which should fail the build rather than half-parse.
 */
export function parseCensusSheet(matrix: CellValue[][], valueCount: number): CensusRow[] {
  const rows: CensusRow[] = [];
  for (const cells of matrix) {
    const name = cells[1];
    if (typeof name !== "string" || name.trim() === "") continue;
    if (typeof cells[2] !== "number") continue; // titles, headers, spacers

    const lines = name
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const nameEn = lines[lines.length - 1]!;

    const values: number[] = [];
    for (let i = 0; i < valueCount; i++) {
      const v = cells[2 + i];
      if (typeof v !== "number") {
        throw new Error(`census: row "${nameEn}" has non-numeric value in column ${2 + i} (layout change?)`);
      }
      values.push(v);
    }

    const kind = nameEn.includes("Sri Lanka") ? "country" : nameEn.endsWith("District") ? "district" : "ds";
    rows.push({ kind, nameEn: kind === "country" ? "Sri Lanka" : nameEn, values });
  }
  return rows;
}

/** Lowercase, strip everything but letters/digits — "Ja-Ela" and "Ja Ela" collide, as intended. */
export function normalizeName(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "");
}

/**
 * Census spelling -> COD-AB spelling, both normalized. Hand-verified against
 * the 2024 final-report tables and the COD-AB ADM3 release this pipeline
 * ships: every entry is the same DS division under a spelling variant
 * (Mathugama/Matugama), a transliteration difference (Vadamaradchy/
 * Vadamaradchchi), or a parenthetical qualifier only one side carries
 * ("Koralai Pattu North (Vaharai)" vs "Koralai Pattu North"). Matching is
 * still constrained to the parent district, so an alias can never pull a row
 * into another district.
 */
export const DS_NAME_ALIASES: Record<string, string> = {
  // Colombo .. Kalutara
  mathugama: "matugama",
  walallavita: "walallawita",
  // Kandy
  thumpane: "tumpane",
  pujapitiya: "poojapitiya",
  udadumbara: "ududumbara",
  kandyfourgravetsgangawatakorale: "gangawatakorale",
  delthota: "deltota",
  // Matale
  ambangangakorale: "ambanganga",
  laggalapallegama: "laggala",
  // Nuwara Eliya
  hanguranketha: "hanguranketa",
  ambagamuwa: "ambagamuwakorale",
  // Galle
  benthota: "bentota",
  welivitiyadivithura: "welivitiyadivitura",
  gonapeenuwala: "gonapinuwala",
  gallefourgravets: "galle4gravets",
  // Matara
  pitabeddara: "pitabaddara",
  kamburupitiya: "kaburupitiya",
  weligam: "weligama",
  dickwella: "dikwella",
  // Jaffna
  islandnorthkayts: "islandsnorthkayts",
  valikamamnorth: "valikamamnorththllippalai",
  vadamaradchysouthwestkaraveddy: "vadamaradchchisouthwestkaraveddy",
  vadamaradchyeast: "vadamaradchchieast",
  vadamaradchynorthpointperdro: "vadamaradchinorthpointpedro",
  thenmaradchychavakachcheri: "thenmaradchichavakachcheri",
  islandsouthvelanai: "islandssouthvelanai",
  // Mannar
  nanattan: "nanaddan",
  // Mullaitivu
  puthukudiyiruppu: "puthukkudiyiruppu",
  oddusudan: "oddusuddan",
  // Batticaloa
  koralaipattunorthvaharai: "koralaipattunorth",
  koralaipattuwestoddamavadi: "koralaipattuwest",
  koralaipattuvalachchenai: "koralaipattu",
  koralaipattusouthkiran: "koralaipattusouth",
  manmunaipattuaraipattai: "manmunaipattu",
  poratheevupattu: "porativupattu",
  // Ampara
  samanthurai: "sammanthurai",
  sainthamarathu: "sainthamaruthu",
  karaitheevu: "karaitivu",
  ninthavur: "nintavur",
  addalachchenai: "addalaichenai",
  alayadiwembu: "alayadivembu",
  thirukkovil: "tirukkovil",
  pothuvil: "pottuvil",
  // Trincomalee
  kuchchaveli: "kuchchaweli",
  trincomaleetownandgravets: "towngravets",
  kanthale: "kantale",
  muttur: "muthur",
  verugaleachchilampattu: "verugal",
  // Kurunegala
  kotavehera: "kotawehera",
  polpithigama: "polpitigama",
  weerambugedara: "weerabugedara",
  // Puttalam
  vanathawilluwa: "vanathavilluwa",
  // Anuradhapura
  horowpothana: "horowpathana",
  galenbindunuwewa: "galenbidunuwewa",
  thambuttegama: "thambuththegama",
  // Polonnaruwa
  hingurakgoda: "higurakgoda",
  // Badulla
  meegahakivula: "meegahakiula",
  kandaketiya: "kandeketiya",
  // Monaragala
  moneragala: "monaragala",
  thanamalvila: "thanamalwila",
  // Ratnapura
  kahawatta: "kahawattha",
  // Kegalle
  aranayaka: "aranayake",
  bulathkohupitiya: "bulathkohipitiya",
  yatiyanthota: "yatiyantota",
  dehiovita: "dehiowita",
};

/** Census district spelling -> COD-AB adm2 spelling (normalized). */
export const DISTRICT_NAME_ALIASES: Record<string, string> = {
  moneragala: "monaragala",
};

/** The subset of COD-AB ADM3 feature properties the matcher needs. */
export interface Adm3Properties {
  adm2_name: string;
  adm2_pcode: string;
  adm3_name: string;
  adm3_pcode: string;
}

export interface DsMatchIndex {
  /** normalized adm2 name -> district pcode */
  districtByName: Map<string, string>;
  /** district pcode -> (normalized adm3 name -> DS pcode) */
  dsByDistrict: Map<string, Map<string, string>>;
}

export function buildDsMatchIndex(features: { properties: Adm3Properties }[]): DsMatchIndex {
  const districtByName = new Map<string, string>();
  const dsByDistrict = new Map<string, Map<string, string>>();
  for (const f of features) {
    const p = f.properties;
    districtByName.set(normalizeName(p.adm2_name), p.adm2_pcode);
    let ds = dsByDistrict.get(p.adm2_pcode);
    if (!ds) {
      ds = new Map();
      dsByDistrict.set(p.adm2_pcode, ds);
    }
    ds.set(normalizeName(p.adm3_name), p.adm3_pcode);
  }
  return { districtByName, dsByDistrict };
}

/** "Colombo District" -> LK11; null when the district name is unknown. */
export function matchDistrict(index: DsMatchIndex, censusName: string): string | null {
  const stripped = censusName.replace(/\s+District$/, "");
  const norm = normalizeName(stripped);
  return index.districtByName.get(DISTRICT_NAME_ALIASES[norm] ?? norm) ?? null;
}

/** DS division name -> ADM3 pcode, constrained to the parent district; null when unmatched. */
export function matchDs(index: DsMatchIndex, districtPcode: string, censusName: string): string | null {
  const norm = normalizeName(censusName);
  const ds = index.dsByDistrict.get(districtPcode);
  return ds?.get(DS_NAME_ALIASES[norm] ?? norm) ?? null;
}
