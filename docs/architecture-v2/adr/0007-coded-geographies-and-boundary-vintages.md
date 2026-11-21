# ADR 0007 — All coded geographies in one table; vintaged boundaries; one reference vintage

Status: proposed · 2026-11-21 · Detail: [02 §2](../02-standard-model.md#2-geography)

## Context

0001 has `geo.admin_units` (p-code keyed, `^LK[0-9]*$`, levels 0–4) and puts everything else at `geo_level = 9`. Sources also publish by electoral district, polling division, local authority, MOH area, police division, river basin and station. Boundaries change (COD-AB 339 DS divisions, census 340, older sets 331; GN recodes 2012→2024).

## Decision

`geo.units` holds every coded geography with a scheme; admin p-codes stay bare, others are `scheme:code`. Geometry lives in `geo.unit_geoms` per boundary set (vintage). Each scheme has exactly one reference vintage. Official data stays on the vintage it was published for; comparable series are produced by `crosswalk_rebase` as derived data flagged `REBASED`. Name → code matching happens once in the pipeline through reviewed alias tables and is stored in `geo.crosswalks`. `geo.admin_units` remains as a view. The p-code pattern and level names come from the country profile.

## Consequences

- Elections, health areas and local authorities become first-class without special tables.
- A new COD-AB edition is a planned contract event with a crosswalk, not a silent overwrite.
- Retired units remain resolvable (with successors).
- Queries that assumed `geo.admin_units` is a table keep working through the view.

## Alternatives rejected

- **Admin units only, everything else as features**: observations for electorates/MOH areas would have no proper key.
- **Silently re-key history onto current boundaries**: destroys what the source actually said.
- **Separate table per scheme**: N code paths for the same operations.
