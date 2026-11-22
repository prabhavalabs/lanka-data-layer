-- 0004 — WP-03: unattended operation and the emit-only monitoring surface.
--
-- 1. ops.jobs — the Postgres job queue of `lanka worker` (07 §3, ADR 0011):
--    FOR UPDATE SKIP LOCKED claims per lane, priorities, slot weights,
--    session advisory lock per running job (crash detection), heartbeats,
--    retries with backoff, dedupe while queued/running, cancellation.
-- 2. Release trains (03 §8, ADR 0012): approved versions wait for the next
--    train; ops.trains records every train; data.releases.train_id.
-- 3. Review records (03 §6): ops.reviews; versions gain approved_at.
-- 4. Scheduling inputs synced from the registry (03 §4): paused,
--    schedulable, check_every, stale_after, window, ops.release_calendar;
--    ops.source_freshness computes every source's state.
-- 5. Emit-only monitoring (07 §4, ADR 0016): ops.incidents, ops.workers,
--    ops.backups, and ops.run_events generalised into the event feed
--    (strictly increasing ids in commit order via ops.emit). ops.alerts and
--    the webhook/heartbeat delivery state are dropped.
-- 6. Roles and grants (05 §7): lanka_owner owns everything, lanka_ingest
--    (worker), lanka_api (read-only server), lanka_recipe (read-only
--    recipes). ops.apply_grants() is re-run by every `lanka db migrate`.

-- ═════════════════════════════ alerts removed ══════════════════════════════
DROP TABLE ops.alerts;

-- ═════════════════════════════ scheduling inputs ═══════════════════════════
ALTER TABLE catalog.sources
  ADD COLUMN paused       boolean NOT NULL DEFAULT false,
  ADD COLUMN schedulable  boolean NOT NULL DEFAULT false,   -- an implemented connector exists (registry sync)
  ADD COLUMN check_every  interval,                         -- schedule.check_every; NULL = default by cadence
  ADD COLUMN stale_after  interval,                         -- schedule.stale_after; NULL = 2 × cadence
  ADD COLUMN check_window text CHECK (check_window ~ '^[0-2][0-9]:[0-5][0-9]-[0-2][0-9]:[0-5][0-9]$');

-- Expected upstream editions (registry `schedule.release_calendar`).
CREATE TABLE ops.release_calendar (
  source_id  text NOT NULL REFERENCES catalog.sources (id) ON DELETE CASCADE,
  edition    text NOT NULL,
  expected   date NOT NULL,
  PRIMARY KEY (source_id, edition)
);

-- How often `discover` runs by default (03 §4).
CREATE FUNCTION ops.default_check_every(cadence text) RETURNS interval
LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE cadence
    WHEN 'realtime' THEN interval '15 minutes' WHEN 'daily' THEN interval '6 hours'
    WHEN 'weekly' THEN interval '1 day'        WHEN 'monthly' THEN interval '1 day'
    WHEN 'quarterly' THEN interval '3 days'    WHEN 'annual' THEN interval '7 days'
    WHEN 'irregular' THEN interval '7 days'    ELSE NULL END   -- one_off: manual (or first run)
$$;

-- The publication cadence itself (overdue = 2 × this by default).
CREATE FUNCTION ops.cadence_interval(cadence text) RETURNS interval
LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE cadence
    WHEN 'realtime' THEN interval '1 hour'     WHEN 'daily' THEN interval '1 day'
    WHEN 'weekly' THEN interval '7 days'       WHEN 'monthly' THEN interval '1 month'
    WHEN 'quarterly' THEN interval '3 months'  WHEN 'annual' THEN interval '1 year'
    ELSE NULL END
$$;

-- First instant at or after `at` inside a local-time politeness window
-- ('01:00-05:00', may wrap midnight); `at` itself when there is no window.
CREATE FUNCTION ops.window_start(p_window text, p_tz text, p_at timestamptz) RETURNS timestamptz
LANGUAGE plpgsql STABLE AS $$
DECLARE
  s time; e time; loc timestamp; d date;
