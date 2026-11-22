-- 0005 — WP-03: serving projections (05 §5).
--
-- serve.area_latest: the latest released value of every (geography,
-- indicator, dims), so an area profile is one primary-key range scan instead
-- of one index probe per (partition, indicator). A plain table maintained by
-- the release train for the datasets it changes (delete by dataset, insert
-- from the released rows); disposable — `lanka serve rebuild` regenerates it.
-- serve.indicator_coverage and serve.search come with WP-13.

CREATE SCHEMA IF NOT EXISTS serve;

CREATE TABLE serve.area_latest (
  geo_key        int4 NOT NULL,
  indicator_key  int4 NOT NULL,
  dims_key       int4 NOT NULL,
  time_start     date NOT NULL,
  freq           "char" NOT NULL,
  value          float8,
  lo             float8,
  hi             float8,
  basis          int2 NOT NULL,
  status         int2 NOT NULL,
  flags          int4 NOT NULL,
  obs_id         int8 NOT NULL,
  dataset_key    int4 NOT NULL,
  PRIMARY KEY (geo_key, indicator_key, dims_key)
);
CREATE INDEX area_latest_ds ON serve.area_latest (dataset_key);   -- delete-by-dataset at release

-- Replaces a dataset's rows with the latest period of each (geo, indicator,
-- dims) among the rows of released seq `p_seq`. Returns the rows written.
CREATE FUNCTION serve.refresh_area_latest(p_dataset_key int4, p_seq int4) RETURNS bigint
LANGUAGE plpgsql AS $$
DECLARE
  n bigint;
BEGIN
  DELETE FROM serve.area_latest WHERE dataset_key = p_dataset_key;
  IF p_seq IS NULL THEN
    RETURN 0;
  END IF;
  INSERT INTO serve.area_latest (geo_key, indicator_key, dims_key, time_start, freq, value, lo, hi,
                                 basis, status, flags, obs_id, dataset_key)
  SELECT DISTINCT ON (o.geo_key, o.indicator_key, o.dims_key)
         o.geo_key, o.indicator_key, o.dims_key, o.time_start, o.freq, o.value, o.lo, o.hi,
         o.basis, o.status, o.flags, o.obs_id, o.dataset_key
  FROM data.observations o
  WHERE o.dataset_key = p_dataset_key AND o.vfrom <= p_seq AND o.vto >= p_seq
  ORDER BY o.geo_key, o.indicator_key, o.dims_key, o.time_start DESC, o.obs_id DESC;
  GET DIAGNOSTICS n = ROW_COUNT;
  RETURN n;
END $$;

-- Initial fill from the current release.
SELECT serve.refresh_area_latest(dataset_key, seq) FROM data.current_versions;
ANALYZE serve.area_latest;

SELECT ops.apply_grants();
