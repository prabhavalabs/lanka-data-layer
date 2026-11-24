-- 0018: the civic record, track C1 (13 §2.1): ministries and the institutions
-- in their charge, over time, from the President's Article 44 gazettes.
--
-- Schema `civic`, owner `lanka_owner`. `lanka_ingest` writes (the parser and
-- `lanka civic build`), `lanka_api` reads. Evidence is the archived gazette in
-- `docs.*`: every portfolio and item carries the page it was read from.
-- Grants are re-applied by `civic.apply_grants()`, which `lanka db migrate`
-- runs after `docs.apply_grants()`.

CREATE SCHEMA IF NOT EXISTS civic;

-- One row per instrument (a gazette that sets out or amends the schedule).
-- kind: baseline (the whole schedule, "determined the number of Ministers"),
-- supplement (subjects and functions for the Ministers just appointed: a run
-- of them makes up a structure), amendment (changes a named gazette).
CREATE TABLE civic.instruments (
  doc_id         text PRIMARY KEY REFERENCES docs.documents (doc_id),
  kind           text NOT NULL CHECK (kind IN ('baseline', 'supplement', 'amendment')),
  effective_from date NOT NULL,
  amends         text REFERENCES civic.instruments (doc_id),  -- the baseline an amendment amends
  head_of_state  text,
  parser         text NOT NULL,
  parsed_at      timestamptz,
  status         text NOT NULL CHECK (status IN ('validated', 'needs_review', 'approved', 'rejected')),
  checks         jsonb NOT NULL DEFAULT '[]'::jsonb           -- [{check, ok, detail}]
);
CREATE INDEX instruments_effective ON civic.instruments (effective_from DESC);
CREATE INDEX instruments_amends ON civic.instruments (amends) WHERE amends IS NOT NULL;

-- One row per minister heading of a baseline.
CREATE TABLE civic.portfolios (
  doc_id        text NOT NULL REFERENCES civic.instruments (doc_id) ON DELETE CASCADE,
  seq           int  NOT NULL,                  -- the heading number, "01"
  title         text NOT NULL,                  -- "Minister of Defence"
  portfolio_key text NOT NULL,                  -- slug of the title without "Minister of": "defence"
  first_page    int,
  PRIMARY KEY (doc_id, seq)
);
CREATE INDEX portfolios_key ON civic.portfolios (portfolio_key);

-- The numbered lines of the three columns.
CREATE TABLE civic.items (
  doc_id     text NOT NULL,
  seq        int  NOT NULL,
  col        text NOT NULL CHECK (col IN ('function', 'institution', 'law')),
  item_no    int  NOT NULL,
  text       text NOT NULL,
  entity_key text,                              -- set for institutions
  page       int,
  PRIMARY KEY (doc_id, seq, col, item_no),
  FOREIGN KEY (doc_id, seq) REFERENCES civic.portfolios (doc_id, seq) ON DELETE CASCADE
);
CREATE INDEX items_entity ON civic.items (entity_key) WHERE entity_key IS NOT NULL;

-- Institutions, one row per normalised name (after aliases).
CREATE TABLE civic.entities (
  entity_key text PRIMARY KEY,
  name       text NOT NULL,
  kind       text NOT NULL CHECK (kind IN
               ('department', 'statutory_body', 'corporation', 'company', 'fund', 'university', 'force', 'other')),
  aliases    text[] NOT NULL DEFAULT '{}'
);

-- Which portfolio an institution sat under, and when (rebuilt by `lanka civic build`).
CREATE TABLE civic.assignments (
  entity_key      text NOT NULL REFERENCES civic.entities (entity_key),
  doc_id          text REFERENCES civic.instruments (doc_id),
  seq             int,
  portfolio_title text,
  portfolio_key   text,
  valid_from      date NOT NULL,
  valid_to        date,                         -- null = still in force
  page            int,
  PRIMARY KEY (entity_key, valid_from),
  CHECK (valid_to IS NULL OR valid_to > valid_from)
);
CREATE INDEX assignments_portfolio ON civic.assignments (portfolio_key, valid_from, valid_to);
CREATE INDEX assignments_validity ON civic.assignments (valid_from, valid_to);
CREATE INDEX assignments_doc ON civic.assignments (doc_id);

-- Ownership + grants, idempotent (same shape as docs.apply_grants()).
CREATE FUNCTION civic.apply_grants() RETURNS void LANGUAGE plpgsql AS $$
DECLARE
  r record;
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_owner') THEN
    IF (SELECT pg_get_userbyid(nspowner) FROM pg_namespace WHERE nspname = 'civic') <> 'lanka_owner' THEN
      ALTER SCHEMA civic OWNER TO lanka_owner;
    END IF;
    FOR r IN SELECT c.oid::regclass AS rel, c.relkind
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'civic' AND c.relkind IN ('r', 'p', 'v', 'S')
               AND pg_get_userbyid(c.relowner) <> 'lanka_owner'
               AND NOT (c.relkind = 'S' AND EXISTS (
                     SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid
                       AND d.deptype IN ('a', 'i')))
    LOOP
      EXECUTE format('ALTER %s %s OWNER TO lanka_owner',
                     CASE r.relkind WHEN 'v' THEN 'VIEW' WHEN 'S' THEN 'SEQUENCE' ELSE 'TABLE' END, r.rel);
    END LOOP;
    FOR r IN SELECT p.oid::regprocedure AS fn
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE n.nspname = 'civic' AND pg_get_userbyid(p.proowner) <> 'lanka_owner'
               AND p.oid <> 'civic.apply_grants()'::regprocedure
    LOOP
      EXECUTE format('ALTER ROUTINE %s OWNER TO lanka_owner', r.fn);
    END LOOP;
  END IF;

  REVOKE ALL ON SCHEMA civic FROM PUBLIC;
  REVOKE ALL ON ALL TABLES IN SCHEMA civic FROM PUBLIC;
  -- the other application roles never see the civic plane
  FOR r IN SELECT rolname FROM pg_roles WHERE rolname IN ('lanka_recipe', 'lanka_app', 'lanka_analysis') LOOP
    EXECUTE format('REVOKE ALL ON SCHEMA civic FROM %I', r.rolname);
    EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA civic FROM %I', r.rolname);
  END LOOP;

  -- lanka_ingest: the parser and the build write rows, never DDL
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_ingest') THEN
    GRANT USAGE ON SCHEMA civic TO lanka_ingest;
    GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA civic TO lanka_ingest;
  END IF;
  -- lanka_api: reads
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_api') THEN
    GRANT USAGE ON SCHEMA civic TO lanka_api;
    GRANT SELECT ON ALL TABLES IN SCHEMA civic TO lanka_api;
  END IF;
  REVOKE ALL ON FUNCTION civic.apply_grants() FROM PUBLIC;
END $$;

SELECT civic.apply_grants();
