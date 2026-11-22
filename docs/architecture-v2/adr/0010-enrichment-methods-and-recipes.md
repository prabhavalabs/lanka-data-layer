# ADR 0010 — Enrichment as a method library plus declarative recipes

Status: proposed · 2026-11-21 · Detail: [04](../04-enrichment.md)

## Context

The value-added programme (20 products, ~108 person-days) needs crosswalks, interpolation, dasymetric mapping, zonal statistics, routing, exposure, small-area estimation and composites. The foundation has one hand-coded derived indicator (sex ratio) inside a connector and an `enrichment` free-text block in the registry.

## Decision

A crate `lanka-enrich` holds **methods**: versioned Rust implementations of a `Method` trait with declared inputs, a parameter schema, an uncertainty kind, mandatory flags, and an `explain` function. **Recipes** in `registry/recipes/` bind methods, parameters and input datasets to one output dataset. A run records a `data.derivations` row with a fingerprint over recipe, method versions, params, tool versions and input dsvs; a recipe is stale when no derivation has the current fingerprint. Recipes run inside the release train in topological order. Tabular methods use SQL in Postgres; grid methods use Rust over mmap arrays; routing uses Valhalla in a batch profile; statistics are Rust checked against R fixtures. The runner, not the method, enforces propagation of `basis`, `status` and flags. Estimates live in separate indicators (`est.`, `idx.`, `acc.`, `exp.`).

## Consequences

- A derived dataset is an ordinary versioned dataset with complete lineage.
- Incremental recompute falls out of fingerprints; a tools-image upgrade correctly invalidates dependent recipes.
- New products are mostly YAML once the method exists.
- No Python/R at runtime: statistical methods must be implemented and validated in Rust.

## Alternatives rejected

- **dbt**: SQL-only; cannot express grid, routing or statistical methods; no uncertainty contract.
- **Python (geopandas/rasterio/statsmodels) workers**: fastest to prototype, but a second runtime, heavier images and weaker determinism on a 15 GB host; revisit only if a method proves impractical in Rust.
- **Dagster/Airflow for the DAG**: a second product for a DAG of ~40 nodes.
- **Derived indicators coded inside connectors** (status quo): no lineage, no reuse.
