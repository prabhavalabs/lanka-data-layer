-- Lanka Data Layer v2 — foundation schema.
-- Design: docs/data-platform-v2.md. Four schemas:
--   catalog  registry mirror (licences, sources, datasets, indicators) — synced from registry/*.yaml
--   ops      ingestion runs, events, raw artifacts, alerts (monitoring)
--   geo      admin units (p-code keyed, PostGIS), boundary sets, crosswalks, grid layer index, features
--   data     immutable dataset versions, observations (row-level provenance), releases, changelog

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE SCHEMA IF NOT EXISTS catalog;
CREATE SCHEMA IF NOT EXISTS ops;
CREATE SCHEMA IF NOT EXISTS geo;
CREATE SCHEMA IF NOT EXISTS data;

-- ── enums (4 bytes on disk; observation rows carry three of them) ──────────
CREATE TYPE catalog.licence_status AS ENUM ('open', 'attribution', 'permission_unclear', 'restricted');
CREATE TYPE catalog.publication_class AS ENUM ('A', 'B', 'C');
CREATE TYPE data.obs_status AS ENUM (
  'reported', 'revised', 'provisional', 'estimate', 'forecast', 'modelled',
  'derived', 'not_available', 'nil', 'suppressed');
CREATE TYPE data.extraction_method AS ENUM (
  'api', 'sheet_cell', 'csv_row', 'pdf_text', 'pdf_reviewed', 'llm_dual',
  'manual', 'spatial', 'derived');
CREATE TYPE data.review_status AS ENUM ('auto_accepted', 'pending', 'approved', 'rejected');
CREATE TYPE data.version_status AS ENUM (
  'staged', 'in_review', 'published', 'superseded', 'rejected', 'withdrawn');
CREATE TYPE ops.run_status AS ENUM ('running', 'succeeded', 'no_change', 'needs_review', 'failed');
CREATE TYPE ops.event_level AS ENUM ('debug', 'info', 'warn', 'error');

-- ═════════════════════════════ catalog ═════════════════════════════════════
CREATE TABLE catalog.licences (
  id           text PRIMARY KEY,
  status       catalog.licence_status NOT NULL,
  url          text,
  attribution  text
);

CREATE TABLE catalog.sources (
  id                    text PRIMARY KEY,
  publisher             text NOT NULL,
  title                 text NOT NULL,
  homepage              text NOT NULL,
  access_method         text NOT NULL,
  endpoint              text NOT NULL,
  auth                  text NOT NULL,
  licence_id            text NOT NULL REFERENCES catalog.licences (id),
  cadence               text NOT NULL,
  release_calendar_url  text,
  expected_lag_days     integer,
  geo_levels            text[] NOT NULL,
  geo_join              text NOT NULL,
  themes                text[] NOT NULL,
  priority              text NOT NULL,
  effort                text NOT NULL,
  status                text NOT NULL,
  verified_at           date,
  notes                 text,
  spec                  jsonb NOT NULL,          -- the registry YAML, verbatim as JSON
  spec_sha256           bytea NOT NULL,
  synced_at             timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE catalog.datasets (
  id                 text PRIMARY KEY,
  title              text NOT NULL,
  description        text NOT NULL,
  method             text NOT NULL,
  limits             text[] NOT NULL,
  publication_class  catalog.publication_class NOT NULL,
  time_from          text,
  time_to            text,
  enrichment         jsonb,
  spec               jsonb NOT NULL,
  spec_sha256        bytea NOT NULL,
  synced_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX datasets_title_trgm ON catalog.datasets USING gin (title gin_trgm_ops);

CREATE TABLE catalog.dataset_sources (
  dataset_id  text NOT NULL REFERENCES catalog.datasets (id) ON DELETE CASCADE,
  source_id   text NOT NULL REFERENCES catalog.sources (id),
  PRIMARY KEY (dataset_id, source_id)
);
CREATE INDEX dataset_sources_source ON catalog.dataset_sources (source_id);

CREATE TABLE catalog.indicators (
  id           text PRIMARY KEY,
  dataset_id   text NOT NULL REFERENCES catalog.datasets (id) ON DELETE CASCADE,
  label        text NOT NULL,
  unit         text NOT NULL,
  dims         text[] NOT NULL DEFAULT '{}',
  geo_levels   text[] NOT NULL,
  period_type  text NOT NULL,
  sort         integer NOT NULL DEFAULT 0
);
CREATE INDEX indicators_dataset ON catalog.indicators (dataset_id, sort);
CREATE INDEX indicators_label_trgm ON catalog.indicators USING gin (label gin_trgm_ops);

-- ═════════════════════════════ ops ═════════════════════════════════════════
-- Every upstream file/response we ever fetched. Content-addressed: the bytes
-- live at $LANKA_DATA_DIR/raw/<sha[0:2]>/<sha[2:4]>/<sha> on local disk.
CREATE TABLE ops.artifacts (
  id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  sha256         bytea NOT NULL UNIQUE CHECK (octet_length(sha256) = 32),
  source_id      text NOT NULL REFERENCES catalog.sources (id),
  url            text NOT NULL,
  fetched_at     timestamptz NOT NULL,
  bytes          bigint NOT NULL,
  media_type     text,
  etag           text,
  last_modified  text,
  edition        text,
  archive_path   text NOT NULL
);
CREATE INDEX artifacts_source ON ops.artifacts (source_id, fetched_at DESC);

CREATE TABLE ops.ingestion_runs (
  id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  source_id       text NOT NULL REFERENCES catalog.sources (id),
  trigger         text NOT NULL CHECK (trigger IN ('manual', 'schedule', 'retry', 'test')),
  status          ops.run_status NOT NULL DEFAULT 'running',
  started_at      timestamptz NOT NULL DEFAULT now(),
  finished_at     timestamptz,
  host            text,
  engine_version  text NOT NULL,
  artifacts       integer NOT NULL DEFAULT 0,
  rows_parsed     bigint NOT NULL DEFAULT 0,
  rows_written    bigint NOT NULL DEFAULT 0,
  message         text,
  error           text
);
CREATE INDEX runs_source_started ON ops.ingestion_runs (source_id, started_at DESC);
-- At most one running run per source (also enforced by an advisory lock).
CREATE UNIQUE INDEX runs_one_running ON ops.ingestion_runs (source_id) WHERE status = 'running';

CREATE TABLE ops.run_events (
  id       bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  run_id   bigint NOT NULL REFERENCES ops.ingestion_runs (id) ON DELETE CASCADE,
  at       timestamptz NOT NULL DEFAULT clock_timestamp(),
  level    ops.event_level NOT NULL,
  stage    text NOT NULL CHECK (stage IN (
             'discover', 'fetch', 'archive', 'parse', 'validate', 'stage', 'publish', 'alert', 'run')),
  message  text NOT NULL,
  data     jsonb
);
CREATE INDEX run_events_run ON ops.run_events (run_id, id);

CREATE TABLE ops.run_artifacts (
  run_id       bigint NOT NULL REFERENCES ops.ingestion_runs (id) ON DELETE CASCADE,
  artifact_id  bigint NOT NULL REFERENCES ops.artifacts (id),
  PRIMARY KEY (run_id, artifact_id)
);

CREATE TABLE ops.alerts (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  source_id     text REFERENCES catalog.sources (id),
  run_id        bigint REFERENCES ops.ingestion_runs (id) ON DELETE SET NULL,
  kind          text NOT NULL,      -- run_failed | overdue | validation_failed | heartbeat_missed
  message       text NOT NULL,
  created_at    timestamptz NOT NULL DEFAULT now(),
  delivered_at  timestamptz,
  channel       text,
  error         text
);
CREATE INDEX alerts_undelivered ON ops.alerts (created_at) WHERE delivered_at IS NULL;

-- ═════════════════════════════ geo ═════════════════════════════════════════
CREATE TABLE geo.boundary_sets (
  id         text PRIMARY KEY,      -- 'cod-ab-lka-2022', 'dcs-2012-gnd', 'ec-pd', …
  title      text NOT NULL,
  source_id  text REFERENCES catalog.sources (id),
  vintage    text,
  notes      text
);

-- Admin hierarchy ADM0–ADM4. p-codes are the only join key.
CREATE TABLE geo.admin_units (
  pcode            text PRIMARY KEY CHECK (pcode ~ '^LK[0-9]*$'),
  level            smallint NOT NULL CHECK (level BETWEEN 0 AND 4),
  name_en          text NOT NULL,
  name_si          text,
  name_ta          text,
  parent_pcode     text REFERENCES geo.admin_units (pcode) DEFERRABLE INITIALLY DEFERRED,
  adm1_pcode       text,
  adm2_pcode       text,
  adm3_pcode       text,
  boundary_set     text NOT NULL REFERENCES geo.boundary_sets (id),
  area_km2         double precision,
  centroid         geometry(Point, 4326),            -- ST_PointOnSurface: always inside
  bbox             box2d,
  geom             geometry(MultiPolygon, 4326) NOT NULL,
  geom_simplified  geometry(MultiPolygon, 4326),     -- ~0.0005° for highlights
  geom_sha256      bytea,                            -- sha256 of the source GeoJSON (cheap diffs)
  artifact_id      bigint REFERENCES ops.artifacts (id),
  updated_at       timestamptz NOT NULL DEFAULT now(),
  CHECK ((level = 0) = (parent_pcode IS NULL))
);
CREATE INDEX admin_units_parent ON geo.admin_units (parent_pcode);
CREATE INDEX admin_units_level ON geo.admin_units (level);
CREATE INDEX admin_units_geom ON geo.admin_units USING gist (geom);
CREATE INDEX admin_units_name_trgm ON geo.admin_units USING gin (name_en gin_trgm_ops);

-- Crosswalks are data: name→p-code matches, old→new boundary splits.
CREATE TABLE geo.crosswalks (
  from_set   text NOT NULL,         -- 'name:dcs-cph2024', 'dcs-2012-gnd', …
  from_code  text NOT NULL,
  to_set     text NOT NULL,         -- 'cod-ab-lka-2022'
  to_code    text NOT NULL,
  weight     double precision NOT NULL DEFAULT 1 CHECK (weight > 0 AND weight <= 1),
  method     text NOT NULL CHECK (method IN (
               'exact_code', 'name_exact', 'name_alias', 'area_overlap', 'pop_overlap', 'manual')),
  source_id  text REFERENCES catalog.sources (id),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (from_set, from_code, to_set, to_code)
);
CREATE INDEX crosswalks_to ON geo.crosswalks (to_set, to_code);

-- Grid layers: values live in memory-mapped little-endian arrays + COGs on
-- disk (see design doc §5); Postgres only indexes them.
CREATE TABLE geo.grid_layers (
  id            text NOT NULL,
  period        text NOT NULL,
  dataset_id    text NOT NULL REFERENCES catalog.datasets (id),
  indicator_id  text REFERENCES catalog.indicators (id),
  dtype         text NOT NULL CHECK (dtype IN ('u8', 'u16', 'i16', 'u32', 'f32')),
  scale         double precision NOT NULL DEFAULT 1,
  "offset"      double precision NOT NULL DEFAULT 0,
  nodata        double precision,
  unit          text NOT NULL,
  zonal         text NOT NULL CHECK (zonal IN ('sum', 'mean', 'pop_weighted_mean', 'class_share', 'max')),
  source_res_m  double precision NOT NULL,
  bin_path      text NOT NULL,
  cog_path      text,
  sha256        bytea NOT NULL,
  version_id    bigint,
  PRIMARY KEY (id, period)
);

-- ═════════════════════════════ data ════════════════════════════════════════
-- Immutable dataset versions. dsv = 'dsv_' + first 16 hex of the sha256 of
-- the canonical (sorted, fixed-format) row stream — an unchanged source
-- yields an unchanged dsv, so re-runs are no-ops.
CREATE TABLE data.dataset_versions (
  id                 bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  dataset_id         text NOT NULL REFERENCES catalog.datasets (id),
  dsv                text NOT NULL UNIQUE,
  content_sha256     bytea NOT NULL,
  row_count          bigint NOT NULL,
  status             data.version_status NOT NULL DEFAULT 'staged',
  publication_class  catalog.publication_class NOT NULL,
  vintage            text,
  run_id             bigint REFERENCES ops.ingestion_runs (id),
  supersedes_id      bigint REFERENCES data.dataset_versions (id),
  created_at         timestamptz NOT NULL DEFAULT now(),
  published_at       timestamptz,
  reviewed_by        text,
  reviewed_at        timestamptz,
  review_note        text,
  validation         jsonb,           -- validator report
  diff               jsonb            -- counts vs. the version it supersedes
);
CREATE INDEX dataset_versions_dataset ON data.dataset_versions (dataset_id, created_at DESC);

CREATE TABLE data.dataset_version_artifacts (
  version_id   bigint NOT NULL REFERENCES data.dataset_versions (id) ON DELETE CASCADE,
  artifact_id  bigint NOT NULL REFERENCES ops.artifacts (id),
  PRIMARY KEY (version_id, artifact_id)
);

-- Releases: data_version (YYYYMMDD.N) = an immutable map dataset → version.
CREATE TABLE data.releases (
  data_version  text PRIMARY KEY CHECK (data_version ~ '^[0-9]{8}\.[0-9]+$'),
  seq           bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  created_at    timestamptz NOT NULL DEFAULT now(),
  notes         text,
  run_id        bigint REFERENCES ops.ingestion_runs (id)
);
CREATE TABLE data.release_datasets (
  data_version  text NOT NULL REFERENCES data.releases (data_version) ON DELETE CASCADE,
  dataset_id    text NOT NULL REFERENCES catalog.datasets (id),
  version_id    bigint NOT NULL REFERENCES data.dataset_versions (id),
  PRIMARY KEY (data_version, dataset_id)
);

CREATE TABLE data.changelog (
  data_version     text NOT NULL REFERENCES data.releases (data_version) ON DELETE CASCADE,
  dataset_id       text NOT NULL REFERENCES catalog.datasets (id),
  from_version_id  bigint REFERENCES data.dataset_versions (id),
  to_version_id    bigint NOT NULL REFERENCES data.dataset_versions (id),
  rows_added       bigint NOT NULL,
  rows_removed     bigint NOT NULL,
  rows_changed     bigint NOT NULL,
  summary          text NOT NULL,
  PRIMARY KEY (data_version, dataset_id)
);

-- Observations: long, narrow, keyed. LIST-partitioned by dataset so each
-- dataset's rows + indexes are physically separate (pruned by every query,
-- dropped/rebuilt independently). Provenance is inline (1:1 with the row):
-- artifact (→ sha256 + archived bytes), locator, verbatim token, method,
-- confidence and review status.
CREATE TABLE data.observations (
  obs_id        bigint GENERATED BY DEFAULT AS IDENTITY,
  dataset_id    text NOT NULL,
  version_id    bigint NOT NULL,
  indicator_id  text NOT NULL,
  geo_code      text NOT NULL,       -- p-code ('LK' = national) or another coded geography
  geo_level     smallint NOT NULL,   -- 0..4 admin level; 9 = non-admin code
  period        text NOT NULL,       -- '2024', '2024-Q2', '2026-08', '2026-11-20'
  period_start  date NOT NULL,
  dims          text NOT NULL DEFAULT '',   -- canonical 'age=0-14;sex=f' (keys sorted)
  value         double precision,
  status        data.obs_status NOT NULL DEFAULT 'reported',
  artifact_id   bigint,              -- NULL only for method = 'derived'
  locator       jsonb,
  verbatim      text,
  method        data.extraction_method NOT NULL,
  confidence    real,
  review        data.review_status NOT NULL,
  PRIMARY KEY (dataset_id, version_id, indicator_id, geo_code, period, dims)
) PARTITION BY LIST (dataset_id);

-- Partition indexes are created per partition by ensure_obs_partition.
CREATE TABLE data.observations_default PARTITION OF data.observations DEFAULT;

CREATE OR REPLACE FUNCTION data.ensure_obs_partition(p_dataset text)
RETURNS text LANGUAGE plpgsql AS $$
DECLARE
  part text := 'observations_' || substr(regexp_replace(lower(p_dataset), '[^a-z0-9]+', '_', 'g'), 1, 48);
BEGIN
  IF to_regclass('data.' || part) IS NULL THEN
    EXECUTE format('CREATE TABLE data.%I PARTITION OF data.observations FOR VALUES IN (%L)', part, p_dataset);
    -- area profile: everything about one place
    EXECUTE format('CREATE INDEX %I ON data.%I (geo_code, version_id, indicator_id, period_start DESC)',
                   part || '_geo', part);
    -- choropleth: one indicator, one period, one level
    EXECUTE format('CREATE INDEX %I ON data.%I (version_id, indicator_id, period, geo_level) INCLUDE (geo_code, value, dims)',
                   part || '_map', part);
    -- provenance lookup by observation id
    EXECUTE format('CREATE INDEX %I ON data.%I (obs_id)', part || '_obs', part);
  END IF;
  RETURN part;
END $$;

-- The current release and the dataset versions it pins.
CREATE VIEW data.current_release AS
  SELECT data_version, created_at FROM data.releases ORDER BY seq DESC LIMIT 1;

CREATE VIEW data.current_versions AS
  SELECT rd.dataset_id, rd.version_id, v.dsv, v.vintage, v.row_count, v.published_at
  FROM data.release_datasets rd
  JOIN data.current_release cr USING (data_version)
  JOIN data.dataset_versions v ON v.id = rd.version_id;

-- Every observation of the dataset versions pinned by the current release.
CREATE VIEW data.current_observations AS
  SELECT o.* FROM data.observations o
  JOIN data.current_versions cv ON cv.dataset_id = o.dataset_id AND cv.version_id = o.version_id;

-- Points and lines that are not admin units (POIs, stations, roads …).
CREATE TABLE geo.features (
  id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  dataset_id   text NOT NULL REFERENCES catalog.datasets (id),
  version_id   bigint NOT NULL REFERENCES data.dataset_versions (id),
  source_key   text NOT NULL,          -- e.g. 'node/123456789' for OSM
  kind         text NOT NULL,
  name_en      text,
  name_si      text,
  name_ta      text,
  geom         geometry(Geometry, 4326) NOT NULL,
  cell_id      integer,
  adm4_pcode   text REFERENCES geo.admin_units (pcode),
  attrs        jsonb,
  artifact_id  bigint REFERENCES ops.artifacts (id),
  locator      jsonb,
  UNIQUE (version_id, source_key)
);
CREATE INDEX features_geom ON geo.features USING gist (geom);
CREATE INDEX features_dataset ON geo.features (dataset_id, version_id, kind);
CREATE INDEX features_adm4 ON geo.features (adm4_pcode);

-- ── freshness / monitoring view ────────────────────────────────────────────
CREATE VIEW ops.source_health AS
WITH last AS (
  SELECT DISTINCT ON (source_id) source_id, id AS last_run_id, status AS last_status,
         started_at AS last_started_at, finished_at AS last_finished_at, error AS last_error
  FROM ops.ingestion_runs ORDER BY source_id, started_at DESC
), ok AS (
  SELECT source_id, max(finished_at) AS last_success_at
  FROM ops.ingestion_runs WHERE status IN ('succeeded', 'no_change', 'needs_review')
  GROUP BY source_id
), fails AS (
  SELECT r.source_id, count(*) AS consecutive_failures
  FROM ops.ingestion_runs r
  LEFT JOIN ok USING (source_id)
  WHERE r.status = 'failed' AND (ok.last_success_at IS NULL OR r.started_at > ok.last_success_at)
  GROUP BY r.source_id
)
SELECT s.id AS source_id, s.cadence, s.status AS registry_status,
       last.last_run_id, last.last_status, last.last_started_at, last.last_finished_at, last.last_error,
       ok.last_success_at, coalesce(fails.consecutive_failures, 0) AS consecutive_failures,
       CASE s.cadence
         WHEN 'realtime' THEN interval '1 hour'  WHEN 'daily' THEN interval '1 day'
         WHEN 'weekly' THEN interval '7 days'    WHEN 'monthly' THEN interval '1 month'
         WHEN 'quarterly' THEN interval '3 months' WHEN 'annual' THEN interval '1 year'
         ELSE NULL END AS check_every
FROM catalog.sources s
LEFT JOIN last ON last.source_id = s.id
LEFT JOIN ok ON ok.source_id = s.id
LEFT JOIN fails ON fails.source_id = s.id;

-- Notify servers when a release is published so they swap their cache.
CREATE OR REPLACE FUNCTION data.notify_release() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_notify('lanka_release', NEW.data_version);
  RETURN NEW;
END $$;
CREATE TRIGGER releases_notify AFTER INSERT ON data.releases
  FOR EACH ROW EXECUTE FUNCTION data.notify_release();
