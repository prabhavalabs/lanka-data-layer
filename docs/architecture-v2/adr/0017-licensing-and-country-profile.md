# ADR 0017 — Apache-2.0 engine, open spec, tiered data redistribution, country as configuration

Status: proposed · 2026-11-21 · Detail: [08](../08-openness-and-governance.md)

## Context

The goal is a standard open data layer others can adopt or fork. The engine is MIT today. 86 of 174 sources are `permission_unclear`. The engine contains Sri Lanka literals (`^LK[0-9]*$`, `BOUNDARY_DATASET`, grid constants).

## Decision

Engine Apache-2.0 (pending the owner's confirmation); the standard in `spec/` and documentation CC BY 4.0; schemas, codelists and the profile CC0; registry metadata CC BY 4.0; data under source licences with a computed dataset licence and four redistribution tiers (`bulk`, `api`, `link`, `internal`); OSM-derived feature datasets isolated under ODbL. A documented takedown process with suspension within two working days. Everything country-specific moves to `country/<iso>/profile.yaml` and `registry/`; global sources ship as registry templates; CI builds a release for a synthetic second country.

## Consequences

- Forking for another country is configuration plus a registry, starting with ~40 global sources.
- Permission-unclear data is relayed with attribution but never repackaged into aggregate bundles, which keeps takedown tractable.
- One deployment serves one country (no multi-tenant model).
- Relicensing requires the sole copyright holder's decision before outside contributions arrive.

## Alternatives rejected

- **Keep MIT**: workable; no patent grant, weaker fit for institutional adopters.
- **AGPL**: deters the government/NGO self-hosting the project wants.
- **Withhold permission-unclear data until permission is granted**: removes 122 of 148 parity rows; revisit per publisher if a reply is negative.
- **Multi-country engine with a country dimension**: complexity in every key for a need nobody has stated.
