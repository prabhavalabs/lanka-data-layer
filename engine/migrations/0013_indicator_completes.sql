-- 0013: roll-ups that complete an official indicator (04 §8.1).
--
-- `census24.pop_total` holds what DCS printed "plus computed roll-ups where
-- DCS printed none, marked as such". Roll-ups are produced by recipes into
-- their own derived dataset (an indicator belongs to one dataset, and
-- observations are partitioned by dataset), so the link is declared on the
-- roll-up indicator: `completes = 'census24.pop_total'`. Readers (frame,
-- map, area profile) serve those rows under the completed indicator's id at
-- levels where it has none, with `basis = computed`, the roll-up's obs_id
-- (provenance) and its dataset version in meta.

ALTER TABLE catalog.indicators
  ADD COLUMN completes text REFERENCES catalog.indicators(id)
    ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED;

CREATE INDEX indicators_completes ON catalog.indicators (completes) WHERE completes IS NOT NULL;
