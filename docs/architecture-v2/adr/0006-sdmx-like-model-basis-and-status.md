# ADR 0006 — SDMX information model without an SDMX service; `basis` and `status` split

Status: proposed · 2026-11-21 · Detail: [02 §3–§5, §10](../02-standard-model.md)

## Context

The layer must be a standard others can adopt. Statistical offices exchange data in SDMX terms. The foundation has free-text units, undeclared dimension values, and one enum (`obs_status`) that mixes how a value was obtained (estimate, modelled, derived) with its state (provisional, revised, suppressed).

## Decision

Adopt the SDMX information model flattened into one observation record: indicator (concept + measure), codelist-backed dimensions with explicit totals, attributes. Units are UCUM codes (ISO 4217 for money) with a power-of-ten multiplier. Two orthogonal fields replace `obs_status`: `basis` (`reported | computed | estimated | forecast`) and `status` (SDMX `CL_OBS_STATUS`-aligned). Indicators carry `measure_type`, `aggregation`, numerator/denominator and `class` (`official | third_party | derived | estimated`). Uncertainty is `lo`/`hi` or a quality class. Interchange is SDMX-CSV export; there is no SDMX REST endpoint.

## Consequences

- Roll-up behaviour is data (`aggregation`), not code per dataset.
- "Is this an official figure?" is answerable per row and enforceable by validators.
- The 263 registry datasets need a one-off mechanical migration with a review list.
- Statistical users get a familiar export without us operating an SDMX stack.

## Alternatives rejected

- **Full SDMX 3.0 REST + SDMX-ML**: large surface, few consumers in this audience.
- **RDF Data Cube as the native model**: fine as an export, poor as a storage and API model.
- **Keep free-text units and a single status enum**: blocks safe aggregation and blurs estimates with official data.
