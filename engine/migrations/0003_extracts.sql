-- 0003 — WP-02: L1 extracts, change detection, slim observation indexes.
--
-- 1. ops.extracts / ops.extract_artifacts (01-layers.md §L1) and the link
--    from dataset versions to the extracts they were standardised from.
-- 2. Change detection inputs: the candidate-set fingerprint and the input
--    hash (connector version + specs + artifact set) on every run, and the
--    HTTP status / request hash on artifacts (03 §1, 01 §L0).
-- 3. Slimmer per-partition indexes (measured on a synthetic 10 M-row
--    partition: indexes 2.45× heap → 1.02×, see engine/README.md).

-- ═════════════════════════════ L1 extracts ═════════════════════════════════
-- One row per content-addressed Parquet file at store/extract/ab/cd/<sha>.parquet.
CREATE TABLE ops.extracts (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  sha256           bytea NOT NULL UNIQUE CHECK (octet_length(sha256) = 32),
  source_id        text  NOT NULL REFERENCES catalog.sources (id),
  shape            text  NOT NULL CHECK (shape IN ('cells', 'records', 'features')),
  extractor        text  NOT NULL,          -- connector / driver id ('http_json', 'dcs-census-2024', …)
  extractor_ver    text  NOT NULL,          -- semver of the driver/connector
  template_sha256  bytea,                   -- registry/templates/*.yaml or connector spec hash, if any
  model            text,                    -- class C only: provider/model id
  prompt_sha256    bytea,                   -- class C only
  rows             bigint NOT NULL,
  bytes            bigint NOT NULL,
  store_key        text  NOT NULL,          -- key under store/extract/
  created_at       timestamptz NOT NULL DEFAULT now(),
  run_id           bigint REFERENCES ops.ingestion_runs (id)
);
CREATE INDEX extracts_source ON ops.extracts (source_id, created_at DESC);
CREATE INDEX extracts_run ON ops.extracts (run_id);

-- The artifacts an extract read, by position: `artifact_idx` in the Parquet
-- rows indexes this list. `key`/`url` are the candidate's (an artifact row
-- is shared by every candidate that returned the same bytes).
CREATE TABLE ops.extract_artifacts (
  extract_id   bigint NOT NULL REFERENCES ops.extracts (id) ON DELETE CASCADE,
  idx          int    NOT NULL,
  artifact_id  bigint NOT NULL REFERENCES ops.artifacts (id),
  key          text   NOT NULL,
  url          text   NOT NULL,
  PRIMARY KEY (extract_id, idx)
);
CREATE INDEX extract_artifacts_artifact ON ops.extract_artifacts (artifact_id);

-- The extracts a run produced or reused (restandardise reads the latest set).
CREATE TABLE ops.run_extracts (
  run_id      bigint NOT NULL REFERENCES ops.ingestion_runs (id) ON DELETE CASCADE,
  extract_id  bigint NOT NULL REFERENCES ops.extracts (id),
  PRIMARY KEY (run_id, extract_id)
);

CREATE TABLE data.dataset_version_extracts (
  version_id  bigint NOT NULL REFERENCES data.dataset_versions (id) ON DELETE CASCADE,
  extract_id  bigint NOT NULL REFERENCES ops.extracts (id),
  PRIMARY KEY (version_id, extract_id)
);

COMMENT ON COLUMN data.obs_provenance.extract_id IS 'ops.extracts.id the row was standardised from (no FK: extracts follow their own retention)';
COMMENT ON COLUMN data.obs_provenance.extract_row IS 'row index inside that extract';

-- ═════════════════════════════ change detection ════════════════════════════
ALTER TABLE ops.artifacts
  ADD COLUMN http_status             int2,
  ADD COLUMN request_headers_sha256  bytea,
  ADD COLUMN purged_at               timestamptz;
CREATE INDEX artifacts_source_url ON ops.artifacts (source_id, url, fetched_at DESC);

ALTER TABLE ops.ingestion_runs
  ADD COLUMN connector_version text,
  ADD COLUMN fingerprint       bytea CHECK (octet_length(fingerprint) = 32),
  ADD COLUMN input_hash        bytea CHECK (octet_length(input_hash) = 32),
  DROP CONSTRAINT ingestion_runs_trigger_check,
  ADD CONSTRAINT ingestion_runs_trigger_check
    CHECK (trigger IN ('manual', 'schedule', 'retry', 'test', 'backfill', 'restandardise'));

ALTER TABLE ops.run_events
  DROP CONSTRAINT run_events_stage_check,
  ADD CONSTRAINT run_events_stage_check CHECK (stage IN (
    'discover', 'fetch', 'archive', 'extract', 'standardise', 'parse', 'validate', 'stage', 'publish',
    'alert', 'run'));

-- ═════════════════════════════ slim observation indexes ════════════════════
-- 0002 carried every value column in the INCLUDE lists of `_series` and
-- `_map` so both reads were index-only; at 10 M rows that was 2.45× the heap.
-- Series and map reads return tens to a few hundred rows that sit together in
-- the heap (rows are inserted per version in source order), so a narrow key
-- plus heap fetch costs a few buffer hits. `_map` drops `freq` (one indicator
-- has one frequency) and its key is highly repetitive, so B-tree
-- deduplication shrinks it to ~7 % of the 0002 index.
CREATE OR REPLACE FUNCTION data.obs_partition_indexes(part text)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
  -- series for <= 25 geographies; area profiles (one geo, many indicators)
  EXECUTE format('CREATE INDEX IF NOT EXISTS %I ON data.%I (indicator_key, geo_key, dims_key, time_start)
                  WHERE vto = 2147483647', part || '_series', part);
  -- choropleth: one range scan per (indicator, period, level, dims)
  EXECUTE format('CREATE INDEX IF NOT EXISTS %I ON data.%I (indicator_key, time_start, geo_level, dims_key)
                  WHERE vto = 2147483647', part || '_map', part);
  -- provenance / history lookup by id (ids are assigned in insert order)
  EXECUTE format('CREATE INDEX IF NOT EXISTS %I ON data.%I USING brin (obs_id) WITH (pages_per_range = 16)',
                 part || '_obs', part);
END $$;

CREATE OR REPLACE FUNCTION data.ensure_obs_partition(p_dataset text)
RETURNS text LANGUAGE plpgsql AS $$
DECLARE
  k    int4;
  part text;
  prov text;
BEGIN
  SELECT key INTO k FROM catalog.datasets WHERE id = p_dataset;
  IF k IS NULL THEN
    RAISE EXCEPTION 'ensure_obs_partition: unknown dataset %', p_dataset;
  END IF;
  part := 'observations_' || k;
  prov := 'obs_provenance_' || k;
  IF to_regclass('data.' || part) IS NULL THEN
    EXECUTE format('CREATE TABLE data.%I PARTITION OF data.observations FOR VALUES IN (%s)
                    WITH (fillfactor = 100, autovacuum_vacuum_scale_factor = 0.02,
                          autovacuum_vacuum_insert_scale_factor = 0.1)', part, k);
    PERFORM data.obs_partition_indexes(part);
    EXECUTE format('COMMENT ON TABLE data.%I IS %L', part, 'observations of dataset ' || p_dataset);
  END IF;
  IF to_regclass('data.' || prov) IS NULL THEN
    EXECUTE format('CREATE TABLE data.%I PARTITION OF data.obs_provenance FOR VALUES IN (%s)', prov, k);
  END IF;
  RETURN part;
END $$;

-- Rebuild the indexes of every existing partition.
DO $$
DECLARE
  r record;
BEGIN
  FOR r IN SELECT c.relname FROM pg_inherits i
           JOIN pg_class c ON c.oid = i.inhrelid
           WHERE i.inhparent = 'data.observations'::regclass AND c.relname ~ '^observations_[0-9]+$'
  LOOP
    EXECUTE format('DROP INDEX IF EXISTS data.%I, data.%I', r.relname || '_series', r.relname || '_map');
    PERFORM data.obs_partition_indexes(r.relname);
  END LOOP;
END $$;
