# 03 — Ingestion

## 1. Pipeline

```
discover ─► fetch ─► archive ─► extract ─► standardise ─► validate ─► stage ─► gate ─► review? ─► approved
  │           │         L0         L1            │            │         L2       │
  └ candidate └ conditional        (sandbox for  └ codes,     └ schema  COPY     └ class A + clean diff → approved
    set hash    GET, retries        documents)     units,       expectations      otherwise → in_review
                                                   periods      cross-checks
```

Changes against the implemented pipeline (`engine/crates/lanka-ingest/src/pipeline.rs`):

| Stage | Today | Target |
|---|---|---|
| discover | list candidates | + returns a **candidate-set fingerprint**; if it equals the last successful run's, the run ends `no_change` before fetching anything |
| fetch | 6 concurrent, 3 attempts | + conditional GET (`If-None-Match`/`If-Modified-Since` from the last artifact), per-host concurrency 2 and 1 req/s default, `Retry-After` honoured, streamed to disk (no whole-body `Bytes` for files > 64 MB) |
| archive | sha256 → disk | unchanged, via `BlobStore` |
| parse | bytes → rows | split into **extract** (bytes → L1 extract, persisted, sandboxed for documents) and **standardise** (extract → typed outputs, pure) |
| validate | built-in checks | + declarative expectations + cross-source checks (§5) |
| stage | COPY into the partition | COPY into `stage.*` (unlogged), set-based diff, then range-versioned insert ([05 §2](05-storage-and-performance.md#2-observations)) |
| gate | ≤ 1 % changed/removed | per-dataset thresholds from the registry, defaulting to the same 1 % |
| publish | creates a release | marks the version `approved`; the **release train** publishes (§8) |

Run statuses: `running, succeeded, no_change, needs_review, failed` (unchanged) plus `skipped` (source `status: blocked` or paused).

## 2. Connector SDK

Two ways to add a source. Both produce the same `Output`s and go through the same pipeline.

### 2.1 The traits (`engine/crates/lanka-ingest/src/connector.rs`, rewritten)

```rust
/// What to fetch. Unchanged fields from today, plus conditional-GET hints.
pub struct Candidate { pub key: String, pub url: String, pub local_path: Option<PathBuf>,
                       pub media_type: Option<String>, pub edition: Option<String>,
                       pub method: HttpMethod, pub body: Option<Bytes>, pub headers: Vec<(String, String)> }

pub struct Discovery { pub candidates: Vec<Candidate>,
                       /// Hash of everything that would change if upstream changed (URLs, lastupdated
                       /// stamps, resource ids). None = always fetch.
                       pub fingerprint: Option<[u8; 32]> }

/// An archived artifact handed to extract(): a path, not bytes — large files are never held in RAM.
pub struct Fetched { pub key: String, pub url: String, pub artifact_id: i64, pub sha256: [u8; 32],
                     pub path: PathBuf, pub bytes: u64, pub media_type: Option<String> }

#[async_trait]
pub trait Connector: Send + Sync {
    fn source_id(&self) -> &str;
    fn version(&self) -> &str;                         // semver; part of every extract and run record
    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery>;          // network allowed
    /// Bytes → extract tables. No network, no database. Documents go through ctx.sandbox.
    async fn extract(&self, ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>>;
    /// Extract → standard-model outputs. Pure: reads only the extract, the registry and ctx.lookup.
    fn standardise(&self, ctx: &StdCtx<'_>, extracts: &[ExtractRef], sink: &mut dyn OutputSink) -> Result<()>;
}

pub enum ExtractOut { Cells(CellTable), Records(RecordTable), Features(FeatureTable), Raster(RasterRef) }

/// Streaming sink — a connector never builds a Vec of a million rows.
pub trait OutputSink {
    fn begin(&mut self, out: OutputHeader) -> Result<()>;      // dataset_id, kind, vintage, boundary_set
    fn obs(&mut self, row: ObsRow) -> Result<()>;
    fn unit(&mut self, row: UnitIn) -> Result<()>;
    fn feature(&mut self, row: FeatureIn) -> Result<()>;
    fn crosswalk(&mut self, row: CrosswalkRow) -> Result<()>;
    fn grid(&mut self, layer: GridLayerIn) -> Result<()>;      // path to a warped array produced via the sandbox
    fn hf(&mut self, row: HfRow) -> Result<()>;                // sub-daily
    fn end(&mut self) -> Result<()>;
}

pub struct ObsRow {
    pub indicator_id: String, pub geo: GeoRef, pub period: Period, pub dims: Dims,
    pub value: Option<f64>, pub lo: Option<f64>, pub hi: Option<f64>,
    pub basis: Basis, pub status: Status, pub flags: Flags,
    pub prov: CellProv,          // extract index + row in extract → locator + verbatim are read from L1
    pub method: ExtractionMethod, pub confidence: Option<f32>,
}
pub enum GeoRef { Code(String), Name { name: String, scheme: &'static str, level: i16, within: Option<String>, alias_set: &'static str } }
```

`GeoRef::Name` is how a connector says "the source printed a name"; the **pipeline**, not the connector, resolves it through `registry/crosswalks/` and `geo.crosswalks`, records the match, and fails or queues unmatched names. Connectors no longer do their own name matching (today `dcs_census.rs` does).

`StdCtx.lookup` is a read-only snapshot (units, codelists, indicators, alias tables) loaded once per run; `standardise` gets no `PgPool`.

### 2.2 Declarative drivers — the default path

A **driver** is a generic `Connector` implemented once in Rust and configured by `registry/connectors/<source_id>.yaml`. Target: ≥ 70 % of the 174 sources need no Rust.

| Driver | Covers | Sources (examples) |
|---|---|---|
| `http_json` | REST/JSON APIs with pagination; JSONPath-like selectors | World Bank, IMF, FAOSTAT, ILOSTAT, UN SDG, WHO GHO, NSO generation, Open-Meteo, OpenAQ |
| `sdmx` | SDMX-JSON/CSV dataflows | ILOSTAT, UN, IMF where available |
| `ckan` | CKAN `package_show` → resources | HDX (COD-AB, COD-PS, WFP prices) |
| `csv` | CSV/TSV in files or zips; long or wide | WFP, GDACS, OurAirports, GeoNames, UCDP |
| `xlsx_table` | workbooks with stable sheets; header detection by anchor text | CBSL tables, PUCSL, census XLSX, IMMR |
| `html_table` | tables on pages (static HTML) | DCS agriculture pages, BOI performance |
| `pdf_table` | text-layer PDFs with a table template (§3) | Statistical Abstract, Pocket Book, LFS, HIES, Treasury |
| `geo_file` | GeoJSON, shapefile, GeoPackage, KML → features or units | RDA GeoJSON, WDPA, HydroSHEDS |
| `osm_pbf` | Geofabrik extract + tag filters → features | all `osm-*` datasets (one fetch, many outputs) |
| `raster` | GeoTIFF/NetCDF/COG → canonical grid layer (resampling rule per layer) | WorldPop, WorldCover, CHIRPS, DEM, VIIRS, JRC flood |
| `gtfs` | GTFS feeds → stops, routes, frequencies | `lk-gtfs-feeds` |
| `git_files` | files in a git repository at a commit | nuuuwan repositories |

Example (`registry/connectors/cbsl-exchange-rates.yaml`):

```yaml
source: cbsl
driver: xlsx_table
version: 1.0.0
discover:
  kind: url_pattern                    # | listing_page | ckan | static | api_cursor
  url: https://www.cbsl.gov.lk/sites/default/files/cbslweb_documents/statistics/sheets/table4.11_{yyyymmdd}.xlsx
  probe: { latest: 14d, step: 1d }     # HEAD the last 14 dated URLs
  fingerprint: [url, etag, last_modified]
extract:
  tables:
    - id: usd_monthly
      sheet: { match: "Table 4.11" }
      header: { anchor: "Period", rows: 2 }
      rows: { from: after_header, until: { blank_rows: 3 } }
standardise:
  dataset: cbsl-exchange-rates
  outputs:
    - indicator: cbsl.fx_usd_avg
      table: usd_monthly
      geo: LK
      period: { column: "Period", parse: "%Y %B", freq: M }
      value: { column: "Average", number: { thousands: ",", missing: ["-", "n.a.", ""] } }
      basis: reported
expect:                                 # dataset-specific expectations, merged with the registry's
  - { check: range, indicator: cbsl.fx_usd_avg, min: 50, max: 1000 }
  - { check: max_step_change, indicator: cbsl.fx_usd_avg, pct: 40 }
```

Selectors are intentionally small (column by header text or index, anchor text, regex capture, number/date parsers, wide→long melt with a dimension mapping, constant fields, lookup through a codelist alias table). When a spec needs logic the selector language does not have, the source gets a **code connector** — the escape hatch is Rust, not a scripting language embedded in YAML.

`registry/schema/connector.schema.json` validates these files; `lanka connector test <source> --from <dir>` runs extract + standardise against fixtures and prints the first rows and every expectation.

### 2.3 Code connectors

Rust modules in `engine/crates/lanka-ingest/src/connectors/` for irregular sources: multi-table census workbooks with footnote logic (`dcs_census.rs`, kept), COD-AB hierarchy assembly (`cod_ab.rs`, kept), CBSL weekly indicators, Election Commission results, DMC situation reports. They implement the same trait and may call driver helpers (`xlsx::read_table`, `pdf::cells`).

The registry line `connector: driver | code` on each source replaces the hard-coded `connector_for` match; code connectors register through a `inventory`-style static table **[A: or a plain `match` in `connectors/mod.rs` if the `inventory` crate is unwanted — equivalent]**.

**No TypeScript connectors.** The v1 parsers are ported once; a second runtime in the ingest path doubles the sandbox, CI and dependency surface for no capability Rust lacks here (calamine for XLSX, poppler via the sandbox for PDF, `scraper` for HTML).

## 3. Documents: PDF and XLSX with locators

### 3.1 Extraction mechanics

| Input | Tool (in the sandbox, [07 §5](07-operations.md#5-security)) | Output |
|---|---|---|
| PDF with text layer | `pdftotext -bbox-layout` → words with page + bbox | `cells` extract via a template |
| PDF page image (for evidence crops and OCR) | `pdftoppm -r 150 -png` | PNG per page, cropped per cell for evidence sheets |
| Scanned PDF | `tesseract` (`eng+sin+tam`) → hOCR words with bbox | `cells` extract with `reader = ocr`, every row flagged `OCR`, class forced to C |
| XLSX/XLS | `calamine` in-process (pure Rust; size and cell-count limits) | `cells` extract with `sheet` + A1 reference + merged-cell resolution |
| HTML table | `scraper` in-process | `cells` extract with CSS selector locator |

### 3.2 Table templates

`registry/templates/<source>/<table>.yaml` tells `pdf_table` where a table is and how to read it. Templates are per table *layout*, with an `editions` list so layout drift is a new entry, not a new connector.

```yaml
id: dcs-abstract/ch2-table-2.4-population-by-district
editions:
  - match: { edition: "2024..2026" }
    locate:
      page: { find_text: "Table 2.4", then_text: "Mid-year population" }   # never a hard-coded page number alone
      span_pages: 2
    columns:                                   # x-ranges are found from header text, not pixels
      - { key: district, header: "District", type: label }
      - { key: total,    header: "Total",    type: number }
      - { key: male,     header: "Male",     type: number }
      - { key: female,   header: "Female",   type: number }
    rows: { start_after: "District", stop_at: ["Source:", "Note:"], label_column: district }
    number: { thousands: ",", decimal: ".", dash: nil, blank: missing, unit_mult: 3 }
    checks:
      - { sum: [male, female], equals: total, tol: 1 }
      - { sum_rows: all_but_last, equals_row: "Sri Lanka", tol: 2 }
```

Algorithm: locate page(s) by anchor text → cluster words into lines by y, into columns by the header words' x-extents → bind each numeric token to (row label, column key) → emit one `cells` row per token with `page`, `bbox`, `row_header`, `col_header`, `text` (verbatim). Template `checks` run on the extract itself, before any mapping, and fail the extract.

A template whose anchors are not found raises `template_miss` (run fails, incident opened) — the most common upstream change, and the one dc's pipeline could not see.

### 3.3 Class C — LLM-assisted, human-signed

Used only when no stable template is economical: one-off reports, scanned tables, project lists in prose (`lk-major-projects`, `boi-enterprises`, parts of `expressways`). Nine datasets today.

```
lanka extract propose <source> --artifact <sha> --table <hint>     # operator-run, never scheduled
   1. sandbox: text layer / OCR words with bboxes (same as class B)
   2. two independent model readings (different prompts; provider and model from config) → JSON cells
   3. anchoring: every proposed token must match a word run in step 1 on the same page
   4. confidence per cell: 1.0 both agree + anchored · 0.8 anchored single · 0.5 disagree · 0 unanchored (dropped)
   5. write a `cells` extract (reader = llm_dual, model id, prompt hash) + an evidence sheet
lanka review open <extract>      # per-cell sign-off UI for every cell < 1.0, plus a 10 % random sample of the rest
lanka review approve …           # only then can standardise run on it
```

Rules:

- **The LLM never produces a value that is not anchored to source text**, never fills gaps, never does arithmetic. It proposes *which printed token belongs to which row and column*.
- The reading is persisted as an L1 extract; pipelines re-run from the extract, so the model is never called twice for the same published number.
- LLM access is an **operator tool**: `lanka extract propose` runs on the operator's machine or on the VPS with `LANKA_LLM_ENDPOINT`/`LANKA_LLM_API_KEY` set; the scheduler and worker never have those variables. Provider is pluggable (any OpenAI-compatible or Anthropic-compatible endpoint, including a local model); a per-invocation spend cap is mandatory (`--max-usd`, default 5).
- Document text is untrusted input to the model: prompts instruct extraction only, the output schema is fixed JSON, and anything not matching the schema or the anchoring step is discarded — so a prompt-injection in a PDF can at worst produce cells that fail anchoring.
- The same tool may also **propose a class-B template** from a new layout (`lanka template propose`); the template is reviewed as a registry PR and from then on extraction is deterministic. This is the preferred use: LLM writes the template once, not the numbers every time.

## 4. Scheduling and change detection

Registry-driven. Nothing about scheduling is in code or in unit files.

```yaml
# registry/sources/<id>.yaml  (additions to the existing schema)
schedule:
  check_every: P1D           # how often discover runs (ISO 8601 duration); default derived from cadence
  window: "01:00-05:00"      # local time; optional politeness window
  release_calendar:          # optional, entered yearly
    - { edition: "LFS 2026 Q3", expected: 2026-12-15 }
  expected_lag_days: 75
  stale_after: P120D         # overdue threshold; default 2 × cadence
connector: driver            # | code
paused: false
```

Scheduler tick (inside `lanka worker`, every 60 s):

1. `SELECT` sources where `status = active AND NOT paused AND now() ≥ last_check + check_every` (or a calendar date has been reached, or a manual/backfill/retry job exists).
2. Enqueue `ops.jobs(kind='ingest', source_id, …)` unless one is queued/running (unique partial index).
3. Workers claim with `FOR UPDATE SKIP LOCKED`; concurrency 2 for ingest, 1 for heavy (`raster`, `osm_pbf`, recipes, exports) — enforced by a `weight` column summed against `LANKA_WORKER_SLOTS=4`.

Default `check_every` by cadence: realtime 15 min, daily 6 h, weekly 1 d, monthly 1 d, quarterly 3 d, annual 7 d, irregular 7 d, one_off never (manual).

Freshness states (extends `query::freshness_state`): `fresh`, `due`, `overdue`, `failing` (≥ 3 consecutive failures), `never_run`, `new_upstream` (discover saw an unpublished candidate), `in_review`, `expected_missing` (a calendar date passed by > 3 days with no new candidate), `paused`, `retired`.

**Change detection is three-layered**, cheapest first: (1) candidate-set fingerprint at discover; (2) artifact sha256 after fetch — identical bytes end the run; (3) content digest after standardise — different bytes, same values = `no_change` (already implemented).

## 5. Validation framework

Three tiers. All results are stored (`data.dataset_versions.validation` summary + `ops.check_results` rows) and shown on evidence sheets and `/v2/datasets/{id}/quality`.

### 5.1 Schema checks (always on, severity error)

Indicator belongs to the dataset; unit, dims and codes exist; period parses and matches the indicator's `freq`; geo code exists and is valid for the version's boundary set; value finite; key unique; `basis` allowed for the indicator class; estimated rows have `lo`/`hi` or a quality class; provenance present for extracted rows; `flags` use registered bits.

### 5.2 Expectations (declarative, per dataset)

`expect:` in `registry/datasets/<id>.yaml` or the connector spec. Each has `severity: error | warn` (default error for class A, since nobody reviews it).

| Check | Meaning |
|---|---|
| `row_count` `{min, max}` / `row_count_change` `{pct}` | size sanity vs previous version |
| `completeness` `{indicator, level, min_units}` | e.g. all 25 districts present |
| `range` `{indicator, min, max}` | |
| `sum_to_parent` `{indicator, tol_pct}` | children sum to parent (default tolerance 0.5 %) |
| `sum_dims` `{indicator, dim, tol}` | breakdown sums to the `_T` member |
| `ratio_consistency` `{indicator}` | ratio equals numerator / denominator within rounding |
| `monotonic` `{indicator, direction}` | cumulative series |
| `max_step_change` `{indicator, pct}` | period-to-period jump |
| `revision_limit` `{pct_changed, pct_removed}` | the class A gate (default 1 %/1 %) |
| `no_future` / `period_continuity` | no periods after today; no gaps in a regular series |
| `geometry` `{valid, within_bbox, nesting_tol, overlap_tol}` | features and boundaries |
| `sql` `{query, expect: empty}` | escape hatch; runs against `stage.*` with a read-only role and a 10 s timeout |

### 5.3 Cross-source consistency checks

`registry/checks/*.yaml`, run by the `check` job after every release train (they need published versions from several datasets). They never block a release; a failure sets `SOURCE_INCONSISTENT` on nothing by itself — it opens an `ops.incidents` row and appears on both datasets' quality pages. A maintainer then decides (fix a parser, add a limit note, or accept with a documented tolerance).

```yaml
id: pop-national-census-vs-wdi
left:  { indicator: census24.pop_total, geo: LK, period: "2024", dims: { sex: _T } }
right: { indicator: wdi.sp_pop_totl,    geo: LK, period: "2024" }
compare: { rel_diff_max: 0.03 }
note: Census is de facto on census night; WDI is a mid-year de jure estimate.
```

Initial set (WP-16): national population (census vs WDI vs WPP vs sum of WorldPop cells); district population (census vs mid-year estimates); GDP (DCS vs WDI vs IMF); CPI (DCS vs CBSL); paddy production (DCS vs FAOSTAT); facility counts (MoH list vs OSM vs healthsites); road length (RDA vs OSM by class); admin area (COD-AB vs Survey Department abstract figures).

## 6. Review workflow

Applies to class B (every new version), class C (every cell < 1.0 + sample), class A when the gate demotes it, and derived datasets on first release or method bump ([04](04-enrichment.md#publication-of-derived-data)).

```sql
CREATE TABLE ops.reviews (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  version_id  bigint NOT NULL REFERENCES data.dataset_versions (id),
  state       text NOT NULL CHECK (state IN ('open','approved','rejected','changes_requested')),
  opened_at   timestamptz NOT NULL DEFAULT now(),
  reviewer    text, decided_at timestamptz, note text,
  evidence_key text NOT NULL,             -- store/evidence/<dsv>/index.html
  checklist   jsonb NOT NULL              -- what the reviewer ticked
);
CREATE TABLE ops.review_cells (           -- class C per-cell sign-off; class B spot checks
  review_id bigint NOT NULL REFERENCES ops.reviews (id) ON DELETE CASCADE,
  extract_id bigint NOT NULL, row_idx int NOT NULL,
  decision text NOT NULL CHECK (decision IN ('accept','correct','reject')),
  corrected_text text, reviewer text NOT NULL, at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (review_id, extract_id, row_idx)
);
```

**Evidence sheet** (`lanka_ingest::evidence`, built by `lanka ingest run` and by worker jobs that leave a version in review; static HTML written to `store/evidence/<dsv>/`, opened with `lanka review open <dsv>` which serves it on `127.0.0.1` only):

- header: dataset, source edition, artifact sha256 + URL, extractor + template version, run id;
- validation report: every expectation with pass/fail and numbers;
- diff vs the current version: added / removed / changed keys, largest absolute and relative changes first;
- per table: the page image with cell boxes overlaid; per cell: printed token, parsed value, previous value, delta, flags; cells ordered by risk (new template edition, low confidence, large change, failed check neighbours);
- name-match table: every `GeoRef::Name` resolution with method and alternatives considered;
- a checklist the reviewer must tick (totals agree with the printed totals; units and multipliers read from the table header; footnotes read; edition is the one intended).

A correction (`decision = correct`) writes a **new extract** (reader `manual`, parent extract recorded) — the original reading is never edited. Approving records `reviewed_by/at` on the version and marks it `approved` for the next train. Reviewer identity is a handle from `LANKA_REVIEWERS` (allow-list in config); there are no accounts.

Budget **[A, from the parity audit]**: 0.1–0.25 person-days per class-B release, ~3–4 person-days a month at steady state. The ordering-by-risk and the template checks exist to keep it there.

## 7. Backfills, failures, idempotency

**Backfill** = running a source over historical editions. `lanka ingest backfill <source> --editions 2015..2025` enqueues one job per edition (oldest first) with `trigger = backfill`; each produces artifacts and extracts; the standardise step merges all editions' extracts into **one** dataset version (latest edition wins per key, older editions fill periods the newest dropped, every row keeps the artifact it actually came from). The vintage of the version is the newest edition; per-row provenance tells the truth.

**Re-standardise** (`lanka ingest restandardise <dataset>`) re-runs standardise over stored extracts after a registry/crosswalk/codelist change. No network.

**Failure handling**

| Failure | Behaviour |
|---|---|
| Network/5xx/timeouts | 3 attempts with backoff inside the run; then run `failed`; scheduler retries at 15 min, 1 h, 6 h, then at `check_every` |
| 404 on a pattern URL | `expected_missing` if within the lag window; `failed` + incident `url_pattern_broken` beyond it |
| Template miss / header not found | run `failed`, incident `template_miss`, extract kept for inspection |
| Expectation error | version `rejected` automatically for class A (kept 180 days for inspection), run `needs_review` for B/C with the failure on the evidence sheet |
| Unmatched geography name | class A: failed; B/C: `needs_review` with the unmatched list |
| Worker crash | job row's advisory lock is free ⇒ `running` job is provably abandoned ⇒ re-queued (max 3 attempts) — same mechanism the foundation uses for runs |
| Disk > 85 % | worker refuses to start `raster`/`osm_pbf`/`export` jobs; incident `disk_pressure` |

**Idempotency** — every stage is safe to repeat:

- archive: content-addressed;
- extract: keyed by (artifact set, extractor version, template hash) → same extract hash;
- standardise + stage: version keyed by dsv; staging an existing dsv is a no-op that returns the existing version;
- approve/release: guarded by status transitions and one transaction-level advisory lock for the release train;
- jobs: `(kind, dedupe_key)` unique while queued/running.

## 8. Release train

One `release` job (scheduled 20:30 UTC = 02:00 Asia/Colombo; `lanka release cut` for manual) — see [ADR 0012](adr/0012-daily-release-train.md).

```
1. lock (pg_advisory_xact_lock)
2. collect dataset versions in status 'approved' (not yet released)            → if none: exit no_change
3. recompute: for each recipe whose input fingerprint changed, in topological order,
   run it against (current release ∪ approved) → new derived versions; auto-approve if QA gates pass
   and the recipe is 'stable', else leave in_review and keep the previous derived version pinned
4. write data.releases / release_datasets / changelog; close superseded rows (vto); statuses → published/superseded
5. rebuild serve.* for changed datasets; ANALYZE changed partitions
6. COMMIT → NOTIFY lanka_release
7. enqueue export + tiles jobs for changed datasets; flip store/releases/current when they finish
8. enqueue check job (cross-source checks, reproducibility sample)
```

Steps 1–6 are one transaction for the pointer flip; recipe computation (step 3) happens before the lock is taken for long-running recipes and is only *selected* inside it. If a derived dataset cannot be recomputed (QA gate fails), the release still goes out with the previous derived version and the dataset card shows `stale_inputs: true` — official data is never held back by a failing model.

**Intraday cuts.** A dataset marked `live: true` in the registry (daily exchange rates, daily dengue or reservoir figures) triggers an extra train when it has a newly approved version, at most once per hour. A cut is a pointer flip plus exports for the changed datasets only, so this is cheap; the hourly cap bounds release-number churn. There is no "head" read path: every response is pinned to a cut release, which keeps one rule for caching and citation. Sub-daily data (`data.obs_hf`) is the single exception — it is not release-pinned and is labelled `live` in `meta`.
