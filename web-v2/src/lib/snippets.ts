// Example calls shown on the landing page, dataset pages and /developers.
// They follow the planned v2 surface (REST, MCP tools, `lanka` CLI); output values are artificial.
import { API, DOWNLOADS, MCP, SITE } from './site'

export const REST_REQUEST = `$ curl ${API}/areas/LK11`

export const REST_RESPONSE = `{
  "success": true,
  "payload": {
    "pcode": "LK11",
    "level": 2,
    "name": { "en": "Colombo", "si": "කොළඹ", "ta": "கொழும்பு" },
    "parents": ["LK1", "LK"],
    "headline": [
      { "indicator": "pop.total", "value": 2374461, "period": "2024",
        "status": "provisional", "observation_id": "obs_9a41c07e" },
      { "indicator": "pop.density", "value": 3397, "unit": "per km²" }
    ],
    "rank": { "pop.density": { "rank": 1, "of": 25 } }
  },
  "meta": {
    "data_version": "20261118.1",
    "source": [{ "publisher": "Department of Census and Statistics" }],
    "licences": [{ "id": "lk-gov-unstated", "status": "permission_unclear" }],
    "completeness": { "status": "partial",
      "note": "GN level not published for this table" },
    "next": [{ "rel": "series",
      "href": "/v2/indicators/pop.total/series?geo=LK11" }]
  }
}`

export const MCP_CONFIG = `$ claude mcp add --transport http lanka ${MCP}
# anonymous and read-only: no key, no account`

export const MCP_CALL = `{
  "method": "tools/call",
  "params": {
    "name": "get_area_profile",
    "arguments": { "pcode": "LK11", "sections": ["population", "access"] }
  }
}`

export const MCP_RESULT = `{
  "area": { "pcode": "LK11", "name": "Colombo", "level": "district" },
  "population": { "total": 2374461, "period": "2024", "rank": "3 of 25" },
  "access": { "hospital_drive_min": 9.1, "within_30_min": 0.988 },
  "provenance": [{
    "observation_id": "obs_9a41c07e",
    "document": "Census of Population and Housing 2024, preliminary report",
    "locator": "p. 42, Table 2.1, row Colombo",
    "sha256": "5f1c…e09b"
  }],
  "licences": [{ "id": "lk-gov-unstated", "status": "permission_unclear" }],
  "completeness": "partial",
  "data_version": "20261118.1",
  "next": [{ "tool": "compare_areas", "arguments": { "pcodes": ["LK11", "LK12"] } }]
}`

export const CLI_SESSION = `$ lanka pull core
  ✓ lanka-open.sqlite   41.8 MB  sha256 verified
  ✓ admin.pmtiles      148.3 MB  sha256 verified

$ lanka geocode-csv clinics.csv --lat lat --lon lon -o clinics.pcoded.csv
  1,204,518 rows · 0.9 s · 1.34 M rows/s
  + adm1_pcode adm2_pcode adm3_pcode adm4_pcode
  + adm4_name_en adm4_name_si adm4_name_ta postal_code
  318 rows fell outside the island (kept, p-code empty)

$ lanka area LK11 --md | head -4
  # Colombo  ·  කොළඹ  ·  கொழும்பு      LK11  district
  population   2,374,461   2024  provisional
  density          3,397   per km²
  households       641,203`

export const CLI_INSTALL = `$ curl -fsSL ${SITE}/install.sh | sh
$ brew install prabhavalabs/tap/lanka
$ npx @lanka-data-layer/cli area LK11`

export const DUCKDB = `select pcode, population
from '${DOWNLOADS}/census-2024.parquet'
where level = 3
order by population desc
limit 10;`

export const REST_ENDPOINTS: [method: string, path: string, purpose: string][] = [
  ['GET', '/v2/catalog?q=&theme=&level=&licence=', 'Search datasets and indicators'],
  ['GET', '/v2/areas/{pcode}', 'Area profile: hierarchy, headline numbers, rank among siblings'],
  ['GET', '/v2/indicators/{id}/series?geo=LK11,LK12', 'Time series for up to 25 areas'],
  ['GET', '/v2/indicators/{id}/map?level=3&period=2024', 'Compact arrays for a choropleth, no geometry'],
  ['GET', '/v2/compare?geo=LK11,LK21&indicators=…', 'Side-by-side matrix'],
  ['GET', '/v2/point?lat=&lon=&radius_m=', 'Reverse geocode, every grid layer, people in radius'],
  ['GET', '/v2/reach?lat=&lon=&mode=walk&minutes=10,20', 'Isochrones with people and facilities reached'],
  ['GET', '/v2/observations/{id}/provenance', 'Document, page, cell, verbatim value, hash, crop'],
  ['POST', '/v2/query', 'Bounded structured query, up to 5,000 rows'],
  ['GET', '/v2/status', 'Freshness of every source'],
]

export const MCP_TOOLS: [name: string, args: string, returns: string][] = [
  ['search_catalog', 'q, theme, geo_level', 'Ranked datasets and indicators with coverage and licence status'],
  ['describe_dataset', 'dataset_id', 'Indicators, limits, sources, freshness'],
  ['resolve_place', 'q (name, postal code, p-code, lat,lon)', 'Typed matches with p-code chain'],
  ['get_area_profile', 'pcode, sections', 'Headline indicators with rank and national value'],
  ['query_indicator', 'indicator_id, geo or level, period', 'Rows with observation ids and status'],
  ['compare_areas', 'pcodes (2–10), indicators', 'Matrix plus spread per indicator'],
  ['analyze_point', 'lat, lon, radius_m', 'Admin chain, people in radius, nearest facilities'],
  ['find_features', 'dataset, near or bbox or pcode', 'Features with distance and source'],
  ['travel_reach', 'lat, lon, mode, minutes', 'People and facilities per travel-time band'],
  ['get_provenance', 'observation_id', 'Document, page and cell, SHA-256, licence, review'],
]

export const CLI_COMMANDS: [cmd: string, what: string][] = [
  ['lanka pull [core|grids|tiles|all]', 'Download and verify artifacts'],
  ['lanka lookup "nugegoda" | 10250 | LK1103', 'Resolve a name, postal code or p-code'],
  ['lanka reverse 6.9271 79.8612', 'Coordinates to the full p-code chain'],
  ['lanka geocode-csv in.csv --lat lat --lon lon', 'Append p-codes, names and postal code to a spreadsheet'],
  ['lanka area LK11 --json | --md', 'Area profile'],
  ['lanka series pop.total --geo LK11,LK21', 'Time series'],
  ['lanka zonal raster.tif --by adm3 --stat mean', 'Your raster onto our grid and p-codes'],
  ['lanka sql "select … from observations"', 'DuckDB over local Parquet'],
  ['lanka prov obs_9a41c07e', 'Provenance of one value'],
  ['lanka mcp', 'MCP server over stdio, works offline'],
]
