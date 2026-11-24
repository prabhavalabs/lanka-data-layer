# 07 — Operations

## 1. Deployment topology

One host, Docker Compose, every port on `127.0.0.1`, host nginx + certbot as the only public listener. **No Cloudflare**: DNS points straight at the VPS; TLS terminates in nginx; DDoS posture is rate limits + cache + Hetzner's network-level protection.

```yaml
# infra/v2/docker-compose.yml (target shape; `db` exists today)
name: lanka-v2
services:
  db:        # imresamu/postgis:18-3.6.1-trixie (kept) · 127.0.0.1:55432 · mem_limit 6g · shm_size 1g · volume lanka_v2_pgdata
  serve:     # ghcr.io/…/lanka:<tag> · command: lanka serve · 127.0.0.1:8601 · DATABASE_URL as lanka_api
             # volumes: /srv/lanka/store:/store:ro · mem_limit 1g · read_only rootfs · user 10001 · cap_drop ALL
  worker:    # same image · command: lanka worker · no published port · DATABASE_URL as lanka_ingest (+ LANKA_RECIPE_DATABASE_URL)
             # volumes: /srv/lanka/store:/store:rw, /srv/lanka/registry:/registry:ro, lanka_run:/run/lanka
             # mem_limit 2g · cpus 5 · user 10001 · cap_drop ALL
  tools:     # ghcr.io/…/lanka-tools:<tag> · command: lanka-extractd --socket /run/lanka/extract.sock
             # network_mode: none · read_only rootfs · tmpfs /tmp (size 2g) · volumes: /srv/lanka/store:/store:ro,
             # /srv/lanka/store/tmp:/work:rw, lanka_run:/run/lanka · mem_limit 2g · pids_limit 256 · user 10002
             # cap_drop ALL · security_opt no-new-privileges
  valhalla:  # profiles: [batch] · ghcr.io/valhalla/valhalla:<pinned> · 127.0.0.1:8602 · mem_limit 3g
             # started/stopped by the operator or by `lanka recipe run` through `docker compose --profile batch`
  backup:    # profiles: [backup] · pgBackRest + restic image, run by a host systemd timer (the one timer in the system)
volumes: { lanka_v2_pgdata: {}, lanka_run: {} }
```

Two images:

| Image | Base | Contains |
|---|---|---|
| `lanka` | `gcr.io/distroless/cc-debian13` | the `lanka` binary (static-ish; rustls, plus the image's libssl3 for the profile's `fetch.legacy_tls_hosts`), CA certificates, tzdata |
| `lanka-tools` | `debian:trixie-slim` | `lanka-extractd`, `poppler-utils`, `tesseract-ocr` + `-sin` + `-tam`, `gdal-bin`, `tippecanoe`, `pmtiles`; versions pinned and printed by `lanka-extractd --versions` (recorded in every extract/derivation) |

How the worker starts Valhalla without the Docker socket: it does not. Routing recipes are `schedule: monthly|manual`; the operator (or a host systemd timer calling `infra/v2/run-batch.sh`) brings the `batch` profile up, the worker sees the service healthy and runs the queued routing jobs, the script brings it down. The worker never controls containers.

Host layout:

```
/srv/lanka/store/      blob store (LANKA_STORE_DIR)            /srv/lanka/registry/   git checkout of registry/ at the deployed tag
/srv/lanka/valhalla/   routing tiles                           /etc/lanka/v2.env      secrets (0600 root)
/var/cache/nginx/lanka proxy cache                             /var/log/lanka/        JSON logs (journald for containers)
```

nginx (`infra/nginx/lanka-v2.conf`): `location /v2/` and `/mcp` → `127.0.0.1:8601` with `proxy_cache`; `location /dl/` → `alias /srv/lanka/store/` restricted by an allow-list of subtrees (`datasets/`, `releases/`, `tiles/`) — `raw/`, `extract/`, `evidence/`, `tmp/`, `grids/` are never reachable; `location /v1/` unchanged.

## 2. Process model

| Process | Count | State |
|---|---|---|
| `lanka serve` | 1 (N behind nginx `upstream` after scale-out S3) | stateless; catalogue snapshot + mmaps |
| `lanka worker` | exactly 1 leader (advisory lock `lanka_worker_leader`); a second instance idles as standby | all state in Postgres |
| `lanka-extractd` | 1 | stateless; one request at a time per kind |

