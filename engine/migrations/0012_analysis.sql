-- 0012 — the analysis engine (WP-29; 11 §9, ADR 0021/0022).
--
-- Schema `analysis`: immutable, fingerprinted results (`an_<16 hex>`), the
-- dataset versions each result used (takedown, licence changes), the
-- aligned-column cache and spatial weights (WP-30 fills these two). Bulky
-- outputs live in the blob store under `analysis/<id>/`; the exact result
-- bytes are `analysis/<id>/result.json` (the jsonb copy is for queries).
--
-- Role `lanka_analysis`: reads the public data plane, writes `analysis.*`
-- only. `lanka_api` reads `analysis.*` (cached results are public reads).
-- Grants are re-applied by `analysis.apply_grants()`, which `lanka db
-- migrate` runs after `ops.apply_grants()` and `app.apply_grants()`.

CREATE SCHEMA IF NOT EXISTS analysis;

DO $$
DECLARE
  stmt text;
BEGIN
  FOREACH stmt IN ARRAY ARRAY[
    'CREATE ROLE lanka_analysis LOGIN',
    'ALTER ROLE lanka_analysis SET statement_timeout = ''30s''',
    'ALTER ROLE lanka_analysis SET idle_in_transaction_session_timeout = ''30s'''
  ] LOOP
    BEGIN
      EXECUTE stmt;
    EXCEPTION WHEN duplicate_object OR unique_violation THEN
      NULL;
    WHEN OTHERS THEN
      RAISE NOTICE 'roles: % — %', stmt, SQLERRM;
    END;
  END LOOP;
END $$;

CREATE TABLE analysis.results (
  id             text PRIMARY KEY CHECK (id ~ '^an_[0-9a-f]{16}$'),
  fingerprint    bytea NOT NULL UNIQUE CHECK (octet_length(fingerprint) = 32),
  method         text NOT NULL,
  method_version text NOT NULL,
  request        jsonb NOT NULL,                       -- resolved
  state          text NOT NULL CHECK (state IN ('queued','running','ready','failed','blocked')),
  result         jsonb,                                -- the result document (exact bytes in the blob store)
  result_sha256  bytea CHECK (result_sha256 IS NULL OR octet_length(result_sha256) = 32),
  outputs        jsonb NOT NULL DEFAULT '[]',          -- [{id, kind, blob, bytes, sha256}]
  basis          int2,
  licence_id     text,
  redistribution text,
  workspace_id   bigint,                               -- NULL = all inputs public
  data_version   text NOT NULL,
  cost           int4,
  elapsed_ms     int4,
  job_id         bigint,
  created_at     timestamptz NOT NULL DEFAULT now(),
  last_read_at   date NOT NULL DEFAULT current_date,
  pinned         boolean NOT NULL DEFAULT false,
  CHECK (id = 'an_' || left(encode(fingerprint, 'hex'), 16))
);
COMMENT ON TABLE analysis.results IS
  'Immutable analysis results, id = an_ || first 16 hex of the fingerprint (11 §9). Written by lanka_analysis only.';
CREATE INDEX results_eviction ON analysis.results (last_read_at) WHERE NOT pinned;

CREATE TABLE analysis.result_inputs (
  result_id   text NOT NULL REFERENCES analysis.results (id) ON DELETE CASCADE,
  dataset_ref text NOT NULL,
  dsv         text NOT NULL,
  role        text NOT NULL,
  PRIMARY KEY (result_id, dataset_ref, role)
);
CREATE INDEX result_inputs_by_dataset ON analysis.result_inputs (dataset_ref, dsv);

CREATE TABLE analysis.aligned (
  key          bytea PRIMARY KEY,
  support      jsonb NOT NULL,
  blob_key     text NOT NULL,
  bytes        int8 NOT NULL,
  workspace_id bigint,
  created_at   timestamptz NOT NULL DEFAULT now(),
  last_read_at date NOT NULL DEFAULT current_date
);

CREATE TABLE analysis.spatial_weights (
  boundary_set text NOT NULL,
  scheme_id    text NOT NULL,
  level        int2 NOT NULL,
  kind         text NOT NULL,                          -- queen | knn6 | band_5km
  n            int4 NOT NULL,
  nnz          int8 NOT NULL,
  islands      int4[] NOT NULL,
  blob_key     text NOT NULL,
  sha256       bytea NOT NULL,
  PRIMARY KEY (boundary_set, scheme_id, level, kind)
);

CREATE FUNCTION analysis.apply_grants() RETURNS void LANGUAGE plpgsql AS $$
DECLARE
  r        record;
  readable text;
BEGIN
  SELECT string_agg(quote_ident(nspname), ', ' ORDER BY nspname) INTO readable
    FROM pg_namespace WHERE nspname = ANY (ARRAY['catalog', 'geo', 'data', 'serve']);

  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_owner') THEN
    IF (SELECT pg_get_userbyid(nspowner) FROM pg_namespace WHERE nspname = 'analysis') <> 'lanka_owner' THEN
      ALTER SCHEMA analysis OWNER TO lanka_owner;
    END IF;
    FOR r IN SELECT c.oid::regclass AS rel
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'analysis' AND c.relkind IN ('r', 'p')
               AND pg_get_userbyid(c.relowner) <> 'lanka_owner'
    LOOP
      EXECUTE format('ALTER TABLE %s OWNER TO lanka_owner', r.rel);
    END LOOP;
  END IF;

  REVOKE ALL ON SCHEMA analysis FROM PUBLIC;
  REVOKE ALL ON ALL TABLES IN SCHEMA analysis FROM PUBLIC;
  FOR r IN SELECT rolname FROM pg_roles WHERE rolname IN ('lanka_ingest', 'lanka_recipe', 'lanka_app') LOOP
    EXECUTE format('REVOKE ALL ON SCHEMA analysis FROM %I', r.rolname);
    EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA analysis FROM %I', r.rolname);
  END LOOP;

  -- lanka_analysis: reads the public plane, writes analysis.* only
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_analysis') THEN
    EXECUTE format('GRANT USAGE ON SCHEMA %s TO lanka_analysis', readable);
    EXECUTE format('GRANT SELECT ON ALL TABLES IN SCHEMA %s TO lanka_analysis', readable);
    GRANT USAGE ON SCHEMA analysis TO lanka_analysis;
    GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA analysis TO lanka_analysis;
  END IF;
  -- lanka_api: reads cached results
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_api') THEN
    GRANT USAGE ON SCHEMA analysis TO lanka_api;
    GRANT SELECT ON ALL TABLES IN SCHEMA analysis TO lanka_api;
  END IF;
  REVOKE ALL ON FUNCTION analysis.apply_grants() FROM PUBLIC;
END $$;

SELECT analysis.apply_grants();
