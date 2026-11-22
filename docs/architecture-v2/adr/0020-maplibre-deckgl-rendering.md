# ADR 0020 — MapLibre owns the map; deck.gl is interleaved for data layers and blending

Status: proposed · 2026-11-22 · Detail: [10 §4](../10-map-composition.md#4-client-rendering)

## Context

A map may hold 10+ layers of mixed kinds with per-layer opacity and blend modes, and must stay interactive on a mid-range phone. MapLibre renders vector tiles, labels and feature-state choropleths well but has no blend modes and no GPU aggregation; deck.gl has both but weaker labels and area cartography.

## Decision

One MapLibre map owns the WebGL context. deck.gl is mounted as an interleaved overlay so each deck layer draws into the same framebuffer at its position in the stack. Area, line, label and hillshade layers default to MapLibre (feature-state join); rasters (value tiles with a ramp shader), symbols, large point sets, clusters, heat maps and bins use deck.gl. A layer whose `blend` is not `normal` is always drawn by deck.gl (polygon types through `MVTLayer` on the same tileset). Two small compilers, `toMapLibre` and `toDeck`, translate the one style model. Supported blend modes are those expressible as fixed-function blend state: `multiply`, `screen`, `darken`, `lighten`, `add`. Budgets for a 12-layer map are part of the design and are enforced by a headless-browser CI benchmark.

## Consequences

- One context, one camera, correct ordering between both renderers.
- Area types have two render paths; the style model is kept small to make that affordable.
- `overlay`, `soft-light` and other non-separable modes are not available in `/1`.
- The interleaving of the pinned MapLibre and deck.gl majors must be verified in a spike before the front-end track commits.

## Alternatives rejected

- **MapLibre only**: no blend modes, no GPU aggregation, no client-side raster ramps without custom layers.
- **deck.gl for every data layer**: loses MapLibre's label placement and cheap feature-state restyling for the most common layer type.
- **Stacked canvases with CSS `mix-blend-mode`**: one WebGL context per blended layer; context limits and memory on phones.
- **Off-screen compositing per layer for full Photoshop blend modes**: a render target per layer at 10+ layers breaks the frame budget.
