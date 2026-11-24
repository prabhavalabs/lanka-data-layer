-- 0019: the civic record, track C2 (13 §2.2): amendments applied.
--
-- `lanka civic extract` reads an amending gazette into its changes, one row
-- each, in the order the schedule prints them. `lanka civic build` applies
-- them, in date order, to the structure in force, records what happened to
-- each change, and writes the structure as amended under the amendment's own
-- doc_id in `civic.portfolios` and `civic.items`. Those rows say, through
-- `source_doc`, which gazette each heading and item is printed in, so every
-- line keeps its evidence after any number of amendments.

CREATE TABLE civic.amendment_changes (
  doc_id     text NOT NULL REFERENCES civic.instruments (doc_id) ON DELETE CASCADE,
  change_no  int  NOT NULL,                 -- order in the schedule, from 1
  clause     text NOT NULL,                 -- "(2) (f)"
  heading    jsonb,                         -- {no, title} as the clause names it
  lang_only  text CHECK (lang_only IN ('si', 'ta')),   -- made in that text only
  op         text NOT NULL CHECK (op IN ('omit', 'insert', 'substitute', 'renumber',
               'replace_column', 'replace_columns', 'rename_heading', 'replace_heading',
               'insert_heading', 'omit_heading', 'renumber_headings', 'unparsed')),
  col        text CHECK (col IN ('function', 'institution', 'law')),
  spec       jsonb NOT NULL,                -- the change as read
  page       int,
  text       text NOT NULL,                 -- the sentence as printed
  -- set by `lanka civic build` (null until the build reaches the amendment)
  applied    boolean,
  ok         boolean,
  note       text,
  PRIMARY KEY (doc_id, change_no)
);

-- The gazette a heading or item is printed in, when it is not `doc_id`
-- (rows of an amended structure carry lines from earlier gazettes).
ALTER TABLE civic.portfolios ADD COLUMN source_doc text REFERENCES docs.documents (doc_id);
ALTER TABLE civic.items ADD COLUMN source_doc text REFERENCES docs.documents (doc_id);
CREATE INDEX items_source ON civic.items (source_doc) WHERE source_doc IS NOT NULL;

SELECT civic.apply_grants();
