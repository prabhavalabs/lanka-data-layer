-- 0008 — a dataset may return to earlier content (A → B → A).
--
-- The dsv is a function of content only (05 §3: path-independent), so a
-- version that re-delivers the content of an older, superseded version has
-- that version's dsv. It is still a new version: a new seq, new rows
-- (vfrom = seq) that reinstate the old values on the range-versioned model
-- (ADR 0004), its own review/approval and its own release. The old version
-- row stays untouched, so every release keeps resolving to what it served.
--
-- `dsv` therefore names content, not a version row: it is no longer unique
-- across a dataset's history. Two rows with one dsv can never both be open
-- (staged / in_review / approved): that is what review commands address.

ALTER TABLE data.dataset_versions DROP CONSTRAINT IF EXISTS dataset_versions_dsv_key;
CREATE INDEX IF NOT EXISTS dataset_versions_dsv ON data.dataset_versions (dsv);
CREATE UNIQUE INDEX IF NOT EXISTS dataset_versions_dsv_open ON data.dataset_versions (dsv)
  WHERE status IN ('staged', 'in_review', 'approved');
