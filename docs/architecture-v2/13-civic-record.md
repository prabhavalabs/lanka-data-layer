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
