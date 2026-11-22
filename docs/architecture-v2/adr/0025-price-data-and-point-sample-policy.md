# ADR 0025 — No scraped listings; price data by licence or BYOD; a fenced interpolation rule for point samples

Status: proposed · 2026-11-22 · Amends the interpolation policy of [04 §3](../04-enrichment.md#3-method-library-v1-of-the-library) · Detail: [11 §3.4](../11-analysis-engine.md#34-interpolation-policy-for-point-samples-amends-04-3), [11 §10](../11-analysis-engine.md#10-data-reality-property-prices)

## Context

The motivating analysis needs property prices below district level. No open source exists in Sri Lanka: CBSL publishes Colombo-district indices; the Valuation Department and the land registry publish nothing reusable; listing portals hold asking prices under proprietary terms and with personal contact details.

## Decision

1. The platform does not scrape listing portals and does not accept scraped listings into the public layer. Price data arrives as (a) the CBSL series already registered, as time-series context; (b) aggregated products obtained under a written licence from portals or public bodies; (c) data users upload privately (BYOD). No "estimated land price" indicator is manufactured from proxies.
2. A clearly labelled **synthetic** sample dataset supports documentation, tests and the front-end; the same analysis pattern is demonstrated publicly on open data.
3. Point samples of an intensive socio-economic quantity are aligned by per-unit robust aggregation (`point_aggregate`, minimum n, bootstrap interval) by default. Spatial interpolation of such samples is allowed only inside analysis, on request, on a transformed scale, masked to built-up cells and a maximum distance from samples, with cross-validation error reported and the guard `INTERP_SOCIO`. The rule that published recipes interpolate only continuous physical fields, and never counts or administrative totals, is unchanged.

## Consequences

- The headline example cannot be shown with real prices on the public site until a licence exists; that is stated openly.
- The project keeps its licence-honesty position and avoids contract, copyright and personal-data exposure.
- Users with their own price data get the full analysis today.

## Alternatives rejected

- **Scraping with attribution**: breaches terms of use, collects personal data, and would make every downstream product untrustworthy.
- **A modelled price surface from night lights and buildings**: a proxy presented as a price.
- **Forbidding interpolation of prices entirely**: the user's requirement names it; a fenced, labelled version is more honest than users doing it elsewhere without caveats.
