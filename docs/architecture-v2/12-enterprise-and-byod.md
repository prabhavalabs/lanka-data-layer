# 12 — Workspaces, private data and deployment profiles

The data layer of documents 01–09 is public, anonymous and read-only. Composable maps ([10](10-map-composition.md)) and analysis ([11](11-analysis-engine.md)) add things that belong to *someone*: a saved map, an uploaded file, an API key, a private dataset. This document designs that ownership layer so that it serves an individual with one map and an enterprise running its own instance, without weakening any guarantee of the open layer and without a platform team to run it.

Markers: **[A]** = assumption to verify in the named work package.

## 1. Growth path

Each stage is usable on its own and adds no moving part the previous stage needs.

| Stage | Who gets what | New moving parts | Work packages |
|---|---|---|---|
| 0 — anonymous (default, forever free) | Read everything public; build maps; share by snapshot link; run analyses on public data within anonymous limits; embed | schema `app` with one table; role `lanka_app` | WP-26…30 |
| 1 — individual | Sign in; personal workspace; named, versioned, private or public maps; API keys with higher limits | OIDC sign-in, sessions | WP-31 |
| 2 — private data | Upload CSV/XLSX/GeoJSON/GeoParquet; geocode to p-codes and grid; analyse against public layers; keep private or publish as community data | schema `ws`, upload pipeline | WP-32, WP-33 |
| 3 — team | Shared workspaces, roles, projects, white-label embeds, audit export | embed profiles | WP-35 |
| 4 — enterprise / on-prem | The whole stack inside the customer's network, their identity provider, their private registry overlay, the public layer synced from upstream, scheduled private connectors | deployment profile `private` | WP-36 |

What stays true at every stage: anonymous read access to public data is never gated, throttled below the published anonymous limits, or made conditional on an account.

## 2. Two planes

```
                         host nginx :443
        ┌──────────────────────┴───────────────────────────┐
   /v2/*  /mcp  /dl/*                                 /app/v1/*
   PUBLIC DATA PLANE                                  APP PLANE
   anonymous · GET (+ POST /v2/analyses → 303 to a     credentials · writes · never cached by nginx
   cacheable GET) · world-cacheable
   the open standard's HTTP contract                  product API, versioned separately
        │                                                  │
   pool A: role lanka_api (read-only)                 pool C: role lanka_app
   pool B: role lanka_analysis                             │
        │                                                  │
   catalog.* geo.* data.* serve.*   analysis.*        app.*  (identity, maps, keys, audit)
   (unchanged; no account can write here)             ws.*   (private data, same shapes as data.*)
```

