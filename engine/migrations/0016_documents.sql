-- 0016 — the documents archive (13 §1.1): schema `docs`.
--
-- Collections of official documents (extraordinary gazettes first), one row
-- per document with its upstream identity, an append-only revision history of
-- the upstream record, one file row per language, the full-text index and one
-- row per harvest run. The PDFs and their text are blobs in the store
-- (`raw/`, `text/`); the rows here are small.
--
-- Owner `lanka_owner`; `lanka_ingest` writes, `lanka_api` reads. Grants are
-- re-applied by `docs.apply_grants()`, which `lanka db migrate` runs after
-- `ops.apply_grants()`, `app.apply_grants()` and `analysis.apply_grants()`.

CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE SCHEMA IF NOT EXISTS docs;

CREATE TABLE docs.collections (
  id           text PRIMARY KEY,           -- 'lk-gazette-extraordinary', 'lk-gazette-weekly', 'lk-acts', 'lk-bills'
  title        text NOT NULL,
  publisher    text,
  home_url     text,
  licence      text NOT NULL,              -- 'public-domain-official-text'
  licence_note text,
  harvest      jsonb NOT NULL DEFAULT '{}' -- driver settings (page size, politeness)
);
COMMENT ON TABLE docs.collections IS
  'Document collections and how to harvest them (13 §1.1). harvest: {pageUrl, apiEndpoint, pageSize, fileProxy, docPrefix, …}.';

CREATE TABLE docs.documents (
  doc_id              text PRIMARY KEY,    -- 'gze-2515-04': collection prefix + number
  collection          text NOT NULL REFERENCES docs.collections (id),
  number              text,                -- '2515'
  sub_number          text,                -- '04'
  issued_on           date NOT NULL,
  title               jsonb NOT NULL,      -- {"en": …, "si": …, "ta": …}
  keywords            jsonb,
  upstream_id         text NOT NULL,       -- the site's uuid
  upstream_updated_at timestamptz,
  meta_hash           text NOT NULL,       -- sha256 of the normalised upstream record
  first_seen_at       timestamptz NOT NULL DEFAULT now(),
  last_seen_at        timestamptz NOT NULL DEFAULT now(),
  withdrawn_at        timestamptz,         -- set when a full sweep no longer lists it; cleared if it returns
  UNIQUE (collection, upstream_id)
);
CREATE INDEX documents_collection_issued ON docs.documents (collection, issued_on DESC);
CREATE INDEX documents_withdrawn ON docs.documents (collection) WHERE withdrawn_at IS NOT NULL;
DO $$
DECLARE
  ext_schema text;
BEGIN
  SELECT n.nspname INTO ext_schema
    FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace
   WHERE e.extname = 'pg_trgm';
  IF ext_schema IS NOT NULL THEN
    EXECUTE format(
      'CREATE INDEX documents_title_en_trgm ON docs.documents USING gin ((title ->> ''en'') %I.gin_trgm_ops)',
      ext_schema);
  END IF;
END $$;

CREATE TABLE docs.document_revisions (    -- append-only: every change of the upstream record
  doc_id    text NOT NULL REFERENCES docs.documents (doc_id),
  seen_at   timestamptz NOT NULL,
  meta_hash text NOT NULL,
  record    jsonb NOT NULL,                -- the normalised record that was hashed
  PRIMARY KEY (doc_id, seen_at)
);

CREATE TABLE docs.files (
  doc_id        text NOT NULL REFERENCES docs.documents (doc_id),
  lang          text NOT NULL CHECK (lang IN ('si', 'ta', 'en')),
  upstream_path text NOT NULL,             -- 'extra-gazette-content/G 48538 (E)_1794986733.pdf'
  original_name text,
  status        text NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'ok', 'missing', 'corrupt', 'failed')),
  sha256        text,                      -- the blob lives at raw/<sha256> (lanka-store, Ns::Raw)
  bytes         bigint,
  pages         int,
  fetched_at    timestamptz,
  attempts      int NOT NULL DEFAULT 0,
  last_error    text,
  text_status   text CHECK (text_status IN ('none', 'text', 'ocr', 'empty')),
  text_sha256   text,                      -- text/<sha>.txt: UTF-8 text, pages separated by \f
  note          text,                      -- non-fatal remarks (OCR page cap, text extraction failure)
  PRIMARY KEY (doc_id, lang)
);
CREATE INDEX files_work ON docs.files (status, attempts) WHERE status IN ('pending', 'failed');
CREATE INDEX files_text_todo ON docs.files (doc_id) WHERE status = 'ok' AND text_status IS NOT DISTINCT FROM 'none';

