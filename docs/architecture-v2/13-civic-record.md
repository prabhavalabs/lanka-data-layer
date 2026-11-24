# 13: The civic record: documents, the state over time, and the views on them

Status: design, 2026-11-24. Track B (documents) is being built first; tracks C and D follow.

The engine so far answers *where* and *how many*. This part answers *who governs, what was decided, and when*, and links the answers to places through the same p-codes. The Lanka Data Foundation ([opendata.lk](https://opendata.lk), [LDFLK](https://github.com/LDFLK)) shows that the demand is real. Its OpenGIN models government structure with start and end times, and its archiver has kept 28,095 extraordinary gazettes since 2015.

We take that essence and build it differently:

| | LDF today | Here |
|---|---|---|
| Storage | Neo4j + Postgres + MongoDB, no transactions across them | One Postgres, transactional, same release trains as everything else |
| Gazette harvest | Hand-copied Next.js action id (already stale on 2026-11-24) | Resolved from the site's own JavaScript on every run, re-resolved on rejection |
| Coverage | Extraordinary gazettes, English | Extraordinary and weekly gazettes, acts, bills; Sinhala, Tamil and English |
| Disappearance | An "unavailable" flag | Every upstream sighting recorded; a withdrawn document stays with `withdrawn_at` |
| Text | LLM categorisation | Text layer plus OCR in the sandbox, full-text search; LLM output only as reviewed, labelled extractions |
| Place | District names as text | Every mention resolved to a p-code, with the page and words it came from |
| Licence | CC BY-NC-SA (no commercial reuse) | Official texts are not under copyright (Intellectual Property Act No. 36 of 2003, s. 8: official texts of a legislative, administrative or legal nature); our metadata CC BY 4.0 |

## 1. Documents (track B)

### 1.1 Model

The schema is `docs` (migration `0016_documents.sql`), owned by `lanka_owner`. `lanka_ingest` writes to it and `lanka_api` reads it.

```sql
docs.collections (
  id text primary key,                 -- 'lk-gazette-extraordinary', 'lk-gazette-weekly', 'lk-acts', 'lk-bills'
  title text, publisher text, home_url text,
  licence text, licence_note text,     -- 'public-domain-official-text', 'IP Act 36/2003 s.8'
  harvest jsonb                        -- driver settings (page size, politeness)
)
docs.documents (
  doc_id text primary key,             -- stable, readable: 'gze-2515-04' (collection prefix + number)
  collection text references docs.collections,
  number text, sub_number text,        -- '2515', '04'
  issued_on date not null,
  title jsonb not null,                -- {"en": "...", "si": "...", "ta": "..."}; upstream fills all three with English today
  keywords jsonb,
  upstream_id text not null,           -- the site's uuid
  upstream_updated_at timestamptz,
  meta_hash text not null,             -- sha256 of the normalised upstream record
  first_seen_at timestamptz not null, last_seen_at timestamptz not null,
  withdrawn_at timestamptz,            -- set when a full sweep no longer lists it; cleared if it returns
  unique (collection, upstream_id)
)
docs.document_revisions (             -- append-only: every change of the upstream record
  doc_id text references docs.documents, seen_at timestamptz, meta_hash text, record jsonb,
  primary key (doc_id, seen_at)
)
docs.files (
  doc_id text references docs.documents, lang text check (lang in ('si','ta','en')),
  upstream_path text not null,         -- 'extra-gazette-content/G 48538 (E)_1794986733.pdf'
  original_name text,
  status text not null,                -- pending | ok | missing (404) | corrupt (not a PDF / unreadable) | failed (retryable)
  sha256 text, bytes bigint, pages int,   -- the blob lives at raw/<sha256> (lanka-store, Ns::Raw)
  fetched_at timestamptz, attempts int not null default 0, last_error text,
  text_status text,                    -- none | text | ocr | empty
  text_sha256 text,                    -- text/<sha>.txt: UTF-8 text, pages separated by \f
  primary key (doc_id, lang)
)
docs.search (                          -- one row per document and language with text
  doc_id text, lang text, tsv tsvector not null,   -- 'simple' config: Sinhala and Tamil have no stemmer
  primary key (doc_id, lang)
)  -- GIN index on tsv; trigram index on documents.title->>'en'
docs.harvests (                        -- one row per run, for status and incidents
  id bigserial, collection text, started_at, finished_at, mode text,  -- incremental | sweep | files
  listed int, new int, changed int, withdrawn int, files_ok int, files_failed int, error text
)
```

The text and the PDFs are blobs in the store (`raw/`, `text/`), which the backups already cover (07 §6.1). The rows are small: about 40,000 extraordinary gazettes, each with up to three files.

### 1.2 Harvester: `lanka docs harvest <collection> [--mode incremental|sweep|files] [--since DATE] [--limit N]`

The harvester is a new module, `lanka-ingest/src/docs/`, run from the CLI and as a scheduled job.

1. **Resolve the action id.** GET the collection page (for example `https://documents.gov.lk/web/extra_gazettes`), collect its `/_next/static/chunks/*.js`, and find `createServerReference)("<id>", …, "TableDataAction")`. The id is cached in `ops` state. If a call returns anything but an RSC frame with `data`, resolve the id once more and retry. If it still fails, the run fails with `upstream_changed`, which raises a P1 incident, not silent emptiness.
2. **List.** POST to the page with `next-action: <id>` and body `[{"apiEndpoint": "<collection endpoint>", "page": n, "limit": L, "q": "", "search": "", "forwarded": {}}]`. Parse the `1:` RSC frame (`data[]`, `pagination.total`). One request runs at a time: the upstream takes about 70 s per page, so L = 200 and the timeout is 300 s.
   - `incremental` (daily): walk from page 1 until a full page has no new record and no changed `updatedAt`.
   - `sweep` (weekly): walk every page. Any document not seen gets `withdrawn_at`.
3. **Upsert.** Normalise the record and hash it. If the document is new, insert it. If the hash differs, append a revision and update the row. Either way, bump `last_seen_at`. Each file becomes a `docs.files` row with status `pending`.
4. **Files** (`--mode files`, and the tail of every run). GET `https://documents.gov.lk/api/content-file-proxy?file=<upstream_path>` with 2 requests at a time.
   - Stream the response into the store; the blob is named by its sha256.
   - Check the `%PDF-` magic and get the page count through extractd.
   - Text comes from `pdf_words`. If a page has no text layer, it gets `ocr` with `sin+tam+eng` as chosen by the file's language, capped by the sandbox's limits.
   - Write the text blob and upsert `docs.search`.
   - A 404 sets `missing`. A broken file sets `corrupt`, and its blob is still kept as evidence. Anything else sets `failed`, retried with backoff up to 5 times over later runs.
5. **Record** the run in `docs.harvests` and mark the source fresh (`ops`), so `/v2/status` and incidents cover it like any source.

Politeness: one listing request at a time, two file downloads, a `User-Agent` naming the project and its URL, and an honest backoff on 429 and 5xx. The first full backfill is about 200 listing pages (about 4 hours) plus about 100,000 files. It can be resumed at any point, because every step is an upsert keyed on upstream ids and blob hashes.

### 1.3 API

- `GET /v2/documents?q=&collection=&from=&to=&lang=&page=` returns hits with `doc_id`, number, date, titles, languages available, and a highlighted snippet (`ts_headline`).
- `GET /v2/documents/{doc_id}` returns the record, its revisions, its files (status, pages, sha256, download link) and the mentions from track C.
- `GET /v2/documents/{doc_id}/{lang}.pdf` serves the archived blob, with the source URL in a `Link: rel="via"` header. `GET /v2/documents/{doc_id}/{lang}.txt` serves the text.
- `GET /v2/documents/stats?collection=&by=day|month|year` returns counts, for the calendar heatmap (track D).
- MCP: `search_documents` and `get_document`, which return text with the page number, so an assistant can quote and cite.

### 1.4 Off-site copy on GitHub (needs the owner's go-ahead)

The PDFs are large and immutable, which suits GitHub Release assets: up to 2 GiB per asset, kept out of git history so clones stay small.

- A data repository (`prabhavalabs/lanka-civic-archive`) gets one release per month, `gazettes-2026-11`.
- Each release carries a tar of that month's PDFs, named by sha256, plus `manifest.jsonl` (doc_id, lang, sha256, bytes, source URL).
- `metadata/<year>.jsonl`, the documents and revisions, is committed as text so the history is diffable.
- `lanka docs mirror --to github` builds and uploads only the months whose manifest changed. Restore is the reverse: `lanka docs restore --from github`.

## 2. The state over time (track C)

This track gets its own schema, `civic`. It follows OpenGIN's essence: every fact has a validity interval. It differs in three ways: it is relational, it is bitemporal, and every fact carries evidence.

```sql
civic.entities (id text pk, kind text,   -- ministry | department | statutory_body | office | person | local_authority | court
                name jsonb, pcode text,  -- the place the entity governs or sits in, when it has one (district secretariat → adm2)
                valid_from date, valid_to date, recorded_at timestamptz)
civic.relations (subject text, predicate text, object text,  -- department under ministry, person holds office, ministry has function
                 valid_from date, valid_to date,
                 evidence_doc text references docs.documents, evidence_page int, evidence_quote text,
                 recorded_at timestamptz, superseded_at timestamptz)   -- correcting a fact supersedes it, never deletes it
civic.functions (id, entity, text jsonb, valid_from, valid_to, evidence_doc ...)  -- "subjects and functions" lines
```

Sources, in order of use:

1. The gazettes that assign subjects, functions and departments to ministries. These come after every cabinet change. Extraction is proposed by rules and LLM, then reviewed, using the existing review gate for class B.
2. Appointment gazettes: ministers, secretaries and heads of department.
3. Parliament members and cabinet decisions (registered sources `parliament-lk` and `nuuuwan-lk-cabinet-decisions`).

Queries this answers: who held this portfolio on a date, which ministry a department sat under over time (a flow view), and which bodies govern a given p-code.

### 2.1 Build C1: ministries and their institutions over time (binding contract)

**Instruments.** Under Article 44(1) of the Constitution, the President notifies by gazette the Cabinet ministers, their subjects and functions, and the departments, statutory institutions and public corporations in each minister's charge. A **baseline** sets out the whole schedule. Its notification reads "…the subjects and functions and Departments… shall be as set out in the schedule…". Its notification says the President has "determined the number of Ministers". Examples are 1651/20 (2010), 1896/28 and 1897/15 (January 2015; 1897/15 "amends" 1896/28 by substituting the whole schedule, so it is a baseline), 1933/13 (2015), 2103/33 (2018), 2153/12 (2019), 2187/27 (2020), 2281/41 and 2289/43 (2022), and 2403/53 and 2412/08 (2024).

A **supplement** assigns subjects and functions only to the Ministers just appointed ("…to be assigned to the Ministers appointed under Article 43… have been determined…"), and says nothing about the number of ministers. The crisis gazettes of November 2018 are supplements: 2096/17 (19 ministers), 2096/25 (2) and 2096/68 (10) together form a cabinet of 31, like the baseline 2103/33 that follows. The structure in force is the latest baseline. A run of supplements after a baseline (or after another supplement) accumulates into a new structure that **replaces** the previous one from the first supplement's date. Each later supplement in the run adds or replaces headings by `portfolio_key`, from its own date. A later baseline starts afresh. The same accumulation serves the build, `structure` and `diff`.

An **amendment** amends a named baseline "with effect from" a date. It does so heading by heading and column by column: insert an item after item N, omit item N, substitute item N, or renumber N–M. 2458/65 (2025), which amends 2412/08, is an example.

Each baseline's or supplement's schedule repeats one block per minister (the **table layout**, 2010–2019 and 2022–2025):
- a heading, `NN. Minister of …` (also `(N) Minister of …`, `NN Minister…`, and `Ministry of …` in 2010), followed by `(Contd.)` on continuation pages. A new minister can start in the middle of a page, with or without a repeated column header;
- three columns: Column I, *Subjects and Functions*, as a numbered list; Column II, *Departments, Statutory Institutions and Public Corporations*, as a numbered list; Column III, *Laws, Acts and Ordinances to be Implemented*, one per act (with bullets on some pages, with hanging indents or only act names on others).

2187/27 (2020) has a second, narrative **policy-statement layout**: `N.0 Minister of …` sections, each with the parts *Purview*, *Subjects and Functions*, *Special Priorities* and *Related Institutional and Legal Framework*, followed by `N.k State Minister of …` sections. It is read into the same model: the Cabinet minister's numbered *Subjects and Functions* are the functions; the institutions of the framework tables of the Cabinet minister and then of his State Ministers (whose institutions are in the ministry's charge) are the institutions, numbered 1..n in reading order (groups such as `1. Treasury Affairs` with roman-numbered institutions are labels, not institutions); the bullets of the right band are the laws. State Ministers' own functions are left out.

**C1 scope:**
- Baselines are parsed fully.
- Amendments are recorded as timeline events, with the baseline they amend and their effective date.
- Applying amendment operations is C2, and goes through review.

**Schema** (migration `0018_civic.sql`, schema `civic`, owner `lanka_owner`; `lanka_ingest` writes and `lanka_api` reads, with grants by `civic.apply_grants()`):

```sql
civic.instruments (doc_id text pk references docs.documents, kind text check (kind in ('baseline','supplement','amendment')),
  effective_from date not null, amends text references civic.instruments,   -- the baseline or supplement an amendment amends
  head_of_state text, parser text not null, parsed_at timestamptz,
  status text not null check (status in ('validated','needs_review','approved','rejected')),
  checks jsonb not null default '[]')          -- [{check, ok, detail}] from validation, shown to reviewers
civic.portfolios (doc_id text references civic.instruments, seq int,   -- the heading number, "01"
  title text not null,                       -- "Minister of Defence"
  portfolio_key text not null,               -- slug of the title without "Minister of": "defence"
  first_page int, primary key (doc_id, seq))
civic.items (doc_id text, seq int, col text check (col in ('function','institution','law')), item_no int,
  text text not null, entity_key text,       -- set for institutions
  page int, primary key (doc_id, seq, col, item_no), foreign key (doc_id, seq) references civic.portfolios)
civic.entities (entity_key text pk, name text not null, kind text not null
  check (kind in ('department','statutory_body','corporation','company','fund','university','force','other')),
  aliases text[] not null default '{}')
civic.assignments (entity_key text references civic.entities, doc_id text references civic.instruments, seq int,
  portfolio_title text, portfolio_key text, valid_from date not null, valid_to date,   -- null = still in force
  page int, primary key (entity_key, valid_from))
```

**Entity keys.** An entity key is the institution's name lower-cased, with these changes:
- `&` becomes `and`;
- "Ltd", "Ltd." and "(Pvt)" become `limited`;
- a leading "the" is dropped;
- punctuation and extra spaces are removed;
- the result is slugged.

`registry/civic/aliases.yaml` maps renamed or reworded institutions to one key, for example `{ "sri lanka telecom regulatory commission": "telecommunications-regulatory-commission-of-sri-lanka" }`, and is reviewed like any registry change. A hyphen between two letters (`Ter-minal`, `Non- Governmental`, set by a line wrap) and zero-width characters never change a key. In an alias file, a lower-case slug is taken as a key and anything else as a name. The entity's name is its latest wording that has no wrap hyphen; its other wordings are its `aliases`. Kind is guessed from the name: Department…, …Authority/Board/Commission/Council → `statutory_body`, …Corporation → `corporation`, …Limited/PLC → `company`, …Fund → `fund`, University… → `university`, Sri Lanka Army/Navy/Air Force/Police → `force`.

**Parser** (`lanka-ingest/src/civic/`). The parser reads the English PDF's words with their boxes, through extractd, either `pdf_words` or the local `pdftotext -bbox-layout`. On each page:
- the x positions of the "Column I/II/III" header words split the page into three bands;
- a minister heading is a line matching `^\(?\d{1,3}[.)]?\s*Minist(er|ry) of` next to a column header. A new heading must be numbered like the minister in progress (a `(Contd.)` heading) or the next one, and be followed by item 1, so an item such as `2. Minister of X to be consulted` stays an item. A header printed out of place on one page is replaced by the bands of the page before;
- items are numbered lines. Wrapped lines join until the next number (a number printed with a typo, such as `3l.`, `15,` or `23,.`, is read when it is the next one expected). A line back at the number's edge under hanging text is a note and stays out of the item. A line ending in `-` before a lower-case letter is a word split by the line break and joins without the hyphen (`Authori-` `ty`); before a capital the hyphen is real and stays, with no space. This holds in all three columns. Column III items start at a bullet when the page has bullets, otherwise at the left edge when the page indents wrapped lines, otherwise after a line that completes an act name, a pattern of the form `… Act No. N of YYYY`, `(Chapter N)` or `Ordinance`.
- the 2020 layout is read by `schedule_b` when the table parser finds nothing (see above).

**Validation**, stored in `checks` (`[{check, ok, detail}]`):
- `schedule_found`: at least one table or policy-statement section with a minister's heading was read;
- `headings_sequential`: heading numbers run 1..n with no gap;
- `items_sequential`: inside each heading, Column I and Column II numbers run 1..k with no gap;
- `institution_text_clean`: no institution's text holds an act citation (`No. 37`, `of 1991`, `… Act,`, `Ordinance`, a trailing number) or ends by repeating a word already in the name: both mean words of a neighbouring column were glued on. A list of two things (`… and …`, commas, parentheses) may repeat a word;
- `headings_have_items`: every heading has at least one item;
- `heading_count`: the heading count matches the count of distinct minister headings found in the plain text (a heading's number and words that the PDF's text flow splits across lines still count);
- for a supplement, `run_coverage`: the supplements of its run up to and including it cover at least 30% of the ministers of the baseline before the run;
- for an amendment, `amended_baseline`: the gazette it names is an extracted baseline or supplement (a named amendment resolves to the baseline it amends);
- `parser_warnings` (information only): what the parser set aside, such as an unnumbered note ("Institution being reviewed") or a header printed out of place.

All checks passing → `validated`; otherwise `needs_review`. A reviewer can `approve` or `reject` (recorded in `checks`). The API serves `validated` and `approved` instruments only.

**Exceptions.** If a gazette really skips a number, no parser can fix it. `registry/civic/exceptions.yaml` lists, per `doc_id`, `expected_gaps: [{heading, column: I|II, missing: [N], note}]`, each checked against the PDF page by a reviewer. `items_sequential` then passes, with the exceptions quoted in its `detail`. `expected_citations: [{heading, item_no, note}]` likewise lets an institution whose real name cites an act pass `institution_text_clean` (2153/12, heading 28 item 1). Today: 1651/20 (heading 02 Column II prints 42 then 44; heading 43 Column II prints 5 then 7) and 1933/13 (heading 38 Column I prints 4 then 6).

**Build** (`lanka civic build`). Take the validated or approved baselines and supplements and compose them into structures (see Instruments): a baseline, or a run of supplements accumulated. Structure *i* holds from its own date (the date of its latest gazette) until structure *i+1*'s date, or until now for the last one. Its institutions become `assignments` with that interval, and an entity in consecutive structures under the same `portfolio_key` keeps one continuous row; the row's `doc_id`, `seq` and `page` are those of the gazette that first listed it there. An institution listed under two ministers in one structure keeps its first listing. The build is idempotent and replaces `civic.assignments` and `civic.entities` in one transaction.

**CLI:**
- `lanka civic aliases suggest [--min 0.85]` compares every pair of entity keys (pg_trgm similarity of the de-slugged names), skips pairs that already resolve to one key or that `aliases.yaml` already joins, and writes the candidates, most similar first, to `registry/civic/alias-suggestions.yaml` (`{a, b, similarity, seen_in: [gazette numbers]}`). It is a review aid: nothing is ever applied automatically;
- `lanka civic discover` lists candidate instruments from `docs.documents` titles and text.
- `lanka civic extract <doc_id>|--all` parses and validates.
- `lanka civic approve|reject <doc_id> --by <handle>`.
- `lanka civic build` rebuilds `civic.assignments`.
- `lanka civic show --at YYYY-MM-DD` prints the structure in force on that date.

**API** (read-only, `lanka_api`):

| Endpoint | Returns |
|---|---|
| `GET /v2/civic/instruments` | `[{doc_id, kind, effective_from, amends, head_of_state, status, portfolios, institutions, title, number}]`, newest first, every instrument of every kind and status (including `needs_review`; `portfolios` and `institutions` are 0 when unparsed), `number` as `"2412/08"`, `title` the English title of the document, for the timeline |
| `GET /v2/civic/structure?at=YYYY-MM-DD` | `{at, instrument:{doc_id, effective_from, number}, composed_of:[{doc_id, effective_from}], amendments_since:[{doc_id, effective_from, title}], portfolios:[{doc_id, seq, title, short_title, portfolio_key, page, functions:[{item_no, text}], institutions:[{item_no, text, entity_key, kind, page}], laws:[{item_no, text}]}]}`. Without `at`, today. `instrument` is the latest gazette of the structure; `composed_of` lists the gazettes it is made of (one baseline, or the run of supplements); each portfolio carries the `doc_id` it is printed in, which its pages refer to. `title` is as printed (`Minister of Defence`), `short_title` has the `Minister of`/`Minister for` prefix dropped (`Defence`). For a date before the first parsed schedule: 404 with `payload.problem = {code: "not_in_coverage", earliest: "YYYY-MM-DD"}`. |
| `GET /v2/civic/diff?from=DATE&to=DATE` | `{from:{doc_id, effective_from}, to:{…}, moved:[{entity_key, name, kind, from:{portfolio_key, title, short_title, page}, to:{…}}], added:[{entity_key, name, kind, to:{…}}], removed:[{entity_key, name, kind, from:{…}}], portfolios:{added:[title], removed:[title], kept:[{from_title, to_title, from_key, to_key, overlap, renamed}]}, counts:{moved, added, removed, portfolios_added, portfolios_removed, portfolios_renamed}}`. Both dates use the composed structure. `page` is the institution's page in that side's gazette. 404 as for `structure`. |
| `GET /v2/civic/entities?q=&limit=` | `[{entity_key, name, kind, current:{portfolio_title} or null}]` |
| `GET /v2/civic/entities/{key}` | `{entity_key, name, kind, aliases, history:[{portfolio_title, portfolio_key, valid_from, valid_to, doc_id, page}]}` |
| `GET /v2/documents/{doc_id}` | `{doc_id, number, issued_on, title, files:[{lang, status, pages, bytes, sha256}], source_url}` (from 1.3) |
| `GET /v2/documents/{doc_id}/{lang}.pdf` | The archived blob, `application/pdf`, with a `Link: <source_url>; rel="via"` header. The web links to it with `#page=N`. |

**Renamed ministries in `diff`.** Two ministries (one from each side) are the same ministry, `kept`, when they have the same `portfolio_key`, or their institutions overlap: Jaccard at least 0.5, or the new one keeps at least 60% of the old one's institutions. `overlap` is the Jaccard value and `renamed` is true when the keys differ. One ministry can pair with several (two merged into one). An institution counts as `moved` only when its old and new ministries are not a kept pair; `portfolios.added` and `removed` are the ministries with no pair.

**View** (`/government` in web-v2):
- **Timeline.** Every instrument on one axis, with baselines as tall marks and amendments as small marks, banded by head of state. The selected date drives everything below it.
- **Structure at a date.** One card per ministry, showing its institution count and kind chips, with institutions listed and functions folded. A search box highlights an institution wherever it is.
- **Compare.** Pick two dates; the default is the previous baseline against the current one. Counts sit at the top: moved, added, removed, ministries added and removed. A flow diagram has old ministries on the left and new on the right, with a ribbon for each group of institutions that moved. Lists follow.
- **Institution history.** A drawer with one bar per assignment over time, coloured by ministry.
- **Evidence.** Every item and assignment links to the gazette PDF at its page.

## 3. Views (track D)

These views make the data understandable at a glance. They are built in web-v2 with the existing chart and map stack and the same design system.

1. A **chart for every statistical dataset**: time series, ranking bars and distribution by admin level, next to the map. These come from the observations we already have, so this ships first.
2. **Gazette calendar heatmap**: documents per day since 2015. Click a day to see the list.
3. **Document reader**: the PDF next to its text, with a language switch, revisions, and mentions highlighted, each linking to its place on the map.
4. **Government over time**: an org chart at any date, with a time slider. A flow diagram shows departments moving between ministries across cabinets, and a portfolio timeline shows who held each portfolio.
5. **Place profile, civic tab**: the bodies that govern this p-code, recent gazettes that mention it, and its electoral results.
6. **Stories**: short, sourced explainers built from the above. Every number links to its dataset version.

## 4. Accountability (track E, after C)

- An automated **government website scorecard**, checked weekly: TLS, availability, mobile accessibility (axe), page weight, and whether the site still lists its RTI officer. Results are kept as observations per agency, so the score has history, and agencies with a place are mapped.
- RTI requests and required-meeting tracking, in the same evidence model.

## 5. Order of work

1. B1: migration, harvester (listing and files), CLI, tests against recorded fixtures. The backfill then runs on the SSD store.
2. D1: dataset charts. D2: calendar heatmap and document search and reader. This needs the B API.
3. B2: weekly gazettes, acts and bills, plus the GitHub mirror once approved.
4. C1: civic schema and the review flow for function gazettes. D4 follows.
5. E1: website scorecard.
