# 10 — Map composition

Users build maps the way they build an image in a layer-based editor: add any dataset, indicator, feature set, grid or analysis result as a layer; reorder, group, hide, fade and blend layers; configure each one; then save, share, embed, fork or export the result.

The design has four parts:

1. an open, versioned **map document** (`ldl-map/1`) that *is* the map — everything else is derived from it;
2. three **data delivery shapes** the API serves layers in (frames, value tiles, columnar features), added to the tiles of [05 §9](05-storage-and-performance.md#9-tiles);
3. a **client rendering strategy** on MapLibre + deck.gl with stated budgets;
4. **storage, permalinks, embeds and export** in the app plane ([12](12-enterprise-and-byod.md)).

Markers: **[A]** = assumption to be verified by the work package that first depends on it.

## 1. Principles

| # | Rule | Consequence |
|---|---|---|
| 1 | **The document is the map.** A map is one JSON document; there is no hidden server-side state. | Fork = copy the JSON. Self-hosters, the CLI and MCP can read and write maps. The web app is one renderer of the spec, not its owner. |
| 2 | **A document never contains data values.** It references datasets, indicators, grids and analyses by identifier. | Licence tiers, attribution and estimate labelling are enforced where the data is served, so a shared map cannot strip them. The single exception is `inline` GeoJSON for annotations (≤ 100 KB). |
| 3 | **Pinned or floating, explicitly.** A document either follows `latest` or is frozen to a release. | A cited or embedded map shows the same numbers forever; a dashboard map updates itself. |
| 4 | **Styling is client-side and costs no request.** Classification, ramps, opacity, blend, filters on loaded data and time stepping never hit the network. | The server ships values once; ten restyles are free. |
| 5 | **One style model, any layer.** Visual channels (fill, stroke, size, label) are bound to fields through scales. | The same classification UI and legend code serve choropleths, symbols, points and rasters. |
| 6 | **Analysis layers are ordinary layers.** An analysis result is a source; it renders with the same layer types. | No special "analysis map"; see [11](11-analysis-engine.md). |

## 2. The map document — `ldl-map/1`

Normative JSON Schema: `spec/map/1/map.schema.json` (CC BY 4.0, part of the published standard, [08 §3](08-openness-and-governance.md#3-the-spec)). TypeScript mirror: [ui-contract-explore.md](ui-contract-explore.md). Rust types: crate `lanka-mapdoc` (`serde` structs, validation, resolution).

### 2.1 Top level

| Field | Type | Meaning |
|---|---|---|
| `spec` | `"ldl-map/1"` | Required. Readers MUST reject an unknown major, MUST ignore unknown `x-*` keys, and MUST preserve them on save. |
| `id` | string | `m_…` for a saved map, `ms_…` for a snapshot; absent in an unsaved document. Not part of the content hash. |
| `title`, `description` | text or `{en, si, ta}` | Plain text. `description` allows a CommonMark subset (paragraphs, emphasis, links); no HTML, no images. |
| `pin` | `"latest"` \| `{release}` | Which release every public source resolves against (§2.3). |
| `view` | `{center:[lon,lat], zoom, bearing?, pitch?, bounds?}` | Initial camera. `bounds` wins over `center/zoom` when the viewport differs from the author's. |
| `extent` | `{geo}` \| `{bbox}` \| null | Default spatial scope of sources and analyses (e.g. Western Province `LK1`). A layer or source may override it. |
| `basemap` | object | `style` (`ldl-light`, `ldl-dark`, `ldl-mono`, `none`, or a URL to a MapLibre style on self-hosts), `labels: top\|bottom\|off`, `hillshade: {on, exaggeration}`. |
| `time` | object \| null | Global time cursor: `{cursor, range:[from,to], freq, playback:{fps, loop}}` in the period grammar of [02 §5](02-standard-model.md#5-time). |
| `sources` | map id → Source | What data exists in the map (§2.2). Layers reference sources; two layers on one source share one fetch. |
| `analyses` | map id → AnalysisRequest | Analysis requests embedded in the map ([11 §5](11-analysis-engine.md#5-requests-and-results)). The *request* is stored, never the result. |
| `layers` | LayerNode[] | Ordered **bottom → top** (draw order, as MapLibre). The layer panel shows the reverse. A node is a layer or a group (§2.4). |
| `panels` | Panel[] | Non-map views linked to the map: `legend`, `chart` (scatter, histogram, series bound to a source or analysis), `stats` (analysis result card), `text`. |
| `meta` | object | `created_at`, `updated_at`, `author` (handle), `forked_from` (`id@version` or snapshot id), `licence` of the document itself (default `CC-BY-4.0`), `generator`. |

### 2.2 Sources

A source names data, a spatial support and a period binding. It never names a URL (except `external`).

| `kind` | Fields | Resolves to |
|---|---|---|
| `indicator` | `indicator` (or `indicators[]` ≤ 8), `level` (0–4, scheme-local, or `{"byZoom": [[0,2],[8,3],[10,4]]}`), `scheme?` (default `admin`), `parent?`, `period` (`"latest"`, a period code, a list, `{from,to}`, or `{"bind":"time"}`), `dims?` | a **frame** (§3.1) joined to the boundary tileset by code |
| `boundaries` | `scheme`, `level`, `parent?` | the scheme's vector tileset, no values |
| `features` | `dataset`, `kind?`, `within?`, `fields?[]` | vector tiles, columnar points or GeoJSON by size (§3.3) |
| `grid` | `grid` (grid layer id or indicator id with `dims`), `period` | value tiles (§3.2) |
| `analysis` | `ref` (key in `analyses`), `output` (output id of the result) | a frame, a grid or features, as the result declares |
| `inline` | `geojson` (≤ 100 KB) | client-only annotations, drawn shapes |
| `external` | `type: geojson\|pmtiles\|raster-xyz`, `url`, `attribution` (required) | fetched **by the browser only**; the server never requests it (no SSRF). Marked `external` in the layer panel; excluded from data export; disabled inside embeds unless the embed profile allows the host. |

Identifiers are those of [02 §1](02-standard-model.md#1-identifiers). Workspace data uses the `@workspace/id` form ([12 §4](12-enterprise-and-byod.md#4-the-workspace-data-plane)): `@acme/re.asking_price_median`.

Every source accepts `version`: omitted (inherit the document `pin`), `"latest"`, or `{dsv}`. Workspace datasets are not part of releases, so they are always `latest` or `{dsv}`.

### 2.3 Pinning and resolution

`POST /app/v1/maps/resolve` (body: a document; anonymous allowed) and `GET /app/v1/maps/{id}/resolved` return the **resolved document**: the same document plus a `resolved` block that the renderer uses and never edits.

```jsonc
"resolved": {
  "data_version": "20270315.1",
  "sources": {
    "exposure": { "dsv": "dsv_41c0a9e77b1d2f08", "frame": "/v2/r/20270315.1/frame?…", "tileset": "admin",
                  "tiles": "/dl/tiles/admin/9f3a1c2e77b04d15.pmtiles", "source_layer": "adm4", "join": "pcode",
                  "class": "estimated", "basis": ["estimated"], "access": "public" },
    "flood":    { "dsv": "dsv_7ab2…", "values": "/v2/r/20270315.1/grids/haz.flood_depth.rp100/values/{z}/{x}/{y}.png",
                  "encoding": "ldl-v16", "scale": 0.01, "offset": 0, "nodata": 65535, "access": "public" }
  },
  "analyses": { "rel": { "id": "an_5e1f0c22ab904d71", "state": "ready" } },
  "attribution": [ … same shape as meta.attribution … ],
  "notices": [ { "code": "ESTIMATE", "text": "…" }, { "code": "COMMUNITY_DATA", "text": "…" } ],
  "problems": [ { "path": "/sources/old", "code": "WITHDRAWN", "hint": { "successor": "…" } } ]
}
```

Rules:

- `pin: "latest"` resolves to the current release; the response is cacheable for 300 s. `pin: {release}` resolves to immutable URLs.
- **Freeze** rewrites `pin` to the current release, writes each analysis fingerprint into `analyses.*.result`, and writes resolved class breaks into every scale (`breaks`). A frozen document renders identically without recomputing classification. Publishing, embedding and "cite this map" freeze by default.
- A source the viewer may not read resolves to `access: "denied"`; the renderer shows a placeholder row ("private data") and draws the rest.
- `attribution` and `notices` are computed from the sources (licence walk through derivations, estimate notice, community-data notice). The renderer MUST display them; the embed route cannot hide them.
- A withdrawn or renamed dataset becomes a `problems[]` entry with a repair hint, not a failed map.

### 2.4 Layer nodes

```jsonc
// group
{ "id": "g_hazard", "type": "group", "name": "Hazard", "visible": true, "opacity": 1, "collapsed": false, "children": [ … ] }

// layer (common fields)
{ "id": "l_expo", "type": "choropleth", "name": "Population in flood zone (%)",
  "source": "exposure", "visible": true, "opacity": 0.85, "blend": "normal",
  "zoom": [5, 22], "filter": { … CQL2-JSON … }, "clip": { "geo": "LK1" },
  "style": { … }, "legend": { "show": true, "title": null },
  "interaction": { "hover": true, "select": true, "tooltip": ["name", "value", "basis"] },
  "locked": false, "x-notes": "…" }
```

- Group `visible` and `opacity` multiply into children. Groups nest to depth 3. A group has no blend mode.
- `blend`: `normal | multiply | screen | darken | lighten | add`. `overlay`, `soft-light` and the other non-separable modes are not in `/1` (§4.2).
- `filter` is a **CQL2-JSON** subset (`=`, `<>`, `<`, `<=`, `>`, `>=`, `and`, `or`, `not`, `in`, `between`, `isNull`, `like`) over the layer's fields. It is the same grammar OGC API Features Part 3 uses ([06 §3](06-serving.md#3-ogc-api-alignment)); the client compiles it for the renderer and the server pushes it down when it fetches features.
- `clip` hides everything outside a unit or bbox (a clipping mask).
- Limits: 32 layers, 8 groups, 16 sources, 6 analyses, 256 KB canonical JSON per document.

### 2.5 Layer types

| `type` | Source kinds | Draws | Type-specific style |
|---|---|---|---|
| `choropleth` | indicator, analysis (frame) | units filled by one field | `fill` scale |
| `bivariate` | indicator ×2 (one source with two columns), analysis | units filled by a 3×3 or 4×4 palette | `fill.bivariate {x, y, palette}` |
| `symbol` | indicator, analysis, features | proportional or graduated symbols at unit centroids or points | `size` scale (area-proportional by default), `fill`, `shape: circle\|square\|bar` |
| `point` | features, inline | circles or icons | `fill` (categorical or numeric), `size`, `icon` |
| `cluster` | features (points) | zoom-dependent clusters with counts | `radius`, `fill`, `aggregate: count\|sum(field)` |
| `heatmap` | features (points), analysis (grid) | kernel density surface | `radius` (px or metres), `weight` field, `ramp`, `intensity` |
| `bins` | features (points) | hexagon or square bins aggregated in the browser | `cell` (metres), `aggregate`, `fill` scale, `elevation?` |
| `line` | features, boundaries, inline | lines | `stroke {color, width, dash}`, either bound to a field |
| `polygon` | features, boundaries, analysis, inline | areas (hazard zones, isochrones, buffers, protected areas) | `fill`, `stroke`, `pattern?` |
| `raster` | grid, analysis (grid) | a value grid coloured in the browser | `fill` scale on the cell value, `range` (mask outside), `resample: nearest\|linear` |
| `hillshade` | grid (`dem`) | shaded relief | `exaggeration`, `shadow`, `highlight` |
| `label` | any vector source | text | `text` (field or template), `size`, `halo`, `priority` field |
| `annotation` | inline | notes, arrows, callouts | — |

Isochrones, hot-spot maps, LISA cluster maps, inside/outside zones and interpolated surfaces are **not** layer types: they are analysis outputs drawn with `polygon`, `choropleth` or `raster`. Reserved for a later minor version: `flow` (origin–destination arcs), `extrusion` (3-D).

### 2.6 Style model

A visual channel is a constant or a field bound through a scale:

```jsonc
"style": {
  "fill":   { "field": "value",
              "scale": { "type": "quantile", "classes": 5, "ramp": "ldl.seq.teal", "reverse": false,
                         "domain": "extent", "across": "all_periods", "nodata": "#e6e6e6",
                         "breaks": [2.1, 5.4, 9.8, 17.3] } },        // present once frozen or when type = manual
  "stroke": { "color": "#ffffff", "width": 0.4 },
  "label":  { "text": "{name}", "size": 11, "zoom": [11, 22] }
}
```

| Scale `type` | Parameters | Notes |
|---|---|---|
| `quantile`, `equal`, `jenks`, `stddev`, `headtail`, `pretty` | `classes` (2–9) | `jenks` is Fisher–Jenks; above 5,000 values it runs on a deterministic sample (seeded by the source id) |
| `manual` | `breaks[]` | |
| `continuous` | `transform: linear\|log\|sqrt\|symlog`, `clamp: [p2, p98]` or absolute | unclassed |
| `categorical` | `mapping` (code → colour) or a `ramp` of kind `cat` | codes come from the indicator's codelist |
| `diverging` | `mid` (0, mean, median, or a number), `classes` | for change and residual maps |

- `domain`: `extent` (units in the document extent — the default), `data` (units currently loaded), `national` (all units at the level, from `serve.indicator_coverage`), so a provincial map can be classed against the national distribution.
- `across`: `all_periods` (default: one set of breaks for the whole animation, so colours are comparable over time) or `period`.
- Ramps are named (`ldl.seq.*`, `ldl.div.*`, `ldl.cat.*`, `ldl.biv.*`, plus the ColorBrewer and viridis families) or an explicit colour array. The ramp registry is a JSON file in `spec/map/1/ramps.json`; every sequential and diverging ramp in it is checked for colour-vision-deficiency safety in CI.
- **One implementation of classification.** Breaks are computed by `lanka-stats` ([11 §8](11-analysis-engine.md#8-where-the-computation-runs)), compiled to WebAssembly for the browser and linked natively into the server, CLI and MCP. A legend drawn in the browser and one produced by the API have the same breaks by construction.
- Estimated data is visible in the style, not only in the legend: a `choropleth` or `symbol` whose column contains `estimated|forecast` rows gets the estimate badge in its legend, and `style.uncertainty: "hatch" | "fade" | "off"` (default `hatch` when the layer has `lo`/`hi` and the relative interval width exceeds 50 %) marks low-confidence units on the map. This extends [04 §8](04-enrichment.md#8-labelling) to maps.

### 2.7 Time

- `time.cursor` is a period code. A source with `period: {"bind": "time"}` shows the observation for the cursor; `match: "exact"` (default, gaps are no-data) or `"asof"` (latest period ≤ cursor, shown with its own period in the tooltip).
- The frame for a bound source carries every period in `time.range` (§3.1). Stepping time changes one paint expression; it does not fetch and does not rebuild feature state (§4.3).
- Layers with different frequencies animate on the document's `time.freq`; a coarser layer holds its value across finer steps.
- Official and estimated series are never spliced in one layer ([04 §8](04-enrichment.md#8-labelling) rule 7). A continuous animation over census years plus intercensal estimates is two sources and the layer panel shows which one is on screen.

## 3. Serving layer data

Three shapes, chosen by the resolver from the source kind and size. All are under `/v2` (public data) and mirrored under `/app/v1/ws/{workspace}/` for workspace data.

| Shape | For | Endpoint | Client join |
|---|---|---|---|
| **Frame** | values on coded units (admin and other schemes), analysis results per unit | `GET /v2/frame` | `feature-state` on the boundary tileset, by code |
| **Value tiles** | grids | `GET /v2/grids/{id}/values/{z}/{x}/{y}.png` | colour ramp in a fragment shader |
| **Features** | points, lines, polygons | PMTiles · `…/points?format=arrow` · GeoJSON | direct |

### 3.1 Frames

A frame is a column-aligned table on one spatial support. It generalises `/v2/indicators/{id}/map` (kept as an alias for the one-indicator, one-period case) to several indicators and periods.

```
GET /v2/frame?indicators=exp.flood_pop_share,est.pop_density
            &level=4&parent=LK1&periods=2026            (or from=&to=)
            &dims=exp.flood_pop_share:return_period=RP100
            &format=json|arrow|csv|parquet
```

```jsonc
"payload": {
  "support": { "kind": "admin", "scheme": "admin", "level": 4, "parent": "LK1",
               "boundary_set": "cod-ab-lka-2022", "n": 2505 },
  "codes":   ["LK1103005", "LK1103010", …],
  "columns": [
    { "id": "c0", "indicator": "exp.flood_pop_share", "dims": {"return_period": "RP100"}, "period": "2026",
      "unit": "%", "class": "estimated", "ci_level": 0.9,
      "values": [12.4, 0, null, …], "lo": [9.1, 0, null, …], "hi": [16.0, 0.4, null, …],
      "basis": "estimated",                       // scalar when uniform, else an int array (codes of 05 §2)
      "status": "normal", "flags": null,
      "stats": { "n": 2498, "min": 0, "max": 81.2, "mean": 7.9, "q": [0, 0, 0.6, 3.1, 10.2, 31.5, 81.2] } },
    { "id": "c1", "indicator": "est.pop_density", … }
  ]
}
```

- A column whose indicator has no rows at the level but is completed by a roll-up ([04 §8](04-enrichment.md#8-labelling), e.g. `census24.pop_total` at ADM1) carries `completed_from: "rollup.census24.pop_total"`, the roll-up's `dataset` and `basis: "computed"`.
- One code array, one value array per (indicator, dims, period). `format=arrow` returns the same table as an Arrow IPC stream (`Float32` values, dictionary-encoded small-int columns) for frames above ~20,000 cells.
- `dims` codes match without regard to case (`RP100` = `rp100`; the columns carry the stored spelling) and the resolver writes the stored spelling into frame URLs. A key that is not a dimension of the indicator(s), or codes that match no published combination, are a 400 whose envelope carries `payload.problem` = `{code: "UNKNOWN_DIM", given, unknown, valid_dims}` or `{code: "DIMS_NO_MATCH", indicator, given, valid: {dim: [codes]}, combinations}`; the resolver reports the same as a `NOT_FOUND` problem at `/sources/{id}/dims/{dim}` with `valid` / `valid_dims` in its hint.
- Limits: 64 columns; `codes × columns` ≤ 500,000 cells anonymous, 5 M with a key. The whole island at ADM4 × 13 annual periods is 182,559 cells ≈ 0.7 MB as Arrow before compression.
- Backed by the `_map` index of [05 §2.3](05-storage-and-performance.md#23-indexes-per-partition-created-by-ensure_obs_partition): one index-only range scan per column. New `Backend::frame(&FramePlan)` in `lanka-query`; the offline pack backend implements it, so the CLI and `lanka-stats` work offline.
- Latency target **[A]**: ADM4 island-wide, 2 columns: p50 25 ms, p99 80 ms; cached by nginx like every other `/v2` response.

The boundary tileset (`admin`, already planned) gains two requirements: `promoteId` on `pcode` for every level's source layer, and a `centroids` source layer (one point per unit with `pcode`, `level`, `name_*`) for `symbol` and `label` layers.

Admin levels are zoom-gated to hold tile sizes (largest tile ≈ 230 KB gzip at z9 over Colombo; no feature is coalesced or dropped): `adm0`–`adm2` from z0, `adm3` from z6 (simplified), `adm4` from z9, centroids from their level's zoom, maxzoom 12 (over-zoom above). The gates sit one zoom below the `byZoom` example (`[[0,2],[8,3],[10,4]]`) so a level is in the tiles before a map switches to it. The TileJSON states them truthfully in `vector_layers[].minzoom`, and the resolver's `frame`/`boundaries` shapes carry `minzoom` of their `source_layer` (and of each `by_zoom` entry); a client must not draw a level below its `minzoom`.

### 3.2 Value tiles

[05 §9](05-storage-and-performance.md#9-tiles) renders grid PNG tiles server-side with a fixed ramp. That stays for simple clients (QGIS, static images). Composable maps need the **values**, so the ramp, range mask and opacity can change without a request.

- Encoding `ldl-v16`: an 8-bit RGBA PNG, 256 px. `R×256 + G` = quantised value `q` (u16); physical value = `q × scale + offset`; `q = 65535` = no data; `B` = share of valid source cells in the pixel (0–255, meaningful on overview levels); `A` = 255 always, so no browser premultiplies the payload. Lossless, decoded natively by `createImageBitmap` off the main thread (with `premultiplyAlpha: 'none'`, `colorSpaceConversion: 'none'`).
- `f32` layers are quantised with the `scale`/`offset` already recorded in `geo.grid_layers`; the quantisation error is stated in the grid's metadata. Categorical layers (`u8` land cover) put the class in `G` and the grid metadata carries the legend.
- Zooms 5–11 (the grid's native resolution is ≈ z11 **[A]**); z5–z10 come from an overview pyramid computed once per layer-period with the reducer the layer declares (`overview: mean | sum | max | mode`). The pyramid is stored beside the array (+ 33 % bytes). Above z11 the client over-zooms.
- URL carries the layer's `sha16`, so tiles are `immutable`; nginx caches them.
- `dem` is served as a `raster-dem` tileset (Terrarium encoding, PMTiles, built once from Copernicus DEM) for the `hillshade` layer.

### 3.3 Features

| Size of the (filtered) set | Shape | Why |
|---|---|---|
| ≤ 2,000 features | GeoJSON, `GET /v2/features/{dataset}` (exists) | simplest; small |
| points, ≤ 250,000 | **columnar points**: `GET /v2/features/{dataset}/points?fields=a,b&within=&bbox=&format=arrow` → Arrow IPC (`lon`, `lat` as `Float32`, `key`, requested fields dictionary- or numeric-encoded) | one request; feeds `point`, `cluster`, `heatmap`, `bins` and client filters with no parsing; ≈ 12–20 bytes per point |
| more points, or any lines/polygons | PMTiles (exists) | viewport-bounded |
| density of very large point sets | server-side count grid, delivered as value tiles | the browser never holds millions of points |

`fields` are the dataset's declared **feature fields** (name, type, unit, measure type), a registry property of `features` datasets that WP-04 introduces with `tiles: {attrs[]}`; undeclared `attrs` keys are not served.

### 3.4 What is deliberately not built

- No per-indicator choropleth tiles and no dynamic `ST_AsMVT` ([ADR 0014](adr/0014-pmtiles-and-http-caching.md) stands).
- No server-side map rendering and no headless browser: image export is client-side (§6.3).
- No WMS/WMTS. QGIS users have OGC API Features/Tiles and COG.

## 4. Client rendering

Stack: MapLibre GL JS as the map and the owner of the WebGL context; deck.gl mounted with `MapboxOverlay` in **interleaved** mode, so deck layers draw into MapLibre's framebuffer at a chosen position in the layer stack. **[A: verify deck.gl 9 interleaving against the MapLibre major pinned by `web-v2` in a spike at the start of the front-end track.]**

### 4.1 Which renderer draws what

| Layer type | Default renderer | How |
|---|---|---|
| `choropleth`, `bivariate` | MapLibre `fill` (+ `line`) | boundary tileset + `feature-state`; colour is a paint expression over the state key |
| `polygon`, `line`, `label`, `annotation` | MapLibre `fill` / `line` / `symbol` | label collision and text shaping are MapLibre's strength |
| `hillshade` | MapLibre `hillshade` | `raster-dem` source |
| `symbol` | deck `ScatterplotLayer` (or `ColumnLayer` for bars) | centroids + frame values as binary attributes |
| `point` | MapLibre `circle` ≤ 20,000 points; deck `ScatterplotLayer` above | binary attributes from Arrow |
| `cluster` | deck `ScatterplotLayer` + `TextLayer` fed by a Supercluster index in a worker | MapLibre clustering needs a GeoJSON source; this path works from Arrow |
| `heatmap` | deck `HeatmapLayer` | weight and radius are uniforms |
| `bins` | deck `HexagonLayer` / `GridLayer` (GPU aggregation) | |
| `raster` | deck `TileLayer` → custom `ValueBitmapLayer` | fragment shader decodes `ldl-v16`, applies range mask and the ramp from a 256×1 texture |

Two compilers turn the style model into renderer input: `toMapLibre(style)` → paint/layout expressions; `toDeck(style)` → accessors, uniforms and `updateTriggers`. They share the scale code (WASM `lanka-stats` for breaks, one TypeScript colour module). The style model is small on purpose so each compiler stays a few hundred lines.

### 4.2 Opacity and blend modes

- **Opacity** is a paint property (MapLibre) or the `opacity` prop (deck): no re-upload.
- **Blend modes** exist only in deck.gl (WebGL blend state per layer); MapLibre has none. Therefore: a layer with `blend ≠ normal` is always drawn by deck. For polygon-family types that means deck `MVTLayer` over the same tileset with the same style through `toDeck`. `normal` keeps the cheaper MapLibre path.

| `blend` | Blend state (premultiplied alpha) **[A: confirm in the spike]** |
|---|---|
| `multiply` | colour: `DST_COLOR`, `ONE_MINUS_SRC_ALPHA`, add |
| `screen` | `ONE`, `ONE_MINUS_SRC_COLOR`, add |
| `add` | `ONE`, `ONE`, add |
| `darken` / `lighten` | `ONE`, `ONE`, `MIN` / `MAX` (WebGL 2) |

Modes that need the destination colour inside the shader (`overlay`, `soft-light`, `color`, `hue`) would need an off-screen pass per layer; they are out of `/1`. The first front-end increment ships `normal` and `multiply` (the cartographically useful pair: a hazard raster multiplied over a choropleth, a choropleth over hillshade).

### 4.3 Joining values: feature state

For a `choropleth` on source `S` with columns `c0…cn`:

1. Fetch the frame once (TanStack Query key = the resolved frame URL).
2. In one batch, `setFeatureState({source, sourceLayer, id: code}, {"S.c0": v0, …, "S.cn": vn})` for each code. 14,043 GN units × 13 periods is one pass of ≈ 20–40 ms **[A]**, off the critical path (`requestIdleCallback` chunks of 2,000).
3. The layer's `fill-color` is the compiled scale over `["feature-state", "S.<column>"]`.
4. Restyle (ramp, classes, breaks) = `setPaintProperty`. **Time step = swap the column name inside the same expression.** Neither touches feature state or the network.

State keys are namespaced by source, so any number of layers share one boundary tileset and one copy of the geometry. Sources with more than 64 period columns keep a sliding window of 64 in state.

### 4.4 Budgets

Reference devices: a 2022 mid-range laptop (integrated GPU) and a 2022 mid-range Android phone. A map with **12 visible layers** (1 basemap, 1 hillshade, 2 rasters, 3 choropleth/bivariate, 2 point layers of 50,000, 1 heatmap, 1 boundaries, 1 labels) must meet:

| Budget | Laptop | Phone |
|---|---|---|
| First meaningful map (basemap + topmost data layer), cold cache, 10 Mbit/s | ≤ 2.0 s | ≤ 3.5 s |
| All 12 layers drawn | ≤ 4.0 s | ≤ 7.0 s |
| Pan/zoom frame rate | ≥ 50 fps | ≥ 30 fps |
| Restyle (ramp, classes, opacity, blend, visibility) | ≤ 50 ms, 0 requests | ≤ 100 ms |
| Time step | ≤ 60 ms, 0 requests | ≤ 120 ms |
| Reorder layers | ≤ 100 ms, 0 requests | ≤ 200 ms |
| Transfer for first view (compressed) | ≤ 6 MB | ≤ 4 MB |
| JS heap + GPU memory | ≤ 600 MB | ≤ 300 MB |

How the budgets are held:

- **Shared geometry.** One boundary source per scheme for all area layers; one DEM source.
- **Hidden layers cost nothing.** A layer that is invisible, out of its zoom range, or inside a hidden group has no renderer layer and issues no request; its data is fetched on first reveal.
- **Top-down loading.** Requests are issued in panel order from the top, six at a time per origin class; a camera move aborts tile requests for the old viewport (`AbortController`).
- **Caps with a reason shown in the panel** (not silent drops): 4 visible `raster` layers, 2 visible `heatmap`/`bins`, 500,000 points in memory across layers, 32 layers per document. On a phone: 2, 1, 150,000.
- **Level by zoom.** `level: {"byZoom": …}` keeps the number of drawn polygons near 3,000: provinces/districts when zoomed out, GN divisions when zoomed in.
- **Workers.** Arrow decode, Supercluster, Jenks and all `lanka-stats` calls run in one shared worker; the main thread only applies results.
- **Quality switch.** `quality: auto` halves raster tile resolution and heatmap radius precision when the measured frame time exceeds 33 ms for two seconds.
- A CI job renders three reference documents (the example of §7, a 20-layer stress map, a 60-period animation) in headless Chromium and fails on a > 30 % regression of load time or frame time — the same rule the API benchmarks use ([05 §11](05-storage-and-performance.md#11-latency-targets)).

## 5. Storage

Maps live in the **app plane** (schema `app`, role `lanka_app`; [12 §2](12-enterprise-and-byod.md#2-two-planes)). The public data role still cannot write anything.

```sql
-- Anonymous, immutable, content-addressed. No account needed.
CREATE TABLE app.map_snapshots (
  id          text PRIMARY KEY,                 -- 'ms_' || first 16 hex of sha256(JCS(doc without id/meta.updated_at))
  doc         jsonb NOT NULL,
  doc_sha256  bytea NOT NULL UNIQUE,
  bytes       int4  NOT NULL CHECK (bytes <= 262144),
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  bigint REFERENCES app.users (id),         -- NULL = anonymous
  created_ip_hash bytea,                                -- salted, rotated monthly; abuse control only
  last_read_at date NOT NULL DEFAULT current_date,      -- touched at most once a day
  withdrawn   boolean NOT NULL DEFAULT false
);

-- Named, owned, versioned.
CREATE TABLE app.maps (
  id           text PRIMARY KEY,                -- 'm_' || 12 Crockford base32 chars
  workspace_id bigint NOT NULL REFERENCES app.workspaces (id),
  project_id   bigint REFERENCES app.projects (id),
  slug         text NOT NULL,
  visibility   text NOT NULL DEFAULT 'private' CHECK (visibility IN ('private','unlisted','public')),
  head_seq     int4 NOT NULL,
  published_seq int4,                           -- the version shown at the public permalink; NULL = head
  forked_from  text,                            -- 'm_…@3' or 'ms_…'
  created_by   bigint NOT NULL REFERENCES app.users (id),
  created_at   timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at   timestamptz,
  UNIQUE (workspace_id, slug)
);
CREATE TABLE app.map_versions (
  map_id text NOT NULL REFERENCES app.maps (id) ON DELETE CASCADE,
  seq    int4 NOT NULL,
  doc    jsonb NOT NULL, doc_sha256 bytea NOT NULL,
  frozen boolean NOT NULL,                      -- pin is a release and breaks/analysis results are written in
  message text, created_by bigint NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (map_id, seq)
);
CREATE TABLE app.map_refs (                     -- what a map version depends on: access checks, takedown, result pinning
  map_id text NOT NULL, seq int4 NOT NULL,
  ref_kind text NOT NULL CHECK (ref_kind IN ('dataset','ws_dataset','analysis','external_host')),
  ref text NOT NULL,
  PRIMARY KEY (map_id, seq, ref_kind, ref)
);
```

- A snapshot's id is a hash of its canonical JSON (RFC 8785), so creating the same map twice returns the same link and nothing can be edited after sharing.
- Snapshots unread for 24 months are deleted, except those created by a signed-in user.
- Versions are immutable; "save" appends. `app.map_refs` is filled at save time by `lanka-mapdoc` and answers "which maps use this dataset" for takedown ([08 §6](08-openness-and-governance.md#6-takedown)) and "which analysis results must not be evicted" ([11 §9](11-analysis-engine.md#9-caching-and-materialisation)).
- Documents are validated against the schema and limits on every write. Text fields are stored as text and rendered as text.

## 6. Permalinks, embeds, forks, export

### 6.1 API (app plane)

| Call | Auth | Does |
|---|---|---|
| `POST /app/v1/maps/snapshots` | none (5 / hour / IP) or key | store a document, return `{id: "ms_…", url}`; idempotent |
| `GET /app/v1/maps/{id}` · `/{id}/versions` · `/{id}@{seq}` | by visibility | the document (snapshot or map version) |
| `GET /app/v1/maps/{id}/resolved` · `POST /app/v1/maps/resolve` | by visibility / none | §2.3 |
| `POST /app/v1/workspaces/{ws}/maps` · `PUT /app/v1/maps/{id}` (appends a version) · `PATCH` (visibility, slug, publish) · `DELETE` | editor | saved maps |
| `POST /app/v1/maps/{id}/fork` | signed in (or none → returns a document to edit locally) | copy with `meta.forked_from` |
| `GET /app/v1/maps/{id}/package` | by visibility | Frictionless `datapackage.json` listing, per layer, the download URLs of the data behind it and its licence |
| `GET /v2/oembed?url=` | none | extended to map URLs ([06 §7](06-serving.md#7-embeds-and-discoverability)) |

`GET /v2/embed/spec?type=map` of [06 §7](06-serving.md#7-embeds-and-discoverability) is superseded for maps: the embed spec of a map **is** its resolved document.

### 6.2 URLs

| URL | Meaning |
|---|---|
| `/m/{ms_…}` | snapshot — immutable |
| `/m/{m_…}` | saved map, published version (floats if `pin: latest`) |
| `/m/{m_…}@{seq}` | one exact version — the citable form when the version is frozen |
| `/@{workspace}/{slug}` | vanity URL of a public or unlisted map |
| `/embed/m/{id}[@seq]?controls=legend,layers,time,search&interactive=1&lang=&theme=` | iframe target |
| `/id/map/{id}` | persistent URI (303 to the page or the JSON, as in [02 §1](02-standard-model.md#1-identifiers)) |
| `/explore#d=<deflate+base64url>` | a whole document in the URL fragment: no server state. Used when the app plane is off (read-only mirrors) and as the editor's local draft format; limited to documents ≤ 6 KB compressed |

View state that is not part of the document (camera after the user panned, the time cursor, selected unit) goes in the query string (`?v=79.98,6.86,9.2&t=2024&sel=LK1103005`) so a link can point *into* a shared map without forking it.

### 6.3 Embeds

- The embed route renders the resolved document with a reduced chrome. Attribution, estimate and community-data notices are part of the resolved document and are always drawn.
- Host page integration by `postMessage` (origin-checked): commands `ldl:setView`, `ldl:setTime`, `ldl:setLayerVisibility`, `ldl:select`; events `ldl:ready`, `ldl:viewchange`, `ldl:select {geo, values}`. The message schema is versioned in `spec/map/1/embed-messages.schema.json`.
- `Content-Security-Policy: frame-ancestors` is `*` for public and unlisted maps and the embed profile's origin list for private ones; private embeds need an embed token ([12 §7](12-enterprise-and-byod.md#7-white-label-embeds)).

### 6.4 Export

| Export | How |
|---|---|
| Image (PNG, 1×–3×) | client-side: re-render at the target pixel ratio, composite legend, title, scale bar, north arrow, attribution and notices, `canvas.toBlob`. Attribution is drawn into the bitmap. |
| Print / PDF | browser print of a print layout route (`/m/{id}/print?size=a4&orientation=landscape`) |
| Legend (SVG) | client-side from the style model |
| Data | the `package` call above: per layer, links to `/v2/frame?format=csv|parquet`, feature GeoParquet, grid COG, analysis result bundle. Tier rules apply because these are the ordinary endpoints: a `link`-tier or private layer appears in the package as metadata only. |
| Document | the JSON itself (`.ldlmap.json`) — importable by any deployment that has the referenced datasets |
| Share-preview image | uploaded by the client at save time for signed-in users (PNG ≤ 200 KB, stored in the blob store); snapshots get a generic card |

## 7. Worked example — "Flood exposure vs property prices, Western Province"

The price data in this example is the **synthetic sample** `@demo/property-asking-prices-sample` ([11 §10](11-analysis-engine.md#10-data-reality-property-prices)); no open sub-district price data exists for Sri Lanka. `exp.flood_pop_share` is the share-of-population indicator of `derived-flood-exposure`.

```jsonc
{
  "spec": "ldl-map/1",
  "id": "m_7k2q9x4w8n1c",
  "title": { "en": "Flood exposure vs property prices — Western Province" },
  "description": "Where modelled 1-in-100-year river flooding coincides with lower asking prices. Associations, not causes.",
  "pin": { "release": "20270315.1" },
  "extent": { "geo": "LK1" },
  "view": { "center": [79.98, 6.86], "zoom": 9.2, "bounds": [79.80, 6.38, 80.25, 7.30] },
  "basemap": { "style": "ldl-light", "labels": "top", "hillshade": { "on": true, "exaggeration": 0.5 } },
  "time": null,

  "sources": {
    "flood":    { "kind": "grid", "grid": "haz.flood_depth", "dims": { "return_period": "RP100" }, "period": "latest" },
    "exposure": { "kind": "indicator", "indicator": "exp.flood_pop_share", "dims": { "return_period": "RP100" },
                  "level": 4, "parent": "LK1", "period": "latest" },
    "prices":   { "kind": "indicator", "indicator": "@demo/re.asking_price_per_perch_median",
                  "level": 4, "parent": "LK1", "period": "2026", "version": { "dsv": "dsv_0d7e55aa91c3b672" } },
    "listings": { "kind": "features", "dataset": "@demo/property-asking-prices-sample",
                  "fields": ["price_per_perch", "land_use", "listed_month"], "within": "LK1",
                  "version": { "dsv": "dsv_3a91f0e2c7d45b10" } },
    "rel_biv":  { "kind": "analysis", "ref": "rel",  "output": "bivariate" },
    "rel_lee":  { "kind": "analysis", "ref": "rel",  "output": "local_lee" },
    "zone_pts": { "kind": "analysis", "ref": "zone", "output": "points" },
    "dsd":      { "kind": "boundaries", "scheme": "admin", "level": 3, "parent": "LK1" }
  },

  "analyses": {
    "rel": {
      "spec": "ldl-analysis/1", "method": "relate",
      "support": { "kind": "admin", "level": 4, "within": "LK1" },
      "variables": {
        "x": { "source": "exposure" },
        "y": { "source": "prices", "transform": "log" },
        "controls": [
          { "indicator": "est.pop_density", "period": "2026", "transform": "log" },
          { "indicator": "acc.travel_time_city", "dims": { "city": "colombo", "mode": "car" } }
        ]
      },
      "params": { "correlation": ["pearson", "spearman"], "regression": "ols", "robust": true,
                  "spatial": { "weights": "queen", "local": "lee", "permutations": 999 },
                  "bivariate": { "classes": 3, "method": "quantile" }, "scale_sensitivity": [3, 4] },
      "result": "an_5e1f0c22ab904d71"
    },
    "zone": {
      "spec": "ldl-analysis/1", "method": "group_compare",
      "support": { "kind": "points", "source": "listings" },
      "variables": { "y": { "field": "price_per_perch", "transform": "log" } },
      "params": { "zone": { "source": "flood", "where": { "op": ">=", "args": [{ "property": "value" }, 0.15] } },
                  "rings_m": [0, 250, 500], "compare": "inside_vs_ring", "within_groups": { "level": 3 },
                  "tests": ["welch", "mann_whitney"], "bootstrap": 2000 },
      "result": "an_9b40d6e1173c2a58"
    }
  },

  "layers": [
    { "id": "l_flood", "type": "raster", "name": "Flood depth, 1-in-100-year (m)", "source": "flood",
      "visible": true, "opacity": 0.9, "blend": "multiply",
      "style": { "fill": { "field": "value", "scale": { "type": "manual", "breaks": [0.15, 0.5, 1, 2],
                                                         "ramp": "ldl.seq.blue", "nodata": "transparent" } },
                 "range": [0.15, null], "resample": "linear" } },

    { "id": "g_relationship", "type": "group", "name": "Exposure × price", "visible": true, "opacity": 1, "children": [
      { "id": "l_biv", "type": "bivariate", "name": "Flood exposure × median asking price", "source": "rel_biv",
        "visible": true, "opacity": 0.8, "blend": "normal", "zoom": [5, 22],
        "style": { "fill": { "bivariate": {
                     "x": { "field": "x", "scale": { "type": "quantile", "classes": 3, "breaks": [0.4, 6.9] } },
                     "y": { "field": "y", "scale": { "type": "quantile", "classes": 3, "breaks": [1450000, 3900000] } },
                     "palette": "ldl.biv.blue-orange" } },
                   "stroke": { "color": "#ffffff", "width": 0.3 }, "uncertainty": "hatch" },
        "interaction": { "hover": true, "select": true, "tooltip": ["name", "x", "y", "n_listings", "basis"] } },

      { "id": "l_lee", "type": "polygon", "name": "Significant local association (Lee's L, FDR 5 %)", "source": "rel_lee",
        "visible": true, "opacity": 1, "blend": "normal",
        "filter": { "op": "<=", "args": [{ "property": "p_fdr" }, 0.05] },
        "style": { "fill": "none",
                   "stroke": { "width": 1.6, "color": { "field": "cluster", "scale": { "type": "categorical",
                     "mapping": { "HH": "#b2182b", "HL": "#7b3294", "LH": "#008837", "LL": "#2166ac" } } } } } }
    ] },

    { "id": "l_listings", "type": "point", "name": "Sample listings: inside / near / outside flood zone",
      "source": "zone_pts", "visible": false, "opacity": 0.9, "blend": "normal", "zoom": [11, 22],
      "style": { "size": 4,
                 "fill": { "field": "ring", "scale": { "type": "categorical",
                   "mapping": { "inside": "#08519c", "0-250": "#6baed6", "250-500": "#c6dbef" } } } } },

    { "id": "l_dsd", "type": "line", "name": "DS divisions", "source": "dsd", "visible": true, "opacity": 0.7,
      "style": { "stroke": { "color": "#3a3a3a", "width": 0.8 } } },
    { "id": "l_dsd_labels", "type": "label", "name": "DS division names", "source": "dsd", "visible": true,
      "zoom": [9, 12], "style": { "label": { "text": "{name}", "size": 11, "halo": "#ffffff" } } }
  ],

  "panels": [
    { "id": "p_legend",  "type": "legend" },
    { "id": "p_scatter", "type": "chart", "chart": "scatter", "analysis": "rel", "linked": "l_biv",
      "options": { "fit": ["ols", "robust"], "density": "hexbin", "brush": true } },
    { "id": "p_stats",   "type": "stats", "analysis": "rel" },
    { "id": "p_zone",    "type": "stats", "analysis": "zone" }
  ],

  "meta": { "author": "@demo", "licence": "CC-BY-4.0", "created_at": "2027-03-16T04:12:00Z",
            "forked_from": null, "generator": "lanka-web 2.3.0" }
}
```

### API calls behind it

| # | Call | Returns | Cache |
|---|---|---|---|
| 1 | `GET /app/v1/maps/m_7k2q9x4w8n1c/resolved` | the document + `resolved` block (dsvs, URLs, attribution, notices, analysis states) | 300 s; immutable for `@seq` |
| 2 | `GET /dl/tiles/admin/9f3a1c2e77b04d15.pmtiles` (range requests) | boundary geometry, ADM3 + ADM4 + centroids | immutable |
| 3 | `GET /v2/r/20270315.1/grids/haz.flood_depth.rp100/values/{z}/{x}/{y}.png` | `ldl-v16` value tiles for layer `l_flood` | immutable |
| 4 | `POST /app/v1/ws/demo/analyses` with `analyses.rel` → `303` → `GET /app/v1/ws/demo/analyses/an_5e1f0c22ab904d71` | the result: estimates, diagnostics, guards, outputs ([11 §5](11-analysis-engine.md#5-requests-and-results)). Already computed when the map was frozen, so this is a read. | immutable, private scope |
| 5 | `GET …/analyses/an_5e1f…/outputs/bivariate?format=arrow` | frame: `codes`, `x`, `y`, `class`, `n_listings`, `basis` → layer `l_biv` and the scatter panel | immutable |
| 6 | `GET …/analyses/an_5e1f…/outputs/local_lee?format=arrow` | frame: `codes`, `L`, `p`, `p_fdr`, `cluster` → layer `l_lee` | immutable |
| 7 | `GET /app/v1/ws/demo/analyses/an_9b40d6e1173c2a58` + `/outputs/points?format=arrow` | group comparison result; points with `ring`, `log_price`, `depth_m` → layer `l_listings` (fetched on first reveal) | immutable |
| 8 | `GET /dl/tiles/dem/<sha16>.pmtiles` | `raster-dem` for hillshade | immutable |
| 9 | `GET /app/v1/maps/m_7k2q9x4w8n1c/package` (on "Export data") | `datapackage.json` with per-layer download links and licences | 300 s |

Calls 4–7 go through the app plane because one input (`@demo/…`) is workspace data; the same analysis on public inputs only would be `POST /v2/analyses` and world-cacheable. The direct frames for `exposure` and `prices` are not fetched: no layer draws them directly (the analysis output carries both columns).

What the reader sees first: GN divisions in a 3×3 palette (dark = high exposure and high price, the off-diagonal colours = the pattern in question), the flood raster multiplied on top, outlines around divisions where the local association is significant, a scatter with OLS and robust fits, and a stats card whose first lines are the guard-rail messages.
