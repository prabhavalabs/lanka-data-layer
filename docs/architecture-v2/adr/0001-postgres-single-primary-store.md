# ADR 0001 — PostgreSQL + PostGIS is the single primary store

Status: proposed · 2026-11-21 · Detail: [05](../05-storage-and-performance.md)

## Context

The owner's binding constraints name PostgreSQL (+PostGIS) as the primary database on one self-hosted VPS (8 vCPU / 15 GB / 150 GB). The foundation already runs PostgreSQL 18 + PostGIS 3.6 with four schemas. The earlier revamp plan proposed SQLite for serving and DuckDB + Parquet for build-time work.

## Decision

All catalogue, operational, tabular, vector and lineage data lives in one PostgreSQL database with six schemas: `catalog`, `ops`, `geo`, `data`, `serve`, `stage`. Grid arrays and files live on the blob store and are only indexed in Postgres. SQLite survives as an export format (offline pack, legacy `lanka.sqlite`); Parquet is an export and extract format, not a query engine in the server.

## Consequences

- One backup story, one access-control story (roles), one query dialect in the server.
- Recipes can join inputs with SQL where the data already is.
- Postgres memory (6 GB cap) is the largest fixed consumer on the host.
- Offline use needs a second backend (ADR 0015).

## Alternatives rejected

- **SQLite as the serving store + DuckDB for builds** (earlier plan): contradicts the constraint; single-writer; no row-level concurrency for a continuously ingesting system.
- **A separate analytics store (ClickHouse, DuckDB server)**: a second database to operate for ~10^8 rows that Postgres handles with partitioning.
- **TimescaleDB / Citus extensions**: licence and upgrade coupling for features plain declarative partitioning already provides here.
