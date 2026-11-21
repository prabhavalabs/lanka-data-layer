# ADR 0004 — Narrow observations with integer keys and version ranges

Status: proposed · 2026-11-21 · Detail: [05 §2–§3](../05-storage-and-performance.md#2-observations)

## Context

0001 stores text keys, inline JSONB provenance, and a full copy of the dataset per version (4 versions retained). At the planned scale (~10^8 rows, ADM4 series, daily sources) that means ~220 B rows, a rewrite of millions of rows to append one period, and no history beyond four versions. Only ~10,000 rows exist today, so changing is cheap now and expensive later.

## Decision

`data.observations` keeps LIST partitioning by dataset but uses surrogate `int4` keys (dataset, indicator, geo, dims), `(freq, time_start)` for the period, `int2` codes for `basis`/`status`, a `flags` bitmask, `lo`/`hi`, and `(vfrom, vto)` dataset-local version ranges. Staging inserts only new or changed rows; release closes superseded rows; current reads use partial indexes `WHERE vto = MAX`. The dsv becomes an additive multiset hash, so it is incremental and order-independent.

## Consequences

- ~72–88 B rows; appending a period writes only that period; full `as_of` history at no extra cost.
- One open (unreleased) version per dataset at a time.
- The `vto` close is a non-HOT update; batched per release and vacuumed.
- Surrogate keys are internal; APIs, exports and hashes use codes, so p-codes remain the only admin join key and dsvs are database-independent.
- Existing four dsvs change once (old values kept as `legacy_dsv`).

## Alternatives rejected

- **Full copy per version** (status quo): simple, but fails on daily and large series.
- **Delta versions (version = base + patches)**: every read must merge a chain.
- **Temporal tables / `tstzrange` with exclusion constraints**: heavier indexes, same semantics.
- **Text keys kept**: ~40 B/row and much larger indexes for no functional gain.
- **Partition by hash or by time instead of dataset**: loses per-dataset drop/rebuild/takedown and pruning by a constant.
