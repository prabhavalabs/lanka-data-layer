# ADR 0011 — One worker process with a Postgres job queue

Status: proposed · 2026-11-21 · Detail: [07 §3](../07-operations.md#3-job-runner), [03 §4](../03-ingestion.md#4-scheduling-and-change-detection)

## Context

The foundation plans a `lanka scheduler` loop that calls `run_source` directly. The target system also runs recipes, release trains, exports, tile builds, backfills and checks, with heavy and light jobs competing for one host.

## Decision

`lanka worker` = scheduler tick (60 s, plus `LISTEN`) + executor over `ops.jobs` claimed with `FOR UPDATE SKIP LOCKED`, priorities, slot weights (one heavy job at a time) and advisory-lock crash detection. One leader at a time (advisory lock); a second instance is a standby. Due-ness is computed from the registry and run history. The only host timer is the backup (and the optional batch-profile script), which must work when the application is down.

## Consequences

- All job state is queryable SQL and appears in `/v2/status`.
- No broker, no extra service; restart-safe by construction.
- Throughput is bounded by one host (acceptable: ~10^2–10^3 jobs/day).
- Moving the worker to a second box (scale-out S2) is a connection-string change.

## Alternatives rejected

- **systemd timers per source**: due-ness is data, not unit files; 174 units; no retries or dedupe.
- **cron in a container**: silent death — the reference platform's failure mode.
- **Airflow / Dagster / Temporal**: a second product with its own database and UI.
- **Redis/NATS queue, pgmq extension**: another dependency for what one table does.
- **GitHub Actions as scheduler**: best-effort timing, external dependency for production.