BEGIN
  IF p_window IS NULL THEN
    RETURN p_at;
  END IF;
  s := split_part(p_window, '-', 1)::time;
  e := split_part(p_window, '-', 2)::time;
  loc := p_at AT TIME ZONE p_tz;
  IF s <= e THEN
    IF loc::time >= s AND loc::time < e THEN RETURN p_at; END IF;
    d := loc::date + CASE WHEN loc::time >= e THEN 1 ELSE 0 END;
  ELSE
    IF loc::time >= s OR loc::time < e THEN RETURN p_at; END IF;
    d := loc::date;
  END IF;
  RETURN (d + s) AT TIME ZONE p_tz;
END $$;

-- ═════════════════════════════ job queue ═══════════════════════════════════
CREATE TABLE ops.jobs (
  id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  kind              text NOT NULL CHECK (kind IN ('ingest', 'backfill', 'restandardise', 'recipe', 'release',
                      'export', 'tiles', 'check', 'maintenance', 'analysis', 'upload', 'sync')),
  lane              text NOT NULL DEFAULT 'batch' CHECK (lane IN ('batch', 'interactive')),
  dedupe_key        text NOT NULL,             -- 'ingest:worldbank-wdi', 'release:daily:20261122', …
  payload           jsonb NOT NULL DEFAULT '{}',
  state             text NOT NULL DEFAULT 'queued'
                      CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  priority          int2 NOT NULL DEFAULT 100, -- lower first: release 10, ingest 50, export 100, recipe 150, backfill 200
  weight            int2 NOT NULL DEFAULT 1 CHECK (weight BETWEEN 1 AND 16),  -- slots consumed
  run_after         timestamptz NOT NULL DEFAULT now(),
  attempts          int2 NOT NULL DEFAULT 0,
  max_attempts      int2 NOT NULL DEFAULT 3 CHECK (max_attempts >= 1),
  locked_by         text,                      -- worker id
  locked_at         timestamptz,
  heartbeat_at      timestamptz,
  lease_until       timestamptz,               -- heartbeat + lease; the advisory lock is authoritative
  cancel_requested  boolean NOT NULL DEFAULT false,
  trigger           text NOT NULL CHECK (trigger IN ('schedule', 'manual', 'retry', 'backfill', 'train', 'upstream_change')),
  created_at        timestamptz NOT NULL DEFAULT now(),
  started_at        timestamptz,
  finished_at       timestamptz,
  result            jsonb,
  error             text,
  CHECK (state <> 'running' OR locked_by IS NOT NULL)
);
-- Idempotent enqueue: one live job per dedupe key.
CREATE UNIQUE INDEX jobs_dedupe ON ops.jobs (dedupe_key) WHERE state IN ('queued', 'running');
CREATE INDEX jobs_claim ON ops.jobs (lane, priority, run_after, id) WHERE state = 'queued';
CREATE INDEX jobs_running ON ops.jobs (locked_by) WHERE state = 'running';
CREATE INDEX jobs_kind_created ON ops.jobs (kind, created_at DESC);

-- Wake workers on enqueue / re-queue (the 60 s tick is the fallback).
CREATE FUNCTION ops.notify_jobs() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_notify('lanka_jobs', NEW.lane);
  RETURN NEW;
END $$;
CREATE TRIGGER jobs_notify AFTER INSERT OR UPDATE OF state ON ops.jobs
  FOR EACH ROW WHEN (NEW.state = 'queued') EXECUTE FUNCTION ops.notify_jobs();

ALTER TABLE ops.ingestion_runs ADD COLUMN job_id bigint REFERENCES ops.jobs (id) ON DELETE SET NULL;
CREATE INDEX runs_job ON ops.ingestion_runs (job_id) WHERE job_id IS NOT NULL;
CREATE INDEX runs_started ON ops.ingestion_runs (started_at DESC);

-- One row per worker process (leader or standby), updated every tick.
CREATE TABLE ops.workers (
  id              text PRIMARY KEY,            -- host:pid:started
  lane            text NOT NULL CHECK (lane IN ('batch', 'interactive')),
  role            text NOT NULL CHECK (role IN ('leader', 'standby', 'stopped')),
  host            text,
  pid             int4,
  engine_version  text NOT NULL,
  git_rev         text,
  slots           int2 NOT NULL,
  started_at      timestamptz NOT NULL DEFAULT now(),
  last_tick_at    timestamptz,
  stopped_at      timestamptz,
  store_pct       real,                        -- blob store filesystem usage at the last tick
  detail          jsonb
);
CREATE INDEX workers_tick ON ops.workers (lane, last_tick_at DESC);

