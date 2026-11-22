-- 0009 — the app plane, stage 0: schema `app`, anonymous map snapshots and
-- the role `lanka_app` (WP-28; 12 §1–§2, 10 §5, ADR 0023).
--
-- Two planes in one database. The public data plane (`/v2`, roles
-- `lanka_api` / `lanka_analysis`) never sees `app.*`; the app plane
-- (`/app/v1`, role `lanka_app`) reads the public plane and writes only
-- `app.*` (and `ws.*` from WP-32). Grants are re-applied by
-- `app.apply_grants()`, which `lanka db migrate` runs after
-- `ops.apply_grants()` so objects created by later migrations are covered.

CREATE SCHEMA IF NOT EXISTS app;

-- ═════════════════════════════ role ═══════════════════════════════════════
-- Cluster-wide, and several databases (tests) may migrate at once: every
-- statement tolerates a concurrent twin (as 0004).
DO $$
DECLARE
  stmt text;
BEGIN
  FOREACH stmt IN ARRAY ARRAY[
    'CREATE ROLE lanka_app LOGIN',
    'ALTER ROLE lanka_app SET statement_timeout = ''5s''',
    'ALTER ROLE lanka_app SET idle_in_transaction_session_timeout = ''10s'''
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

-- ═════════════════════════════ snapshots (10 §5) ══════════════════════════
-- Anonymous, immutable, content-addressed: the id is 'ms_' || the first 16
-- hex of sha256(RFC 8785 canonical JSON of the document without `id`,
-- `meta.updated_at` and null members) — `lanka_mapdoc::canon::snapshot_id`.
-- Creating the same map twice returns the same link; nothing can be edited
-- after sharing. `doc` is stored with `id` set to the snapshot id.
CREATE TABLE app.map_snapshots (
  id              text PRIMARY KEY CHECK (id ~ '^ms_[0-9a-f]{16}$'),
  doc             jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object' AND doc ->> 'spec' = 'ldl-map/1'),
  doc_sha256      bytea NOT NULL UNIQUE CHECK (octet_length(doc_sha256) = 32),
  -- canonical bytes of the content (the 256 KB document limit)
  bytes           int4 NOT NULL CHECK (bytes > 0 AND bytes <= 262144),
  created_at      timestamptz NOT NULL DEFAULT now(),
  -- NULL = anonymous. The foreign key to app.users arrives with the
  -- accounts migration (WP-31).
  created_by      bigint,
  -- salted (monthly rotated salt) hash of the client address; abuse
  -- control only, never shown
  created_ip_hash bytea CHECK (created_ip_hash IS NULL OR octet_length(created_ip_hash) = 32),
  -- touched at most once a day by reads; anonymous snapshots unread for
  -- 24 months are deleted
  last_read_at    date NOT NULL DEFAULT current_date,
  -- takedown (08 §6): a withdrawn snapshot answers 410 and cannot be re-created
  withdrawn       boolean NOT NULL DEFAULT false,
  CHECK (id = 'ms_' || left(encode(doc_sha256, 'hex'), 16)),
  CHECK (doc ->> 'id' = id)
);

COMMENT ON TABLE app.map_snapshots IS
  'Anonymous, immutable, content-addressed ldl-map/1 documents (10 §5). Written by lanka_app only.';

-- the 24-month expiry sweep (anonymous snapshots only)
CREATE INDEX map_snapshots_expiry ON app.map_snapshots (last_read_at) WHERE created_by IS NULL;
-- abuse review: what one client address created recently
CREATE INDEX map_snapshots_ip ON app.map_snapshots (created_ip_hash, created_at)
  WHERE created_ip_hash IS NOT NULL;

-- ═════════════════════════════ grants (12 §2) ═════════════════════════════
-- Ownership + grants of the app plane, idempotent. `lanka_app`: SELECT on
-- the public plane (catalog, geo, data, serve); SELECT/INSERT/UPDATE/DELETE
-- on app.* only. No other application role has any privilege on `app`.
CREATE FUNCTION app.apply_grants() RETURNS void LANGUAGE plpgsql AS $$
DECLARE
  r        record;
  readable text;
BEGIN
  SELECT string_agg(quote_ident(nspname), ', ' ORDER BY nspname) INTO readable
    FROM pg_namespace WHERE nspname = ANY (ARRAY['catalog', 'geo', 'data', 'serve']);

  -- ownership → lanka_owner (as ops.apply_grants does for the data plane)
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_owner') THEN
    IF (SELECT pg_get_userbyid(nspowner) FROM pg_namespace WHERE nspname = 'app') <> 'lanka_owner' THEN
      ALTER SCHEMA app OWNER TO lanka_owner;
    END IF;
    FOR r IN SELECT c.oid::regclass AS rel, c.relkind
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'app' AND c.relkind IN ('r', 'p', 'v', 'm', 'S')
               AND pg_get_userbyid(c.relowner) <> 'lanka_owner'
               AND NOT (c.relkind = 'S' AND EXISTS (
                     SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid
                       AND d.deptype IN ('a', 'i')))
    LOOP
      EXECUTE format('ALTER %s %s OWNER TO lanka_owner',
                     CASE r.relkind WHEN 'v' THEN 'VIEW' WHEN 'm' THEN 'MATERIALIZED VIEW'
                                    WHEN 'S' THEN 'SEQUENCE' ELSE 'TABLE' END, r.rel);
    END LOOP;
    FOR r IN SELECT p.oid::regprocedure AS fn
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE n.nspname = 'app' AND pg_get_userbyid(p.proowner) <> 'lanka_owner'
               AND p.oid <> 'app.apply_grants()'::regprocedure
    LOOP
      EXECUTE format('ALTER ROUTINE %s OWNER TO lanka_owner', r.fn);
    END LOOP;
  END IF;

  -- nobody but lanka_app (and the owner) may see the app plane
  REVOKE ALL ON SCHEMA app FROM PUBLIC;
  REVOKE ALL ON ALL TABLES IN SCHEMA app FROM PUBLIC;
  FOR r IN SELECT rolname FROM pg_roles
           WHERE rolname IN ('lanka_api', 'lanka_ingest', 'lanka_recipe', 'lanka_analysis') LOOP
    EXECUTE format('REVOKE ALL ON SCHEMA app FROM %I', r.rolname);
    EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA app FROM %I', r.rolname);
    EXECUTE format('REVOKE ALL ON ALL SEQUENCES IN SCHEMA app FROM %I', r.rolname);
  END LOOP;

  -- lanka_app: writes app.*, reads the public plane, nothing else
  GRANT USAGE ON SCHEMA app TO lanka_app;
  GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA app TO lanka_app;
  GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA app TO lanka_app;
  IF readable IS NOT NULL THEN
    EXECUTE format('GRANT USAGE ON SCHEMA %s TO lanka_app', readable);
    EXECUTE format('GRANT SELECT ON ALL TABLES IN SCHEMA %s TO lanka_app', readable);
  END IF;

  REVOKE ALL ON FUNCTION app.apply_grants() FROM PUBLIC;
END $$;

SELECT app.apply_grants();
