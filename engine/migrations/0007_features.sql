-- 0007 — WP-04: features (points, lines, polygons) on the range-versioned
-- model, and the tileset index (05 §4, §9; ADR 0004, ADR 0014).
--
-- geo.features is rebuilt like data.observations: LIST (dataset_key)
-- partitions, rows immutable except the `vto` close at release, a version
-- = a dataset-local seq. Staging writes only new or changed features
-- (`vfrom = seq`); the rows they replace are listed in data.version_closes
-- (shared with observations: feature ids live in the same
-- `dataset_key << 40 | seq` space, from catalog.datasets.obs_seq) and closed
-- by the release train. A daily OSM refresh therefore writes the features
-- that changed upstream, not the dataset.
--
-- Derived per row by the pipeline (not part of the content hash): `rep`
-- (representative point: the point itself, the midpoint along a line, an
-- interior point of a polygon), `cell_id` (canonical grid cell of `rep`,
-- computed in Rust by lanka-core's grid — the formula is not repeated in SQL) and
-- `unit_key` (deepest admin unit of the reference boundary set containing
-- `rep`; `unit_match` says how it was found).
--
-- The 0001 table never held rows (no loader existed); it is replaced.

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM geo.features) THEN
    RAISE EXCEPTION '0007: geo.features is not empty; migrate its rows first';
  END IF;
END $$;
DROP TABLE geo.features;

CREATE TABLE geo.features (
  feature_id      int8 NOT NULL,              -- dataset_key << 40 | seq
  dataset_key     int4 NOT NULL,
  feature_key     text NOT NULL,              -- upstream key, e.g. 'way/123456789'
  kind            text NOT NULL,
  vfrom           int4 NOT NULL,
  vto             int4 NOT NULL DEFAULT 2147483647,
  name_en         text,
  name_si         text,
  name_ta         text,
  geom            geometry(Geometry, 4326) NOT NULL,
  rep             geometry(Point, 4326) NOT NULL,
  cell_id         int4 NOT NULL,
  unit_key        int4,
  unit_match      int2,                       -- 0 contains, 1 nearest ≤ 2 km, 2 nearest beyond
  attrs           jsonb NOT NULL DEFAULT '{}',
  flags           int4 NOT NULL DEFAULT 0,
  entity_id       text,
  content_sha256  bytea NOT NULL,             -- canonical row hash (diff + dsv multiset)
  artifact_id     int8,
  extract_id      int8,
  extract_row     int4,
  PRIMARY KEY (dataset_key, feature_id),
  CHECK (vto >= vfrom - 1)
) PARTITION BY LIST (dataset_key);
CREATE TABLE geo.features_default PARTITION OF geo.features DEFAULT;

COMMENT ON TABLE geo.features IS
  'Range-versioned features (WP-04): current = vfrom <= released seq AND vto = 2147483647';

-- The only UPDATE features ever see is the vto close at release.
CREATE FUNCTION geo.features_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF (NEW.feature_id, NEW.dataset_key, NEW.feature_key, NEW.kind, NEW.vfrom, NEW.content_sha256)
     IS DISTINCT FROM
     (OLD.feature_id, OLD.dataset_key, OLD.feature_key, OLD.kind, OLD.vfrom, OLD.content_sha256)
  THEN
    RAISE EXCEPTION 'geo.features rows are immutable except vto (feature_id %)', OLD.feature_id;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER features_guard BEFORE UPDATE ON geo.features
  FOR EACH ROW EXECUTE FUNCTION geo.features_guard();

-- One partition per features dataset, with the indexes of 05 §4. Called at
-- registry sync for datasets of kind `features`; idempotent. `searchable`
-- adds the trigram index on name_en (declared in the registry).
CREATE FUNCTION geo.ensure_feature_partition(p_dataset text, p_searchable boolean DEFAULT false)
RETURNS text LANGUAGE plpgsql
SECURITY DEFINER SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
  k    int4;
  part text;
BEGIN
  SELECT key INTO k FROM catalog.datasets WHERE id = p_dataset;
  IF k IS NULL THEN
    RAISE EXCEPTION 'ensure_feature_partition: unknown dataset %', p_dataset;
  END IF;
  part := 'features_' || k;
  IF to_regclass('geo.' || part) IS NULL THEN
    EXECUTE format('CREATE TABLE geo.%I PARTITION OF geo.features FOR VALUES IN (%s)
                    WITH (autovacuum_vacuum_insert_scale_factor = 0.1)', part, k);
    EXECUTE format('CREATE INDEX %I ON geo.%I USING gist (geom) WHERE vto = 2147483647',
                   part || '_geom', part);
    EXECUTE format('CREATE INDEX %I ON geo.%I USING gist (rep) WHERE vto = 2147483647',
                   part || '_rep', part);
    EXECUTE format('CREATE INDEX %I ON geo.%I (kind, unit_key) WHERE vto = 2147483647',
                   part || '_kind_unit', part);
    EXECUTE format('CREATE INDEX %I ON geo.%I (feature_key, vfrom) WHERE vto = 2147483647',
                   part || '_key', part);
    EXECUTE format('COMMENT ON TABLE geo.%I IS %L', part, 'features of dataset ' || p_dataset);
  END IF;
  IF p_searchable AND to_regclass('geo.' || part || '_name') IS NULL THEN
    EXECUTE format('CREATE INDEX %I ON geo.%I USING gin (name_en gin_trgm_ops) WHERE vto = 2147483647',
                   part || '_name', part);
  END IF;
  RETURN part;
END $$;
REVOKE ALL ON FUNCTION geo.ensure_feature_partition(text, boolean) FROM PUBLIC;

-- Vector tilesets (05 §9): one row per build; the newest build of a tileset
-- is the one served. Files are content-named in the store:
-- tiles/<id>/<sha16>.pmtiles.
CREATE TABLE geo.tilesets (
  id           text NOT NULL,
  sha16        text NOT NULL CHECK (sha16 ~ '^[0-9a-f]{16}$'),
  path         text NOT NULL,                 -- relative to the store root
  bytes        int8 NOT NULL,
  minzoom      int2 NOT NULL,
  maxzoom      int2 NOT NULL,
  bounds       float8[],                      -- [west, south, east, north]
  layers       jsonb NOT NULL,                -- [{id, dataset, fields, minzoom, maxzoom, features}]
  dataset_ids  text[] NOT NULL DEFAULT '{}',
  version_ids  int8[] NOT NULL DEFAULT '{}',
  input_hash   bytea NOT NULL,                -- spec + dataset versions + tool version
  tool         text NOT NULL,                 -- e.g. 'tippecanoe v2.79.0'
  attribution  text,
  build_ms     int8,
  built_at     timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (id, sha16)
);
CREATE INDEX tilesets_latest ON geo.tilesets (id, built_at DESC);

CREATE VIEW geo.current_tilesets AS
  SELECT DISTINCT ON (id) * FROM geo.tilesets ORDER BY id, built_at DESC, sha16;

SELECT ops.apply_grants();
GRANT EXECUTE ON FUNCTION geo.ensure_feature_partition(text, boolean) TO lanka_ingest;