-- ═════════════════════════════ reviews and trains ══════════════════════════
ALTER TABLE data.dataset_versions ADD COLUMN approved_at timestamptz;
UPDATE data.dataset_versions SET approved_at = coalesce(reviewed_at, published_at, created_at)
  WHERE status IN ('approved', 'published', 'superseded');
CREATE INDEX dataset_versions_approved ON data.dataset_versions (approved_at) WHERE status = 'approved';

-- 03 §6. One open review per version; the decision history stays.
CREATE TABLE ops.reviews (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  version_id    bigint NOT NULL REFERENCES data.dataset_versions (id) ON DELETE CASCADE,
  state         text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'approved', 'rejected', 'changes_requested')),
  opened_at     timestamptz NOT NULL DEFAULT now(),
  reason        text,                         -- why the version needs a person (class, gate failure)
  reviewer      text,
  decided_at    timestamptz,
  note          text,
  evidence_key  text,                         -- store/evidence/<dsv>/index.html (WP-08)
  checklist     jsonb NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX reviews_one_open ON ops.reviews (version_id) WHERE state = 'open';
INSERT INTO ops.reviews (version_id, state, opened_at, reason)
  SELECT id, 'open', created_at, 'class ' || publication_class::text || ' requires human review'
  FROM data.dataset_versions WHERE status = 'in_review';
INSERT INTO ops.reviews (version_id, state, opened_at, reason, reviewer, decided_at, note)
  SELECT id, 'approved', created_at, 'class ' || publication_class::text || ' requires human review',
         reviewed_by, reviewed_at, review_note
  FROM data.dataset_versions WHERE reviewed_by IS NOT NULL AND status IN ('approved', 'published', 'superseded');

-- 03 §8. Every train, including the ones that found nothing to release.
CREATE TABLE ops.trains (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  job_id        bigint REFERENCES ops.jobs (id) ON DELETE SET NULL,
  trigger       text NOT NULL CHECK (trigger IN ('schedule', 'manual', 'live', 'test')),
  started_at    timestamptz NOT NULL DEFAULT now(),
  finished_at   timestamptz,
  outcome       text NOT NULL DEFAULT 'running' CHECK (outcome IN ('running', 'released', 'no_change', 'failed')),
  data_version  text REFERENCES data.releases (data_version),
  versions      int4 NOT NULL DEFAULT 0,      -- dataset versions released by this train
  recipes       jsonb,                        -- derived recompute report (placeholder until WP-06)
  error         text,
  detail        jsonb
);
CREATE INDEX trains_started ON ops.trains (started_at DESC);
ALTER TABLE data.releases ADD COLUMN train_id bigint REFERENCES ops.trains (id);

-- ═════════════════════════════ monitoring ══════════════════════════════════
-- Persistent conditions, opened and closed automatically by the worker.
CREATE TABLE ops.incidents (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  kind          text NOT NULL,   -- source_failing | expected_missing | train_failed | job_failed | disk_pressure |
                                 -- backup_stale | default_partition_rows | template_miss | url_pattern_broken | …
  subject       text NOT NULL,   -- source / dataset / job dedupe key
  opened_at     timestamptz NOT NULL DEFAULT now(),
  last_seen_at  timestamptz NOT NULL DEFAULT now(),
  closed_at     timestamptz,
  detail        jsonb
);
CREATE UNIQUE INDEX incidents_open ON ops.incidents (kind, subject) WHERE closed_at IS NULL;
CREATE INDEX incidents_opened ON ops.incidents (opened_at DESC);

-- `lanka ops record-backup` (backup and restore-drill scripts end with it).
CREATE TABLE ops.backups (
  id      bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  kind    text NOT NULL CHECK (kind IN ('pg', 'store', 'drill')),
  at      timestamptz NOT NULL DEFAULT now(),
  ok      boolean NOT NULL,
  detail  jsonb
);
CREATE INDEX backups_kind_at ON ops.backups (kind, at DESC);

