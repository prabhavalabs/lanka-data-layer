# ADR 0021 — Analysis is align → compute → guard over the enrichment methods, with fingerprinted results

Status: proposed · 2026-11-22 · Detail: [11](../11-analysis-engine.md)

## Context

Users want to relate any two or more layers (flood hazard vs property prices) visually and statistically. Enrichment ([ADR 0010](0010-enrichment-methods-and-recipes.md)) produces curated datasets inside the release train; it is not an on-demand capability.

## Decision

A crate `lanka-analysis` runs requests (`ldl-analysis/1`) through a fixed pipeline: resolve and pin inputs → plan (target support, alignment method per variable, cost) → **align** every variable onto one support using the enrichment methods (`rollup`, `grid_zonal`, `features_zonal`, `crosswalk_rebase`, `dasymetric`, `spatial_interp`) plus `point_aggregate` and `sample` → **compute** with a pure kernel from `lanka-stats` → evaluate **guards** → present outputs as frames, grids or features with suggested map layers. Planner rules are binding: aggregate up by default, never disaggregate silently, prefer point support when a variable is a point sample, do all polygon overlays on the canonical grid. Results are immutable and identified by the recipe fingerprint rule applied to the resolved request (`an_<hash>`), cached in schema `analysis`, and never written to `data.*`. Cheap analyses run synchronously in `lanka serve`; others run in an `interactive` job lane served by a second worker process (`analyst`). `lanka-stats` has no I/O and compiles to WebAssembly, so browser previews and classification use the same code. A useful analysis is promoted to a recipe through the normal review path.

## Consequences

- One implementation of every statistic across server, CLI, MCP and browser.
- Identical requests are computed once; a release that does not change an input keeps the cache.
- Results are reproducible and citable (`/id/analysis/{id}`).
- Statistics must be implemented and validated in Rust against R fixtures; one more compose service and ~1.5 GB of the memory budget.

## Alternatives rejected

- **PostGIS/SQL for the statistics**: no permutation inference, spatial regression or robust fits; heavy load on the primary.
- **A Python/R worker (PySAL, spdep)**: fastest to build, but a second runtime, no browser reuse, weaker determinism — the same reasoning as ADR 0010.
- **Client-side only**: cannot see the grid store, derivation graph or private data rules; results would not be citable.
- **Making every analysis a recipe**: analyses are user-driven and unreviewed; they must not enter releases.
