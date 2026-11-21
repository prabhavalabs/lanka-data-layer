# ADR 0015 — One query core with a Postgres backend and an offline pack backend

Status: proposed · 2026-11-21 · Detail: [06 §1](../06-serving.md#1-one-query-core-two-backends)

## Context

REST, OGC, MCP, CLI and exports must agree on every number. The CLI must also work offline (`geocode-csv`, area and series lookups) where there is no Postgres.

## Decision

`lanka-query` owns request types, validation, identifier resolution, period and roll-up semantics, envelope/`meta`, hints and `next` links. Storage access is a `Backend` trait with two implementations: `PgBackend` (server) and `PackBackend` (SQLite pack + grid arrays from a release). Transports are thin. A conformance corpus of request → payload runs against both backends in CI and must produce identical JSON.

## Consequences

- New transports or formats cost a thin adapter.
- ~12 SQL statements exist in two dialects; the corpus is the guard against drift.
- The pack format becomes a published artifact with its own compatibility rule (tied to the spec MAJOR).

## Alternatives rejected

- **Offline mode = embedded Postgres**: not practical in a single-file CLI.
- **Server on SQLite/DuckDB too**: contradicts ADR 0001.
- **Offline CLI reading Parquet via DuckDB**: a large native dependency and a third SQL dialect.
- **Separate implementations per transport**: the divergence this ADR exists to prevent.
