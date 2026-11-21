# 01 — Data lifecycle layers

Six layers. Each has one owner (the code that may write it), one location, and an immutability rule. A layer is only ever written from the layer directly below it, so any release can be rebuilt from L0 plus a code revision.

```
L0 raw ──► L1 extract ──► L2 standardised ──► L3 enriched ──► L4 release ──► L5 serving
bytes      cells/rows      observations         estimates       pinned map     materialised
           + locators      features, grids      composites      + files        for speed
```

| Layer | What | Where | Written by | Immutable? | Public? |
|---|---|---|---|---|---|
| L0 raw archive | Every upstream file or API response, byte for byte | `store/raw/<aa>/<bb>/<sha256>` + `ops.artifacts` | pipeline `archive` stage | Yes, write-once | Only for `open`/`attribution` licences; otherwise only the sha256 and URL |
| L1 extract | What a parser read out of an artifact: cell tables, record tables, feature tables, each with locators and verbatim text | `store/extract/<aa>/<bb>/<sha256>.parquet` + `ops.extracts` | `Connector::extract` (in the sandbox for documents) | Yes, content-addressed | No (review material) |
| L2 standardised | Extracts mapped to the standard model: observations, units, features, grid layers, codelists | Postgres `data.*`, `geo.*`; `store/grids/` | pipeline `standardise → validate → stage` | Versions are immutable; rows are range-versioned | Via L4 only |
| L3 enriched | Outputs of recipes over pinned L2/L3 versions | Same tables as L2, `catalog.datasets.kind = 'derived'`; lineage in `data.derivations` | `lanka-enrich` recipe runner | Same as L2 | Via L4 only |
| L4 release | `data_version` → {dataset → dsv}, plus the materialised distributions | `data.releases`, `data.release_datasets`; `store/datasets/<id>/<dsv>/`, `store/releases/<data_version>/`, `store/tiles/` | `release` and `export` jobs | Yes | Yes (by redistribution tier) |
| L5 serving | Read-optimised projections of the current release | Postgres `serve.*`; mmap grid files; in-process catalogue snapshot; nginx cache | `release` job (incremental) and `lanka serve` | Disposable; rebuilt from L4 | Yes |

## Object store decision: filesystem now, Garage when a threshold is crossed

**Decision.** The blob store is a directory on the VPS NVMe (`/srv/lanka/store`, env `LANKA_STORE_DIR`), written through one trait. nginx serves the public subtrees directly.

```rust
// engine/crates/lanka-store/src/lib.rs
pub enum Ns { Raw, Extract, Grid, Tile, Dataset, Release, Evidence, Tmp }

pub struct BlobRef { pub ns: Ns, pub key: String, pub sha256: [u8; 32], pub bytes: u64 }

#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Content-addressed write (Raw, Extract). Idempotent; returns created = false when present.
    async fn put_cas(&self, ns: Ns, bytes: Bytes) -> Result<(BlobRef, bool)>;
    /// Named immutable write (Dataset, Release, Grid, Tile). Fails if the key exists with other content.
    async fn put_named(&self, ns: Ns, key: &str, src: &Path) -> Result<BlobRef>;
    async fn get(&self, r: &BlobRef) -> Result<Bytes>;                 // re-verifies sha256
    async fn reader(&self, ns: Ns, key: &str) -> Result<BoxAsyncRead>;
    async fn exists(&self, ns: Ns, key: &str) -> Result<bool>;
    async fn delete(&self, ns: Ns, key: &str) -> Result<()>;           // retention + takedown only
    /// A real path when the backend is local; grids need it for mmap.
    fn local_path(&self, ns: Ns, key: &str) -> Option<PathBuf>;
}
pub struct FsStore { root: PathBuf }      // now
pub struct S3Store { /* endpoint, bucket-per-namespace */ }   // WP-23, Garage
```

Why the filesystem: one host, one disk, no network hop, `sendfile` for downloads, `mmap` for grids, restic for backup, zero processes to run. An S3 daemon on the same disk adds a failure mode and buys nothing until there is a second machine.

