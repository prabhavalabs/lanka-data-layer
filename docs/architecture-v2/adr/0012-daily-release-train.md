# ADR 0012 — Releases are cut by a train, not by each publish

Status: proposed · 2026-11-21 · Detail: [03 §8](../03-ingestion.md#8-release-train)

## Context

In the foundation every class-A publish and every approval creates a new release immediately. With ~170 sources, daily feeds and derived datasets, that yields many releases a day, each of which could leave derived data inconsistent with its inputs, and each triggering export work.

## Decision

Publishing marks a version `approved`. A `release` job (daily at 20:30 UTC; manual `lanka release cut`; intraday at most hourly when a `live` dataset has an approved version) collects approved versions, recomputes stale recipes in topological order, then writes one release that pins official and derived versions together, closes superseded rows, refreshes `serve.*` and notifies servers. A failing recipe keeps its previous version and never blocks official data. Sub-daily data (`data.obs_hf`) is outside releases.

## Consequences

- Every release is internally consistent and citable; release numbers stay readable.
- Up to a day (an hour for `live`) between approval and visibility.
- Exports are per dsv, so a train costs only what changed.
- ETags depend on the dsvs a response touches, not on the release id, so unrelated responses stay cached.

## Alternatives rejected

- **Release per publish** (status quo): inconsistent derived data between cuts, release-number churn.
- **No releases, only per-dataset versions**: no reproducible "state of the layer" to cite or mirror.
- **Weekly releases**: too slow for dengue, prices, exchange rates.
