-- 0017 — `lanka docs harvest --mode gaps` (13 §1.2): search every gazette
-- number to store what offset paging skipped. The harvest log accepts it.

ALTER TABLE docs.harvests DROP CONSTRAINT harvests_mode_check;
ALTER TABLE docs.harvests ADD CONSTRAINT harvests_mode_check
  CHECK (mode IN ('incremental', 'sweep', 'files', 'text', 'gaps'));
