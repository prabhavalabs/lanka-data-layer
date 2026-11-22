# ADR 0024 — Private data lives in a parallel `ws` schema and enters through the ingestion pipeline

Status: proposed · 2026-11-22 · Detail: [12 §4–§5](../12-enterprise-and-byod.md#4-the-workspace-data-plane)

## Context

Users and enterprises hold data that is not public (prices, customers, assets, survey samples) and want to map and analyse it against the public layer.

## Decision

Schema `ws` repeats the standard model's shapes (`datasets`, `indicators`, `dataset_versions`, `observations`, `features`) with a `workspace_id`, row-level security and dataset keys from 1,000,000 upward. Private identifiers are written `@workspace/id`. Uploads (CSV, XLSX, GeoJSON, GeoParquet; Shapefile/GeoPackage/KML through the sandbox) pass the same stages as upstream documents — archive, extract, standardise, validate, version — with two interactive steps: a confirmed column **mapping** stored with the version, and a **match report** for geography that could not be resolved. Geocoding modes are p-code, reviewed name aliases, coordinates through the canonical grid, place/postal lookup, geometry, and cell id; there is no street geocoder and names are never guessed. Data is private by default; `public` visibility lists it as community data (class `user`, always labelled) and does not make it part of the registry or of releases. `lanka-query` takes a `Scope` so frames, tiles, analysis and exports work on both planes.

## Consequences

- Every feature built for public data works on private data with no second implementation.
- Private data never enters releases, packs, bulk downloads or `/v2`.
- Uploads are a new attack surface, handled by the existing untrusted-document controls.
- The platform takes on deletion, minimisation and rights obligations for user content.

## Alternatives rejected

- **Private datasets in `data.observations` with a visibility flag**: one missed predicate leaks data; partitions and releases mix tenants.
- **A per-tenant database or schema**: operational cost per tenant; cross-tenant maintenance.
- **Client-side-only "local layers"**: cannot be analysed with server methods, shared with a team, or audited (kept only as `inline`/`external` sources for small cases).
- **A third-party geocoding service**: sends user data out and adds a paid dependency.