-- The event feed: run events plus job, train, review, incident and worker
-- events. Ids are strictly increasing in commit order because every insert
-- goes through ops.emit, which serialises on a transaction advisory lock
-- (callers emit in autocommit statements), so a poller keeps one integer.
UPDATE ops.run_events SET stage = 'run' WHERE stage = 'alert';
ALTER TABLE ops.run_events
  ALTER COLUMN run_id DROP NOT NULL,
  ADD COLUMN kind        text,
  ADD COLUMN source_id   text,
  ADD COLUMN dataset_id  text,
  ADD COLUMN job_id      bigint,
  DROP CONSTRAINT run_events_stage_check,
  ADD CONSTRAINT run_events_stage_check CHECK (stage IN (
    'discover', 'fetch', 'archive', 'extract', 'standardise', 'parse', 'validate', 'stage', 'publish',
    'run', 'recipe', 'release', 'export', 'job', 'review', 'incident', 'worker', 'schedule'));
UPDATE ops.run_events e SET source_id = r.source_id, kind = e.stage
  FROM ops.ingestion_runs r WHERE r.id = e.run_id;
UPDATE ops.run_events SET kind = stage WHERE kind IS NULL;
ALTER TABLE ops.run_events ALTER COLUMN kind SET NOT NULL;
CREATE INDEX run_events_source ON ops.run_events (source_id, id);
COMMENT ON TABLE ops.run_events IS 'append-only event feed (/v2/status/events); insert through ops.emit only';

CREATE FUNCTION ops.emit(p_level ops.event_level, p_stage text, p_kind text, p_message text,
                         p_data jsonb DEFAULT NULL, p_run_id bigint DEFAULT NULL, p_job_id bigint DEFAULT NULL,
                         p_source_id text DEFAULT NULL, p_dataset_id text DEFAULT NULL)
RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE
  new_id bigint;
BEGIN
  -- Held to commit: ids are handed out in the order events become visible.
  PERFORM pg_advisory_xact_lock(hashtext('lanka-events'));
  INSERT INTO ops.run_events (run_id, level, stage, kind, message, data, job_id, source_id, dataset_id)
  VALUES (p_run_id, p_level, p_stage, coalesce(p_kind, p_stage), p_message, p_data, p_job_id,
          coalesce(p_source_id, (SELECT source_id FROM ops.ingestion_runs WHERE id = p_run_id)), p_dataset_id)
  RETURNING id INTO new_id;
  RETURN new_id;
END $$;

-- ═════════════════════════════ source freshness ════════════════════════════
DROP VIEW ops.source_health;

