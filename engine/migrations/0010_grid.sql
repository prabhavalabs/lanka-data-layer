-- 0010_grid.sql — WP-05 grid engine (05 §8, ADR 0013).
--
-- geo.grid_layers (0001) indexes the canonical-grid arrays of the blob store
-- (grids/<layer>/<period>/<sha16>.bin + .tif). This migration adds what the
-- grid engine and the server need: the indicator dimensions a layer carries,
-- how coarser pixels are made (overview), whether `lanka serve` maps it,
-- statistics and a rendering style, the resampling rule, and the side table
-- that turns a lookup layer's u16 slots (gnd_idx) into geo.units keys.

ALTER TABLE geo.grid_layers
  ADD COLUMN dims        text NOT NULL DEFAULT '',
  ADD COLUMN overview    text NOT NULL DEFAULT 'mean' CHECK (overview IN ('mean', 'sum', 'max', 'mode')),
  ADD COLUMN serve       boolean NOT NULL DEFAULT false,
  ADD COLUMN resampling  text,
  ADD COLUMN stats       jsonb,
  ADD COLUMN style       jsonb,
  ADD COLUMN bytes       bigint,
  ADD COLUMN cog_bytes   bigint,
  ADD COLUMN created_at  timestamptz NOT NULL DEFAULT now();

-- Lookup layers (gnd_idx, land_mask) carry no zonal statistic.
ALTER TABLE geo.grid_layers DROP CONSTRAINT grid_layers_zonal_check;
ALTER TABLE geo.grid_layers ADD CONSTRAINT grid_layers_zonal_check
  CHECK (zonal IN ('sum', 'mean', 'pop_weighted_mean', 'class_share', 'max', 'none'));

ALTER TABLE geo.grid_layers ADD CONSTRAINT grid_layers_version_fkey
  FOREIGN KEY (version_id) REFERENCES data.dataset_versions (id);

CREATE INDEX grid_layers_dataset ON geo.grid_layers (dataset_id);
CREATE INDEX grid_layers_indicator ON geo.grid_layers (indicator_id, dims);

-- Slot → unit of a lookup layer: gnd_idx value `idx` (1…65535) is unit
-- `unit_key` of `boundary_set`. Slot 0 means "no unit" and has no row.
CREATE TABLE geo.grid_lookup_units (
  layer_id      text NOT NULL,
  period        text NOT NULL,
  idx           integer NOT NULL CHECK (idx BETWEEN 1 AND 65535),
  unit_key      integer NOT NULL REFERENCES geo.units (key),
  boundary_set  text NOT NULL REFERENCES geo.boundary_sets (id),
  PRIMARY KEY (layer_id, period, idx),
  FOREIGN KEY (layer_id, period) REFERENCES geo.grid_layers (id, period) ON DELETE CASCADE
);

COMMENT ON TABLE geo.grid_lookup_units IS
  'gnd_idx slot → geo.units.key for the boundary set the lookup layer was built from (05 §8)';