Zero-downtime deploy of `serve`: start the new container on `127.0.0.1:8611`, health-check, switch the nginx upstream (`nginx -s reload`), stop the old one. `infra/v2/deploy.sh` does this; the worker is restarted between jobs (SIGTERM → finishes the current stage, re-queues the job).

## 3. Job runner

```sql
CREATE TABLE ops.jobs (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  kind        text NOT NULL CHECK (kind IN ('ingest','backfill','restandardise','recipe','release','export','tiles','check','maintenance')),
  dedupe_key  text NOT NULL,                 -- e.g. 'ingest:worldbank-wdi', 'export:<dsv>'
  payload     jsonb NOT NULL DEFAULT '{}',
  state       text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued','running','succeeded','failed','cancelled')),
  priority    int2 NOT NULL DEFAULT 100,     -- lower runs first: release 10, ingest 50, export 100, recipe 150, backfill 200
  weight      int2 NOT NULL DEFAULT 1,       -- slots consumed (heavy = 4)
  run_after   timestamptz NOT NULL DEFAULT now(),
  attempts    int2 NOT NULL DEFAULT 0, max_attempts int2 NOT NULL DEFAULT 3,
  locked_by   text, locked_at timestamptz, heartbeat_at timestamptz,
  trigger     text NOT NULL CHECK (trigger IN ('schedule','manual','retry','backfill','train','upstream_change')),
  created_at  timestamptz NOT NULL DEFAULT now(), finished_at timestamptz,
  result      jsonb, error text
);
CREATE UNIQUE INDEX jobs_dedupe ON ops.jobs (dedupe_key) WHERE state IN ('queued','running');
CREATE INDEX jobs_claim ON ops.jobs (priority, run_after, id) WHERE state = 'queued';
```

