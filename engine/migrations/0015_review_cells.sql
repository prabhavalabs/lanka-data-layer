-- 0015 — WP-08: evidence-based review (03 §3.3, §6).
--
-- 1. A review is about a dataset version (class B/C versions, gate
--    demotions — unchanged) or about an extract (class C: an operator-run,
--    LLM-assisted reading must be signed cell by cell before standardise
--    may use it). Exactly one of the two.
-- 2. ops.review_cells: per-cell sign-off (class C cells below confidence
--    1.0 plus a sample; class B spot checks). `correct` carries the text
--    as printed; the original reading is never edited.
-- 3. ops.extracts gains the reader that produced it, the parent of a
--    corrected (manual) extract, and the extract-level approval.

ALTER TABLE ops.reviews
  ALTER COLUMN version_id DROP NOT NULL,
  ADD COLUMN extract_id bigint REFERENCES ops.extracts (id) ON DELETE CASCADE,
  ADD CONSTRAINT reviews_subject CHECK ((version_id IS NULL) <> (extract_id IS NULL));
CREATE UNIQUE INDEX reviews_one_open_extract ON ops.reviews (extract_id)
  WHERE state = 'open' AND extract_id IS NOT NULL;

CREATE TABLE ops.review_cells (
  review_id       bigint NOT NULL REFERENCES ops.reviews (id) ON DELETE CASCADE,
  extract_id      bigint NOT NULL REFERENCES ops.extracts (id),
  row_idx         int    NOT NULL CHECK (row_idx >= 0),      -- row of the extract (cell index)
  decision        text   NOT NULL CHECK (decision IN ('accept', 'correct', 'reject')),
  corrected_text  text,
  reviewer        text   NOT NULL,
  at              timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (review_id, extract_id, row_idx),
  CHECK ((decision = 'correct') = (corrected_text IS NOT NULL))
);

ALTER TABLE ops.extracts
  ADD COLUMN reader       text,                                -- pdftotext | ocr | llm_dual | manual | …
  ADD COLUMN parent_id    bigint REFERENCES ops.extracts (id), -- a corrected extract's original reading
  ADD COLUMN approved_by  text,                                -- class C: signed off (extract review)
  ADD COLUMN approved_at  timestamptz;

COMMENT ON TABLE ops.review_cells IS 'per-cell sign-off of a review (03 §6); a correction writes a new manual extract';