-- Every source's state (03 §4): fresh | due | overdue | failing | in_review |
-- expected_missing | never_run | paused | retired, plus when it is next due.
-- A plain view: ~200 sources, read by the scheduler tick and /v2/status.
CREATE VIEW ops.source_freshness AS
WITH runs AS (
  SELECT id, source_id, status, started_at, finished_at, error FROM ops.ingestion_runs
  WHERE trigger <> 'restandardise'
), last AS (
  SELECT DISTINCT ON (source_id) source_id, id AS last_run_id, status::text AS last_status,
         started_at AS last_run_at, finished_at AS last_finished_at, error AS last_error
  FROM runs ORDER BY source_id, id DESC
), ok AS (
  SELECT source_id, max(finished_at) AS last_success_at
  FROM runs WHERE status IN ('succeeded', 'no_change', 'needs_review') GROUP BY source_id
), produced AS (   -- runs that found new content
  SELECT source_id, max(started_at) AS last_new_at
  FROM runs WHERE status IN ('succeeded', 'needs_review') GROUP BY source_id
), fails AS (
  SELECT r.source_id, count(*) AS n
  FROM runs r LEFT JOIN ok USING (source_id)
  WHERE r.status = 'failed' AND (ok.last_success_at IS NULL OR r.started_at > ok.last_success_at)
  GROUP BY r.source_id
), review AS (
  SELECT DISTINCT ds.source_id
  FROM data.dataset_versions v JOIN catalog.dataset_sources ds ON ds.dataset_id = v.dataset_id
  WHERE v.status = 'in_review'
), cal AS (        -- the earliest expected edition not yet followed by new content
  SELECT DISTINCT ON (c.source_id) c.source_id, c.edition, c.expected
  FROM ops.release_calendar c LEFT JOIN produced p USING (source_id)
  WHERE p.last_new_at IS NULL OR p.last_new_at < c.expected
  ORDER BY c.source_id, c.expected
), base AS (
  SELECT s.id AS source_id, s.cadence, s.status AS registry_status, s.priority, s.paused, s.schedulable,
         coalesce(s.check_every, ops.default_check_every(s.cadence)) AS check_every,
         coalesce(s.stale_after, 2 * ops.cadence_interval(s.cadence)) AS stale_after,
         s.check_window, last.last_run_id, last.last_status, last.last_run_at, last.last_finished_at,
         last.last_error, ok.last_success_at, coalesce(fails.n, 0) AS consecutive_failures,
         cal.edition AS expected_edition, cal.expected AS expected_date,
         (review.source_id IS NOT NULL) AS in_review
  FROM catalog.sources s
  LEFT JOIN last ON last.source_id = s.id
  LEFT JOIN ok ON ok.source_id = s.id
  LEFT JOIN fails ON fails.source_id = s.id
  LEFT JOIN review ON review.source_id = s.id
  LEFT JOIN cal ON cal.source_id = s.id
), timed AS (
  SELECT b.*,
         CASE
           WHEN b.registry_status IN ('retired', 'blocked') OR b.paused THEN NULL
           -- a calendar date that has been reached since the last check makes the source due now
           WHEN b.expected_date IS NOT NULL AND b.expected_date <= current_date
                AND (b.last_run_at IS NULL OR b.last_run_at < b.expected_date) THEN b.expected_date::timestamptz
           WHEN b.last_run_at IS NULL THEN now()
           WHEN b.check_every IS NULL THEN   -- one_off: until it has succeeded once, daily
             CASE WHEN b.last_success_at IS NULL THEN b.last_run_at + interval '1 day' END
           ELSE b.last_run_at + b.check_every
         END AS next_check_at
  FROM base b
)
SELECT t.*,
       CASE
         WHEN t.registry_status = 'retired' THEN 'retired'
         WHEN t.paused OR t.registry_status = 'blocked' THEN 'paused'
         WHEN t.consecutive_failures >= 3 THEN 'failing'
         WHEN t.in_review THEN 'in_review'
         WHEN t.expected_date IS NOT NULL AND t.expected_date + 3 < current_date THEN 'expected_missing'
         WHEN t.last_success_at IS NULL THEN 'never_run'
         WHEN t.stale_after IS NOT NULL AND t.last_success_at + t.stale_after < now() THEN 'overdue'
         WHEN t.next_check_at IS NOT NULL AND t.next_check_at <= now() THEN 'due'
         ELSE 'fresh'
       END AS state
FROM timed t;

-- ═════════════════════════════ roles (05 §7) ═══════════════════════════════
-- Roles are cluster-wide and several databases (tests) may migrate at once:
-- every statement tolerates a concurrent twin.
DO $$
DECLARE
  stmt text;
