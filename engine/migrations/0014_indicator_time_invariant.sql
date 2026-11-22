-- 0014: time-invariant indicators (11 §6, PERIOD_MISMATCH).
--
-- Terrain, static hazard maps and climate normals describe a state that
-- does not move on the time scale of the other variables: elevation dated
-- 2015 is the elevation in 2024. The registry declares such indicators
-- `time_invariant: true`; the analysis engine never blocks on their period
-- gap (it reports the gap as a warn).

ALTER TABLE catalog.indicators ADD COLUMN time_invariant boolean NOT NULL DEFAULT false;
