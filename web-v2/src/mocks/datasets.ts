// Mock dataset catalogue. Publishers and licence situations are realistic;
// row counts, hashes, versions and preview values are artificial.
import type { Dataset, DatasetDetail, LicenceStatus, ThemeId } from '@/api/types'

const LIC = {
  unclear: { id: 'lk-gov-unstated', label: 'No stated licence', status: 'unclear' as LicenceStatus },
  odbl: { id: 'ODbL-1.0', label: 'ODbL 1.0', status: 'attribution' as LicenceStatus },
  ccby: { id: 'CC-BY-4.0', label: 'CC BY 4.0', status: 'attribution' as LicenceStatus },
  ccbyigo: { id: 'CC-BY-3.0-IGO', label: 'CC BY 3.0 IGO', status: 'attribution' as LicenceStatus },
  cc0: { id: 'CC0-1.0', label: 'CC0 1.0', status: 'open' as LicenceStatus },
  pd: { id: 'public-domain', label: 'Public domain', status: 'open' as LicenceStatus },
}

type Seed = [id: string, title: string, summary: string, theme: ThemeId, publisher: string, publisherType: Dataset['publisherType'], lic: keyof typeof LIC, cadence: string, levels: string[], rows: number, indicators: number, vintage: string, updated: string, freshness: Dataset['freshness'], formats: string[], sizeMb: number]