Claim: `UPDATE ops.jobs SET state='running', locked_by=$1, … WHERE id = (SELECT id FROM ops.jobs WHERE state='queued' AND run_after <= now() AND weight <= $free ORDER BY priority, run_after, id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING *`. Each running job also holds a session advisory lock on its id; a `running` row whose lock is free is abandoned and is re-queued by the next tick (the foundation's crash-detection idea, generalised from runs to jobs). `LISTEN lanka_jobs` wakes the worker immediately on enqueue; the 60 s tick is the fallback.

`ops.ingestion_runs` and `ops.run_events` stay as the per-run record (an ingest job creates a run); `run_events.stage` gains `extract`, `standardise`, `recipe`, `release`, `export`.

## 4. Health and status APIs

The platform **emits** state; it does not notify anyone. Alert channels, paging, dead-man's switches and uptime checks belong to the owner's separate monitoring system, which polls these endpoints. Consequently `LANKA_ALERT_WEBHOOK`, `LANKA_HEARTBEAT_URL`, `run::raise_alert` and `ops.alerts` are removed ([ADR 0016](adr/0016-status-apis-only.md)).

| Endpoint | Purpose | Shape |
|---|---|---|
| `GET /v2/health` | liveness + readiness for nginx and the monitor; < 5 ms; never cached | `200 {status:"ok", data_version, db:{ok, latency_ms}, store:{ok}, grids_mapped, uptime_s, engine_version, git_rev}`; `503` when Postgres is unreachable or no release is loaded. `?deep=1` adds PostGIS version, migration version, replica lag |
| `GET /v2/status` | one-screen summary; the monitor's main probe | `{overall: ok\|degraded\|failing, generated_at, worker:{leader, last_tick_at, tick_age_s, queue:{queued, running, oldest_queued_age_s}}, release:{data_version, cut_at, age_h, last_train:{at, outcome}}, sources:{fresh, due, overdue, failing, in_review, expected_missing, paused, never_run}, incidents_open, disk:{store_pct, pg_pct}, backups:{pg_last_ok_at, store_last_ok_at}}` |
| `GET /v2/status/sources?state=` | per-source freshness | `[{source_id, state, cadence, last_success_at, last_run:{id, status, at}, consecutive_failures, next_check_at, expected:{edition, date}}]` |
| `GET /v2/status/datasets?state=` | per-dataset currency | `[{dataset_id, dsv, published_at, vintage, stale_inputs, review_open, quality:{errors, warnings}}]` |
| `GET /v2/status/runs?since=&source=&status=&cursor=` | run history | paged run records with counts and outcome |
| `GET /v2/status/events?after=<event_id>&level=warn&limit=` | **run-event feed**: append-only, strictly increasing ids, so a poller keeps one integer of state | `[{id, at, level, kind, source_id?, dataset_id?, run_id?, job_id?, message, data}]`; `Accept: application/x-ndjson` supported |
| `GET /v2/status/incidents?open=1` | conditions that persist until resolved | `[{id, kind, subject, opened_at, last_seen_at, closed_at, detail}]` |

`overall` rule (computed, documented, stable): `failing` if the worker tick is older than 5 min, or the last train failed, or any P0 source is `failing`; `degraded` if any source is `failing`/`overdue`/`expected_missing`, the queue's oldest job is > 6 h, disk > 85 %, or the last successful backup is > 36 h old; else `ok`. The HTTP status of `/v2/status` is always 200 (the body says what is wrong) so that a monitor distinguishes "platform says it is failing" from "platform unreachable".

```sql
CREATE TABLE ops.incidents (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  kind text NOT NULL,        -- source_failing | template_miss | url_pattern_broken | expected_missing | recipe_failed |
                             -- cross_check_failed | repro_mismatch | disk_pressure | backup_stale | default_partition_rows
  subject text NOT NULL,     -- source/dataset/recipe id
  opened_at timestamptz NOT NULL DEFAULT now(), last_seen_at timestamptz NOT NULL DEFAULT now(), closed_at timestamptz,
  detail jsonb
);
CREATE UNIQUE INDEX incidents_open ON ops.incidents (kind, subject) WHERE closed_at IS NULL;
```

Incidents open and close automatically (a source that succeeds closes its `source_failing` incident). Backup freshness reaches the database because the backup script ends by calling `lanka ops record-backup --kind pg|store --ok`.

Internal observability (not public): JSON logs to stdout → journald; `GET /internal/metrics` (Prometheus text format, bound to `127.0.0.1:8691`, not proxied) with request histograms by route, pool saturation, cache-hit ratio, job durations, rows staged, grid page faults. It exists so the owner's monitor *can* scrape it; nothing in this design depends on a scraper being present. `pg_stat_statements` + the 250 ms slow-query log stay on.

## 5. Security

Threat model (public data plane): the platform is public, read-only and anonymous; there are no user accounts and no personal data. When the app plane is enabled, the additional assets, threats and controls are those of [12 §11](12-enterprise-and-byod.md#11-security-what-changes). What needs protecting is (1) integrity of published numbers, (2) availability, (3) the host — the realistic attack surface being **untrusted documents** from ~170 upstream sites parsed by C/C++ tools.

### 5.1 Untrusted document handling

| Control | Detail |
|---|---|
| Isolation | PDF, OCR, raster, vector-conversion and tile tools run only in the `tools` container: `network_mode: none`, read-only root filesystem, `tmpfs` scratch, non-root uid, all capabilities dropped, `no-new-privileges`, default seccomp, a process cap (`deploy.resources.limits.pids`), a memory cap. It sees the store read-only plus one writable work directory. It has no database credentials and no secrets. |
| Protocol | The worker talks to `lanka-extractd` over a Unix socket on a shared volume: `{op, input: blob key, args (validated enum, never a shell string), limits}` → `{ok, outputs: [work-dir paths], stats}` or an error. No shell is involved; arguments are passed as argv arrays from a fixed allow-list per op. |
| Per-request limits | wall-clock timeout (60 s PDF text, 300 s OCR, 900 s raster warp, 1,800 s tippecanoe), `RLIMIT_AS`, `RLIMIT_FSIZE`, `RLIMIT_NOFILE`, `RLIMIT_CPU`; max pages 2,000; max output 2 GB; the child is killed and the work directory wiped on any breach. |
| In-process parsers | Pure-Rust parsers only (`calamine`, `csv`, `serde_json`, `quick-xml`, `osmpbf`, `zip`) with explicit caps: decompressed size ≤ 20× and ≤ 4 GB, ≤ 5 M cells per workbook, ≤ 200 MB JSON, entry count and path-traversal checks on archives. `unsafe_code = "forbid"` stays at workspace level. |
| Fetching | Only URLs derived from registry entries (host allow-list = hosts named in `registry/sources/`), HTTPS required except for listed legacy `.gov.lk` hosts, redirects followed only within the allow-list, private/link-local address ranges refused (SSRF), response size capped per source (default 512 MB, 8 GB for rasters/PBF), content-type sniffed against the expected media type. |
| Output handling | Extracted text is data: it is never interpolated into SQL (binary COPY + bound parameters only), shell, HTML without escaping (evidence sheets escape everything and are served on `127.0.0.1`), or LLM prompts without the anchoring gate ([03 §3.3](03-ingestion.md#33-class-c--llm-assisted-human-signed)). |
| Formula/CSV injection | CSV exports prefix cells beginning with `= + - @` (text columns only) with `'`. |

### 5.2 Database and services

- Roles of [05 §7](05-storage-and-performance.md#7-roles-pooling-timeouts): the public process connects as `lanka_api` (read-only, 5 s timeout). A SQL injection in the API — already structurally prevented by prepared statements — could still not write.
- Postgres listens on the compose network and `127.0.0.1:55432` only; `scram-sha-256`; no superuser use by applications.
- No endpoint accepts SQL or a URL to fetch. No `/v2` endpoint accepts a file upload; uploads exist only in the app plane and are handled as untrusted documents ([12 §5](12-enterprise-and-byod.md#5-bring-your-own-data)).
- Containers: non-root, read-only root filesystems, `cap_drop: ALL`, no Docker socket anywhere.
- Host: existing hardening (key-only SSH, ufw allowing 22/80/443, fail2ban, unattended-upgrades) is assumed from the VPS notes **[A]**.

### 5.3 Abuse and availability

nginx `limit_req` zones keyed on `$binary_remote_addr`: `api` 20 r/s burst 60; `heavy` (`/v2/data`, `/v2/radius`, OGC items, grid PNG tiles) 5 r/s burst 20; `mcp` 60 r/min; `dl` connection limit 4 per IP and `limit_rate_after 50m` → `limit_rate 5m`. Responses are cacheable by construction, so the cache absorbs repeated load. `lanka serve` adds a global concurrency limit (`tower::limit`, 64 in flight) and per-route timeouts; over the limit it sheds with 503 + `Retry-After` rather than queueing.

### 5.4 Secrets

`/etc/lanka/v2.env` (root, 0600), injected by Compose `env_file`; never in the repository, images or registry. Contents: three database passwords, upstream API keys (per source, `LANKA_KEY_<SOURCE_ID>`; only the worker gets them), backup repository credentials (only the backup profile gets them). `LANKA_LLM_*` exists only in the operator's shell when running `lanka extract propose`. Rotation: database passwords by `ALTER ROLE` + env change + restart (documented runbook, twice a year); upstream keys on publisher notice. Logs never print values of `LANKA_KEY_*` (redaction layer in the HTTP client's tracing).

### 5.5 Supply chain

Exact-pinned crate versions (kept) + `Cargo.lock` committed; `cargo deny` (advisories, licences, bans) and `cargo audit` in CI; images pinned by digest; `lanka-tools` rebuilt monthly for Debian security updates (a rebuild that changes a tool version shows up as stale recipes — intended); release binaries built in CI with provenance attestation (as the npm packages already have).

## 6. Backups and recovery

| What | Tool | Schedule | Retention | Target |
|---|---|---|---|---|
| Postgres | **pgBackRest**: weekly full, daily differential, continuous WAL archive (`archive_mode = on`) → point-in-time recovery | 01:00 local | 4 fulls + their diffs/WAL | off-site SSH/SFTP repository (a Hetzner Storage Box or any SSH host the owner controls), client-side encrypted |
| Blob store (`raw/`, `extract/`, `evidence/`, `datasets/`, `releases/`, `grids/`, `tiles/`) | **restic**, same target, separate repository | 01:30 local | 7 daily, 4 weekly, 12 monthly | same |
| Registry, code, infra | git (already) | — | — | GitHub + any mirror |
| Secrets | operator's password manager | on change | — | — |

`raw/` is the irreplaceable part (documents disappear from government sites); everything else can be rebuilt from raw + extracts + code, so if the off-site quota is tight the order of sacrifice is `tiles/` → `grids/` → `datasets/`.

RPO: ≤ 5 min for Postgres (WAL archive timeout 300 s), ≤ 24 h for the store. RTO target: 2 h to a fresh VPS (`infra/v2/restore.sh`: provision → restore pgBackRest → restore restic → `lanka serve rebuild` → deploy).

**Restore drill**, quarterly, scripted (`infra/v2/restore-drill.sh`): restore the latest backup into a scratch container, run `lanka rebuild --verify` on three datasets and the conformance corpus against it, record the outcome with `lanka ops record-backup --kind drill`. `/v2/status.backups` exposes last-ok times; a stale backup becomes an incident.

## 7. CI/CD

GitHub Actions is used for build and test only (never as the production scheduler).

| Workflow | Trigger | Does |
|---|---|---|
| `engine-ci` | PR, push | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` (unit, property, golden), `cargo deny`; Postgres service container for integration tests; OpenAPI diff against `spec/openapi/v2.json` |
| `registry-ci` | PR touching `registry/` | `lanka registry validate`, `lanka recipe check`, `lanka connector test` for every changed connector with fixtures, licence-computation check, DAG acyclicity |
| `conformance` | PR, nightly | corpus against PgBackend and PackBackend; OGC ETS (Features Core), DCAT SHACL, Frictionless, STAC, COG validators on a fixture release |
| `images` | tag `v*` | build `lanka` and `lanka-tools` (amd64), SBOM, provenance attestation, push to GHCR |
| `bench` | nightly on `develop` | benchmark suite on a fixed fixture database; fails on > 30 % regression |

Deploy: `infra/v2/deploy.sh <tag>` over SSH (extends the existing `infra/deploy.sh` pattern): pull images by digest → `lanka db migrate` (as `lanka_owner`) → sync `/srv/lanka/registry` to the tag → `lanka registry sync` → blue/green `serve` → restart `worker`. Migrations are forward-only and backward-compatible with the previous binary for one release (expand → migrate → contract), so a rollback is "redeploy the previous tag".

Branching and commits follow the repository rules (base `develop`, `main` = production, `type: description`).

## 8. Testing strategy

| Kind | Scope | Where |
|---|---|---|
| Unit | grid vectors (kept, shared JSON with TS), p-codes, periods, units, dims canonicalisation, multiset hash, envelope, locator grammar | each crate |
| Property (`proptest`) | period parse/format round-trip; dims canonicalisation idempotent; multiset hash order-independence and add/remove inverse; range-version invariants (for any sequence of stage/release/reject operations, reading version *k* returns exactly the rows staged for *k*); method invariants ([04 §9](04-enrichment.md#9-testing-methods)); `cell_id` ↔ `cell_bounds` consistency | `lanka-core`, `lanka-ingest`, `lanka-enrich` |
| Golden files | per connector: fixture artifact → expected extract (Parquet hash) → expected rows (dsv). Fixtures are small real files where the licence allows committing them, otherwise synthetic files of the same layout; large real files are cached in CI by sha256 and fetched from the project's own `/dl` | `engine/crates/lanka-ingest/tests/fixtures/<source>/` |
| Template tests | each PDF template edition against one stored page set; asserts cell count, the template's own sum checks, and 5 pinned cells | `registry/templates/**/tests/` |
| Integration (Postgres) | the existing end-to-end test (throwaway database) extended: migrations from 0001 → head with data, range versioning, release train, recipe fixpoint, withdrawal, roles (API role cannot write) | `engine/crates/lanka-ingest/tests/pg_integration.rs` |
| Conformance corpus | request → payload, both backends | `engine/tests/corpus/` |
| **Parity corpus vs v1** | (a) data: every value in `lanka.sqlite` (admin_population, admin_stats, elections, cells sample, cell_lookup sample) equals the v2 value for the mapped indicator — the 8,395/8,395 census comparison generalised; (b) API: recorded `/v1` responses replayed against the Rust `/v1` implementation | `engine/tests/parity_v1/` |
| Parity vs reference platform | for each dc-parity row marked done: record count, period coverage and a sample of values compared with the reference platform's public figures; results stored as `docs/data/parity-report.md` (generated) | `tools/parity/` |
| Reproducibility | nightly rebuild of three random published versions ([01](01-layers.md#reproducibility)) | `check` job |
| Fork test | build a release for a toy second country profile (`country/zz-test/`: 3 levels, 20 units, a 0.01° grid, two datasets) in CI — proves no `LK` assumptions leak into the engine | `engine/tests/fork/` |
| Load | `oha` scenarios against a fixture release | `bench` workflow |
| Security | fuzz targets (`cargo fuzz`) for the period parser, dims parser, locator parser, connector-spec selectors and the PMTiles reader; a corpus of malformed PDFs/XLSX/zips through the sandbox asserting clean failure within limits | `engine/fuzz/` |
