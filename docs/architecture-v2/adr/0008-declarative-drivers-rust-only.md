# ADR 0008 — Declarative drivers for regular sources; code connectors in Rust; no TypeScript

Status: proposed · 2026-11-21 · Detail: [03 §2](../03-ingestion.md#2-connector-sdk)

## Context

174 sources. Three connectors exist, each hand-written Rust (192–640 lines). Writing ~170 more that way is the bulk of the parity cost and excludes non-Rust contributors. The constraints allow TypeScript for document connectors "only if justified".

## Decision

Twelve generic drivers (`http_json`, `sdmx`, `ckan`, `csv`, `xlsx_table`, `html_table`, `pdf_table`, `geo_file`, `osm_pbf`, `raster`, `gtfs`, `git_files`) are implemented once in Rust and configured per source by `registry/connectors/<id>.yaml` (JSON-Schema validated, testable with `lanka connector test`). Irregular sources get a Rust code connector implementing the same trait. The selector language stays small; when it is not enough the escape hatch is Rust. TypeScript is not used in ingestion.

## Consequences

- Target ≥ 70 % of sources with no Rust; contributions become registry PRs with fixtures.
- Driver bugs are fixed once for all sources.
- A spec language to document and keep small.
- `world_bank.rs` is replaced by YAML; `dcs_census.rs` and `cod_ab.rs` stay as code.

## Alternatives rejected

- **A Rust module per source**: highest cost, lowest contributor reach.
- **TypeScript connectors (port v1 parsers as-is)**: a second runtime in the sandbox, CI and supply chain, for no capability Rust lacks (calamine, poppler, scraper).
- **Embedded scripting (Lua/JS/WASM) in specs**: a sandbox and a language to maintain; hides logic from review.
- **Singer/Airbyte/Meltano connectors**: built for SaaS → warehouse replication, not cell-level provenance of PDF tables.