Why Garage when the time comes: it is built for small self-hosted clusters (1–3 nodes, heterogeneous disks), one static binary, low memory, S3 API sufficient for our use (PUT/GET/HEAD/range/multipart/static-website). MinIO is rejected: the community edition has been reduced to a maintenance-mode, source-only distribution and its operational model targets larger erasure-coded clusters **[A: re-check MinIO's licence/packaging status at WP-23]**. SeaweedFS is rejected: master + volume + filer is three roles for a problem we do not have.

Switch triggers (any one): a second box needs to read or write blobs; store > 100 GB; downloads > 30 % of nginx bandwidth for a month. With `S3Store`, grids are synced to a local cache directory at release time because mmap needs a file.

### Store layout

```
/srv/lanka/store/
  raw/ab/cd/<sha256>                              L0   (existing layout, kept)
  extract/ab/cd/<sha256>.parquet                  L1
  grids/<layer_id>/<period>/<sha16>.bin           L2/L3  little-endian array, canonical grid
  grids/<layer_id>/<period>/<sha16>.tif           COG of the same layer (download + STAC asset)
  tiles/<tileset>/<sha16>.pmtiles                 L4
  datasets/<dataset_id>/<dsv>/                    L4  immutable distributions of one dataset version
      data.parquet  data.csv.gz  features.parquet (GeoParquet)  datapackage.json  README.md  CITATION.cff
  releases/<data_version>/                        L4
      manifest.json  CHANGELOG.md  catalog.jsonld  datapackage.json  stac/catalog.json …
      packs/lanka-core.pack.zst  packs/lanka-geocoder.pack.zst
  releases/current -> <data_version>              symlink, flipped atomically by the release job
  evidence/<dsv>/index.html + crops/*.png         review evidence sheets (private)
  tmp/                                            same filesystem → rename is atomic
```

`store/datasets/<id>/<dsv>/` is keyed by dsv, not by release: a dataset that did not change between releases is not re-exported, and a release manifest is a list of pointers. This is what makes a daily release cheap.

## L0 — raw archive

- Unchanged from the foundation: sha256-addressed, temp + fsync + rename, deduplicated, `ops.artifacts` row per fetch (URL, fetched_at, ETag, Last-Modified, edition, media type).
- Add: `ops.artifacts.http_status`, `request_headers_sha256` (for API responses that depend on parameters), `wayback_url` (optional, filled by a low-priority job that asks the Internet Archive to save public documents — durable public evidence for `permission_unclear` sources we cannot redistribute ourselves).
- API responses are archived too (they are the evidence for class A values). Paginated responses are archived per page.
- Retention: forever for documents and one-off files. For high-churn API/raster inputs, see the table below.
- Takedown: the only reason a raw object is ever deleted besides retention ([08 §6](08-openness-and-governance.md#6-takedown)).

## L1 — extracts (new layer)

An extract is the parser's reading of one or more artifacts, before any mapping to indicators or p-codes. It exists because:

1. **Review needs it.** An evidence sheet shows printed token, page crop and locator per cell; that is extract data, not observation data.
2. **Re-standardising must not re-parse.** A crosswalk fix or an indicator rename re-runs `standardise` over stored extracts in seconds, without touching poppler or the network.
3. **Class C is not reproducible from raw + code.** An LLM reading is not deterministic. The reviewed extract *is* the durable record; reproducibility for class C means "raw + reviewed extract + code", and the extract carries the model id and prompt hash.

Format: Parquet (zstd), one file per extract, one of three shapes.

| Shape | Columns | Used by |
|---|---|---|
| `cells` | `table_id, r, c, text, page, bbox[4], sheet, a1, row_header, col_header, artifact_idx, conf, reader` | PDF, XLSX, HTML tables |
| `records` | `record_idx, path, fields (map<string,string>), artifact_idx` | JSON/CSV/API sources |
| `features` | `source_key, kind, tags (map), wkb, artifact_idx` | OSM, GeoJSON, shapefile, GTFS |

Raster sources have no L1 file: the warp to the canonical grid is deterministic, and the L2 grid array is the first derived object.

```sql
CREATE TABLE ops.extracts (
  id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  sha256          bytea NOT NULL UNIQUE,
  source_id       text  NOT NULL REFERENCES catalog.sources (id),
  shape           text  NOT NULL CHECK (shape IN ('cells','records','features')),
  extractor       text  NOT NULL,          -- 'pdf_table', 'xlsx_table', 'llm_dual', 'osm_pbf', …
  extractor_ver   text  NOT NULL,          -- semver of the driver/connector
  template_sha256 bytea,                   -- registry/templates/*.yaml content hash, if any
  model           text,                    -- class C only: provider/model id
  prompt_sha256   bytea,                   -- class C only
  rows            bigint NOT NULL,
  created_at      timestamptz NOT NULL DEFAULT now(),
  run_id          bigint REFERENCES ops.ingestion_runs (id)
);
CREATE TABLE ops.extract_artifacts (extract_id bigint, artifact_id bigint, idx int, PRIMARY KEY (extract_id, idx));
```

A deterministic extractor produces the same extract hash for the same artifact and extractor version, so re-runs are no-ops at this layer as well.

## L2 — standardised

The standard model ([02](02-standard-model.md)) in Postgres. One dataset version per (dataset, content). Rows carry `vfrom`/`vto` (dataset-local version sequence numbers): a row unchanged across versions is stored once ([05 §2](05-storage-and-performance.md#2-observations)).

Dataset kinds (`catalog.datasets.kind`): `indicators` (observations), `features` (points/lines/polygons), `boundaries` (units + geometry), `grid` (arrays), `crosswalk`, `codelist`, `derived` is **not** a kind — derivation is a property (`catalog.datasets.recipe_id IS NOT NULL`) because a derived dataset is still one of the kinds above.

## L3 — enriched

Same physical tables as L2. What makes a version L3 is a `data.derivations` row naming the recipe, method version, parameters and the exact input versions. L3 versions may be inputs to other recipes (the DAG in [04](04-enrichment.md)).

## L4 — releases

`data_version = YYYYMMDD.N` (format unchanged from v1). A release is immutable once cut. It consists of:

- rows in `data.releases`, `data.release_datasets`, `data.changelog`;
- for every dataset whose dsv is new: distributions under `store/datasets/<id>/<dsv>/`;
- `store/releases/<data_version>/manifest.json` — the list of `{dataset, dsv, kind, licence, redistribution, files[{path, bytes, sha256, media_type}]}` plus engine version, git revision, spec version, country profile hash;
- regenerated tilesets for changed tile sources.

Cut by the release train ([03 §8](03-ingestion.md#8-release-train)), not by individual publishes.

## L5 — serving

Rebuildable projections; never a source of truth.

| Object | Purpose | Rebuild |
|---|---|---|
| `serve.area_latest` | latest value of every indicator for every unit (one index scan per area profile instead of ~260 partition probes) | delete + insert for changed datasets at release |
| `serve.indicator_coverage` | per indicator × level: periods, min/max, count, last period | same |
| `serve.search` | trilingual trigram + tsvector index over units, places, datasets, indicators | same |
| mmap grid files | O(1) point reads, radius sums | `lanka serve` re-maps on `NOTIFY` |
| catalogue snapshot | all catalogue rows + current version map in RAM | hot-swapped on `NOTIFY`, 5-minute safety refresh |
| nginx `proxy_cache` | response cache | expires by TTL; versioned URLs are immutable |

## Immutability rules

| Object | Rule | Enforced by |
|---|---|---|
| Raw blob, extract blob | never rewritten | content addressing; `put_cas` |
| Dataset version content | never changes after `staged` | dsv = hash of content; `UPDATE` on `data.observations` allowed only for `vto` (trigger on non-`vto` columns raises) |
| Published dataset version | may only move to `superseded` or `withdrawn` | status transition check in `publish.rs` + CHECK trigger |
| Release | never modified; a correction is a new release | no UPDATE/DELETE grants on `data.releases*` for `lanka_ingest`; `ON DELETE` restricted |
| Distribution files | never rewritten | `put_named` refuses to overwrite different content |
| Registry history | git | — |

Withdrawal (takedown or a discovered error) creates a new release without the version; the old release keeps existing in the database but its files for that dataset are replaced by a `WITHDRAWN.txt` tombstone and the API answers `410 Gone` for versioned URLs of that dataset ([08 §6](08-openness-and-governance.md#6-takedown)).

## Retention

| Object | Kept | Then |
|---|---|---|
| Raw documents (PDF, XLSX, one-off zips) | forever | — |
| Raw API responses, HTML pages | every response that produced a published version, forever; others 90 days | delete blob, keep `ops.artifacts` row with `purged_at` |
| Raw rasters and OSM PBF (large, re-downloadable, upstream-versioned) | latest 2 per source + any that produced a release in the last 12 months | delete blob, keep row + sha256 + upstream version URL |
| Extracts | all that back a published or in-review version; unreviewed rejects 180 days | delete |
| Observation history (closed rows) | all, while the dataset partition is < 20 M rows; otherwise closed rows older than 24 months are dropped after their version has a Parquet distribution in `store/datasets/` | history stays reproducible from the stored distribution |
| `data.obs_hf` (sub-daily) | 24 months in Postgres | monthly Parquet in `store/datasets/<id>/hf/YYYY-MM.parquet` |
| Distributions `store/datasets/<id>/<dsv>/` | every dsv referenced by a release in the last 24 months, plus the first dsv of each calendar year forever | delete files; manifest keeps the hashes |
| Releases | manifests forever; `packs/` for the last 6 releases and every year-end release | delete packs |
| `ops.run_events` | 180 days (`debug` 14 days) | delete |
| nginx cache | 2 GB cap | LRU |

Retention runs as a weekly `maintenance` job that prints what it would delete (`--dry-run` is the default from the CLI).

## Reproducibility

**Guarantee.** For any `data_version` R and dataset D pinned at dsv X: checking out the git revision recorded in R's manifest and running

```
lanka rebuild --release R --dataset D --verify
```

reproduces a version whose dsv equals X, using only L0 blobs (and, for class B/C datasets, the reviewed L1 extract recorded for that version). `--verify` fails on any mismatch.

What makes it hold:

1. **Pinned inputs.** A version records its artifacts (`data.dataset_version_artifacts`), its extract(s) (`data.dataset_version_extracts`), and — for derived versions — `data.derivations.inputs = [{dataset, dsv}]`.
2. **Pinned code.** `engine_version` + `git_rev` on every run and derivation; drivers, templates, recipes and the country profile are hashed into the run record.
3. **Determinism rules** for extract/standardise/recipe code: no network, no wall clock (the run's `as_of` is a parameter), no unordered iteration into output (sort before hashing), seeded RNG only (seed = first 8 bytes of the input fingerprint), fixed float formatting in the canonical row stream (shortest round-trip, `ryu`), and GDAL/tippecanoe/Valhalla versions pinned in the tools image and recorded.
4. **Tool versions are inputs.** A recipe that depends on `gdalwarp` or Valhalla includes the tool version in its fingerprint; upgrading the tools image marks those recipes stale and the next train recomputes them.
5. **A nightly reproducibility check** (`check` job) picks three published versions at random and rebuilds them into a scratch schema; a mismatch opens an `ops.incidents` row of kind `repro_mismatch` (visible in `/v2/status`).

Limits, stated honestly: a version whose raw rasters were purged under the retention table is reproducible only after re-downloading the upstream file and matching its recorded sha256; if upstream has replaced it, the version is marked `reproducible = false` in the manifest. Class C reproducibility is from the reviewed extract, not from the PDF.
