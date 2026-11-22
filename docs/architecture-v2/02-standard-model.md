# 02 — The standard model

This document is the normative core. It will be extracted into `spec/` as **Lanka Data Layer Standard 1.0** (text CC BY 4.0, JSON Schemas alongside) so another deployment can conform to it without reading the engine ([08 §3](08-openness-and-governance.md#3-the-spec)). The words MUST/SHOULD are used in the RFC 2119 sense.

## 1. Identifiers

| Thing | Identifier | Example | Rules |
|---|---|---|---|
| Admin unit | p-code (OCHA COD-AB), bare | `LK`, `LK1`, `LK11`, `LK1103`, `LK1103005` | The only join key for admin data. Pattern comes from the country profile (`^LK[0-9]*$`). Never reused. |
| Other coded geography | `<scheme>:<code>` | `ed:01`, `pd:01A`, `la:LK11-MC-001`, `moh:LK11-014`, `police:WP-03`, `basin:kelani` | Scheme ids are registered in `geo.schemes`. The colon distinguishes them from p-codes. |
| Grid cell | `cell_id` (u32), profile grid | `7312045` | Formula of `docs/architecture.md §1`, unchanged. Wire form in URLs: `cell:7312045`. |
| Feature | `<dataset_id>/<source_key>` | `osm-pois-amenities/node/123456789` | Stable while the upstream key is stable. |
| Entity (conflated register) | `lke_` + 10 Crockford base32 chars | `lke_3f9k2a7q1x` | Minted once by a conflation recipe; persisted in `geo.entities` so it survives re-runs; carries `wikidata_qid` when matched. |
| Dataset | kebab-case slug | `dcs-census-2024-population` | Registry file name. Derived datasets MUST start with `derived-`. |
| Indicator | `<namespace>.<measure>` snake_case | `census24.pop_total`, `est.pop_total` | Globally unique. Namespace `est.` is reserved for estimated/forecast indicators ([04 §8](04-enrichment.md#8-labelling)). |
| Dimension / code | codelist id + code | `sex` / `f`, `age5` / `Y15T19` | Codelists in `registry/codelists/*.yaml`. |
| Dataset version | `dsv_` + 16 hex | `dsv_9c1e0b7a44d2f310` | Content hash ([05 §3](05-storage-and-performance.md#3-versions-and-the-dsv-hash)). |
| Release | `YYYYMMDD.N` | `20261204.1` | UTC date of the cut. |
| Observation | `obs_id` (i64) | `1125899906842733` | `dataset_key << 40 | local sequence`. Stable for as long as the row's content is unchanged across versions. |
| Artifact, extract | sha256 hex | — | Content address. |
| Method, recipe | `<id>@<semver>` | `areal_weighting@1.2.0` | [04](04-enrichment.md). |
| External | Wikidata QID, ISO 3166-2, GADM GID, GeoNames id, OSM id | `Q35482`, `LK-11` | Stored in `geo.unit_xrefs(unit_key, system, value)`; never used as join keys inside the platform. |
| Workspace dataset / indicator | `@<workspace>/<id>` | `@acme/prices-2026`, `@acme/re.asking_price_median` | Private or community data ([12 §4](12-enterprise-and-byod.md#4-the-workspace-data-plane)). The `@` cannot occur in a registry id, so these never shadow public identifiers. Not part of releases. |
| Map, map snapshot | `m_` + 12 Crockford base32 chars; `ms_` + 16 hex | `m_7k2q9x4w8n1c`, `ms_41c0a9e77b1d2f08` | [10 §5](10-map-composition.md#5-storage). A snapshot id is the hash of its canonical JSON. |
| Analysis result | `an_` + 16 hex | `an_5e1f0c22ab904d71` | Fingerprint of the resolved request ([11 §9](11-analysis-engine.md#9-caching-and-materialisation)). |

**Persistent URIs.** Every identified thing has an HTTP URI under the deployment's base (`site.base_url` in the profile; production `https://lanka-data-layer.prabhavalabs.com`):
`/id/area/{code}`, `/id/dataset/{id}`, `/id/indicator/{id}`, `/id/version/{dsv}`, `/id/release/{data_version}`, `/id/obs/{obs_id}`, `/id/feature/{dataset}/{key}`, `/id/entity/{lke}`, `/id/map/{id}`, `/id/analysis/{id}`. They 303-redirect to the web page for browsers and to the `/v2` JSON for `Accept: application/json` / `application/ld+json`. These URIs are what DCAT, PROV and schema.org output uses as `@id`.

**Surrogate keys.** Inside Postgres, observations reference `geo.units.key`, `catalog.indicators.key`, `catalog.dim_sets.key` and `catalog.datasets.key` (all `int4`). A surrogate is assigned once per code, never reassigned, never exposed, and resolved only from the code. This is a storage optimisation; the p-code rule is unchanged.

## 2. Geography

### 2.1 Units, schemes, vintages

```sql
CREATE TABLE geo.schemes (
  id          text PRIMARY KEY,          -- 'admin', 'ed', 'pd', 'la', 'moh', 'police', 'basin', 'station'
  title       text NOT NULL,
  prefix      text,                      -- NULL for 'admin' (bare p-codes)
  hierarchical boolean NOT NULL DEFAULT false,
  nests_in    text REFERENCES geo.schemes (id)   -- e.g. 'pd' nests in 'ed'; 'la' nests in admin level 2
);

CREATE TABLE geo.boundary_sets (            -- kept from 0001, extended
  id          text PRIMARY KEY,           -- 'cod-ab-lka-2022', 'dcs-2012', 'dcs-2001', 'ec-2023'
  scheme_id   text NOT NULL REFERENCES geo.schemes (id),
  title       text NOT NULL,
  valid_from  date, valid_to date,
  source_id   text REFERENCES catalog.sources (id),
  is_reference boolean NOT NULL DEFAULT false   -- exactly one per scheme (partial unique index)
);

CREATE TABLE geo.units (
  key          int4 GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  code         text NOT NULL UNIQUE,
  scheme_id    text NOT NULL REFERENCES geo.schemes (id),
  level        int2 NOT NULL,             -- admin: 0..4; other schemes: scheme-local level, 0-based
  parent_key   int4 REFERENCES geo.units (key),
  anc_keys     int4[] NOT NULL DEFAULT '{}',    -- ancestors root→parent (replaces adm1/2/3_pcode)
  name_en      text NOT NULL, name_si text, name_ta text,
  valid_from   date, valid_to date,       -- NULL valid_to = exists in the reference vintage
  successor    int4[],                    -- for retired codes: what replaced them (informative; weights live in crosswalks)
  wikidata_qid text,
  attrs        jsonb
);
CREATE INDEX units_parent ON geo.units (parent_key);
CREATE INDEX units_scheme_level ON geo.units (scheme_id, level);
CREATE INDEX units_anc ON geo.units USING gin (anc_keys);

CREATE TABLE geo.unit_geoms (
  unit_key     int4 NOT NULL REFERENCES geo.units (key),
  boundary_set text NOT NULL REFERENCES geo.boundary_sets (id),
  geom         geometry(MultiPolygon, 4326) NOT NULL,
  geom_simplified geometry(MultiPolygon, 4326),
  area_km2     double precision NOT NULL,       -- geodesic, ST_Area(geom::geography)
  centroid     geometry(Point, 4326) NOT NULL,  -- ST_PointOnSurface
  bbox         box2d NOT NULL,
  geom_sha256  bytea NOT NULL,
  artifact_id  bigint REFERENCES ops.artifacts (id),
  PRIMARY KEY (unit_key, boundary_set)
);
CREATE INDEX unit_geoms_gist ON geo.unit_geoms USING gist (geom);

-- Compatibility: the shape lanka-core already queries.
CREATE VIEW geo.admin_units AS
  SELECT u.code AS pcode, u.level, u.name_en, u.name_si, u.name_ta, p.code AS parent_pcode,
         g.boundary_set, g.area_km2, g.centroid, g.bbox, g.geom, g.geom_simplified
  FROM geo.units u
  JOIN geo.unit_geoms g ON g.unit_key = u.key
  JOIN geo.boundary_sets b ON b.id = g.boundary_set AND b.is_reference
  LEFT JOIN geo.units p ON p.key = u.parent_key
  WHERE u.scheme_id = 'admin';
```

Rules:

- **One reference vintage per scheme.** For `admin` it is the current COD-AB edition. A new COD-AB edition is a contract event: new boundary set, crosswalk from the old one, `is_reference` flipped in a release whose changelog says so, harmonised series recomputed by the train.
- **Official figures stay on the boundaries they were published for.** A dataset version declares `boundary_set`; its `geo_key`s point at units valid in that set (retired units stay in `geo.units` with `valid_to`). The API returns retired units with `"retired": true` and their successors.
- **Comparable-over-time series are derived data.** `derived-harmonised-boundary-series` re-bases old-vintage figures onto the reference set through `geo.crosswalks`, with `basis = estimated` wherever a weight < 1 was applied.
- **No name joins.** A source that publishes names is matched to codes once, at standardise time, through reviewed alias tables in `registry/crosswalks/` constrained to the parent unit; the match is written to `geo.crosswalks` (`method = name_exact | name_alias`), and an unmatched name fails the run for class A and lands in review for B/C. It is never dropped silently.

### 2.2 Crosswalks

```sql
CREATE TABLE geo.crosswalks (
  from_set    text NOT NULL,      -- boundary set id, or 'name:<source>' for name→code
  from_code   text NOT NULL,
  to_set      text NOT NULL,
  to_code     text NOT NULL,
  weight      double precision NOT NULL CHECK (weight > 0 AND weight <= 1),
  weight_kind text NOT NULL CHECK (weight_kind IN ('identity','area','population','buildings','manual')),
  method      text NOT NULL CHECK (method IN ('exact_code','name_exact','name_alias','area_overlap','pop_overlap','bldg_overlap','manual')),
  version_id  bigint REFERENCES data.dataset_versions (id),   -- crosswalks are versioned datasets (kind = 'crosswalk')
  PRIMARY KEY (from_set, from_code, to_set, to_code, weight_kind)
);
```

Invariant (validator): for every `(from_set, from_code, to_set, weight_kind)`, weights sum to 1 ± 1e-6 unless the source unit partly leaves coverage (flagged).

### 2.3 Geometry and CRS policy

- Storage and exchange CRS: **WGS 84, EPSG:4326**, coordinate order longitude, latitude in GeoJSON (RFC 7946) and OGC API (CRS84).
- Emitted coordinates: **≤ 6 decimal places** (v1 rule, ~0.11 m).
- Areas and lengths: geodesic (`geography` casts). Buffers, distances and overlays that need a planar metric use the profile's `metric_crs` — **EPSG:5235 (SLD99 / Sri Lanka Grid 1999)** **[A: confirm PROJ in the pinned PostGIS image carries the SLD99 transformation; fall back to EPSG:32644 UTM 44N if not]**.
- Validity: every stored geometry passes `ST_IsValid`; repair is `ST_MakeValid(geom, 'method=structure')` followed by polygon extraction, and the repair is counted in the run events.
- Topology: admin levels are checked for nesting (child area outside parent ≤ 0.1 %) and for gaps/overlaps between siblings (≤ 0.05 % of parent area) — warnings, with numbers stored in the validation report.
- Simplification: `geom_simplified` at 0.0005° with `ST_SimplifyPreserveTopology`; tiles are simplified by tippecanoe from full geometry.
- The canonical grid is defined in geographic coordinates; a cell's area varies with latitude (≈ 12,150–12,290 m² across the island **[A]**), so per-km² densities from the grid MUST use the per-row cell area, not a constant.

## 3. Measures, dimensions, attributes

The model is SDMX's, flattened: an observation is a value for one **indicator** (measure + concept) at one **geography**, one **period**, one combination of **dimension codes**, with **attributes**.

```sql
CREATE TABLE catalog.units_of_measure (
  code      text PRIMARY KEY,     -- UCUM case-sensitive code, or '{annotation}' for countables
  label_en  text NOT NULL, label_si text, label_ta text,
  kind      text NOT NULL CHECK (kind IN ('count','ratio','currency','physical','index','score'))
);
-- examples: '{person}', '{household}', '%', '1' (pure ratio), 'km2', 'mm', 'Cel', 'kW.h', 'min',
--           'LKR', 'USD' (ISO 4217, kind = currency), '{index}', '/km2', '{person}/km2', '/1000{person}'

CREATE TABLE catalog.codelists (id text PRIMARY KEY, title text NOT NULL, sdmx_ref text, spec_sha256 bytea NOT NULL);
CREATE TABLE catalog.codes (
  codelist_id text NOT NULL REFERENCES catalog.codelists (id),
  code        text NOT NULL,
  label_en text NOT NULL, label_si text, label_ta text,
  parent_code text, sort int NOT NULL DEFAULT 0,
  is_total    boolean NOT NULL DEFAULT false,      -- the '_T' member
  PRIMARY KEY (codelist_id, code)
);
CREATE TABLE catalog.dim_sets (              -- interned canonical dims strings
  key   int4 GENERATED ALWAYS AS IDENTITY PRIMARY KEY,   -- key 0 is reserved for '' (no dimensions)
  dims  text NOT NULL UNIQUE,                -- 'age5=Y15T19;sex=f' (keys sorted)
  pairs jsonb NOT NULL
);

ALTER TABLE catalog.indicators
  ADD COLUMN key            int4 GENERATED ALWAYS AS IDENTITY UNIQUE,
  ADD COLUMN label_si text, ADD COLUMN label_ta text,
  ADD COLUMN definition     text,
  ADD COLUMN unit_code      text REFERENCES catalog.units_of_measure (code),
  ADD COLUMN unit_mult      int2 NOT NULL DEFAULT 0,     -- power of ten (SDMX UNIT_MULT): 3 = thousands, 6 = millions
  ADD COLUMN decimals       int2,
  ADD COLUMN measure_type   text NOT NULL CHECK (measure_type IN
        ('count','sum','rate','ratio','share','mean','median','index','score','rank','category')),
  ADD COLUMN aggregation    text NOT NULL CHECK (aggregation IN
        ('sum','ratio_of_sums','weighted_mean','min','max','none')),
  ADD COLUMN numerator_id   text REFERENCES catalog.indicators (id),
  ADD COLUMN denominator_id text REFERENCES catalog.indicators (id),
  ADD COLUMN weight_id      text REFERENCES catalog.indicators (id),
  ADD COLUMN class          text NOT NULL CHECK (class IN ('official','third_party','derived','estimated')),
  ADD COLUMN ci_level       real,                        -- 0.95 when lo/hi are a confidence/credible interval
  ADD COLUMN direction      text CHECK (direction IN ('higher_better','lower_better','neutral')),
  ADD COLUMN sdg            text,                        -- '3.8.1'
  ADD COLUMN base_period    text;                        -- for indices: '2021=100'
```

Rules:

- **Unit** is mandatory and is a UCUM code (or ISO 4217 for money). Free-text units (`persons`, `percent`, `ratio` in today's registry) are mapped once by WP-01 (`persons → {person}`, `percent → %`, `ratio → 1`).
- **`aggregation` decides roll-up**, so roll-up is data, not code: `sum` adds children; `ratio_of_sums` recomputes from `numerator_id`/`denominator_id`; `weighted_mean` uses `weight_id`; `none` means the indicator is not rolled up (medians, indices, ranks) and a missing parent stays missing.
- **Dimensions** are codelist-backed. An indicator declares its dimensions as `[{id: sex, codelist: sex}, {id: age, codelist: age5}]`; every observed code must exist; every dimension has a total member (`_T`) and an observation omits a dimension only by using `_T` explicitly. Standard codelists reuse SDMX cross-domain codes where they exist (`CL_SEX`: `F`, `M`, `_T`; `CL_AGE` style `Y0T4`, `Y_GE65`). Country codelists (ethnicity, religion, sector `urban|rural|estate`, crop, season) are defined in `registry/codelists/`.
- **`class`** separates what the number is: `official` (a Sri Lankan public body published it), `third_party` (international organisation, research product, community data such as OSM/WorldPop), `derived` (deterministic arithmetic on the above: sums, ratios, densities, zonal counts), `estimated` (a model or allocation was needed).

## 4. The observation

Logical record (what the API, exports and spec show):

| Field | Type | Meaning |
|---|---|---|
| `indicator` | id | |
| `geo` | code | p-code or `scheme:code` |
| `geo_level` | int | level within the scheme |
| `period` | string | canonical period code (§5) |
| `dims` | object | `{ "sex": "F", "age": "Y15T19" }` |
| `value` | number \| null | in `unit × 10^unit_mult` |
| `lo`, `hi` | number \| null | interval bounds at the indicator's `ci_level`; or min/max across models when `flags` has `MODEL_SPREAD` |
| `basis` | enum | how the value came to exist |
| `status` | enum | its state |
| `flags` | string[] | quality and method flags |
| `obs_id` | int | handle for provenance |

**`basis`** (new; splits the foundation's overloaded `obs_status`):

| Code | Meaning | Allowed in indicator class |
|---|---|---|
| `reported` | Printed or served by the source for this geography, period and breakdown | official, third_party |
| `computed` | Deterministic arithmetic on reported values of the same vintage (sum of children, ratio, density, zonal count of observed features) | official (roll-ups only), derived |
| `estimated` | A model, allocation, crosswalk weight < 1, interpolation or spatial statistic of a modelled surface was needed | estimated, third_party (when the source itself says so) |
| `forecast` | Refers to a period after the source's reference date | estimated, third_party |

**`status`** (maps to SDMX `CL_OBS_STATUS`):

| Code | SDMX | Meaning |
|---|---|---|
| `normal` | A | |
| `provisional` | P | source marks it provisional |
| `revised` | — (kept as attribute) | differs from the previous version's value for the same key |
| `break` | B | series break at this period (method or boundary change) |
| `low_reliability` | U | source or our CV rule says unreliable |
| `missing` | M | source cell blank / "n.a." (value NULL) |
| `nil` | Z-like | source prints a dash meaning zero/not applicable (value NULL or 0 as printed) |
| `suppressed` | Q | withheld for confidentiality or because CV exceeds the publication rule (value NULL) |

Migration from the 0001 enum: `reported→(reported, normal)`, `revised→(reported, revised)`, `provisional→(reported, provisional)`, `estimate|modelled→(estimated, normal)`, `forecast→(forecast, normal)`, `derived→(computed, normal)`, `not_available→(reported, missing)`, `nil→(reported, nil)`, `suppressed→(reported, suppressed)`.

**`flags`** — a 32-bit mask, registered in the spec (bit numbers are part of the standard):

| Bit | Flag | Set when |
|---|---|---|
| 0 | `REBASED` | geography re-based through a crosswalk with any weight < 1 |
| 1 | `SPLIT_UNIT` | the target unit was split or merged between vintages |
| 2 | `DENOM_MISMATCH` | denominator year ≠ numerator year (difference stored in derivation params) |
| 3 | `SMALL_N` | fewer than the method's minimum support (e.g. < 10 grid cells, < 30 sample households) |
| 4 | `MODEL_SPREAD` | `lo`/`hi` are min/max across alternative models, not a statistical interval |
| 5 | `INTERPOLATED_TIME` | value lies between two observed periods |
| 6 | `EXTRAPOLATED_TIME` | value lies beyond the last observed period |
| 7 | `ANCILLARY_WEAK` | dasymetric ancillary fit below the method's threshold |
| 8 | `SOURCE_AMBIGUOUS` | printed table is ambiguous (footnote, merged cell); reviewer chose a reading |
| 9 | `SOURCE_INCONSISTENT` | fails a cross-source or additivity expectation at warning level |
| 10 | `OCR` | read by OCR (no text layer) |
| 11 | `LLM_READ` | class C reading |
| 12 | `PARTIAL_COVERAGE` | input coverage of the unit < 95 % |
| 13 | `TOP_CODED` | source top/bottom-codes the value |
| 14 | `UNOFFICIAL_GEOM` | geometry digitised or approximate (BOI zones, plans) |
| 15 | `SUPERSEDED_BOUNDARY` | geography is not in the reference vintage |
| 16 | `INTERPOLATED_SPACE` | value comes from spatial interpolation of point samples (IDW, kriging) — added 2026-11-22 for [11 §3](11-analysis-engine.md#3-alignment); propagating |
| 17 | `DISAGGREGATED` | value was allocated from a coarser unit (dasymetric allocation, or a parent value inherited by its children) — added 2026-11-22; propagating |
| 18–31 | reserved | |

**Uncertainty.** `lo`/`hi` are mandatory for `class = estimated` indicators unless the indicator declares `quality_class` instead (a dimension-like attribute `A|B|C|D` stored in bits 28–29 of `flags` — reserved for this). Validators reject an estimated dataset version that has neither.

## 5. Time

- **Period codes** (canonical, ISO 8601-derived, matching SDMX time formats):

| Frequency (`freq`) | Code | Example | `time_start` |
|---|---|---|---|
| `A` annual | `YYYY` | `2024` | Jan 1 |
| `F` split/fiscal/agricultural year | `YYYY/YY` | `2024/25` | profile-defined start month per calendar (`fiscal`, `agri`, `academic`); the calendar is an indicator attribute |
| `H` half-year | `YYYY-Hn` | `2024-H1` | |
| `S` season | `YYYY-<season>` | `2024-Maha`, `2024-Yala` | profile-defined (`Maha` starts Sep 1 of the named year, `Yala` Apr 1) **[A: confirm DCS season-year naming convention in WP-12]** |
| `Q` quarter | `YYYY-Qn` | `2024-Q2` | |
| `M` month | `YYYY-MM` | `2026-08` | |
| `W` ISO week | `YYYY-Www` | `2026-W47` | Monday |
| `D` day | `YYYY-MM-DD` | `2026-11-20` | |
| `P` point in time (census night, survey reference date, snapshot) | `YYYY-MM-DD` or `YYYY` | `2024-12-19` | that date; `P` distinguishes a stock at a date from a flow over a day |
| `T` sub-daily | `YYYY-MM-DDThh:mmZ` | `2026-11-20T06:15Z` | stored in `data.obs_hf` only |

  Stored as `(freq "char", time_start date)`; the text code is rendered and parsed by `lanka-core::period`. Multi-year survey periods (`2019-2021`) use `time_start` of the first year, freq `A`, and the attribute `period_span_years` on the indicator.
- **Timezone.** Dates are civil dates in the profile timezone (`Asia/Colombo`); `T` timestamps are UTC on the wire.
- **Three times, never conflated:**
  1. *reference period* — the `period` of the observation;
  2. *vintage* — the edition that stated it (`data.dataset_versions.vintage`, e.g. `Statistical Abstract 2025`, WDI `lastupdated=2026-10-28`);
  3. *knowledge time* — when we published it (`data.releases.created_at`).
- **Revisions.** A changed value for the same key in a later version closes the old row (`vto`) and opens a new one with `status = revised` (unless the source marks it provisional/final itself). `GET …/series?as_of=<data_version>` reads the range; `GET /v2/observations/{obs_id}/history` lists all values that key ever had with their vintages. No "latest wins" overwrite ever happens.
- **Series breaks** are data: `status = break` on the first period after the break and a `breaks[]` entry on the indicator (`{period, reason}`) in the registry.

## 6. Labels and languages

- Three label languages from the profile (`en`, `si`, `ta`); `en` is mandatory, the others optional and fall back to `en`. Fields: `label_*`, `name_*`, `title_*`, `description_*`.
- Translations live in the registry beside the English text (`label: {en: …, si: …, ta: …}`); a string form means English only. Unit names for places come from the boundary source, then Wikidata labels, then GeoNames alternates — the origin is recorded in `geo.units.attrs.name_src`.
- API: `?lang=en|si|ta` (v1 rule kept); JSON-LD output uses language-tagged strings.
- Transliteration is never generated by the platform. A missing Sinhala/Tamil name is missing.

## 7. Naming conventions

| Object | Convention |
|---|---|
| Source id | `<publisher>-<product>` kebab (`dcs-census-2024`, `worldbank-wdi`) |
| Dataset id | `<source family>-<subject>[-<grain>]`; derived: `derived-<subject>` |
| Indicator id | `<namespace>.<measure>[_<qualifier>]`; namespaces are short and stable (`census24`, `wdi`, `lfs`, `hies`, `cbsl`, `osm`, `wp`, `est`, `idx`, `exp`, `acc`) |
| Dimension id | singular noun (`sex`, `age`, `sector`, `crop`) |
| Postgres | `snake_case`, singular enum labels, plural table names |
| Files | `<dataset_id>/<dsv>/data.parquet`; column names equal the logical field names of §4 |

## 8. Licences, attribution, redistribution

```sql
ALTER TABLE catalog.licences
  ADD COLUMN spdx           text,                 -- 'CC-BY-4.0', 'ODbL-1.0', 'CC0-1.0'; NULL for unstated
  ADD COLUMN share_alike    boolean NOT NULL DEFAULT false,
  ADD COLUMN non_commercial boolean NOT NULL DEFAULT false,
  ADD COLUMN redistribution text NOT NULL DEFAULT 'api'
        CHECK (redistribution IN ('bulk','api','link','internal'));
```

| Tier | API values | Bulk files / packs | Raw artifact download | Typical |
|---|---|---|---|---|
| `bulk` | yes | yes | yes | CC0, CC BY, ODbL, CC BY-IGO |
| `api` | yes, with attribution and the licence status in `meta` | per-dataset download only, never in aggregate packs or the npm/CLI packs | no (sha256 + source URL only) | `permission_unclear` government publications |
| `link` | metadata only: what exists, where, how to get it | no | no | `restricted` (ACLED) |
| `internal` | no | no | no | CC BY-NC inputs used for validation only (Meta RWI) |

Rules:

- **A dataset's licence is computed**, not typed: the most restrictive tier among its sources; `share_alike` and `non_commercial` propagate; for derived datasets the computation walks `data.derivations`. The registry may only make it stricter.
- **ODbL isolation.** OSM-derived data lives in datasets that contain only OSM (plus keys), published under ODbL. A recipe that mixes OSM geometry into a non-ODbL product must produce a *Produced Work* (statistic, tile, image) not a derivative database, or the output dataset is ODbL. The licence computation encodes this as: `features` output containing OSM geometry → ODbL; `indicators` output computed from OSM → attribution "contains information from OpenStreetMap", licence of the other inputs. **[A: this is the common community reading of ODbL; have it reviewed before the first mixed product ships.]**
- **Attribution is machine-readable**: `meta.attribution[]` = `{source_id, publisher, text, licence, licence_url, status}` on every response; `CITATION.cff` and `datapackage.json` `sources`/`licenses` in every bundle; a ready-made credit line per dataset at `/v2/datasets/{id}/attribution?format=text|html`.

## 9. Provenance

Two kinds of lineage, both resolvable from an `obs_id`:

1. **Extracted** (classes `official`, `third_party`): `data.obs_provenance` row → artifact (sha256, URL, fetched_at) + extract + locator + verbatim token + extraction method + confidence + review record.
2. **Derived** (classes `derived`, `estimated`): the row's dataset version → `data.derivations` (recipe, method@version, params, code revision) → input versions → recursively, down to extracted rows. `GET /v2/observations/{id}/provenance?depth=n` walks it; the input *rows* for one output row are computed on demand by the method's `explain()` ([04 §6](04-enrichment.md#6-explainability)).

Locator grammar (JSON, one of):

```jsonc
{ "k": "pdf",   "page": 19, "printed_page": 4, "table": "1.4", "row": "Colombo", "col": "Total", "bbox": [72.1, 310.4, 118.9, 322.0] }
{ "k": "sheet", "sheet": "A5", "cell": "C10", "row": "Colombo District", "col": "Total" }
{ "k": "json",  "path": "$[1][3].value", "params": {"indicator": "SP.POP.TOTL"} }
{ "k": "csv",   "line": 482, "col": "value" }
{ "k": "html",  "selector": "table#t3 tr:nth-child(4) td:nth-child(2)" }
{ "k": "feature", "key": "node/123456789", "tag": "amenity" }
{ "k": "grid",  "layer": "worldpop-pop-grid", "period": "2025", "cells": 143 }
```

**W3C PROV alignment** (`GET /v2/observations/{id}/provenance` with `Accept: application/ld+json`, and `prov.jsonld` in each dataset bundle):

| PROV | Ours |
|---|---|
| `prov:Entity` | artifact, extract, dataset version, observation, release |
| `prov:Activity` | ingestion run, recipe run, review, release cut |
| `prov:Agent` | publisher (organisation), engine (software agent, version + git rev), reviewer (person, by handle) |
| `prov:wasDerivedFrom` | version → artifacts/extracts; derived version → input versions |
| `prov:wasGeneratedBy` / `prov:used` | version ← run; run → artifacts |
| `prov:wasAttributedTo` | artifact → publisher; review → reviewer |
| `prov:hadPrimarySource` | observation → artifact (with the locator as `prov:atLocation`) |
| `prov:qualifiedDerivation` + `prov:hadPlan` | derived version → recipe (method@version, params) |

## 10. Conformance to external standards

"Conform" below means a validator in CI checks the output; "align" means we follow the model but do not claim certification.

| Standard | Level | Exactly what we emit | Where |
|---|---|---|---|
| **DCAT-AP 3.0** | conform (mandatory + recommended properties) | `dcat:Catalog` → `dcat:Dataset` (title, description, `dct:publisher`, `dcat:theme` from the EU data-theme vocabulary already used in the registry, `dct:spatial`, `dct:temporal`, `dct:accrualPeriodicity` from the EU frequency vocabulary, `dct:provenance`, `dcat:version` = dsv, `adms:status`) → `dcat:Distribution` (`dcat:downloadURL`, `dct:format`, `dcat:mediaType`, `dct:license`, `spdx:checksum`, `dcat:byteSize`) and `dcat:DataService` for the API | `/v2/catalog.jsonld`, `releases/<v>/catalog.jsonld`; validated with the SEMIC SHACL shapes in CI |
| **schema.org Dataset** | conform (Google Dataset Search requirements) | `Dataset` + `DataDownload` + `DataCatalog`, `variableMeasured` per indicator, `spatialCoverage`, `temporalCoverage`, `license`, `isBasedOn` | `/v2/datasets/{id}` with `Accept: application/ld+json`; the web track embeds it |
| **Frictionless Data Package v2 / Table Schema** | conform | `datapackage.json` per dataset bundle and per release; fields typed, `missingValues`, primary key, foreign keys to the codelist resources shipped in the package | every `store/datasets/<id>/<dsv>/`; `frictionless validate` in CI |
| **SDMX 3.0 information model** | align | concepts, codelists, `OBS_STATUS`, `UNIT_MULT`, `FREQ`, time formats; **export only**: SDMX-CSV 2.0 (`?format=sdmx-csv`) with our dataset as a dataflow. No SDMX REST service, no SDMX-ML | series/data endpoints |
| **OGC API – Features Part 1 (Core) + GeoJSON + OpenAPI 3.0 classes** | conform | landing page, `/conformance`, `/collections`, `/collections/{id}/items` (bbox, datetime, limit, cursor paging via `next` links), `/items/{fid}` | `/v2/ogc/` ; OGC Team Engine ETS in CI |
| **OGC API – Features Part 3 (Filtering)** | partial | `filter-lang=cql2-text`, comparison + `S_INTERSECTS` only | same |
| **OGC API – Tiles Part 1** | conform (Core, Tileset, TileSets list; `WebMercatorQuad`) | tileset metadata + `/tiles/{tms}/{z}/{y}/{x}` | `/v2/ogc/collections/{id}/tiles`, `/v2/ogc/tiles` |
| **STAC 1.1** | conform (static catalog) | one `Collection` per grid dataset, one `Item` per layer × period, assets = COG (+ the `.bin` array), `proj`, `raster` and `file` extensions | `releases/<v>/stac/`, also at `/v2/stac/`; `stac-validator` in CI |
| **PMTiles v3** | conform | vector tilesets (MVT) | `store/tiles/` |
| **GeoParquet 1.1** | conform | WKB geometry column, `geo` metadata, bbox covering column | feature and boundary distributions |
| **Cloud-Optimized GeoTIFF** | conform | internal tiling 512, overviews, DEFLATE/ZSTD, EPSG:4326 aligned to the canonical grid | grid distributions; `rio cogeo validate` in CI |
| **W3C PROV-O** | align | §9 | provenance endpoints, `prov.jsonld` |
| **UCUM** | conform | unit codes | `catalog.units_of_measure` |
| **ISO 8601 / ISO 4217 / ISO 639-1 / ISO 3166-2** | conform | periods, currencies, languages, province/district xrefs | throughout |
| **OpenAPI 3.1** | conform | generated from `utoipa` | `/v2/openapi.json` |
| **MCP** (current revision at build time) | conform | streamable HTTP + stdio | `/mcp`, `lanka mcp` |
| **RFC 7946 GeoJSON, RFC 9457 problem details** | conform | geometry output; error bodies inside our envelope carry `type`, `title`, `status`, `detail` | REST |

Deliberately not adopted: SDMX REST/SDMX-ML (large surface, few users in this audience, SDMX-CSV covers statistical interchange); CSVW (Frictionless covers it); INSPIRE (EU-specific); OGC API Records (DCAT covers discovery); H3/S2 as the canonical grid (the 0.001° grid is contractual; an `h3_r8` xref may be published as a crosswalk dataset later).

## 11. Conformance classes of *our* standard

So that "implements the Lanka Data Layer Standard" means something testable:

| Class | Requires |
|---|---|
| `core` | §1 identifiers, §3–§5 observation model, §8 licence metadata, envelope `meta` fields |
| `geo` | §2 units/schemes/vintages, crosswalk invariant, CRS policy |
| `provenance` | §9 resolvable for every observation; PROV export |
| `release` | immutable versions, release manifest, reproducibility guarantee of [01](01-layers.md#reproducibility) |
| `estimates` | `basis`/`class` rules, intervals or quality class, the labelling rules of [04 §8](04-enrichment.md#8-labelling) |
| `grid` | a profile grid, array format, zonal semantics |

`spec/conformance/` holds a test suite of HTTP requests + JSON Schema assertions that any deployment's `/v2` can be run against (`lanka conformance <base-url>`).