const SEEDS: Seed[] = [
  ['census-2024', 'Census of Population and Housing 2024', 'Population, sex, age bands and households for every unit down to GN division.', 'people', 'Department of Census and Statistics', 'government', 'unclear', 'Decennial', ['ADM0', 'ADM1', 'ADM2', 'ADM3', 'ADM4'], 1_038_024, 64, '2024', '2026-11-17', 'fresh', ['SQLite', 'Parquet', 'CSV'], 41.8],
  ['census-2012', 'Census of Population and Housing 2012', 'The previous full count, re-keyed to current p-codes with a published crosswalk.', 'people', 'Department of Census and Statistics', 'government', 'unclear', 'Closed', ['ADM0', 'ADM1', 'ADM2', 'ADM3', 'ADM4'], 992_610, 58, '2012', '2026-08-02', 'fresh', ['SQLite', 'Parquet', 'CSV'], 38.2],
  ['admin-boundaries', 'Administrative boundaries ADM0–ADM4', 'Official p-coded polygons: 9 provinces, 25 districts, 339 DS divisions, 14,043 GN divisions.', 'boundaries', 'Survey Department · OCHA COD-AB', 'government', 'ccbyigo', 'Annual', ['ADM0', 'ADM1', 'ADM2', 'ADM3', 'ADM4'], 14_417, 0, '2023', '2026-10-30', 'fresh', ['PMTiles', 'GeoParquet', 'FlatGeobuf', 'GeoJSON'], 148.3],
  ['pop-grid-100m', 'Population grid, 100 m', 'Residents per canonical grid cell, constrained to mapped buildings and reconciled to census totals.', 'people', 'Lanka Data Layer (derived)', 'derived', 'ccby', 'Annual', ['Grid 100 m', 'ADM0–ADM4'], 6_561_204, 6, '2024', '2026-11-18', 'fresh', ['COG', 'PMTiles', 'Parquet'], 212.6],
  ['postal-codes', 'Postal codes', 'Five-digit postal codes with post office points and best-fit p-code.', 'boundaries', 'Sri Lanka Post', 'government', 'unclear', 'Irregular', ['Point', 'ADM3'], 1_947, 0, '2025', '2026-06-11', 'due', ['SQLite', 'CSV', 'GeoJSON'], 0.4],
  ['electoral-districts', 'Electoral districts and polling divisions', '22 electoral districts and 160 polling divisions, with the GN divisions each contains.', 'boundaries', 'Election Commission', 'government', 'unclear', 'Per delimitation', ['Electoral district', 'Polling division'], 182, 0, '2024', '2026-09-04', 'fresh', ['PMTiles', 'GeoParquet', 'GeoJSON'], 6.1],
  ['elections-results', 'Election results by polling division', 'Presidential and parliamentary results, 1982 to 2024, valid and rejected votes per party.', 'elections', 'Election Commission', 'government', 'unclear', 'Per election', ['Electoral district', 'Polling division'], 48_612, 22, '1982–2024', '2026-09-04', 'fresh', ['SQLite', 'Parquet', 'CSV'], 5.7],
  ['access-health', 'Travel time to health facilities', 'Drive and walk time from every populated grid cell to the nearest hospital, by facility tier.', 'access', 'Lanka Data Layer (derived)', 'derived', 'odbl', 'Quarterly', ['Grid 100 m', 'ADM0–ADM4'], 6_561_204, 18, '2026 Q3', '2026-10-06', 'fresh', ['COG', 'PMTiles', 'Parquet'], 96.4],
  ['health-facilities', 'Health facilities', 'Hospitals, divisional units and clinics with tier, ownership and bed count where published.', 'access', 'Ministry of Health · OpenStreetMap', 'government', 'odbl', 'Monthly', ['Point'], 1_684, 4, '2026-10', '2026-11-02', 'fresh', ['GeoParquet', 'GeoJSON', 'CSV'], 0.9],
  ['schools', 'Government schools', 'School census list geocoded to points, with type, medium and enrolment band.', 'access', 'Ministry of Education', 'government', 'unclear', 'Annual', ['Point', 'ADM3'], 10_126, 9, '2024', '2026-03-19', 'due', ['GeoParquet', 'GeoJSON', 'CSV'], 2.3],
  ['landcover-10m', 'Land cover, 10 m', 'Eleven land-cover classes aggregated to the canonical grid and to every admin unit.', 'land', 'ESA WorldCover', 'multilateral', 'ccby', 'Irregular', ['Grid 100 m', 'ADM0–ADM4'], 6_561_204, 22, '2021', '2026-05-27', 'fresh', ['COG', 'PMTiles', 'Parquet'], 164.0],
  ['ghsl-built', 'Built-up surface 1975–2030', 'Built-up area and volume in five-year steps, with the 2030 projection flagged as modelled.', 'land', 'European Commission JRC · GHSL', 'multilateral', 'ccby', 'Irregular', ['Grid 100 m', 'ADM0–ADM4'], 6_561_204, 12, '1975–2030', '2026-05-27', 'fresh', ['COG', 'PMTiles', 'Parquet'], 188.5],
  ['protected-areas', 'Protected areas', 'National parks, nature reserves and sanctuaries with designation and gazette reference.', 'land', 'Department of Wildlife Conservation · OpenStreetMap', 'community', 'odbl', 'Monthly', ['Polygon'], 501, 3, '2026-10', '2026-11-02', 'fresh', ['GeoParquet', 'GeoJSON', 'PMTiles'], 3.8],
  ['rain-chirps', 'Rainfall, monthly since 1981', 'Monthly precipitation totals and 1991–2020 normals per grid cell and admin unit.', 'climate', 'UCSB Climate Hazards Center · CHIRPS', 'research', 'cc0', 'Monthly', ['Grid 5 km', 'ADM0–ADM3'], 2_412_880, 14, '1981–2026', '2026-11-12', 'fresh', ['COG', 'Parquet', 'CSV'], 54.9],
  ['flood-exposure', 'Flood exposure', 'Share of people and buildings inside the 1-in-100-year river flood extent.', 'climate', 'Lanka Data Layer (derived)', 'derived', 'ccby', 'Annual', ['ADM2', 'ADM3', 'ADM4'], 28_834, 8, '2025', '2026-02-08', 'stale', ['Parquet', 'CSV'], 1.6],
  ['night-lights', 'Night-time lights', 'Annual mean radiance per grid cell, a proxy for economic activity between surveys.', 'economy', 'NASA · VIIRS Black Marble', 'research', 'pd', 'Annual', ['Grid 500 m', 'ADM0–ADM4'], 1_312_040, 6, '2012–2025', '2026-04-15', 'fresh', ['COG', 'Parquet'], 31.2],
  ['labour-force', 'Labour force survey', 'Participation, unemployment and sector shares. District is the lowest published level.', 'economy', 'Department of Census and Statistics', 'government', 'unclear', 'Quarterly', ['ADM0', 'ADM1', 'ADM2'], 18_450, 26, '2011–2026 Q2', '2026-09-28', 'fresh', ['SQLite', 'Parquet', 'CSV'], 1.1],
  ['railways', 'Railway lines and stations', 'The rail network as routable lines, with 294 stations and halts.', 'transport', 'OpenStreetMap contributors', 'community', 'odbl', 'Monthly', ['Line', 'Point'], 1_618, 4, '2026-10', '2026-11-02', 'fresh', ['GeoParquet', 'GeoJSON', 'PMTiles'], 1.4],
  ['roads', 'Road network', 'Classified roads A to minor, cleaned for routing and travel-time analysis.', 'transport', 'OpenStreetMap contributors', 'community', 'odbl', 'Monthly', ['Line'], 412_906, 5, '2026-10', '2026-11-02', 'fresh', ['GeoParquet', 'PMTiles'], 86.7],
  ['waterways', 'Rivers and water bodies', 'Named rivers, tanks and reservoirs with river-basin membership.', 'land', 'OpenStreetMap contributors', 'community', 'odbl', 'Monthly', ['Line', 'Polygon'], 3_919, 2, '2026-10', '2026-11-02', 'fresh', ['GeoParquet', 'GeoJSON', 'PMTiles'], 7.9],
]

