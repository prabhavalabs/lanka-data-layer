/**
 * Typed spec of every public Lanka Data Layer API endpoint, driving the interactive
 * docs under /docs (web/src/pages/docs/**). This is documentation content,
 * not a runtime client — web/src/lib/api.ts remains the one place actual
 * app code talks to the API.
 *
 * Every `example` value below and every `exampleResponse` was captured by
 * actually calling the running API against the real build artifact
 * (`cd api && LANKA_DB=../foundry/data/artifacts/lanka.sqlite PORT=8600
 * npx tsx src/index.ts`), per docs/architecture.md §4 and api/README.md —
 * those two documents are the contract; api/src/routes/*.ts is the source
 * of truth for exact param/response shapes. Long arrays are truncated with
 * a trailing `// … N more` comment for readability; that makes these
 * strings illustrative JSONC, not parseable JSON — the *live* response the
 * playground fetches is real, parsed JSON, rendered by JsonViewer.
 */

export const DOCS_GROUPS = [
  "Search & Lookup",
  "Admin & Geometry",
  "Population",
  "Elections",
  "Data & Tiles",
] as const;

export type DocsGroup = (typeof DOCS_GROUPS)[number];

export type DocsParamKind = "string" | "integer" | "number" | "boolean" | "enum";

export interface DocsParam {
  name: string;
  location: "path" | "query";
  kind: DocsParamKind;
  required: boolean;
  /** Shown in the parameter table; the playground always prefills with `example`, not this. */
  defaultValue?: string;
  enumValues?: readonly string[];
  /** Free-text constraint description, e.g. "2-80 characters" or "≤ 10". */
  constraints?: string;
  description: string;
  /** Prefills the playground input and is used to build the displayed example request. */
  example: string;
}

/**
 * How the raw response should be treated:
 * - "envelope": the usual `{success,message,payload,meta}` (docs/architecture.md §4).
 * - "geojson": a bare GeoJSON Feature — /v1/admin/:pcode/geometry only.
 * - "binary": raw bytes, not JSON at all — /v1/tiles/:file only.
 */
export type DocsResponseKind = "envelope" | "geojson" | "binary";

/** One documented error response, grounded in api/src/lib/errors.ts's HttpError subclasses and each route's actual `throw` sites — not invented. */
export interface DocsErrorCase {
  status: number;
  code: string;
  description: string;
}

/** One dataset this endpoint's response draws from, for the "Sources" trust row — taken from the endpoint's own captured `meta.source` (or, for the two non-envelope routes, the dataset backing it). */
export interface DocsSource {
  name: string;
  license: string;
}

export interface DocsEndpoint {
  slug: string;
  method: "GET";
  /** Path with `:param` placeholders for path params. */
  pathTemplate: string;
  group: DocsGroup;
  summary: string;
  description: string;
  params: DocsParam[];
  responseKind: DocsResponseKind;
  /** Real (truncated) response, captured from the running API. Display-only. */
  exampleResponse: string;
  notes: string[];
  errors: DocsErrorCase[];
  sources: DocsSource[];
}

