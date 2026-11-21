# ADR 0003 — Six lifecycle layers with a persisted extract layer

Status: proposed · 2026-11-21 · Detail: [01](../01-layers.md)

## Context

The foundation goes from archived bytes straight to observations inside one `parse` call. Review needs the parser's reading (token, page, bbox); a crosswalk or indicator fix should not re-run poppler; and LLM-assisted readings are not reproducible from bytes + code.

## Decision

Six layers: L0 raw → L1 extract → L2 standardised → L3 enriched → L4 release → L5 serving. L1 is new: content-addressed Parquet files of `cells`, `records` or `features` with locators and verbatim text, indexed in `ops.extracts`. `Connector::parse` splits into `extract` (bytes → L1) and `standardise` (L1 → standard model, pure).

## Consequences

- Re-standardising is offline and fast; evidence sheets are a view over L1.
- Class C becomes reproducible from "raw + reviewed extract + code".
- A few GB of extra storage; one more hash per run.
- Rasters skip L1 (the warped grid is the first derived object).

## Alternatives rejected

- **No extract layer** (status quo): review material has to be re-derived; class C is unreproducible.
- **Extracts as Postgres tables**: review data in the hot database, harder retention, no content addressing.
- **Medallion naming (bronze/silver/gold)**: three layers hide the release and serving distinctions that matter operationally here.
