# ADR 0005 — Provenance in a side table; derived lineage through derivations

Status: proposed · 2026-11-21 · Detail: [02 §9](../02-standard-model.md#9-provenance), [05 §2.4](../05-storage-and-performance.md#24-provenance-side-table)

## Context

0001 keeps artifact, locator, verbatim, method, confidence and review inline on every observation, arguing a join would double the hot path. But the hot paths (series, map, profile) never read provenance, and most future rows are derived: their "locator" would be the same formula repeated millions of times.

## Decision

Extracted rows get one row in `data.obs_provenance` (artifact, extract row, locator, verbatim, method, confidence, review), fetched by `obs_id` on demand. Derived rows have none: lineage is `data.derivations` (recipe, method versions, params, input dsvs, tool versions, git rev) plus `Method::explain` for the contributing input rows. Both are exposed as PROV-O.

## Consequences

- Hot table stays narrow; provenance storage is proportional to extracted rows (~10^7), not all rows.
- Provenance of a derived value is computed, not stored; `explain` must be implemented and tested for every method.
- One extra index lookup for the provenance endpoint (not latency-critical).

## Alternatives rejected

- **Inline provenance** (status quo): right for 10^4 rows of document data, wrong for 10^8 derived rows.
- **Per-row lineage edges for derived data**: billions of edges for zonal statistics.
- **Dataset-level provenance only**: loses the cell-level evidence that is the platform's main trust feature.
