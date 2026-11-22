# 06 — Serving

## 1. One query core, two backends

```
                 ┌──────────── transports (thin: parse → call → envelope) ────────────┐
   REST /v2   OGC /v2/ogc   tiles /v2/tiles   MCP /mcp + stdio   CLI `lanka …`   exports (lanka-export)
                 └───────────────────────────────┬────────────────────────────────────┘
                                                 ▼
                          lanka-query  (engine/crates/lanka-query)
     request types · validation · id resolution · period maths · roll-up semantics · breaks · paging
     envelope + meta (attribution, licences, versions, estimate notice) · `next` links · repair hints
                                                 │  trait Backend
                         ┌───────────────────────┴───────────────────────┐
                         ▼                                               ▼
                 PgBackend (server)                             PackBackend (offline)
        Postgres + mmap grids + PMTiles                 SQLite pack + grid arrays from a release
```

```rust
// engine/crates/lanka-query/src/backend.rs
#[async_trait]
pub trait Backend: Send + Sync {
    fn catalog(&self) -> Arc<Catalog>;                       // snapshot: datasets, indicators, units, codelists, versions
    async fn series(&self, q: &SeriesPlan) -> Result<Vec<ObsRow>>;
    async fn map(&self, q: &MapPlan) -> Result<Vec<ObsRow>>;
    async fn area_latest(&self, geo_key: i32, filter: &IndicatorFilter) -> Result<Vec<ObsRow>>;
    async fn obs_by_id(&self, obs_id: i64) -> Result<Option<ObsRow>>;
    async fn provenance(&self, obs_id: i64) -> Result<Option<ProvRow>>;
    async fn derivation(&self, version_id: i64) -> Result<Option<Derivation>>;
    async fn units_at(&self, lon: f64, lat: f64) -> Result<UnitHit>;            // grid first, polygon fallback
    async fn features(&self, q: &FeaturePlan) -> Result<FeaturePage>;
    async fn search(&self, q: &SearchPlan) -> Result<Vec<SearchHit>>;
    fn grid(&self, layer: &str, period: &str) -> Result<Option<GridView<'_>>>;
    async fn status(&self) -> Result<StatusSnapshot>;       // PackBackend returns the release manifest's static view
}
```

Everything with semantics lives above the trait and is written once. The backends contain SQL only: ~12 statements each, in two dialects. A **conformance corpus** (`engine/tests/corpus/*.json`: request → expected payload) runs against both backends in CI; a query that returns different JSON from Postgres and from a pack built from the same release fails the build. This is how "one implementation of every number" survives having an offline mode.

`lanka-core::query` (973 lines today) is split: plans + semantics → `lanka-query`; SQL → `lanka-query::pg`.

## 2. REST v2

Base path `/v2`. JSON. Envelope kept: `{success, message, payload, meta}`.

```jsonc
"meta": {
  "data_version": "20261204.1",
  "contract_version": 2,
  "spec_version": "1.0",
  "dataset_versions": [{ "dataset": "dcs-census-2024-population", "dsv": "dsv_9c1e…", "vintage": "CPH 2024 final" }],
  "attribution": [{ "source_id": "dcs-census-2024", "publisher": "Department of Census and Statistics",
                    "text": "…", "licence": "lk-gov-unstated", "licence_status": "permission_unclear", "redistribution": "api" }],
  "contains": ["reported", "computed"],
  "estimate_notice": null,
  "live": false,
  "next": [{ "rel": "series", "href": "/v2/indicators/census24.pop_total/series?geo=LK11" }],
  "page": { "next_cursor": null }
}
```

`meta.source` (the v1 field) is emitted as an alias of `attribution[].text` for the life of contract 2.

### 2.1 Resources