BEGIN
  FOREACH stmt IN ARRAY ARRAY[
    'CREATE ROLE lanka_owner NOLOGIN',
    'CREATE ROLE lanka_ingest LOGIN',
    'CREATE ROLE lanka_api LOGIN',
    'CREATE ROLE lanka_recipe LOGIN',
    'ALTER ROLE lanka_api SET default_transaction_read_only = on',
    'ALTER ROLE lanka_api SET statement_timeout = ''5s''',
    'ALTER ROLE lanka_api SET idle_in_transaction_session_timeout = ''10s''',
    'ALTER ROLE lanka_api SET plan_cache_mode = force_custom_plan',
    'ALTER ROLE lanka_recipe SET default_transaction_read_only = on',
    'ALTER ROLE lanka_recipe SET statement_timeout = ''30min''',
    'ALTER ROLE lanka_ingest SET statement_timeout = ''15min''',
    -- ANALYZE of partitions the worker loads (they are owned by lanka_owner)
    'GRANT pg_maintain TO lanka_ingest'
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

-- Partitions are created at runtime by the worker: the function runs as the
-- owner of data.observations (lanka_owner after apply_grants).
ALTER FUNCTION data.ensure_obs_partition(text) SECURITY DEFINER SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION data.obs_partition_indexes(text) SET search_path = pg_catalog, pg_temp;

-- Ownership + grants, idempotent; `lanka db migrate` runs it after every
-- migration batch so objects created by later migrations are covered.
CREATE FUNCTION ops.apply_grants() RETURNS void LANGUAGE plpgsql AS $$
DECLARE
  r        record;
  owned    text[] := ARRAY['catalog', 'geo', 'data', 'ops', 'serve', 'stage'];
  present  text;
  readable text;
BEGIN
  SELECT string_agg(quote_ident(nspname), ', ' ORDER BY nspname) INTO present
    FROM pg_namespace WHERE nspname = ANY (owned);
  SELECT string_agg(quote_ident(nspname), ', ' ORDER BY nspname) INTO readable
    FROM pg_namespace WHERE nspname = ANY (ARRAY['catalog', 'geo', 'data', 'serve']);

  -- ownership → lanka_owner
  FOR r IN SELECT nspname FROM pg_namespace
           WHERE nspname = ANY (owned) AND pg_get_userbyid(nspowner) <> 'lanka_owner' LOOP
    EXECUTE format('ALTER SCHEMA %I OWNER TO lanka_owner', r.nspname);
  END LOOP;
  FOR r IN SELECT c.oid::regclass AS rel, c.relkind
           FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
           WHERE n.nspname = ANY (owned) AND c.relkind IN ('r', 'p', 'v', 'm', 'S')
             AND pg_get_userbyid(c.relowner) <> 'lanka_owner'
             -- identity/serial sequences follow their table
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
           WHERE n.nspname = ANY (owned) AND pg_get_userbyid(p.proowner) <> 'lanka_owner'
             AND p.oid <> 'ops.apply_grants()'::regprocedure
  LOOP
    EXECUTE format('ALTER ROUTINE %s OWNER TO lanka_owner', r.fn);
  END LOOP;
  FOR r IN SELECT t.oid::regtype AS ty
           FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
           WHERE n.nspname = ANY (owned) AND t.typtype = 'e' AND pg_get_userbyid(t.typowner) <> 'lanka_owner'
  LOOP
    EXECUTE format('ALTER TYPE %s OWNER TO lanka_owner', r.ty);
  END LOOP;

  -- schemas
  EXECUTE format('GRANT USAGE ON SCHEMA %s TO lanka_ingest', present);
  EXECUTE format('GRANT USAGE ON SCHEMA %s TO lanka_api, lanka_recipe', readable || ', ops');

  -- lanka_api / lanka_recipe: read the data plane; the server also reads the status tables
  EXECUTE format('GRANT SELECT ON ALL TABLES IN SCHEMA %s TO lanka_api, lanka_recipe', readable);
  GRANT SELECT ON ops.ingestion_runs, ops.run_events, ops.incidents, ops.jobs, ops.trains, ops.workers,
                  ops.backups, ops.release_calendar, ops.source_freshness, ops.reviews, ops.artifacts,
                  ops.extracts, ops.extract_artifacts TO lanka_api;
  IF to_regclass('public._sqlx_migrations') IS NOT NULL THEN
    GRANT SELECT ON public._sqlx_migrations TO lanka_api;
  END IF;

  -- lanka_ingest: read/write rows, never DDL; releases are append-only
  EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA %s TO lanka_ingest', present);
  EXECUTE format('GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA %s TO lanka_ingest', present);
  REVOKE UPDATE, DELETE ON data.releases, data.release_datasets, data.changelog FROM lanka_ingest;
  REVOKE INSERT, UPDATE, DELETE ON ops.run_events FROM lanka_ingest;   -- through ops.emit only
  GRANT EXECUTE ON FUNCTION ops.emit(ops.event_level, text, text, text, jsonb, bigint, bigint, text, text)
    TO lanka_ingest;

  -- functions: the partition factory and the event writer are the only privileged entry points
  REVOKE ALL ON FUNCTION data.ensure_obs_partition(text) FROM PUBLIC;
  GRANT EXECUTE ON FUNCTION data.ensure_obs_partition(text) TO lanka_ingest;
  REVOKE ALL ON FUNCTION ops.apply_grants() FROM PUBLIC;
END $$;

-- ops.emit runs as the owner so lanka_ingest cannot write events around it.
ALTER FUNCTION ops.emit(ops.event_level, text, text, text, jsonb, bigint, bigint, text, text)
  SECURITY DEFINER SET search_path = pg_catalog, pg_temp;

SELECT ops.apply_grants();