function h(s: string) {
  let x = 2166136261
  for (let i = 0; i < s.length; i++) x = Math.imul(x ^ s.charCodeAt(i), 16777619)
  return x >>> 0
}
const hex = (seed: string, n: number) => {
  let out = ''
  for (let i = 0; out.length < n; i++) out += h(seed + i).toString(16).padStart(8, '0')
  return out.slice(0, n)
}

export const DATASETS: Dataset[] = SEEDS.map(([id, title, summary, theme, publisher, publisherType, lic, cadence, levels, rows, indicators, vintage, updated, freshness, formats, sizeMb]) => ({
  id, title, summary, theme, publisher, publisherType, licence: LIC[lic], cadence, levels, rows, indicators, vintage, updated, freshness, formats, sizeMb,
  // 12 weeks of API reads, for the catalogue sparkline
  spark: Array.from({ length: 12 }, (_, i) => 40 + ((h(id + i) % 60) + i * (h(id) % 7))),
}))

const LIMITS: Partial<Record<string, string[]>> = {
  'census-2024': [
    'Figures are preliminary. The department revises them when the final tables are published; revised values replace these and the change is recorded in the version diff.',
    'Counts are of usual residents. They do not include short-term visitors or people counted in institutions unless the source table says so.',
    'GN-level age bands are not published. Age by GN division here is absent, not zero.',
    'Boundaries changed between 2012 and 2024. Change figures use our crosswalk and carry an estimate flag where a division was split.',
  ],
  'pop-grid-100m': [
    'This is a model, not a count. Cell values sum to census totals for each GN division but the split within a division follows mapped buildings.',
    'Buildings missing from the base map receive no population. Coverage is weakest in recently settled areas.',
    'Not suitable for identifying individual households, and it should not be used that way.',
  ],
  'access-health': [
    'Travel times assume free-flow speeds by road class. They do not model congestion, flooding or ferry schedules.',
    'A facility being near does not mean a service is available there. Bed counts and specialties are only partly published.',
    'Walk times ignore terrain steepness above 12%.',
  ],
}
const GENERIC_LIMITS = [
  'It records what the source published, at the level the source published it. Gaps are shown as "not held", never filled in.',
  'Positional accuracy follows the source. Features traced from imagery can be tens of metres off.',
  'It is not a legal record. For boundaries and land, the gazette and the Survey Department are authoritative.',
]