| Resource | Endpoint | Notes |
|---|---|---|
| **Catalogue** | `GET /v2/datasets?q=&theme=&level=&class=&licence_status=&kind=&cursor=` | replaces `/v2/catalog` (kept as an alias) |
| | `GET /v2/datasets/{id}` | card: description, method, **limits**, sources, licence, indicators, coverage, current version, freshness, quality summary, distributions |
| | `GET /v2/datasets/{id}/versions`, `/versions/{dsv}`, `/versions/{dsv}/diff?against={dsv}` | version list, validation report, row-level diff (paged) |
| | `GET /v2/datasets/{id}/quality` | expectations, cross-source checks, validation scores |
| | `GET /v2/datasets/{id}/attribution?format=text\|html\|json` | ready-made credit line |
| | `GET /v2/indicators?q=&dataset=&level=&class=&sdg=` · `GET /v2/indicators/{id}` | definition, unit, dims with codelists, aggregation rule, coverage per level, related (`estimates_of`) |
| | `GET /v2/codelists/{id}` · `GET /v2/units` · `GET /v2/sources/{id}` · `GET /v2/methods/{id}` · `GET /v2/recipes/{id}` | reference data |
| | `GET /v2/catalog.jsonld` | DCAT-AP 3 |
| **Data** | `GET /v2/indicators/{id}/series?geo=LK11,LK12&from=&to=&dims=sex:F&basis=&format=` | ≤ 25 geographies; `dims` accepts `key:code` pairs, omitted dimension = `_T` |
| | `GET /v2/indicators/{id}/map?level=3&parent=LK1&period=latest&dims=` | columnar `{codes[], values[], lo[], hi[], basis[], status[], breaks[]}`; no geometry |
| | `GET /v2/data?indicators=a,b&geo=&level=&parent=&from=&to=&dims=&cursor=&format=json\|csv\|sdmx-csv\|parquet` | general tabular slice, cursor-paged (≤ 10,000 rows per page); for larger pulls the response's `next` points at the bulk file |
| | `GET /v2/observations/{obs_id}` · `/provenance?depth=` · `/history` | one value; where it came from; every value that key ever had |
| | `GET /v2/live/{indicator}/series?geo=&from=&to=&step=` | `obs_hf`, optional downsampling (`step=PT1H`, mean/max) |
| **Places** | `GET /v2/areas/{code}` | unit, hierarchy, children, area, bbox, xrefs, values grouped `official / third_party / derived / estimated` from `serve.area_latest` |
| | `GET /v2/areas/{code}/geometry?simplified=1` · `/children?level=` · `/neighbours` · `/history` | geometry (GeoJSON, 6 dp); vintage history + successors |
| | `GET /v2/areas?scheme=&level=&parent=&q=` · `GET /v2/schemes` · `GET /v2/crosswalks?from_set=&to_set=&code=` | |
| | `GET /v2/reverse?lat=&lon=&schemes=admin,ed,la,moh` | deepest unit per scheme + chain + cell; `match: contains\|nearest` (kept) |
| | `GET /v2/lookup?q=&suggest=1` | universal lookup (v1 semantics: coordinate / postal / p-code / fuzzy), over `serve.search` |
| | `GET /v2/compare?geo=LK11,LK12,LK13&indicators=` | side-by-side with ranks within level |
| **Grid** | `GET /v2/point?lat=&lon=&layers=` | values of grid layers at a point + units |
| | `GET /v2/radius?lat=&lon=&km=&layer=` | sums/means within a radius (≤ 50 km) |
| | `GET /v2/grids` · `/v2/grids/{id}` · `/v2/grids/{id}/tiles/{z}/{x}/{y}.png?period=` | layer index, PNG tiles |
| **Features** | `GET /v2/features/{dataset}?bbox=&within={code}&kind=&q=&cursor=` · `/v2/features/{dataset}/{key}` | GeoJSON FeatureCollection in `payload`; also via OGC |
| | `GET /v2/nearest?lat=&lon=&dataset=&kind=&limit=` | KNN, ≤ 50 |
| | `GET /v2/tiles` · `/v2/tiles/{tileset}.json` (TileJSON 3) · `/v2/tiles/{tileset}/{z}/{x}/{y}.mvt` | |
| **Releases** | `GET /v2/releases` · `/v2/releases/{data_version}` · `/v2/releases/{data_version}/changelog` | manifest and changelog |
| **Ops** | `GET /v2/health` · `/v2/status` · `/v2/status/sources` · `/v2/status/datasets` · `/v2/status/runs` · `/v2/status/events` | [07 §4](07-operations.md#4-health-and-status-apis) |
| **Meta** | `GET /v2/openapi.json` · `/v2/docs` · `/llms.txt` · `/v2/conformance` | |

### 2.2 Conventions

- **Versioned URLs.** Every data endpoint exists under `/v2/r/{data_version}/…` with identical semantics pinned to that release (`immutable` caching). Unversioned paths mean "current release" and include `Link: <…/v2/r/20261204.1/…>; rel="permalink"` so a client or a citation can pin what it saw. `?as_of=YYYY-MM-DD` resolves to the last release on or before that date and redirects (302) to the pinned URL.
- **Formats.** `?format=` or `Accept`: `json` (default), `csv` (RFC 4180, header row, logical column names of [02 §4](02-standard-model.md#4-the-observation) + `attribution` in a `# ` comment preamble), `sdmx-csv`, `geojson` (features/areas), `parquet` (`/v2/data` only), `ld+json` (dataset, catalogue, provenance).
- **Language.** `?lang=en|si|ta` (kept).
- **Pagination.** Opaque cursors only (kept).
- **Errors.** `{success:false, message, payload:{type, title, status, detail, hints}}`; `hints` carries repair information: `valid_values`, `did_you_mean`, `nearest_available_period`, `available_levels`. 400 validation, 404 `not_found` / `not_in_coverage`, 410 `withdrawn`, 429 rate-limited, 5xx masked.
- **Limits.** 25 geographies per series, 10,000 rows per page, 50 km radius, 2,000 features per page, 5 s statement timeout; exceeding a limit returns 400 with the bulk-download alternative in `hints`.
- **CORS.** `Access-Control-Allow-Origin: *`, GET/HEAD/OPTIONS only. There is no authenticated surface under `/v2` (analysis requests on public inputs are the one `POST`, [11 §11.1](11-analysis-engine.md#111-rest)); accounts, saved maps and private data live in the separate app plane `/app/v1` ([12 §2](12-enterprise-and-byod.md#2-two-planes), [ADR 0023](adr/0023-app-plane-optional-accounts.md)).
- **OpenAPI 3.1** generated by `utoipa` (kept); the generated document is diffed in CI against `spec/openapi/v2.json` so a breaking change cannot land unnoticed.

## 3. OGC API alignment

Mounted at `/v2/ogc` from the same handlers (`lanka-server/src/ogc.rs`); nothing is implemented twice.

| OGC resource | Ours |
|---|---|
| `/v2/ogc/` landing, `/conformance`, `/api` | static + OpenAPI 3.0 view of the OGC subset |
| `/collections` | one collection per `features` dataset, per boundary scheme × level (`admin-adm2`, …), plus `areas` |
| `/collections/{id}` | extent (bbox, temporal), CRS list (`CRS84`), links to tiles and to the dataset card |
| `/collections/{id}/items?bbox=&datetime=&limit=&{property}=&filter=&filter-lang=cql2-text` | GeoJSON; `next` links carry our cursor; `numberMatched` omitted (allowed) for large collections |
| `/collections/{id}/items/{featureId}` | `featureId` = feature key or unit code |
| `/collections/{id}/tiles`, `/tiles/WebMercatorQuad/{z}/{y}/{x}` | MVT from PMTiles (note OGC's row/col order) |
| `/collections/{id}/items?indicator=&period=` (extension) | unit collections can join one indicator's values as feature properties — a ready choropleth layer for QGIS |

Conformance classes declared: Features Core, GeoJSON, OpenAPI 3.0, Filter (partial: `cql2-text` basic), Tiles Core + Tileset + TileSets list. Not implemented: transactions, CRS other than CRS84, Records, Processes, Coverages (rasters are offered through STAC + COG instead).

QGIS is the acceptance client: "Add WFS / OGC API – Features layer" against `/v2/ogc` must list collections and draw `admin-adm3` with an indicator joined.

## 4. MCP server

Anonymous, read-only. Streamable HTTP at `/mcp` (same process as REST) and stdio via `lanka mcp` (PgBackend with `DATABASE_URL`, PackBackend with `--pack`). Crate `lanka-mcp` on the official Rust MCP SDK (`rmcp`) **[A: pin the SDK and protocol revision at WP-14]**. All tools are annotated `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: false`.

| Tool | Arguments | Returns |
|---|---|---|
| `search_catalog` | `query, theme?, level?, class?` | datasets + indicators with coverage, licence status, `next` |
| `describe_dataset` | `dataset_id` | card: limits first, then indicators, dims, levels, periods, freshness, preset calls |
| `describe_indicator` | `indicator_id` | definition, unit, dims/codes, coverage per level, related estimates |
| `resolve_place` | `query` or `lat, lon` | candidate units with codes, level, parent chain, confidence |
| `get_area_profile` | `geo, themes?, classes?` | grouped latest values with `basis`, period, source |
| `query_indicator` | `indicator_id, geo[] \| (level, parent), from?, to?, dims?` | rows (≤ 500) with `basis`, `status`, `lo/hi`; overflow → download URL |
| `compare_areas` | `geo[], indicator_ids[]` | table + ranks within level |
| `rank_areas` | `indicator_id, level, parent?, period?, order, limit` | top/bottom units |
| `analyze_point` | `lat, lon, radius_km?` | units, grid values, nearest facilities, exposure summaries |
| `find_features` | `dataset, within? \| near?, kind?, limit` | features with codes |
| `get_provenance` | `obs_id, depth?` | document locator or derivation tree |
| `get_method` | `method_id \| dataset_id` | method page text, limits, validation score |
| `list_releases` / `get_changelog` | `since?` / `data_version` | what changed |
| `get_download` | `dataset_id, format?` | URLs, sizes, checksums, licence |
| `travel_reach` (after WP-18) | `lat, lon, mode, minutes` | population and facilities reachable (precomputed surfaces only) |

Result rules: structured content plus a compact text rendering; every result carries `data_version`, `attribution[]`, `licences[]`, `next[]` (ready-to-send follow-up calls) and — when relevant — the estimate notice in the **text** block ([04 §8](04-enrichment.md#8-labelling)). Errors carry the same `hints` as REST. Resources: `lanka://dataset/{id}`, `lanka://area/{code}`, `lanka://method/{id}` (dataset cards and method pages as Markdown). Prompts: none in v1.

Limits: 60 calls/min per IP (nginx zone), 500 rows per result, 256 KB per result. No tool writes, fetches arbitrary URLs, or accepts SQL.

## 5. CLI

One binary, `lanka`. Operator commands need `DATABASE_URL`; user commands work online against any deployment (`--api https://…`, default the public one) or **offline** against a pack (`--pack <dir>` or `LANKA_PACK`, auto-detected at `~/.local/share/lanka/pack`).

```
# user (online or offline)
lanka pull [core|geocoder|dataset <id>…] [--release <v>]      # download + verify sha256 against the manifest
lanka search <text>                                           lanka dataset <id> [--versions]
lanka area <code>                                             lanka reverse <lat> <lon>
lanka series <indicator> --geo LK11,LK12 [--from --to --dims] lanka map <indicator> --level 3 [--parent]
lanka provenance <obs_id> [--depth n]                         lanka export <dataset> --format parquet|csv|geojson
lanka geocode-csv <in.csv> --lat <col> --lon <col> [--schemes admin,ed,la,moh] [--levels 1,2,3,4] [-o out.csv]
lanka join-csv <in.csv> --pcode <col> --indicators a,b [--period latest]     # append indicator columns by p-code
lanka mcp [--pack …]                                          lanka conformance <base-url>

# operator
lanka db migrate | registry validate|sync | connector test <source> | ingest run|backfill|restandardise|status
lanka extract propose | template propose | review list|open|approve|reject
lanka recipe check|run|explain | release cut|list|diff | export build | tiles build
lanka worker | serve | serve rebuild | rebuild --release <v> --dataset <id> --verify | maintenance [--apply] | takedown <dataset>
```

`geocode-csv` is fully offline with the `geocoder` pack: streaming CSV in/out, `cell_id` from the canonical formula → `gnd_idx` array → unit chain; appends `adm1_pcode … adm4_pcode`, names in the chosen language, `match` (`contains|nearest|outside`), and optional scheme columns. Throughput target ≥ 1 M rows/s on a laptop core (it is two array lookups per row) **[A]**. Rows whose cell straddles a boundary are resolved with the polygon set shipped in the pack (the ~4 % of land cells crossed by a GN boundary **[A]** are flagged in a bitmap; only those take the slow path), so offline results equal `/v2/reverse` exactly — this equality is a corpus test.

## 6. Bulk downloads

Served by nginx from the store under `/dl/`. Everything has a sha256 in the release manifest.

```
/dl/releases/<data_version>/
    manifest.json            machine index: datasets, dsvs, files, bytes, sha256, licence, redistribution tier
    CHANGELOG.md             human changelog
    catalog.jsonld           DCAT-AP 3
    datapackage.json         Frictionless package of packages (resources point at /dl/datasets/…)
    stac/catalog.json …      STAC for grid datasets
    packs/lanka-core.pack.zst         offline pack: SQLite (catalogue, units, geoms simplified, bulk-tier observations at ADM0–ADM3,
                                      serve.* equivalents) — target ≤ 400 MB compressed [A]
    packs/lanka-geocoder.pack.zst     gnd_idx + postal_idx + land mask + units + boundary-cell polygons — target ≤ 40 MB [A]
    packs/lanka-full.pack.zst         core + ADM4 observations + feature tables (bulk tier only)
/dl/releases/current -> 302 to the latest
/dl/datasets/<dataset_id>/<dsv>/
    data.parquet             observations, logical columns, zstd, row groups sorted by (indicator, geo, period)
    data.csv.gz              same content
    data.sdmx.csv.gz         SDMX-CSV 2.0
    features.parquet         GeoParquet 1.1 (feature/boundary datasets)   features.geojson.gz   features.fgb (FlatGeobuf)
    <layer>_<period>.tif     COG (grid datasets)
    codelists/*.csv          the codelists the data references
    provenance.parquet       obs_id → artifact sha256, URL, locator, verbatim, method, review (extracted datasets)
    prov.jsonld              PROV-O for the version (derivation tree for derived datasets)
    datapackage.json  README.md (title, description, method, limits, licence, attribution, how to cite)  CITATION.cff  LICENSE.txt
/dl/datasets/<dataset_id>/latest -> 302 to the current dsv
/dl/tiles/<tileset>/<sha16>.pmtiles
```

Rules:

- **Redistribution tier decides presence** ([02 §8](02-standard-model.md#8-licences-attribution-redistribution)): `bulk` datasets are in packs and have every file; `api` datasets have per-dataset files only, with the licence notice at the top of `README.md` and no raw artifact copies; `link` and `internal` have no files.
- The legacy v1 artifact `lanka.sqlite` keeps being produced (by `lanka-export`, from v2 data) until `/v1` is retired, so v1 consumers and the npm packages do not break.
- Parquet is written with the Rust `parquet`/`arrow` crates; GeoParquet metadata by hand (small); FlatGeobuf and COG through GDAL in the sandbox.
- Exports are built per dsv, once. A release that changes three datasets writes three directories and one manifest.

## 7. Embeds and discoverability

The web app is a separate track; the data layer provides what it needs:

- Composed maps are embedded from their resolved map document ([10 §6](10-map-composition.md#6-permalinks-embeds-forks-export)); the `type=map` spec below remains for the single-indicator case.
- `GET /v2/embed/spec?type=map|series|profile&…` returns a self-contained JSON spec (the pinned data URLs, labels, breaks, attribution) that the web track's `/embed/…` route renders. nginx sends `Content-Security-Policy: frame-ancestors *` for `/embed/` only.
- oEmbed: `GET /v2/oembed?url=` for dataset, indicator and area URLs (type `rich`, iframe to `/embed/…`).
- Every embed shows attribution and, when applicable, the estimate notice; both come from the spec, so an embed cannot drop them.
- `/llms.txt` and `/v2/docs` describe the API for agents and people; dataset and indicator pages carry schema.org JSON-LD generated by the API (`Accept: application/ld+json`), so the SPA can inline it without re-deriving anything.
- `sitemap.xml` data (datasets, indicators, areas ADM0–ADM3) is produced by `lanka-export` at each release as `/dl/releases/<v>/sitemap-data.json` for the web build.

## 8. `/v1` compatibility

`/v1` stays on Hono + `lanka.sqlite` until WP-20, which re-implements the v1 routes in `lanka-server` on `lanka-query`, validated against a recorded response corpus (every documented v1 endpoint × a fixed parameter set, byte-compared after key sorting). Once the corpus passes, nginx routes `/v1` to the Rust server and the Hono container is removed. `/v1` then keeps being served by the Rust server, with `Deprecation` and `Sunset` headers (RFC 9745 / RFC 8594), for at least 12 months after `/v2` is declared stable ([08 §5](08-openness-and-governance.md#5-versioning-and-deprecation)).