/** Short mono nav label for the sidebar — path with the leading "/v1/" stripped and ":param" rewritten to "{param}", matching the design's `navLabel` vocabulary (e.g. "postal/{code}", "admin/{pcode}/geometry"). */
export function endpointNavLabel(endpoint: DocsEndpoint): string {
  return endpoint.pathTemplate.replace(/^\/v1\//, "").replace(/:([a-zA-Z_]+)/g, "{$1}");
}

/**
 * Substitutes path params and appends non-empty query params to build a
 * request path (e.g. "/v1/admin/LK1103?include=population,stats"). Shared
 * by the static "example request" display and the playground's live,
 * user-editable inputs — `values` is keyed by param name.
 */
export function buildDocsUrl(endpoint: DocsEndpoint, values: Record<string, string>): string {
  let path = endpoint.pathTemplate;
  const query: string[] = [];
  for (const param of endpoint.params) {
    const value = values[param.name] ?? "";
    if (param.location === "path") {
      path = path.replace(`:${param.name}`, encodeURIComponent(value));
    } else if (param.kind === "boolean") {
      // The API only ever checks for the literal "1" (see lookup.ts's `suggest === "1"`); an
      // unchecked box is omitted entirely rather than sent as the misleading `suggest=false`.
      if (value === "true") query.push(`${param.name}=1`);
    } else if (value !== "") {
      query.push(`${param.name}=${encodeURIComponent(value)}`);
    }
  }
  return query.length > 0 ? `${path}?${query.join("&")}` : path;
}

/** The example request URL, built from every param's `example` value. Display-only. */
export function resolveExampleUrl(endpoint: DocsEndpoint): string {
  const values = Object.fromEntries(endpoint.params.map((p) => [p.name, p.example]));
  return buildDocsUrl(endpoint, values);
}

/** Initial playground state: every param prefilled with its `example` value. */
export function defaultParamValues(endpoint: DocsEndpoint): Record<string, string> {
  return Object.fromEntries(endpoint.params.map((p) => [p.name, p.example]));
}

const LANG_PARAM: DocsParam = {
  name: "lang",
  location: "query",
  kind: "enum",
  required: false,
  defaultValue: "en",
  enumValues: ["en", "si", "ta"],
  constraints: "one of en, si, ta",
  description: "Selects name fields (name_si/name_ta alongside name). Falls back to English when a translation is missing.",
  example: "en",
};

const CACHING_NOTE =
  "Every GET except /v1/health sets ETag: \"<data_version>-<hash>\" and Cache-Control: public, max-age=86400, stale-while-revalidate=604800, and honors If-None-Match with a 304.";

const LANG_NOTE = "lang=en|si|ta (default en); a missing translation falls back to English, it never errors.";

export const DOCS_ENDPOINTS: DocsEndpoint[] = [
  // ---------------------------------------------------------------------
  // Data & Tiles
  // ---------------------------------------------------------------------
  {
    slug: "health",
    method: "GET",
    pathTemplate: "/v1/health",
    group: "Data & Tiles",
    summary: "Liveness probe",
    description:
      "A minimal liveness check. No caching headers, no dataset attribution — excluded from the usual ETag/Cache-Control treatment and from any future auth.",
    params: [],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": { "status": "ok" },
  "meta": { "data_version": "20260812.7", "source": [] }
}`,
    notes: [
      "The only route with no ETag/Cache-Control at all — every other GET carries them.",
      "meta.source is always [] here; there's no dataset to attribute a liveness check to.",
    ],
    errors: [],
    sources: [],
  },
  {
    slug: "datasets",
    method: "GET",
    pathTemplate: "/v1/datasets",
    group: "Data & Tiles",
    summary: "Dataset catalog",
    description:
      "The full catalog of published datasets — source, license, feature count, and bulk-download path for each. This is what powers meta.source attribution on every other route.",
    params: [],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": [
    {
      "id": "admin-units",
      "title": "Administrative units",
      "category": "admin",
      "description": "Sri Lanka's administrative hierarchy (country, provinces, districts, DS divisions, GN divisions), p-code keyed.",
      "source_name": "geoBoundaries + OCHA COD-AB Sri Lanka",
      "source_url": "https://www.geoboundaries.org",
      "license": "CC BY 3.0 IGO / CC BY-IGO",
      "feature_count": 14417,
      "download_path": "downloads/admin-units.csv"
    },
    {
      "id": "cell-lookup",
      "title": "Reverse-geocode grid lookup",
      "category": "admin",
      "...": "cells, country, elections, places, pois, population-2023, postal-codes, protected-areas, railways, roads, waterways",
      "feature_count": 5467317
    }
    // … 11 more datasets (13 total)
  ],
  "meta": { "data_version": "20260812.7", "source": [] }
}`,
    notes: [
      "meta.source is [] here too — this route IS the source table, so it doesn't attribute itself.",
      "feature_count and download_path can be null for datasets that aren't independently downloadable (e.g. derived indexes).",
    ],
    errors: [],
    sources: [],
  },
  {
    slug: "tiles",
    method: "GET",
    pathTemplate: "/v1/tiles/:file",
    group: "Data & Tiles",
    summary: "PMTiles static passthrough",
    description:
      "Streams a *.pmtiles archive from the foundry's tiles directory with HTTP Range support, for MapLibre's pmtiles:// protocol to read directly. Not the usual envelope — raw octet-stream bytes.",
    params: [
      {
        name: "file",
        location: "path",
        kind: "enum",
        required: true,
        enumValues: ["admin.pmtiles", "electoral.pmtiles", "places.pmtiles", "protected.pmtiles", "transport.pmtiles", "water.pmtiles"],
        constraints: "must match ^[a-z0-9-]+\\.pmtiles$",
        description: "Archive filename. Anything outside the whitelist pattern (traversal, dotfiles, wrong extension) is a plain 404, not a 400.",
        example: "admin.pmtiles",
      },
    ],
    responseKind: "binary",
    exampleResponse: `HTTP/1.1 206 Partial Content
accept-ranges: bytes
cache-control: public, max-age=31536000, immutable
content-length: 16
content-range: bytes 0-15/38727653
content-type: application/octet-stream
etag: "20260812.7-msq8g036.nv-n22g5"

<binary pmtiles bytes>`,
    notes: [
      "Deliberately outside the envelope — pmtiles clients fetch() this URL directly and read bytes off the Response.",
      "Supports byte-range requests (Range: bytes=start-end); PMTiles clients rely on this to pull only the header, directory, and specific tiles they need, never the whole archive.",
      "Cache-Control is immutable — a data_version's tiles never change once built.",
      "A missing tiles directory or file is a normal 404 (the foundry's tile-emission step may not have run yet), not a startup error.",
      "The playground below issues a small ranged request (bytes=0-15) rather than downloading the full archive, and shows headers only.",
    ],
    errors: [{ status: 404, code: "not_found", description: "Filename doesn't match the pmtiles whitelist, or the file doesn't exist in LANKA_TILES_DIR." }],
    sources: [
      { name: "OpenStreetMap", license: "ODbL" },
      { name: "geoBoundaries + OCHA COD-AB Sri Lanka", license: "CC BY 3.0 IGO / CC BY-IGO" },
    ],
  },

  // ---------------------------------------------------------------------
  // Search & Lookup
  // ---------------------------------------------------------------------
  {
    slug: "lookup",
    method: "GET",
    pathTemplate: "/v1/lookup",
    group: "Search & Lookup",
    summary: "Universal lookup",
    description:
      "Classifies q before searching, then dispatches: a coordinate pair ('6.93,79.84', with optional °/N/E decoration) → reverse geocode; a 5-digit number → postal code (exact match, then prefix); an LK-prefixed p-code (LK\\d+) → admin unit; anything else → blended place + postal-name fuzzy search. Full mode returns a typed list {type, score, payload}, best match first; ?suggest=1 switches to lightweight typeahead rows capped at 10.",
    params: [
      {
        name: "q",
        location: "query",
        kind: "string",
        required: true,
        constraints: "1-200 characters (trimmed)",
        description: "The query — a coordinate pair, 5-digit postal code, LK p-code, or free text.",
        example: "10500",
      },
      LANG_PARAM,
      {
        name: "suggest",
        location: "query",
        kind: "boolean",
        required: false,
        description: "Set to switch every branch to the lightweight typeahead row shape, capped at 10 results.",
        example: "false",
      },
      {
        name: "limit",
        location: "query",
        kind: "integer",
        required: false,
        defaultValue: "10",
        constraints: "1-50 (full mode); capped at 10 in suggest mode regardless of this value",
        description: "Max results in full mode.",
        example: "10",
      },
    ],
    responseKind: "envelope",
    exampleResponse: `// Full mode, q=10500 (exact 5-digit postal match):
{
  "success": true,
  "message": "OK",
  "payload": [
    { "type": "postal", "score": 1, "payload": { "code": "10500", "name": "Padukka", "lat": 6.8408, "lon": 80.0897 } }
  ],
  "meta": { "data_version": "20260812.7", "source": [{ "name": "GeoNames", "url": "https://download.geonames.org/export/zip/LK.zip", "license": "CC BY 4.0" }] }
}

// Suggest mode, q=colombo (blended free-text branch):
{
  "success": true,
  "message": "OK",
  "payload": [
    { "type": "place", "id": 1, "label": "Colombo", "sublabel": "Colombo District", "lat": 6.9388614, "lon": 79.8542005, "pcode": "LK1103115" }
  ],
  "meta": { "data_version": "20260812.7", "source": [{ "name": "OpenStreetMap", "...": "..." }, { "name": "geoBoundaries + OCHA COD-AB Sri Lanka", "...": "..." }] }
}`,
    notes: [
      CACHING_NOTE,
      LANG_NOTE,
      "Only the coordinate branch can 404 (not_in_coverage, for a point outside the grid bbox). Every other branch treats 'no match' as a normal empty payload array.",
      "Postal matching in the current build is exact-5-digit only: every postal_codes.code is exactly 5 digits, so the 'prefix' sub-branch (matching codes that share a prefix, excluding the exact match) never actually contributes rows yet — typing fewer than 5 digits falls through to the free-text branch instead, which matches place and postal *names*, not partial codes.",
      "Suggest rows: label is the best-lang name (or the code itself, for postal — sublabel carries the place name there); a place's sublabel is its admin parent resolved via cell_lookup when cheap, else its kind; an admin row's sublabel is '<level name> · <parent name>'.",
    ],
    errors: [
      { status: 400, code: "validation_error", description: "q missing/empty, or another query param fails validation." },
      { status: 404, code: "not_in_coverage", description: "q parses as a coordinate pair outside the grid's bbox." },
    ],
    sources: [
      { name: "GeoNames", license: "CC BY 4.0" },
      { name: "OpenStreetMap", license: "ODbL" },
      { name: "geoBoundaries + OCHA COD-AB Sri Lanka", license: "CC BY 3.0 IGO / CC BY-IGO" },
    ],
  },
  {
    slug: "search",
    method: "GET",
    pathTemplate: "/v1/search",
    group: "Search & Lookup",
    summary: "Place search",
    description:
      "FTS5 prefix search over places, re-ranked by score = matchQuality + populationBoost (exact 1.0, prefix 0.9, blended with a log-scaled population boost capped at 0.8).",
    params: [
      {
        name: "q",
        location: "query",
        kind: "string",
        required: true,
        constraints: "2-80 characters",
        description: "Search text.",
        example: "colombo",
      },
      LANG_PARAM,
      {
        name: "limit",
        location: "query",
        kind: "integer",
        required: false,
        defaultValue: "10",
        constraints: "1-50",
        description: "Max results.",
        example: "3",
      },
      {
        name: "min_population",
        location: "query",
        kind: "number",
        required: false,
        constraints: "≥ 0",
        description: "Filters out places below this population.",
        example: "",
      },
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": [
    { "id": 1, "name": "Colombo", "kind": "city", "lat": 6.9388614, "lon": 79.8542005, "population": 753000, "admin_pcode": null, "score": 1.5876795552952618 }
  ],
  "meta": {
    "data_version": "20260812.7",
    "source": [{ "name": "OpenStreetMap", "url": "https://www.openstreetmap.org", "license": "ODbL" }]
  }
}`,
    notes: [CACHING_NOTE, LANG_NOTE, "q shorter than 2 or longer than 80 characters is a 400 validation_error."],
    errors: [{ status: 400, code: "validation_error", description: "q is shorter than 2 or longer than 80 characters, or another query param fails validation." }],
    sources: [{ name: "OpenStreetMap", license: "ODbL" }],
  },
  {
    slug: "reverse",
    method: "GET",
    pathTemplate: "/v1/reverse",
    group: "Search & Lookup",
    summary: "Reverse geocode",
    description:
      "cellId(lat, lon) via the canonical grid, then cell_lookup → admin parent chain. Falls back to the nearest named place within 25km (bbox prefilter + haversine) when cell_lookup has no row for that cell — admin fields are null in that case.",
    params: [
      { name: "lat", location: "query", kind: "number", required: true, description: "Latitude.", example: "6.9271" },
      { name: "lon", location: "query", kind: "number", required: true, description: "Longitude.", example: "79.8612" },
      LANG_PARAM,
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": {
    "cell_id": 8294861,
    "gnd": { "pcode": "LK1103140", "level": 4, "name": "Suduwella", "name_si": "සුදුවැල්ල", "name_ta": null, "parent_pcode": "LK1103", "area_km2": 0.9221773, "centroid": { "lat": 6.92655401, "lon": 79.85914308 } },
    "ds_division": { "pcode": "LK1103", "level": 3, "name": "Colombo", "...": "AdminUnit fields" },
    "district": { "pcode": "LK11", "level": 2, "name": "Colombo District", "...": "AdminUnit fields" },
    "province": { "pcode": "LK1", "level": 1, "name": "Western Province", "...": "AdminUnit fields" },
    "postal_code": "00200",
    "nearest_place": { "id": 446, "name": "Suduwella", "distance_m": 135 }
  },
  "meta": { "data_version": "20260812.7", "source": [ /* WorldPop, admin-units, places, postal-codes */ ] }
}`,
    notes: [
      CACHING_NOTE,
      LANG_NOTE,
      "Out-of-bbox coordinates are a 404 not_in_coverage, not a null payload.",
      "gnd/ds_division/district/province and nearest_place are all null together only when the cell falls in-bbox but has no cell_lookup row and no nearby named place — rare, but not impossible near the coast.",
    ],
    errors: [
      { status: 400, code: "validation_error", description: "lat/lon missing or not finite numbers." },
      { status: 404, code: "not_in_coverage", description: "The point is outside Sri Lankan territory (the grid bbox)." },
    ],
    sources: [
      { name: "geoBoundaries + OCHA COD-AB Sri Lanka", license: "CC BY 3.0 IGO / CC BY-IGO" },
      { name: "GeoNames", license: "CC BY 4.0" },
      { name: "OpenStreetMap", license: "ODbL" },
      { name: "WorldPop", license: "CC BY 4.0" },
    ],
  },
  {
    slug: "postal-code",
    method: "GET",
    pathTemplate: "/v1/postal/:code",
    group: "Search & Lookup",
    summary: "Postal code lookup",
    description:
      "A postal code record — code, name, lat/lon, and its admin chain (gnd/ds_division/district/province), derived from cell_lookup at the code's own coordinates.",
    params: [
      { name: "code", location: "path", kind: "string", required: true, description: "5-digit postal code.", example: "10500" },
      LANG_PARAM,
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": {
    "code": "10500",
    "name": "Padukka",
    "lat": 6.8408,
    "lon": 80.0897,
    "gnd": { "pcode": "LK1118090", "level": 4, "name": "Padukka", "name_si": "පාදුක්ක", "name_ta": null, "parent_pcode": "LK1118", "area_km2": 1.38251408, "centroid": { "lat": 6.83778639, "lon": 80.09284893 } },
    "ds_division": { "pcode": "LK1118", "level": 3, "name": "Padukka", "...": "AdminUnit fields" },
    "district": { "pcode": "LK11", "level": 2, "name": "Colombo District", "...": "AdminUnit fields" },
    "province": { "pcode": "LK1", "level": 1, "name": "Western Province", "...": "AdminUnit fields" }
  },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "GeoNames", "...": "..." }, { "name": "geoBoundaries + OCHA COD-AB Sri Lanka", "...": "..." }] }
}`,
    notes: [
      CACHING_NOTE,
      LANG_NOTE,
      "404 for an unknown code.",
      "postal_codes.admin_pcode is unpopulated in the current artifact, so the admin chain is derived the same way /v1/reverse derives it (cellId → cell_lookup.gnd_pcode → parent chain); if that point isn't covered, the chain is all-null but the record is still a valid 200.",
      "gnd.pcode is what the postal-code demo (/docs/demo) feeds into /v1/admin/:pcode/geometry to draw the boundary glow.",
    ],
    errors: [{ status: 404, code: "not_found", description: "No postal_codes row for that code." }],
    sources: [
      { name: "GeoNames", license: "CC BY 4.0" },
      { name: "geoBoundaries + OCHA COD-AB Sri Lanka", license: "CC BY 3.0 IGO / CC BY-IGO" },
    ],
  },
  {
    slug: "postal-point",
    method: "GET",
    pathTemplate: "/v1/postal",
    group: "Search & Lookup",
    summary: "Postal code at a point",
    description: "The postal code covering (lat, lon), via cell_lookup.",
    params: [
      { name: "lat", location: "query", kind: "number", required: true, description: "Latitude.", example: "6.8408" },
      { name: "lon", location: "query", kind: "number", required: true, description: "Longitude.", example: "80.0897" },
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": { "cell_id": 8529989, "postal_code": "10500", "name": "Padukka" },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "GeoNames", "url": "https://download.geonames.org/export/zip/LK.zip", "license": "CC BY 4.0" }] }
}`,
    notes: [
      CACHING_NOTE,
      "postal_code (and name) are null, not a 404, when the cell has no postal code on record.",
      "Out-of-bbox coordinates are a 404 not_in_coverage.",
    ],
    errors: [
      { status: 400, code: "validation_error", description: "lat/lon missing or not finite numbers." },
      { status: 404, code: "not_in_coverage", description: "The point is outside the grid bbox." },
    ],
    sources: [{ name: "GeoNames", license: "CC BY 4.0" }],
  },

  // ---------------------------------------------------------------------
  // Admin & Geometry
  // ---------------------------------------------------------------------
  {
    slug: "admin-unit",
    method: "GET",
    pathTemplate: "/v1/admin/:pcode",
    group: "Admin & Geometry",
    summary: "Admin unit detail",
    description:
      "An admin unit, its ancestor chain (root → immediate parent), and a children summary (id + name only). ?include=population,stats adds the heavier per-unit breakdowns.",
    params: [
      { name: "pcode", location: "path", kind: "string", required: true, description: "Admin p-code.", example: "LK1103" },
      LANG_PARAM,
      {
        name: "include",
        location: "query",
        kind: "string",
        required: false,
        constraints: "comma-separated subset of: population, stats",
        description: "Adds the named breakdown(s) to the payload.",
        example: "population,stats",
      },
    ],
    responseKind: "envelope",
    exampleResponse: `// GET /v1/admin/LK1103 (no include) — Colombo DS division:
{
  "success": true,
  "message": "OK",
  "payload": {
    "unit": { "pcode": "LK1103", "level": 3, "name": "Colombo", "name_si": "කොළඹ", "name_ta": "கொழும்பு", "parent_pcode": "LK11", "area_km2": 24.53715118, "centroid": { "lat": 6.94698684, "lon": 79.86526281 } },
    "parent_chain": [
      { "pcode": "LK", "level": 0, "name": "Sri Lanka", "...": "AdminUnit fields" },
      { "pcode": "LK1", "level": 1, "name": "Western Province", "...": "AdminUnit fields" },
      { "pcode": "LK11", "level": 2, "name": "Colombo District", "...": "AdminUnit fields" }
    ],
    "children": [
      { "pcode": "LK1103005", "name": "Sammanthranapura" },
      { "pcode": "LK1103010", "name": "Mattakkuliya" }
      // … 33 more children (35 total GN divisions)
    ]
  },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "geoBoundaries + OCHA COD-AB Sri Lanka", "...": "..." }] }
}

// GET /v1/admin/LK11?include=population,stats — Colombo District (has real rows):
{
  "success": true,
  "message": "OK",
  "payload": {
    "unit": { "pcode": "LK11", "level": 2, "name": "Colombo District", "...": "AdminUnit fields" },
    "parent_chain": [ /* LK, LK1 */ ],
    "children": [ { "pcode": "LK1103", "name": "Colombo" } /* … 12 more */ ],
    "population": null,
    "stats": {
      "year": 2012,
      "values": {
        "ethnicity.sinhala": 1732530, "ethnicity.sriLankanTamil": 246932, "ethnicity.moor": 247739,
        "religion.buddhist": 1585000, "religion.hindu": 210000, "religion.muslim": 260000
        // … 9 more ethnicity/religion values
      }
    }
  },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "geoBoundaries + OCHA COD-AB Sri Lanka", "...": "..." }] }
}`,
    notes: [
      CACHING_NOTE,
      LANG_NOTE,
      "404 for an unknown pcode.",
      "population/stats are null (not omitted) when ?include= asked for them but the unit has no rows — LK1103 above has neither; LK11 has stats but not population. This is normal, not an error: census/stats coverage is still being built out level by level.",
      "An unrecognized ?include= value (anything but population or stats) is a 400 validation_error.",
    ],
    errors: [
      { status: 400, code: "validation_error", description: "include contains a value other than population or stats, or pcode/lang fails validation." },
      { status: 404, code: "not_found", description: "No admin_units row for that pcode." },
    ],
    sources: [{ name: "geoBoundaries + OCHA COD-AB Sri Lanka", license: "CC BY 3.0 IGO / CC BY-IGO" }],
  },
  {
    slug: "admin-geometry",
    method: "GET",
    pathTemplate: "/v1/admin/:pcode/geometry",
    group: "Admin & Geometry",
    summary: "Admin boundary geometry",
    description:
      "The unit's simplified boundary as a bare GeoJSON Feature — deliberately NOT the usual envelope, so map libraries (MapLibre source.setData, Leaflet L.geoJSON, …) can hand the response straight to their GeoJSON loader.",
    params: [
      { name: "pcode", location: "path", kind: "string", required: true, description: "Admin p-code.", example: "LK1103" },
      LANG_PARAM,
    ],
    responseKind: "geojson",
    exampleResponse: `{
  "type": "Feature",
  "properties": { "pcode": "LK1103", "level": 3, "name": "Colombo", "name_si": "කොළඹ", "name_ta": "கொழும்பு", "parent_pcode": "LK11", "area_km2": 24.53715118 },
  "geometry": {
    "type": "Polygon",
    "coordinates": [[[79.884976, 6.974658], [79.884262, 6.97783], [79.880245, 6.981013] /* … 124 more points */]]
  }
}`,
    notes: [
      "Not the {success,message,payload,meta} envelope — the response body IS the Feature.",
      "404 if the pcode has no admin_geometry row (not every level/unit necessarily has one).",
      "Cache-Control is immutable (max-age=31536000) — a pcode's geometry never changes within a data_version, stronger than the usual 86400/stale-while-revalidate pair.",
      "data_version still travels via the ETag; there's no per-request meta.source the way envelope routes have.",
      "This is what MapView's setHighlight({kind:'geometry', feature}) consumes for the glowing-outline highlight (see stores/map-store.ts) — the /docs/demo page's boundary glow calls this endpoint the same way.",
    ],
    errors: [
      { status: 400, code: "validation_error", description: "pcode or lang fails validation." },
      { status: 404, code: "not_found", description: "The pcode has no admin_geometry row." },
    ],
    sources: [{ name: "geoBoundaries + OCHA COD-AB Sri Lanka", license: "CC BY 3.0 IGO / CC BY-IGO" }],
  },

  // ---------------------------------------------------------------------
  // Population
  // ---------------------------------------------------------------------
  {
    slug: "population",
    method: "GET",
    pathTemplate: "/v1/population",
    group: "Population",
    summary: "Gridded population at a point",
    description:
      "Population of the grid cell containing (lat, lon), optionally summed over a radius (≤10km) using the same row-band technique as the canonical grid.",
    params: [
      { name: "lat", location: "query", kind: "number", required: true, description: "Latitude.", example: "6.9271" },
      { name: "lon", location: "query", kind: "number", required: true, description: "Longitude.", example: "79.8612" },
      {
        name: "radius",
        location: "query",
        kind: "number",
        required: false,
        constraints: "0-10 (km)",
        description: "Sums population over this radius instead of returning just the point cell.",
        example: "1",
      },
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": {
    "cell_id": 8294861,
    "populated": true,
    "point": { "population": 239.79185485839844, "in_coverage": true },
    "radius": { "radius_km": 1, "population": 56427.975775824656, "cell_count": 294 }
  },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "WorldPop", "url": "https://www.worldpop.org", "license": "CC BY 4.0" }] }
}`,
    notes: [
      CACHING_NOTE,
      "populated is a build-wide flag (does the cells table have any data at all this phase), separate from point.in_coverage (does *this* specific cell have a row). Both can be false together if the gridded-population artifact hasn't been built yet — that's a graceful degrade, not an error.",
      "radius is null unless ?radius was given and > 0.",
      "Population values are floats (WorldPop's per-cell estimate distributed across the grid), not integers — don't assume whole people.",
    ],
    errors: [
      { status: 400, code: "validation_error", description: "lat/lon missing/not finite, or radius outside 0-10." },
      { status: 404, code: "not_in_coverage", description: "The point is outside Sri Lankan territory." },
    ],
    sources: [{ name: "WorldPop", license: "CC BY 4.0" }],
  },
  {
    slug: "population-grid",
    method: "GET",
    pathTemplate: "/v1/population/grid",
    group: "Population",
    summary: "Density grid for map rendering",
    description:
      "Aggregates the canonical fine cells into res-degree buckets and returns compact [lat, lon, population] triples (bucket centers, population rounded) — built for GPU layers (deck.gl columns/heatmaps), not point lookups.",
    params: [
      {
        name: "res",
        location: "query",
        kind: "enum",
        required: false,
        defaultValue: "0.02",
        enumValues: ["0.01", "0.02", "0.05"],
        description: "Bucket size in degrees. Smaller = more buckets (0.01 ≈ 55.7K buckets; 0.05 ≈ 2.4K).",
        example: "0.05",
      },
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": {
    "res": 0.05,
    "count": 2371,
    "cells": [
      [9.825, 79.975, 3100],
      [9.825, 80.025, 2223],
      [9.825, 80.075, 168]
      // … 2368 more [lat, lon, population] triples
    ]
  },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "WorldPop", "url": "https://www.worldpop.org", "license": "CC BY 4.0" }] }
}`,
    notes: [
      CACHING_NOTE,
      "res outside {0.01, 0.02, 0.05} is a 400 validation_error — there's no arbitrary-resolution aggregation.",
      "The first request at a given res pays a one-time ~1-2s scan cost (5.4M rows); the process memoizes every resolution after that for its lifetime, since the DB is read-only.",
      "At res=0.01 this returns ~55,700 rows — the playground below truncates the rendered array, it does not limit what the API itself returns.",
    ],
    errors: [{ status: 400, code: "validation_error", description: "res is not one of 0.01, 0.02, 0.05." }],
    sources: [{ name: "WorldPop", license: "CC BY 4.0" }],
  },

  // ---------------------------------------------------------------------
  // Elections
  // ---------------------------------------------------------------------
  {
    slug: "elections",
    method: "GET",
    pathTemplate: "/v1/elections",
    group: "Elections",
    summary: "Election catalog",
    description: "Every election on record, most recent first.",
    params: [],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": [
    { "id": "parl-2024", "type": "parliamentary", "year": 2024, "date": "2024-11-14", "label": "2024 Parliamentary" },
    { "id": "pres-2024", "type": "presidential", "year": 2024, "date": "2024-09-21", "label": "2024 Presidential" },
    { "id": "pres-2019", "type": "presidential", "year": 2019, "date": "2019-11-16", "label": "2019 Presidential" },
    { "id": "pres-2015", "type": "presidential", "year": 2015, "date": "2015-01-08", "label": "2015 Presidential" }
  ],
  "meta": { "data_version": "20260812.7", "source": [{ "name": "Election Commission of Sri Lanka (via nuuuwan/lk_elections)", "url": "https://github.com/nuuuwan/lk_elections", "license": "open" }] }
}`,
    notes: [CACHING_NOTE, "Small, un-paginated list — one row per election held, not per result."],
    errors: [],
    sources: [{ name: "Election Commission of Sri Lanka (via nuuuwan/lk_elections)", license: "open" }],
  },
  {
    slug: "election-results",
    method: "GET",
    pathTemplate: "/v1/elections/:id/results/:entity",
    group: "Elections",
    summary: "Results for one entity",
    description:
      "Parsed results for one electoral entity (an ED, PD, postal-vote bucket, or the national total) within an election, with party metadata (color, candidate, full name) joined in from election_parties.",
    params: [
      { name: "id", location: "path", kind: "string", required: true, description: "Election id.", example: "parl-2024" },
      { name: "entity", location: "path", kind: "string", required: true, description: "Entity id (ED/PD/postal/national code).", example: "EC-01" },
    ],
    responseKind: "envelope",
    exampleResponse: `{
  "success": true,
  "message": "OK",
  "payload": {
    "election": { "id": "parl-2024", "type": "parliamentary", "year": 2024, "date": "2024-11-14", "label": "2024 Parliamentary" },
    "entity": { "id": "EC-01", "kind": "ED", "name": "Colombo", "ed_id": "EC-01" },
    "electors": 1765351,
    "polled": 1211738,
    "valid": 1149125,
    "rejected": 62613,
    "winner_party": "NPP",
    "winner_votes": 788636,
    "turnout_pct": 68.6,
    "margin": 50.51,
    "other_votes": 41594,
    "parties": [
      { "code": "NPP", "votes": 788636, "pct": 68.63, "candidate": "National People's Power", "name": "National People's Power", "color": "#dc2626" },
      { "code": "SJB", "votes": 208249, "pct": 18.12, "candidate": "Samagi Jana Balawegaya", "name": "Samagi Jana Balawegaya", "color": "#16a34a" }
      // … 3 more parties (5 total)
    ]
  },
  "meta": { "data_version": "20260812.7", "source": [{ "name": "Election Commission of Sri Lanka (via nuuuwan/lk_elections)", "...": "..." }] }
}`,
    notes: [
      CACHING_NOTE,
      "404 (not_found) for an unknown election id, entity id, or a valid pairing with no results row.",
      "parties is sorted by votes descending; a party missing from election_parties still appears with candidate/name/color as null rather than being dropped.",
    ],
    errors: [{ status: 404, code: "not_found", description: "Unknown election id, unknown entity id, or a valid pairing with no results row." }],
    sources: [{ name: "Election Commission of Sri Lanka (via nuuuwan/lk_elections)", license: "open" }],
  },
];

export function getDocsEndpoint(slug: string): DocsEndpoint | undefined {
  return DOCS_ENDPOINTS.find((e) => e.slug === slug);
}

export function docsEndpointsByGroup(): Array<{ group: DocsGroup; endpoints: DocsEndpoint[] }> {
  return DOCS_GROUPS.map((group) => ({
    group,
    endpoints: DOCS_ENDPOINTS.filter((e) => e.group === group),
  }));
}
