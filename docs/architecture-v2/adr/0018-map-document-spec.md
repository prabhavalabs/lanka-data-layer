# ADR 0018 — The map is an open, versioned document (`ldl-map/1`)

Status: proposed · 2026-11-22 · Detail: [10 §2](../10-map-composition.md#2-the-map-document--ldl-map1)

## Context

Users must compose maps from many layers, then save, share, embed, fork and export them. The foundation has only a single-indicator embed spec ([06 §7](../06-serving.md#7-embeds-and-discoverability)); the web mock-ups keep map state in component state.

## Decision

A map is one JSON document, `ldl-map/1`, with a published JSON Schema in `spec/map/1/`. It holds a view, a basemap, named **sources** (references to indicators, features, grids, boundaries, analyses — never data values, never data URLs), embedded **analysis requests**, an ordered tree of **layers and groups** (bottom → top; visibility, opacity, blend, zoom range, CQL2-JSON filter, clip), a small **style model** (visual channels bound to fields through scales), a global time cursor and linked panels. A document is either floating (`pin: "latest"`) or frozen to a release, in which case resolved class breaks and analysis fingerprints are written into it. The server resolves a document to URLs, attribution and notices (`resolved` block); renderers must show them. Unknown `x-*` keys are preserved.

## Consequences

- Fork, export and import are copying JSON; the CLI, MCP and self-hosted instances can read and write maps.
- Licence tiers, attribution and estimate labelling cannot be stripped by a shared map, because data only arrives through the ordinary endpoints.
- A frozen map is citable and renders identically later.
- The spec is a compatibility surface: breaking changes need `ldl-map/2` and a migration.

## Alternatives rejected

- **MapLibre style JSON as the document**: renderer-specific, no notion of indicators, versions, classification methods or analysis; would leak tile URLs.
- **kepler.gl / Felt / Carto map configs**: embed data or vendor concepts; no provenance or licence model.
- **Server-side map state keyed by an id only**: not forkable, not portable, not usable offline.
- **Per-source dsv pins as the only mechanism**: a release is already a consistent dataset → version map; one pin is simpler and consistent across layers.