| Rule | Mechanism |
|---|---|
| A `/v2` response never depends on who asks | `/v2` handlers have no access to pool C and receive no data-access principal — at most a *rate identity* (key id or IP hash) for quotas. This is what keeps every `/v2` response cacheable and the conformance suite meaningful. |
| Analysis cannot change public data | `lanka_analysis` has `SELECT` on `catalog`, `geo`, `data`, `serve`; `INSERT/UPDATE/DELETE` only on `analysis.*`; `INSERT` on `ops.jobs` for kind `analysis`. Results with a workspace input are written and read through pool C instead, under RLS. |
| The public role cannot read private data | `lanka_api` and `lanka_analysis` have no grants on `app.*` or `ws.*`. |
| The app role cannot change public data | `lanka_app` has `SELECT` on `catalog`, `geo`, `data`, `serve`; `INSERT/UPDATE/DELETE` only on `app.*`, `ws.*`, and `INSERT` on `ops.jobs` for kinds `upload` and `analysis`. |
| Tenants cannot read each other | every `ws.*` and workspace-scoped `app.*` table has row-level security on `workspace_id`; the handler sets `SET LOCAL app.workspaces = '{…}'` inside the transaction from the authenticated principal. RLS is the second barrier; the first is that every query takes a `Scope` argument and there is no code path that builds a workspace query without one. |
| Credentials never meet the proxy cache | the session cookie is scoped `Path=/app`; nginx has no `proxy_cache` on `/app/`; `/v2` ignores cookies and treats an API key only as a rate-limit identity (§6). |
| The app plane can be switched off | `LANKA_APP_PLANE=off` (default for the `mirror` profile): `/app/v1` returns 404, the web app hides sign-in, saved maps and uploads, and falls back to URL-fragment map links ([10 §6.2](10-map-composition.md#62-urls)). |

`lanka serve` stays one binary and one process. It gains two small pools (B: 4 connections, C: 8) next to the public pool (reduced from 24 to 20), and the app routes are a crate (`lanka-app`) mounted by `lanka-server`. Total connections stay under `max_connections = 60`.

This replaces two statements of the original set: "no account anywhere" (README principle 8) becomes "no account needed to read"; "there is no authenticated surface" ([06 §2.2](06-serving.md#22-conventions)) becomes "there is no authenticated surface under `/v2`".

## 3. Identity, workspaces, projects

**Sign-in is OpenID Connect only.** No passwords are stored and no e-mail is sent, so there is no credential database to leak and no mail server to run. The public deployment configures two providers (GitHub and Google **[A: owner's choice, Q13]**); an on-prem instance configures its corporate identity provider. Sessions are opaque random tokens in an `HttpOnly; Secure; SameSite=Lax; Path=/app` cookie, stored hashed; cookie-authenticated writes also require a matching `Origin` header.

```sql
CREATE SCHEMA app;

CREATE TABLE app.users (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  handle      text NOT NULL UNIQUE CHECK (handle ~ '^[a-z0-9][a-z0-9-]{1,30}$'),
  display_name text,
  email       text,                               -- from the IdP; used for account recovery and takedown contact only
  created_at  timestamptz NOT NULL DEFAULT now(), last_seen_at date, disabled_at timestamptz
);
CREATE TABLE app.identities (
  issuer text NOT NULL, subject text NOT NULL, user_id bigint NOT NULL REFERENCES app.users (id) ON DELETE CASCADE,
  PRIMARY KEY (issuer, subject)
);
CREATE TABLE app.sessions (
  token_sha256 bytea PRIMARY KEY, user_id bigint NOT NULL REFERENCES app.users (id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL DEFAULT now(), expires_at timestamptz NOT NULL, ip_hash bytea, user_agent text
);

CREATE TABLE app.plans (id text PRIMARY KEY, limits jsonb NOT NULL);       -- 'anonymous', 'free', 'team', 'unlimited'

CREATE TABLE app.workspaces (
  id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  slug       text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,30}$'),      -- written @slug
  kind       text NOT NULL CHECK (kind IN ('personal','team')),
  title      text NOT NULL,
  plan_id    text NOT NULL REFERENCES app.plans (id) DEFAULT 'free',
  settings   jsonb NOT NULL DEFAULT '{}',          -- default visibility, read-audit on/off, allowed external hosts
  created_at timestamptz NOT NULL DEFAULT now(), deleted_at timestamptz
);
CREATE TABLE app.memberships (
  workspace_id bigint NOT NULL REFERENCES app.workspaces (id) ON DELETE CASCADE,
  user_id      bigint NOT NULL REFERENCES app.users (id) ON DELETE CASCADE,
  role         text NOT NULL CHECK (role IN ('owner','admin','editor','viewer')),
  PRIMARY KEY (workspace_id, user_id)
);
CREATE TABLE app.projects (                        -- a folder: groups maps, datasets and analyses
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  workspace_id bigint NOT NULL REFERENCES app.workspaces (id) ON DELETE CASCADE,
  slug text NOT NULL, title text NOT NULL, UNIQUE (workspace_id, slug)
);
```

- A personal workspace `@handle` is created at first sign-in. Handles and workspace slugs share one namespace and exclude reserved words (`demo`, `lanka`, `official`, `gov`, publisher ids from the registry) so a workspace cannot pose as a public body.
- Roles: `viewer` reads; `editor` creates and edits maps, datasets and analyses; `admin` manages members, keys, embed profiles and visibility; `owner` also deletes the workspace and changes the plan. There are no per-object permissions in v1: visibility (`private | unlisted | public`) per map and per dataset is the only finer control.
- Personal data held: handle, display name, e-mail, IdP subject, salted IP hashes. Account deletion removes all of it and the user's personal workspace within 24 hours; team content stays with the team.

## 4. The workspace data plane

**A private dataset is an ordinary dataset in a different schema.** Schema `ws` repeats the shapes of the standard model so that query, frames, tiles, analysis and exports work on it through the same code:

| `ws` table | Mirrors | Differences |
|---|---|---|
| `ws.datasets` | `catalog.datasets` | `workspace_id`, `visibility`, `licence_id` (chosen by the uploader), `synthetic boolean`, `key int4` allocated from 1,000,000 upward |
| `ws.indicators`, `ws.feature_fields` | `catalog.indicators` | `class` is always `user`; unit, `measure_type`, `aggregation` and dimensions are declared in the import mapping |
| `ws.dataset_versions` | `data.dataset_versions` | same `dsv` hash; no release membership; `status: staged \| current \| superseded` |
| `ws.observations` | `data.observations` | identical columns and range versioning; LIST-partitioned by `dataset_key`; RLS |
| `ws.features` | `geo.features` | identical; RLS |
| `ws.grids` (later) | `geo.grid_layers` | user rasters resampled to the canonical grid |

Rules:

- **Identifiers.** `@{workspace}/{dataset}` and `@{workspace}/{namespace}.{measure}`. The `@` cannot occur in a registry id, so a private identifier can never shadow a public one, in any API, document or log.
- **Geography is shared.** `ws.observations.geo_key` references `geo.units`; private data joins to public data on unit keys and cells. Private custom zones are features, not units (support `zones`, WP-37).
- **Key spaces are disjoint.** Public dataset keys stay below 1,000,000; `obs_id = dataset_key << 40 | sequence` therefore never collides and one resolver routes an `obs_id` to the right plane.
- **Class `user` is never mixed silently.** Any response, frame, analysis result or map legend containing workspace data carries `contains: ["user"]` and the notice `COMMUNITY_DATA` ("uploaded by @acme; not reviewed by Lanka Data Layer") or `PRIVATE_DATA`. This extends the labelling rules of [04 §8](04-enrichment.md#8-labelling) with a fifth group beside `official / third_party / derived / estimated`.
- **`lanka-query` gets a scope, not a fork.** `Backend` methods take a `Scope { Public, Workspace(ids) }`; `PgBackend` picks the schema (`data`/`catalog` or `ws`) and the pool from it. A mixed frame (public + private columns) is two plans joined on the code array in Rust.
- **Publishing to the public catalogue is not self-service.** "Public" visibility means listed under the workspace and usable by anyone as `@acme/…`. Becoming a registry dataset (class `third_party`, in releases and bulk downloads) goes through the contribution path of [08 §4](08-openness-and-governance.md#4-contribution-model): a source file, a licence, a review.

API: the workspace mirror of the public resources, under `/app/v1/ws/{workspace}/`: `datasets`, `datasets/{id}`, `indicators/{id}/series`, `frame`, `features/{dataset}` (+ `/points`), `tiles/{dataset}/{z}/{x}/{y}.mvt`, `analyses`. `frame` and `analyses` accept public ids and `@` ids together.

## 5. Bring your own data

Uploads reuse the ingestion pipeline ([03 §1](03-ingestion.md#1-pipeline)) stage for stage. An uploaded file is an untrusted document and gets exactly the controls of [07 §5.1](07-operations.md#51-untrusted-document-handling).

```
upload ─► archive (store/ws/<workspace>/raw/<sha256>) ─► extract (L1 Parquet; pure-Rust parsers, or GDAL in the sandbox)
       ─► PROFILE (column types, candidate roles, sample rows, PII hints)          ◄── user confirms the MAPPING
       ─► standardise (geocode, periods, units, dims) ─► validate ─► MATCH REPORT  ◄── user resolves unmatched rows
       ─► version (dsv, range rows in ws.observations / ws.features) ─► tiles (if > 20,000 features) ─► current
```

### 5.1 Formats and limits

| Format | Parser | Notes |
|---|---|---|
| CSV / TSV | `csv` (Rust) | delimiter, encoding (UTF-8, UTF-16, Windows-1252) and header row detected, shown for confirmation |
| XLSX | `calamine` (Rust) | one sheet per dataset |
| GeoJSON, GeoParquet, Parquet | Rust | CRS must be WGS 84 or declared |
| Shapefile (zip), GeoPackage, KML, FlatGeobuf | `ogr2ogr` in the sandbox sidecar → GeoParquet | reprojected to EPSG:4326 |
| GeoTIFF (later) | `gdalwarp` in the sandbox → canonical grid | `ws.grids` |

Default limits (plan values in §6): 200 MB per file, 5 M cells or 500,000 features per dataset, 2,000 distinct columns refused. The upload is a streamed `PUT` to `/app/v1/ws/{ws}/uploads/{id}/content`; nothing is buffered in memory.

### 5.2 The mapping

The profile step proposes, the user confirms, and the confirmed mapping is stored with the dataset version (so a re-upload of next month's file is one click, and the import is reproducible):

```jsonc
{
  "kind": "features",                               // or "observations"
  "geo":   { "mode": "latlon", "lat": "Latitude", "lon": "Longitude" },
  // other modes: {"mode":"pcode","column":"gnd_code"} · {"mode":"name","columns":{"2":"District","3":"DS Division"},"lang":"en"}
  //              {"mode":"place","column":"Town","hint_district":"District"} · {"mode":"geometry"} · {"mode":"cell","column":"cell_id"}
  "period": { "column": "Listed", "format": "%Y-%m-%d", "freq": "M" },
  "fields": [
    { "column": "Price (LKR)", "id": "price", "type": "number", "unit": "LKR", "measure_type": "sum" },
    { "column": "Perches", "id": "perches", "type": "number", "unit": "{perch}" },
    { "expr": "price / perches", "id": "price_per_perch", "unit": "LKR/{perch}", "measure_type": "ratio" },
    { "column": "Type", "id": "land_use", "type": "category" }
  ],
  "drop": ["Seller", "Phone", "Ad URL"],            // unmapped columns are dropped by default; listed here for the record
  "aggregate": [ { "to": "admin:4", "field": "price_per_perch", "stat": "median", "min_n": 5,
                   "indicator": "re.asking_price_per_perch_median" } ]
}
```

### 5.3 Geocoding modes

| Mode | Engine path | Precision class recorded per row | On failure |
|---|---|---|---|
| `pcode` | validate against `geo.units`, including retired codes (offered a `crosswalk_rebase` to the reference vintage) | `unit` | unmatched codes listed; row excluded only with the user's confirmation |
| `name` | the reviewed alias tables of `registry/crosswalks/`, constrained to the parent unit, in `en`/`si`/`ta`; then trigram candidates from `serve.search` | `unit` | **never guessed**: ambiguous or unmatched names go to the match report with candidates; the user picks or excludes. The choices are saved as workspace aliases. |
| `latlon` | `cell_id` by the canonical formula → `gnd_idx` → unit chain; polygon fallback for boundary cells (the `geocode-csv` path, [06 §5](06-serving.md#5-cli)) | `point` | swapped lat/lon and wrong-hemisphere detection; points outside the land mask reported |
| `place` | `/v2/lookup` semantics: postal code, town, GN name → centroid | `postal`, `place`, `gn`, `dsd`, `district` | as `name` |
| `geometry` | polygons/lines stored as features; `unit_key` by point-on-surface; cell coverage on demand | `geometry` | invalid geometry repaired and counted |
| `cell` | canonical `cell_id` | `cell` | out-of-range ids rejected |

There is no street-address geocoder: no open address dataset exists for Sri Lanka, and the platform does not send user data to a third-party geocoding service. `place` mode says so in the UI and records the coarse precision class. The precision class gates analysis: point-support methods ([11 §3](11-analysis-engine.md#3-alignment)) require `point`; rows at `dsd` precision can only be aggregated to ADM3 or coarser.

The "no name joins" invariant holds: names are resolved to codes once, at import, with a recorded method (`name_exact | name_alias | user_choice`), and never used as keys afterwards.

### 5.4 Privacy and rights

- **Private by default.** Visibility changes are explicit, audited, and need the `admin` role.
- **Data minimisation.** Only mapped columns are stored in L1 and beyond; the raw file is kept for re-mapping (user-deletable, auto-deleted after 30 days unless the user opts to keep it).
- **Personal-data hints.** The profile step flags columns that look like phone numbers, e-mail addresses, national identity card numbers or person names and defaults them to `drop`. It is a hint, not a guarantee; the terms put responsibility on the uploader.
- **Publishing rules.** Making a dataset `unlisted` or `public` requires: a licence chosen from `catalog.licences`, a declaration that the uploader may publish it, and — for point data about people, households or properties — either aggregation with a minimum cell count (default 5; the profile's `suppression.min_count_adm4` applies at ADM4) or explicit confirmation that points are not personal data (e.g. facilities).
- **Not accepted**, by terms and by takedown: data obtained by scraping in breach of a site's terms, data the uploader has no right to hold, microdata about identifiable people. The takedown process of [08 §6](08-openness-and-governance.md#6-takedown) covers workspace content; `lanka takedown` gains `--workspace-dataset`.
- **Deletion is real.** Deleting a dataset drops its partition, tiles, raw and extract blobs, aligned columns and dependent analysis results, within 24 hours; backups age out on the schedule of [07 §6](07-operations.md#6-backups-and-recovery) (stated in the terms). The audit log keeps the event, not the content.
- **Encryption.** Transport TLS; at rest the hosted instance relies on disk encryption of the volume **[A: confirm the VPS volume is LUKS-encrypted or move `store/ws` and the Postgres tablespace for `ws` to an encrypted volume in WP-32]**. Per-workspace envelope encryption is not built; customers who need it use the on-prem profile.

### 5.5 Scheduled private sources

A workspace on the on-prem profile can add connectors in its registry overlay (`LANKA_REGISTRY_DIR` already accepts a list) with `target: {workspace: acme}`; they run in the normal worker with the normal drivers and land in `ws.*`. On the hosted instance, scheduled pulls are limited to the `http_json`, `csv` and `xlsx_table` drivers against public HTTPS hosts, with the fetch controls of [07 §5.1](07-operations.md#51-untrusted-document-handling) (no private address ranges, DNS pinned for the request, size caps) and per-workspace secrets stored encrypted (`app.secrets`, XChaCha20-Poly1305 with a key from the environment).

## 6. API keys, quotas, plans

```sql
CREATE TABLE app.api_keys (
  id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  workspace_id bigint NOT NULL REFERENCES app.workspaces (id) ON DELETE CASCADE,
  name         text NOT NULL,
  prefix       text NOT NULL UNIQUE,              -- 'ldl_live_8f3k2a7q' — shown in the UI and in logs
  secret_sha256 bytea NOT NULL,                   -- sha256 of the 32-byte random secret; the secret is shown once
  scopes       text[] NOT NULL,                   -- 'public' | 'ws:read' | 'ws:write' | 'analysis' | 'maps:write'
  created_by   bigint NOT NULL REFERENCES app.users (id),
  created_at   timestamptz NOT NULL DEFAULT now(), last_used_at date, expires_at timestamptz, revoked_at timestamptz
);
CREATE TABLE app.usage_daily (
  day date NOT NULL, workspace_id bigint, key_id bigint, ip_hash bytea,
  meter text NOT NULL,                            -- 'requests' | 'frame_cells' | 'analysis_cost' | 'upload_bytes' | 'jobs'
  amount bigint NOT NULL,
  PRIMARY KEY (day, meter, workspace_id, key_id, ip_hash)
);
```

- Format `ldl_live_<8 chars>_<43 chars base64url>`; sent as `Authorization: Bearer …`. A plain SHA-256 is sufficient for a 256-bit random secret. The fixed `ldl_` prefix lets secret scanners find leaked keys.
- **What a key is for:** access to workspace data and writes; higher limits on work that reaches the application (analysis, large frames, jobs); attribution of usage. It is **not** needed for, and does not change, any `/v2` read.
- **Enforcement.** nginx keeps the per-IP zones of [07 §5.3](07-operations.md#53-abuse-and-availability) as the abuse ceiling (a request with a well-formed key prefix uses a higher-ceiling zone). The real quotas are token buckets in `lanka serve`, keyed by key id or by IP hash, held in memory and flushed to `app.usage_daily` every minute. Ten invalid keys from one IP in a minute bans that IP from keyed access for 15 minutes. With a second API instance (scale-out S3) the buckets move to a small Postgres table updated in batches; no Redis.
- A response served from the nginx cache is not metered and not limited beyond the IP ceiling. Quotas count work, not reads.

Plans are rows, not code:

| Limit (`app.plans.limits`) | `anonymous` | `free` | `team` | `unlimited` (on-prem default) |
|---|---|---|---|---|
| API requests reaching the app | 20 r/s per IP | 50 r/s per key | 100 r/s | none |
| Sync analyses per hour | 30 | 300 | 3,000 | none |
| Async jobs per hour / concurrent | 3 / 1 | 30 / 2 | 300 / 4 | none |
| Frame cells per request | 500,000 | 5 M | 5 M | none |
| Private datasets / storage | — | 5 / 100 MB | 200 / 20 GB | none |
| Saved maps | snapshots only | 50 | unlimited | none |
| Members | — | 1 | 25 | none |
| White-label embeds, read audit, scheduled pulls | no | no | yes | yes |

Numbers are starting values **[A]**. There is no billing integration: a plan is changed by an operator (`lanka app plan set @acme team`). Whether and how the hosted team plan is charged is an owner decision (Q11); the design only needs the table.

## 7. White-label embeds

```sql
CREATE TABLE app.embed_profiles (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  workspace_id bigint NOT NULL REFERENCES app.workspaces (id) ON DELETE CASCADE,
  name text NOT NULL,
  theme jsonb NOT NULL,                 -- colour tokens, font stack, corner radius, logo blob key, link target
  allowed_origins text[] NOT NULL,      -- → Content-Security-Policy: frame-ancestors
  show_platform_credit boolean NOT NULL DEFAULT true,
  allow_external_hosts text[] NOT NULL DEFAULT '{}'
);
CREATE TABLE app.embed_tokens (         -- read access to exactly one map version and the workspace data it references
  token_sha256 bytea PRIMARY KEY, map_id text NOT NULL, seq int4, profile_id bigint NOT NULL REFERENCES app.embed_profiles (id),
  expires_at timestamptz, revoked_at timestamptz
);
```

| Can be changed by a profile | Cannot be changed |
|---|---|
| Colours, fonts, logo, which controls show, the "Lanka Data Layer" credit (team plan and on-prem) | **Data attribution** for every source in the map, the estimate notice, the community/private-data notice, the guard messages of an analysis panel. These come from the resolved document ([10 §2.3](10-map-composition.md#23-pinning-and-resolution)) and licence terms require them. |

- A private map is embedded with `?token=…`; the token grants read access to that map version and the `ws` datasets in its `app.map_refs`, nothing else, and only inside frames whose origin is in `allowed_origins`.
- Custom domains for embeds on the hosted instance are an operator task (a `server_name` and a certbot certificate per domain); on-prem instances use their own domain.
- A script-tag wrapper (`@lanka-data-layer/embed`, a 5 KB loader that creates the iframe and exposes the `postMessage` API of [10 §6.3](10-map-composition.md#63-embeds) as functions) is published with the web app. There is no non-iframe embed in v1: the iframe is the security boundary.

## 8. Plugin points

| Extension | How | Trust model |
|---|---|---|
| Source connector | declarative YAML driver in a registry overlay ([03 §2.2](03-ingestion.md#22-declarative-drivers--the-default-path)); no code | data-only; reviewed like any registry change |
| Recipe / derived dataset | YAML in the overlay ([04 §4](04-enrichment.md#4-recipes)) | data-only |
| Enrichment method, analysis method, code connector, export format | implement the Rust trait (`Method`, `AnalysisMethod`, `Connector`, `Exporter`) in your own crate and build a **distribution binary**: a ten-line `main.rs` that calls `lanka_cli::run(Registry::default().with(acme_methods::register))`. All crates are published; no fork is needed. | compiled in; trusted by whoever builds the binary |
| Map layer type | `registerLayerType(type, {schema, toRenderer})` in the web app; documents carry it as `x-<vendor>-<type>`; readers that do not know it show a placeholder | client-side code, same origin |
| Basemap style, colour ramps, name aliases, codelists | files in the overlay / country profile | data-only |
| Identity provider | OIDC configuration | — |
| Blob storage | `BlobStore` trait ([01](01-layers.md)) | — |
| Outbound integration | none pushed. Integrators poll `/v2/status/events`, `/v2/releases` and `/app/v1/ws/{ws}/events` ([ADR 0016](adr/0016-status-apis-only.md) stands) | — |

Deliberately not built now: a runtime plugin loader. Sandboxed WebAssembly components for analysis methods (a pure `frame → result` function is an ideal fit) are the growth path when a hosted customer needs a custom method; the `AnalysisMethod::run` signature is already pure for that reason.

## 9. Audit trail

```sql
CREATE TABLE app.audit_log (
  id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  at           timestamptz NOT NULL DEFAULT now(),
  workspace_id bigint,
  actor_kind   text NOT NULL CHECK (actor_kind IN ('user','key','embed','operator','system')),
  actor_id     text NOT NULL,
  action       text NOT NULL,          -- 'dataset.upload', 'dataset.visibility', 'dataset.delete', 'map.publish', 'key.create', …
  object_kind  text, object_id text,
  detail       jsonb NOT NULL DEFAULT '{}',     -- before/after of the changed fields; never data content
  ip_hash      bytea,
  prev_hash    bytea NOT NULL, hash bytea NOT NULL       -- sha256(prev_hash ‖ canonical row): tamper-evident chain per workspace
);
```

- `lanka_app` may `INSERT` and `SELECT` only; a trigger rejects `UPDATE`/`DELETE`. The chain head per workspace is exposed so a customer can anchor it elsewhere.
- Always logged: sign-in, membership and role changes, key creation/revocation, uploads, mapping confirmations, visibility changes, deletions, map publishing, embed-token creation, plan changes, operator access to a workspace, takedowns, analyses with workspace inputs.
- **Read auditing** (who read which private dataset, per request) is off by default and a workspace setting on the team plan and on-prem, because it multiplies write volume; it goes to a monthly-partitioned `app.read_log` with 90-day retention.
- `GET /app/v1/ws/{ws}/audit?from=&to=&format=json|jsonl` (admin role). Retention 13 months hosted; configurable on-prem.

## 10. Deployment profiles

One codebase, one licence (Apache-2.0), one set of images. A profile is a compose override plus an environment file in `infra/v2/profiles/`.

| Profile | For | App plane | Anonymous access | Data | Extra services |
|---|---|---|---|---|---|
| `public` | the hosted instance | on | yes | ingests from upstream sources | `analyst` |
| `mirror` | a read-only copy ([08 §7.1](08-openness-and-governance.md#71-self-host-sri-lankas-layer-a-mirror-or-a-private-instance)) | off | yes | `lanka pull --into-db` | none |
| `private` | an organisation's own instance | on, OIDC to the corporate IdP, plan `unlimited` | off by default (`LANKA_ANONYMOUS=off` puts `/v2` behind the same session or key) | public layer by **upstream sync** + own registry overlay + uploads | `analyst`; optionally `valhalla` always on |
| `laptop` | one analyst, offline | off | — | offline pack; `lanka analyze`, `lanka mcp` | none |

**Upstream sync** (`private` profile): a scheduled `sync` job runs `lanka pull --release current --into-db` against a configured upstream (`LANKA_UPSTREAM=https://lanka-data-layer.prabhavalabs.com`), verifying sha256 against the release manifest, so the private instance holds the same dsvs as the public one and analyses fingerprint identically. Air-gapped sites carry the pack files in by hand (`lanka pull --from-dir`). Only `bulk`-tier datasets arrive this way; `api`-tier datasets are fetched per dataset and keep their licence notice ([06 §6](06-serving.md#6-bulk-downloads)).

Sizing **[A]**: `private` for 50 users with the full public layer: 8 vCPU / 16 GB / 200 GB (the same shape as the public host); `mirror`: 2 vCPU / 4 GB / 40 GB.

**Everything is open source.** There is no enterprise edition and no licence key. What an organisation can pay for is hosting, a managed private instance, onboarding of its data, and support (Q11). This keeps one code path, lets the fork test of [07 §8](07-operations.md#8-testing-strategy) cover the whole product, and means a government body can run the full system without procurement of a licence.

## 11. Security: what changes

The threat model of [07 §5](07-operations.md#5-security) assumed no accounts, no personal data and no uploads. With the app plane on, add:

| Asset / threat | Control |
|---|---|
| Confidentiality of workspace data across tenants | schema separation, role separation, RLS, `Scope`-typed queries (§2); an integration test creates two workspaces and asserts every `/app/v1` route returns 404 for the other's objects; a fuzz test replays the public corpus with a workspace principal and asserts no `ws` row appears under `/v2` |
| Session theft, CSRF | opaque `HttpOnly; Secure; SameSite=Lax` cookie on `Path=/app`; `Origin` check on cookie-authenticated writes; 30-day idle expiry; sign-out everywhere |
| CORS | `/v2`: `Access-Control-Allow-Origin: *`, no credentials (unchanged). `/app/v1`: bearer keys from any origin; cookies only from the deployment's own origin |
| Key leakage | prefix for scanners, hashed at rest, shown once, scoped, revocable, `last_used_at`; never logged (the redaction layer of [07 §5.4](07-operations.md#54-secrets) covers `Authorization`) |
| Malicious uploads | the untrusted-document controls of [07 §5.1](07-operations.md#51-untrusted-document-handling): pure-Rust parsers with caps, GDAL only in the network-less sandbox, formula-injection escaping on export, zip-bomb limits. The sentence "no endpoint accepts … a file upload" now reads "no `/v2` endpoint". |
| Stored content shown to others (titles, descriptions, annotations, field names) | text only; CommonMark subset rendered with an allow-list; no HTML, no images, no scripts; links get `rel="noopener nofollow ugc"` |
| Abuse of anonymous snapshots | 5 per hour per IP, 256 KB, `noindex`, takedown path, `withdrawn` flag |
| Impersonation of official bodies | reserved handles; the `COMMUNITY_DATA` notice; class `user` can never appear in the `official` group |
| Server-side request forgery | the server never fetches a URL from a map document (`external` sources are browser-only); scheduled pulls use the hardened fetcher |
| Resource exhaustion through analysis | cost model, lanes, per-principal budgets, wall-clock limits ([11 §8](11-analysis-engine.md#8-where-the-computation-runs), [§12](11-analysis-engine.md#12-limits)) |
| Personal data | minimal account data (§3); deletion within 24 h; uploader responsibility for content with defaults that reduce risk (§5.4); a privacy notice and terms are prerequisites of enabling sign-in on the public instance (Q12) |
| Backups | the `ws` schema and `store/ws/` are in the same encrypted backup repositories; restore drills include an RLS check |

## 12. What is deliberately not built

- Billing, invoices, payment providers.
- Passwords, e-mail sending, multi-factor of our own (the IdP's applies).
- Per-object ACLs, sharing with named individuals outside a workspace, comments, real-time collaborative editing. Two people editing one map get optimistic concurrency (`If-Match` on `head_seq`) and a merge prompt.
- A SQL console or notebook over private data. The answer remains Parquet export and DuckDB on the user's machine.
- Multi-country tenancy in one deployment ([08 §7.2](08-openness-and-governance.md#72-fork-for-another-country) stands).
- Row-level access rules inside a dataset.
