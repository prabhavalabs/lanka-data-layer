-- 0006_enrichment.sql — WP-06: the enrichment core (04 §7, ADR 0010).
--
-- catalog.methods / recipes / recipe_inputs mirror the method library and
-- registry/recipes/ (synced like every other registry object); a recipe
-- run's lineage is one data.derivations row per (recipe, fingerprint), and
-- data.version_inputs is the normalised "what depends on this version?"
-- index.
--
-- Deviation from 04 §7: data.derivations has its own id and version_id is
-- not unique. A re-run whose output equals the published version (an input
-- changed in a way the output does not see) records its fingerprint against
-- the existing version (outcome 'unchanged'), so the next train reuses it
-- instead of recomputing forever.

CREATE TABLE catalog.methods (
  id          text NOT NULL,
  version     text NOT NULL,
  info        jsonb NOT NULL,
  synced_at   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (id, version)
);

CREATE TABLE catalog.recipes (
  id              text PRIMARY KEY,
  output_dataset  text NOT NULL UNIQUE REFERENCES catalog.datasets (id),
  stability       text NOT NULL CHECK (stability IN ('experimental', 'stable')),
  schedule        text NOT NULL DEFAULT 'daily' CHECK (schedule IN ('daily', 'weekly', 'monthly', 'manual')),
  spec            jsonb NOT NULL,
  spec_sha256     bytea NOT NULL,
  synced_at       timestamptz NOT NULL DEFAULT now()
);

-- The global DAG's edges.
CREATE TABLE catalog.recipe_inputs (
  recipe_id   text NOT NULL REFERENCES catalog.recipes (id) ON DELETE CASCADE,
  port        text NOT NULL,
  dataset_id  text NOT NULL REFERENCES catalog.datasets (id),
  optional    boolean NOT NULL DEFAULT false,
  PRIMARY KEY (recipe_id, port)
);
CREATE INDEX recipe_inputs_dataset ON catalog.recipe_inputs (dataset_id);

CREATE TABLE data.derivations (
  id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  version_id      bigint NOT NULL REFERENCES data.dataset_versions (id),
  recipe_id       text NOT NULL REFERENCES catalog.recipes (id),
  fingerprint     bytea NOT NULL,
  outcome         text NOT NULL CHECK (outcome IN ('new', 'unchanged')),
  methods         jsonb NOT NULL,     -- [{step, id, version}]
  params          jsonb NOT NULL,     -- the recipe spec as executed (steps, with, emit, qa)
  inputs          jsonb NOT NULL,     -- [{port, dataset_id, dataset_key, version_id, seq, dsv, kind, indicators}]
  tools           jsonb NOT NULL,     -- {"gdal": "3.11.x"}
  git_rev         text NOT NULL,
  engine_version  text NOT NULL,
  report          jsonb NOT NULL,     -- steps, QA results, diagnostics, gate decision
  job_id          bigint,
  train_id        bigint,
  started_at      timestamptz,
  finished_at     timestamptz
);
CREATE UNIQUE INDEX derivations_fp ON data.derivations (recipe_id, fingerprint);
CREATE INDEX derivations_version ON data.derivations (version_id, id DESC);

CREATE TABLE data.version_inputs (
  version_id        bigint NOT NULL REFERENCES data.dataset_versions (id),
  input_version_id  bigint NOT NULL REFERENCES data.dataset_versions (id),
  port              text NOT NULL,
  PRIMARY KEY (version_id, input_version_id, port)
);
CREATE INDEX version_inputs_input ON data.version_inputs (input_version_id);

SELECT ops.apply_grants();