CREATE TABLE docs.search (                -- one row per document and language with text
  doc_id text NOT NULL REFERENCES docs.documents (doc_id),
  lang   text NOT NULL CHECK (lang IN ('si', 'ta', 'en')),
  tsv    tsvector NOT NULL,                -- 'simple' config: Sinhala and Tamil have no stemmer
  PRIMARY KEY (doc_id, lang)
);
CREATE INDEX search_tsv ON docs.search USING gin (tsv);

CREATE TABLE docs.harvests (              -- one row per run, for status and incidents
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  collection  text NOT NULL REFERENCES docs.collections (id),
  started_at  timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,                 -- NULL while running (or after a crash)
  mode        text NOT NULL CHECK (mode IN ('incremental', 'sweep', 'files', 'text')),
  listed      int NOT NULL DEFAULT 0,
  new         int NOT NULL DEFAULT 0,
  changed     int NOT NULL DEFAULT 0,
  withdrawn   int NOT NULL DEFAULT 0,
  files_ok    int NOT NULL DEFAULT 0,
  files_failed int NOT NULL DEFAULT 0,
  error       text
);
CREATE INDEX harvests_collection_started ON docs.harvests (collection, started_at DESC);

INSERT INTO docs.collections (id, title, publisher, home_url, licence, licence_note, harvest) VALUES (
  'lk-gazette-extraordinary',
  'Extraordinary Gazettes of Sri Lanka',
  'Department of Government Printing',
  'https://documents.gov.lk/web/extra_gazettes',
  'public-domain-official-text',
  'Official texts of a legislative, administrative or legal nature are not protected by copyright (Intellectual Property Act No. 36 of 2003, s. 8).',
  jsonb_build_object(
    'pageUrl', 'https://documents.gov.lk/web/extra_gazettes',
    'apiEndpoint', 'http://gvp-api:4500/website-data/extra-gazette/get-all',
    'pageSize', 200,
    'fileProxy', 'https://documents.gov.lk/api/content-file-proxy?file=',
    'docPrefix', 'gze'
  )
);

-- Ownership + grants, idempotent (same shape as analysis.apply_grants()).
CREATE FUNCTION docs.apply_grants() RETURNS void LANGUAGE plpgsql AS $$
DECLARE
  r record;
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_owner') THEN
    IF (SELECT pg_get_userbyid(nspowner) FROM pg_namespace WHERE nspname = 'docs') <> 'lanka_owner' THEN
      ALTER SCHEMA docs OWNER TO lanka_owner;
    END IF;
    FOR r IN SELECT c.oid::regclass AS rel, c.relkind
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'docs' AND c.relkind IN ('r', 'p', 'v', 'S')
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
             WHERE n.nspname = 'docs' AND pg_get_userbyid(p.proowner) <> 'lanka_owner'
               AND p.oid <> 'docs.apply_grants()'::regprocedure
    LOOP
      EXECUTE format('ALTER ROUTINE %s OWNER TO lanka_owner', r.fn);
    END LOOP;
  END IF;

  REVOKE ALL ON SCHEMA docs FROM PUBLIC;
  REVOKE ALL ON ALL TABLES IN SCHEMA docs FROM PUBLIC;
  -- the other application roles never see the documents plane
  FOR r IN SELECT rolname FROM pg_roles WHERE rolname IN ('lanka_recipe', 'lanka_app', 'lanka_analysis') LOOP
    EXECUTE format('REVOKE ALL ON SCHEMA docs FROM %I', r.rolname);
    EXECUTE format('REVOKE ALL ON ALL TABLES IN SCHEMA docs FROM %I', r.rolname);
  END LOOP;

  -- lanka_ingest: the harvester writes rows, never DDL; revisions are append-only
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_ingest') THEN
    GRANT USAGE ON SCHEMA docs TO lanka_ingest;
    GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA docs TO lanka_ingest;
    GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA docs TO lanka_ingest;
    REVOKE UPDATE, DELETE ON docs.document_revisions FROM lanka_ingest;
  END IF;
  -- lanka_api: reads
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'lanka_api') THEN
    GRANT USAGE ON SCHEMA docs TO lanka_api;
    GRANT SELECT ON ALL TABLES IN SCHEMA docs TO lanka_api;
  END IF;
  REVOKE ALL ON FUNCTION docs.apply_grants() FROM PUBLIC;
END $$;

SELECT docs.apply_grants();