export function datasetDetail(id: string): DatasetDetail {
  const base = DATASETS.find((d) => d.id === id) ?? DATASETS[0]
  const isCensus = base.id.startsWith('census')
  const columns = isCensus
    ? [
        { name: 'pcode', type: 'text', note: 'Official p-code of the unit (join key)' },
        { name: 'level', type: 'int', note: '0 country … 4 GN division' },
        { name: 'period', type: 'text', note: 'Census reference year' },
        { name: 'population', type: 'int', note: 'Usual residents' },
        { name: 'male', type: 'int', note: '' },
        { name: 'female', type: 'int', note: '' },
        { name: 'households', type: 'int', note: 'Occupied housing units' },
        { name: 'status', type: 'text', note: 'final | provisional | estimate' },
        { name: 'observation_id', type: 'text', note: 'Key into provenance' },
      ]
    : [
        { name: 'id', type: 'text', note: 'Stable feature or row id' },
        { name: 'pcode', type: 'text', note: 'Containing ADM unit (join key)' },
        { name: 'name', type: 'text', note: 'Name as published' },
        { name: 'kind', type: 'text', note: 'Source classification' },
        { name: 'value', type: 'real', note: 'Measure, unit in indicator definition' },
        { name: 'period', type: 'text', note: 'Reference period' },
        { name: 'source_n', type: 'int', note: 'Index into sources below' },
        { name: 'observation_id', type: 'text', note: 'Key into provenance' },
      ]
  const names = ['Colombo', 'Kolonnawa', 'Kaduwela', 'Homagama', 'Seethawaka', 'Padukka', 'Maharagama', 'Kotte', 'Thimbirigasyaya', 'Dehiwala']
  const preview = names.map((n, i) => {
    const p = 60_000 + (h(base.id + n) % 260_000)
    const m = Math.round(p * (0.47 + (h(n) % 30) / 1000))
    return isCensus
      ? [`LK11${String(i * 3 + 3).padStart(2, '0')}`, 3, '2024', p, m, p - m, Math.round(p / 3.7), 'provisional', `obs_${hex(base.id + n, 8)}`]
      : [`${base.id.slice(0, 3)}_${hex(n + base.id, 6)}`, `LK11${String(i * 3 + 3).padStart(2, '0')}`, n, base.theme, Math.round((h(base.id + n) % 9000) / 10) / 10, base.vintage.slice(-4), 1 + (i % 2), `obs_${hex(base.id + n, 8)}`]
  })
  const sizes: Record<string, number> = { SQLite: 1, Parquet: 0.31, CSV: 0.58, GeoParquet: 0.36, FlatGeobuf: 0.74, GeoJSON: 1.9, PMTiles: 0.42, COG: 0.66 }
  const ext: Record<string, string> = { SQLite: 'sqlite', Parquet: 'parquet', CSV: 'csv.gz', GeoParquet: 'geo.parquet', FlatGeobuf: 'fgb', GeoJSON: 'geojson.gz', PMTiles: 'pmtiles', COG: 'tif' }
  return {
    ...base,
    description:
      `${base.summary} Published by ${base.publisher}; ingested, validated and keyed to official p-codes by the foundry, then released as part of data version 20261118.1.`,
    contains: [
      `${base.rows.toLocaleString('en')} rows across ${base.levels.join(', ')}`,
      base.indicators ? `${base.indicators} indicators, each with unit, definition and coverage by level and period` : 'Geometry and identifiers only, no statistical indicators',
      'An observation id on every value, resolving to document, page, cell and file hash',
      `Vintage ${base.vintage}; refresh cadence: ${base.cadence.toLowerCase()}`,
    ],
    method: [
      'Fetch the source file on schedule and store it by SHA-256. Unchanged files are skipped.',
      'Parse into rows with the verbatim printed value kept beside the parsed number.',
      'Join to p-codes by code, never by name. Rows that fail to join are held back and listed.',
      'Validate against printed totals and the previous version; publish with a row-level diff.',
    ],
    limits: LIMITS[base.id] ?? GENERIC_LIMITS,
    sources: [
      { n: 1, title: isCensus ? 'Census of Population and Housing 2024 — Preliminary Report (PDF)' : `${base.title} — primary release`, publisher: base.publisher.split(' · ')[0], url: 'https://example.org/source', licence: base.licence.label, status: base.licence.status, retrieved: base.updated, sha256: hex(base.id + 'a', 64) },
      { n: 2, title: isCensus ? 'Population by GN division, district workbooks (XLSX × 25)' : 'Supplementary tables and metadata', publisher: base.publisher.split(' · ').slice(-1)[0], url: 'https://example.org/source-2', licence: base.licence.label, status: base.licence.status, retrieved: base.updated, sha256: hex(base.id + 'b', 64) },
      { n: 3, title: 'Administrative boundaries COD-AB, used for p-code joins', publisher: 'OCHA / Survey Department', url: 'https://example.org/cod-ab', licence: 'CC BY 3.0 IGO', status: 'attribution', retrieved: '2026-10-30', sha256: hex(base.id + 'c', 64) },
    ],
    columns,
    preview,
    downloads: base.formats.map((f) => ({ format: f, label: `${base.id}.${ext[f] ?? f.toLowerCase()}`, size: `${(base.sizeMb * (sizes[f] ?? 1)).toFixed(base.sizeMb < 5 ? 2 : 1)} MB`, href: `#/releases/20261118.1/${base.id}.${ext[f] ?? f.toLowerCase()}` })),
    versions: [
      { dsv: `dsv_${hex(base.id + 'v4', 6)}`, date: base.updated, note: 'Scheduled refresh; source file changed', added: h(base.id) % 900, changed: h(base.id + 'c') % 4200, removed: h(base.id + 'r') % 40 },
      { dsv: `dsv_${hex(base.id + 'v3', 6)}`, date: '2026-07-14', note: 'Re-keyed 31 GN divisions after gazette amendment', added: 31, changed: 262, removed: 31 },
      { dsv: `dsv_${hex(base.id + 'v2', 6)}`, date: '2026-03-02', note: 'Added verbatim values and page locators', added: 0, changed: base.rows > 50_000 ? 50_000 : base.rows, removed: 0 },
      { dsv: `dsv_${hex(base.id + 'v1', 6)}`, date: '2025-11-20', note: 'First release', added: base.rows, changed: 0, removed: 0 },
    ],
    usedIn: [
      { label: 'Area profiles (all levels)', href: '/area/LK11' },
      { label: 'Explore map', href: '/explore' },
    ],
    citation: `${base.publisher.split(' · ')[0]} (${base.vintage.slice(-4)}). ${base.title}. Retrieved ${base.updated} via Lanka Data Layer, data version 20261118.1, dataset ${base.id}.`,
  }
}
