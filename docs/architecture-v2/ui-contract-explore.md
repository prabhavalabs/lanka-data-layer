# UI contract — Explore: layer panel and analysis

For the web team, to build the layer panel and the analysis UI against mock data **now**. It mirrors the map document spec ([10](10-map-composition.md)), the layer data shapes ([10 §3](10-map-composition.md#3-serving-layer-data)) and the analysis request/response ([11 §5](11-analysis-engine.md#5-requests-and-results)).

Status: **contract v0** (2026-11-22). The JSON Schemas in `spec/map/1/` and `spec/analysis/1/` (WP-26, WP-29) become the source of truth and these types are then generated from them. Until then this file is the source of truth; a change here is a contract change and is announced in the PR title.

Stack assumed: React 19, Vite, TanStack Query, shadcn/ui, MapLibre GL JS, deck.gl (interleaved `MapboxOverlay`).

## 1. What can be built now, and when the real thing arrives

| Front-end increment | Needs (mock now) | Real backend |
|---|---|---|
| FE-1 Layer panel: add, reorder, group, toggle, opacity, blend, per-layer style editor, legend, time slider | this file + fixtures of §8 | — |
| FE-2 Real layers | `GET /v2/frame`, boundary tiles, value tiles, columnar points | WP-26 (frames), WP-04/05 (tiles, grids), WP-27 (value tiles, points) |
| FE-3 Share, fork, embed, image export | `POST /app/v1/maps/snapshots`, `resolve` | WP-28 |
| FE-4 Analysis: relate, inside/outside, clusters, exposure, stats cards | `POST /v2/analyses`, `/plan` | WP-29 (tier 1), WP-30 (spatial) |
| FE-5 Sign-in, saved maps, keys | `/app/v1/me`, `/workspaces`, `/maps` | WP-31 |
| FE-6 Upload wizard | `/app/v1/ws/{ws}/uploads` | WP-32 |

Base URLs: public plane `VITE_API_BASE` + `/v2`; app plane `VITE_API_BASE` + `/app/v1`. Public-plane requests are sent with `credentials: 'omit'`; app-plane requests with `credentials: 'include'`.

## 2. Envelope and shared types

```ts
export type Envelope<T> = { success: true; message: string; payload: T; meta: Meta }
                        | { success: false; message: string; payload: Problem; meta?: Meta }

export type Problem = { type: string; title: string; status: number; detail?: string;
                        hints?: { valid_values?: string[]; did_you_mean?: string[]; available_levels?: number[];
                                  nearest_available_period?: string; download?: string } }

export type Meta = {
  data_version: string
  contract_version: 2
  dataset_versions: { dataset: string; dsv: string; vintage?: string }[]
  attribution: Attribution[]
  contains: DataClass[]                 // which groups of data are in the response
  estimate_notice: string | null
  live: boolean
}

export type Attribution = { source_id: string; publisher: string; text: string; licence: string; licence_url?: string
                            licence_status: 'open' | 'attribution' | 'permission_unclear' | 'restricted'
                            redistribution: 'bulk' | 'api' | 'link' | 'internal' }

export type Text = string | { en: string; si?: string; ta?: string }
export type GeoCode = string                       // p-code 'LK1103005' or 'scheme:code'
export type PeriodCode = string                    // '2024' | '2024-Q2' | '2026-08' | '2024-Maha' | '2026-11-20'
export type Freq = 'A' | 'F' | 'H' | 'S' | 'Q' | 'M' | 'W' | 'D' | 'P'
export type Basis = 'reported' | 'computed' | 'estimated' | 'forecast'
export type Status = 'normal' | 'provisional' | 'revised' | 'break' | 'low_reliability' | 'missing' | 'nil' | 'suppressed'
export type DataClass = 'official' | 'third_party' | 'derived' | 'estimated' | 'user'
export type Dims = Record<string, string>          // { sex: 'F', return_period: 'RP100' }
export type Visibility = 'private' | 'unlisted' | 'public'
```

Identifier forms the UI must handle: public `census24.pop_total`, `derived-flood-exposure`; workspace `@acme/re.asking_price_median`, `@acme/prices-2026`; map `m_…`, snapshot `ms_…`, analysis `an_…`.

## 3. Map document — `ldl-map/1`

```ts
export type MapDocument = {
  spec: 'ldl-map/1'
  id?: string
  title: Text
  description?: string                             // CommonMark subset; render as text + links only
  pin: 'latest' | { release: string }
  view: { center: [lon: number, lat: number]; zoom: number; bearing?: number; pitch?: number
          bounds?: [w: number, s: number, e: number, n: number] }
  extent?: { geo: GeoCode } | { bbox: [number, number, number, number] } | null
  basemap: { style: 'ldl-light' | 'ldl-dark' | 'ldl-mono' | 'none' | (string & {})
             labels: 'top' | 'bottom' | 'off'
             hillshade?: { on: boolean; exaggeration: number } }
  time?: TimeConfig | null
  sources: Record<SourceId, Source>
  analyses?: Record<AnalysisRef, AnalysisRequest & { result?: string }>     // result = 'an_…' once frozen
  layers: LayerNode[]                              // bottom → top. The panel renders the reverse.
  panels?: Panel[]
  meta?: { author?: string; licence?: string; created_at?: string; updated_at?: string
           forked_from?: string | null; generator?: string }
  [x: `x-${string}`]: unknown                      // preserve on save
}

export type SourceId = string
export type AnalysisRef = string
export type LayerId = string

export type TimeConfig = { cursor: PeriodCode; range: [PeriodCode, PeriodCode]; freq: Freq
                           playback?: { fps: number; loop: boolean } }
```

### 3.1 Sources

```ts
export type VersionRef = 'latest' | { dsv: string }           // omitted = inherit document pin
export type PeriodRef = 'latest' | PeriodCode | PeriodCode[] | { from: PeriodCode; to: PeriodCode }
                      | { bind: 'time'; match?: 'exact' | 'asof' }
export type LevelRef = number | { byZoom: [zoom: number, level: number][] }

export type Source =
  | { kind: 'indicator'; indicator: string; indicators?: string[]; level: LevelRef; scheme?: string
      parent?: GeoCode; period: PeriodRef; dims?: Dims; version?: VersionRef }
  | { kind: 'boundaries'; scheme: string; level: number; parent?: GeoCode }
  | { kind: 'features'; dataset: string; featureKind?: string; within?: GeoCode; fields?: string[]; version?: VersionRef }
  | { kind: 'grid'; grid: string; dims?: Dims; period: PeriodRef; version?: VersionRef }
  | { kind: 'analysis'; ref: AnalysisRef; output: string }
  | { kind: 'inline'; geojson: GeoJSON.FeatureCollection }                   // ≤ 100 KB
  | { kind: 'external'; type: 'geojson' | 'pmtiles' | 'raster-xyz'; url: string; attribution: string }
```

### 3.2 Layers

```ts
export type BlendMode = 'normal' | 'multiply' | 'screen' | 'darken' | 'lighten' | 'add'
export type LayerType = 'choropleth' | 'bivariate' | 'symbol' | 'point' | 'cluster' | 'heatmap' | 'bins'
                      | 'line' | 'polygon' | 'raster' | 'hillshade' | 'label' | 'annotation'

export type LayerNode = Layer | LayerGroup

export type LayerGroup = { id: LayerId; type: 'group'; name: string; visible: boolean; opacity: number
                           collapsed?: boolean; children: LayerNode[] }        // depth ≤ 3

export type Layer = {
  id: LayerId
  type: LayerType | `x-${string}`                  // unknown x- types render as a placeholder row
  name: string
  source: SourceId
  visible: boolean
  opacity: number                                  // 0..1
  blend?: BlendMode                                // default 'normal'
  zoom?: [min: number, max: number]
  filter?: Cql2                                    // §3.4
  clip?: { geo: GeoCode } | { bbox: [number, number, number, number] }
  style: LayerStyle
  legend?: { show: boolean; title?: Text | null }
  interaction?: { hover?: boolean; select?: boolean; tooltip?: string[] }     // field names
  locked?: boolean
  [x: `x-${string}`]: unknown
}
```

### 3.3 Style

```ts
export type RampId = string                        // 'ldl.seq.teal' | 'ldl.div.red-blue' | 'ldl.cat.set2' | 'ldl.biv.blue-orange' | …
export type Color = string                         // '#rrggbb' | '#rrggbbaa' | 'transparent' | 'none'

export type ScaleBase = { ramp?: RampId | Color[]; reverse?: boolean; nodata?: Color
                          domain?: 'extent' | 'data' | 'national'            // default 'extent'
                          across?: 'all_periods' | 'period' }                // default 'all_periods'

export type Scale =
  | ScaleBase & { type: 'quantile' | 'equal' | 'jenks' | 'stddev' | 'headtail' | 'pretty'; classes: number; breaks?: number[] }
  | ScaleBase & { type: 'manual'; breaks: number[] }
  | ScaleBase & { type: 'continuous'; transform?: 'linear' | 'log' | 'sqrt' | 'symlog'; clamp?: [number, number] | ['p', number, number] }
  | ScaleBase & { type: 'diverging'; mid: number | 'mean' | 'median'; classes: number; breaks?: number[] }
  | ScaleBase & { type: 'categorical'; mapping?: Record<string, Color> }

/** A visual channel: a constant, or a field through a scale. */
export type Channel<T> = T | { field: string; scale: Scale }
export type BivariateFill = { bivariate: { x: { field: string; scale: Scale }; y: { field: string; scale: Scale }; palette: RampId } }

export type LayerStyle = {
  fill?: Channel<Color> | BivariateFill
  stroke?: { color?: Channel<Color>; width?: Channel<number>; dash?: number[] }
  size?: Channel<number> | { field: string; scale: { type: 'area' | 'radius' | 'graduated'; range: [number, number]; breaks?: number[] } }
  shape?: 'circle' | 'square' | 'bar'
  icon?: string
  label?: { text: string; size?: number; halo?: Color; zoom?: [number, number]; priority?: string }
  // type-specific
  range?: [number | null, number | null]           // raster: mask values outside
  resample?: 'nearest' | 'linear'                  // raster
  radius?: number; intensity?: number; weight?: string          // heatmap
  cell?: number; aggregate?: 'count' | { sum: string } | { mean: string }; elevation?: Channel<number>   // bins, cluster
  exaggeration?: number                            // hillshade
  uncertainty?: 'hatch' | 'fade' | 'off'           // units with wide intervals; default 'hatch' when lo/hi exist
}
```

Field names available to `field`, `filter`, `tooltip`, `label.text`:

| Source kind | Fields |
|---|---|
| indicator (one indicator) | `value`, `lo`, `hi`, `basis`, `status`, `period`, `code`, `name`, `level` |
| indicator (several) | the indicator ids, plus `<id>.lo`, `<id>.hi`, `<id>.basis` |
| analysis | the output's `columns[]` (from the result) |
| features | declared feature fields, `kind`, `name`, `key` |
| grid | `value` |
| boundaries | `code`, `name`, `level`, `parent` |

### 3.4 Filters — CQL2-JSON subset

```ts
export type Cql2 =
  | { op: 'and' | 'or'; args: Cql2[] }
  | { op: 'not'; args: [Cql2] }
  | { op: '=' | '<>' | '<' | '<=' | '>' | '>='; args: [Cql2Operand, Cql2Operand] }
  | { op: 'in'; args: [Cql2Operand, (string | number)[]] }
  | { op: 'between'; args: [Cql2Operand, number, number] }
  | { op: 'like'; args: [Cql2Operand, string] }
  | { op: 'isNull'; args: [Cql2Operand] }
export type Cql2Operand = { property: string } | string | number | boolean
```

### 3.5 Panels

```ts
export type Panel =
  | { id: string; type: 'legend' }
  | { id: string; type: 'chart'; chart: 'scatter' | 'histogram' | 'series' | 'groups'
      source?: SourceId; analysis?: AnalysisRef; linked?: LayerId
      options?: { fit?: ('ols' | 'robust')[]; density?: 'points' | 'hexbin'; brush?: boolean } }
  | { id: string; type: 'stats'; analysis: AnalysisRef }
  | { id: string; type: 'text'; text: string }
```

### 3.6 Limits the editor must enforce

32 layers · 8 groups (depth 3) · 16 sources · 6 analyses · 256 KB canonical JSON · inline GeoJSON 100 KB. Visible at once: 4 `raster`, 2 `heatmap`/`bins`, 500,000 points (phone: 2, 1, 150,000). Over a cap the panel shows the row as "paused — too many layers of this kind visible"; it does not delete or silently hide.

## 4. Resolved document

The renderer never builds a data URL. It asks for the resolved document and uses what it gets.

```ts
export type ResolvedMap = MapDocument & { resolved: Resolved }

export type Resolved = {
  data_version: string
  sources: Record<SourceId, ResolvedSource>
  analyses: Record<AnalysisRef, { id: string; state: AnalysisState }>
  attribution: Attribution[]
  notices: { code: 'ESTIMATE' | 'COMMUNITY_DATA' | 'PRIVATE_DATA' | 'SYNTHETIC' | 'EXTERNAL'; text: string }[]
  problems: { path: string; code: 'WITHDRAWN' | 'NOT_FOUND' | 'RENAMED' | 'LEVEL_UNAVAILABLE' | 'PERIOD_UNAVAILABLE'
              hint?: Record<string, unknown> }[]
}

export type ResolvedSource = { access: 'public' | 'granted' | 'denied'; dsv?: string; class?: DataClass; basis?: Basis[] } & (
  | { shape: 'frame'; frame: string; tileset: string; tiles: string; source_layer: string; join: 'pcode' | 'code'
      centroids_layer: string; periods?: PeriodCode[]; tilejson: string; minzoom: number /* of source_layer */; maxzoom: number
      by_zoom?: { zoom: number; level: number; frame: string; source_layer: string; minzoom: number }[] }
  | { shape: 'boundaries'; tiles: string; source_layer: string; tilejson: string; minzoom: number; maxzoom: number }
  | { shape: 'values'; values: string /* {z}/{x}/{y} template */; encoding: 'ldl-v16'; scale: number; offset: number
      nodata: number; minzoom: number; maxzoom: number; unit: string; stats: ColumnStats; categories?: Record<number, Text> }
  | { shape: 'points'; points: string; count: number; fields: FieldInfo[] }
  | { shape: 'tiles'; tiles: string; source_layers: string[]; fields: FieldInfo[] }
  | { shape: 'geojson'; url: string; fields: FieldInfo[] }
  | { shape: 'dem'; tiles: string; encoding: 'terrarium' }
  | { shape: 'inline' } | { shape: 'external'; url: string }
)

export type FieldInfo = { id: string; label: Text; type: 'number' | 'category' | 'string' | 'date'; unit?: string
                          measure_type?: MeasureType; codes?: Record<string, Text> }
export type MeasureType = 'count' | 'sum' | 'rate' | 'ratio' | 'share' | 'mean' | 'median' | 'index' | 'score' | 'rank' | 'category'
```

UI rules that follow from it:

- Draw `attribution` and every `notices[]` entry. There is no prop to hide them.
- `access: 'denied'` → the row shows a lock and "private data"; the rest of the map renders.
- `problems[]` → an inline repair prompt on the affected row.
- `class: 'estimated'` or `basis` containing `estimated`/`forecast` → the estimate badge on the row and in the legend.

## 5. Layer data

### 5.1 Frame

`GET /v2/frame?indicators=&level=&parent=&periods=|from=&to=&dims=&format=json|arrow` (workspace data: `/app/v1/ws/{ws}/frame`).

```ts
export type Frame = {
  support: { kind: 'admin' | 'scheme'; scheme: string; level: number; parent?: GeoCode; boundary_set: string; n: number }
  codes: GeoCode[]
  columns: FrameColumn[]
}
export type FrameColumn = {
  id: string                                       // 'c0' — the feature-state key is `${sourceId}.${id}`
  indicator: string; dims: Dims; period: PeriodCode
  unit: string; class: DataClass; ci_level?: number
  values: (number | null)[]
  lo?: (number | null)[] | null; hi?: (number | null)[] | null
  basis: Basis | number[]                          // scalar when uniform
  status: Status | number[]
  flags?: number[] | null
  stats: ColumnStats
}
export type ColumnStats = { n: number; min: number; max: number; mean: number
                            q: [p0: number, p5: number, p25: number, p50: number, p75: number, p95: number, p100: number] }
```

`format=arrow`: same table as Arrow IPC; columns `code`, then per frame column `<id>`, `<id>.lo`, `<id>.hi`; column metadata carries the JSON above minus the arrays.

### 5.2 Value tiles (`ldl-v16`)

RGBA PNG. `q = R*256 + G`; value = `q*scale + offset`; `q === 65535` → no data; `B` = valid-cell share 0–255; `A` = 255. Decode with `createImageBitmap(blob, { premultiplyAlpha: 'none', colorSpaceConversion: 'none' })`. The ramp, range mask and class breaks are shader uniforms: changing them must not refetch.

### 5.3 Columnar points

`GET /v2/features/{dataset}/points?fields=&within=&format=arrow` → Arrow IPC: `lon: Float32`, `lat: Float32`, `key: Utf8`, then requested fields (`Float32` or dictionary). JSON fallback for ≤ 2,000 rows: `{ lon: number[]; lat: number[]; key: string[]; fields: Record<string, (number | string | null)[]> }`.

## 6. Analysis

### 6.1 Request

```ts
export type MethodId = 'describe' | 'relate' | 'regress' | 'group_compare' | 'exposure' | 'zonal'
                     | 'autocorr' | 'local_clusters' | 'hotspots' | 'spatial_regress' | 'change' | 'composite'
                     | 'interpolate' | 'isochrone' | 'gwr'

export type Transform = 'identity' | 'log' | 'log1p' | 'sqrt' | 'zscore' | 'rank'

export type SupportRef = 'auto'
  | { kind: 'admin'; level: number; scheme?: string; within?: GeoCode }
  | { kind: 'grid'; factor: 1 | 5 | 10 | 50; within?: GeoCode }
  | { kind: 'points'; source?: SourceId; dataset?: string; within?: GeoCode }

export type AlignSpec = { method: 'identity' | 'rollup' | 'grid_zonal' | 'features_zonal' | 'point_aggregate' | 'sample'
                                  | 'crosswalk_rebase' | 'dasymetric' | 'inherit' | 'spatial_interp'
                          stat?: 'sum' | 'mean' | 'median' | 'pop_weighted_mean' | 'share_where' | 'max' | 'count'
                          min_n?: number; where?: Cql2; [k: string]: unknown }

export type Variable = { transform?: Transform; align?: AlignSpec; label?: Text } & (
  | { source: SourceId }                                                  // inside a map document
  | { indicator: string; dims?: Dims; period?: PeriodRef }
  | { grid: string; dims?: Dims; period?: PeriodRef }
  | { dataset: string; field: string }
  | { field: string }                                                     // a field of the points support
)

export type AnalysisRequest = {
  spec: 'ldl-analysis/1'
  method: MethodId
  pin?: 'latest' | { release: string }
  support: SupportRef
  variables: { x?: Variable; y?: Variable; controls?: Variable[]; weight?: Variable; group?: Variable }
  params?: Record<string, unknown>                 // per method; schema from GET /v2/analysis/methods/{id}
  override?: GuardCode[]
}
```

Parameter shapes for the three methods FE-4 starts with:

```ts
export type RelateParams = { correlation?: ('pearson' | 'spearman')[]; regression?: 'ols' | 'none'; robust?: boolean
  spatial?: { weights: 'queen' | 'knn4' | 'knn6' | 'knn8'; local?: 'lee' | 'moran' | 'none'; permutations?: number } | null
  bivariate?: { classes: 3 | 4; method: 'quantile' | 'equal' | 'jenks' }
  scale_sensitivity?: number[]; uncertainty?: 'analytic' | 'mc' }

export type GroupCompareParams = { zone: { source?: SourceId; grid?: string; dataset?: string; where?: Cql2 }
  rings_m?: number[]; compare: 'inside_vs_outside' | 'inside_vs_ring'; within_groups?: { level: number } | null
  tests?: ('welch' | 'mann_whitney')[]; bootstrap?: number }

export type HotspotsParams = { weights: 'queen' | 'knn8' | { band_km: number }; fdr?: number; permutations?: number }
```

### 6.2 Plan (dry run)

`POST /v2/analyses/plan` — call it whenever the user changes a selection, before offering "Run".

```ts
export type AnalysisPlan = {
  support: Exclude<SupportRef, 'auto'> & { n_expected: number }
  alignment: AlignmentStep[]
  cost: { mode: 'sync' | 'async'; expected_ms: number; within_quota: boolean }
  guards: Guard[]                                  // those knowable before running (ECOLOGICAL, SIZE_CONFOUND, PERIOD_MISMATCH, …)
  alternatives?: { label: string; request: AnalysisRequest }[]      // e.g. "analyse at point level instead"
}
```

### 6.3 Result

```ts
export type AnalysisState = 'queued' | 'running' | 'ready' | 'failed' | 'blocked'
export type Severity = 'note' | 'warn' | 'block'
export type GuardCode = 'NOT_CAUSAL' | 'MIN_N' | 'SMALL_N' | 'MAUP' | 'MAUP_UNSTABLE' | 'ECOLOGICAL' | 'RESIDUAL_AUTOCORR'
  | 'SIZE_CONFOUND' | 'SHARED_LINEAGE' | 'ESTIMATED_INPUT' | 'MODEL_ON_MODEL' | 'INPUT_UNCERTAINTY_IGNORED' | 'PERIOD_MISMATCH'
  | 'LOW_COVERAGE' | 'PSEUDO_REPLICATION' | 'DISAGGREGATED_INPUT' | 'INTERP_SOCIO' | 'OUTLIER_LEVERAGE' | 'NO_CONTROLS'
  | 'MULTICOLLINEAR' | 'MULTIPLE_TESTING' | 'SUPPRESSED_CELLS' | 'SYNTHETIC_INPUT' | 'COMMUNITY_INPUT' | 'EXPLORATORY'

export type Guard = { code: GuardCode; severity: Severity; message: string; detail?: Record<string, unknown>
                      action?: { label?: string; request?: Partial<AnalysisRequest> }; overridden?: boolean }

export type AlignmentStep = { role: string; from: string; to: string; method: string; params?: Record<string, unknown>
                              coverage: number; quality?: 'A' | 'B' | 'C' | 'D'; dropped?: Record<string, number> }

export type Interval = [lo: number, hi: number]

export type AnalysisResult<E = unknown> = {
  spec: 'ldl-analysis-result/1'
  id: string
  state: AnalysisState
  retry_after_s?: number                           // when queued | running
  method: { id: MethodId; version: string; citations: string[] }
  request: AnalysisRequest                         // fully resolved
  inputs: { role: string; dataset: string; dsv: string; indicator?: string; field?: string; period?: PeriodCode
            class: DataClass; licence: string; visibility: Visibility; synthetic?: boolean }[]
  alignment: AlignmentStep[]
  n: { units?: number; points?: number; used: number; dropped: Record<string, number> }
  estimates: E | null                              // null when blocked
  guards: Guard[]                                  // sorted: block, warn, note; NOT_CAUSAL first among notes
  interpretation: { summary: string; caveats: string[] } | null
  outputs: AnalysisOutput[]
  basis: Basis
  licence: { id: string; redistribution: 'bulk' | 'api' | 'link' | 'internal' }
  attribution: Attribution[]
  provenance: { fingerprint: string; seed: string /* u64, decimal digits */; engine: string; git_rev: string; data_version: string
                computed_at: string; elapsed_ms: number }
  citation: { text: string; url: string }
}

export type AnalysisOutput = { id: string; href: string; suggested_layer?: Pick<Layer, 'type' | 'style' | 'filter' | 'name'> } & (
  | { kind: 'frame'; columns: string[] }           // GET href?format=arrow → a Frame whose columns are named by `columns`
  | { kind: 'grid'; values: string; encoding: 'ldl-v16'; scale: number; offset: number; nodata: number }
  | { kind: 'features'; fields: FieldInfo[] }
  | { kind: 'chart'; encoding: 'points' | 'hexbin' | 'histogram' | 'groups' }
)

export type RelateEstimates = {
  pearson?:  { r: number; ci: Interval; ci_level: number; p?: number; p_adjusted?: number; n_effective?: number }
  spearman?: { rho: number; ci: Interval; p_adjusted?: number }
  ols?: { formula: string; coef: Coef[]; r2: number; adj_r2: number; reading: string }
  robust?: { coef: Coef[] }
  lee_l?: { L: number; p_perm: number }
  residual_moran?: { I: number; p_perm: number }
  scale_sensitivity?: { level: number; n: number; pearson_r: number }[]
}
export type Coef = { term: string; b: number; se_hc3?: number; ci?: Interval; p_adjusted?: number }

export type GroupCompareEstimates = {
  groups: { id: string; label: string; n: number; median: number; mean: number; ci: Interval }[]
  contrasts: { a: string; b: string; diff: number; ci: Interval; hodges_lehmann: number; cohens_d: number
               p_welch?: number; p_mann_whitney?: number; reading: string }[]
}

export type ClusterRow = { cluster: 'HH' | 'HL' | 'LH' | 'LL' | 'ns'; p: number; p_fdr: number }     // local_clusters output columns
export type HotspotRow = { z: number; p_fdr: number; band: 'hot99' | 'hot95' | 'hot90' | 'ns' | 'cold90' | 'cold95' | 'cold99' }
```

Chart outputs:

```ts
export type ScatterData = { encoding: 'points'; x: number[]; y: number[]; codes: GeoCode[] }
                        | { encoding: 'hexbin'; cx: number[]; cy: number[]; count: number[]; radius: number }
export type FitLine = { kind: 'ols' | 'robust'; intercept: number; slope: number; band?: { x: number[]; lo: number[]; hi: number[] } }
```

### 6.4 Rendering rules for analysis UI (binding)

1. Guards render **above** estimates, `block` and `warn` expanded, `note` collapsed except `NOT_CAUSAL`, which is always visible.
2. `state: 'blocked'` → no numbers anywhere; show the blocking guard and its `action`.
3. A number is shown with its interval when one exists. A `p` that is absent is not shown and not computed client-side.
4. When `p_adjusted` exists it is the one shown; `p` only in the detail view, labelled "ignoring spatial dependence".
5. Use the server's `interpretation.summary` and `reading` strings verbatim. Do not compose sentences about the result in the client; never use causal wording in labels or tooltips ("associated with", not "effect of").
6. Browser previews (§7) are labelled "preview", have no cite/save/add-to-map actions, and are replaced by the server result when it arrives.
7. "Add to map" inserts `output.suggested_layer` bound to a new `analysis` source and writes the request into `document.analyses`.
8. Every stats card has "Explain" (inputs with versions and licences, alignment steps with drop counts, method citation) and "Cite" (`citation.text`, `citation.url`).

## 7. Client interfaces

```ts
/** Same kernels as the server, compiled to WebAssembly (WP-29). Mock with plain TS for FE-1; keep the interface. */
export interface StatsKernel {
  classify(values: Float64Array, scale: Scale): { breaks: number[]; classOf: Uint8Array }
  bivariate(x: Float64Array, y: Float64Array, sx: Scale, sy: Scale): { xBreaks: number[]; yBreaks: number[]; classOf: Uint8Array }
  describe(values: Float64Array): ColumnStats
  previewRelate(x: Float64Array, y: Float64Array, tx: Transform, ty: Transform): { r: number; rho: number; fit: FitLine; n: number }
  hexbin(x: Float64Array, y: Float64Array, radius: number): Extract<ScatterData, { encoding: 'hexbin' }>
}

/** One per layer type. `toMapLibre` or `toDeck` is chosen by type and blend (10 §4.1–4.2). */
export interface LayerRenderer<D = unknown> {
  type: LayerType | `x-${string}`
  backend(layer: Layer): 'maplibre' | 'deck'
  load(layer: Layer, source: ResolvedSource, signal: AbortSignal): Promise<D>
  mount(ctx: RenderCtx, layer: Layer, data: D): MountedLayer
}
export interface MountedLayer {
  setStyle(style: LayerStyle): void                // no network
  setOpacity(o: number): void; setBlend(b: BlendMode): void; setVisible(v: boolean): void
  setTime(cursor: PeriodCode): void                // no network
  setFilter(f: Cql2 | undefined): void
  moveBefore(id: LayerId | null): void
  legend(): LegendModel
  pick(x: number, y: number): Picked | null
  unmount(): void
}
export type LegendModel =
  | { kind: 'classes'; title: Text; unit?: string; items: { color: Color; label: string; count?: number }[]; nodata?: Color; estimated?: boolean }
  | { kind: 'continuous'; title: Text; unit?: string; ramp: Color[]; domain: [number, number]; transform: string }
  | { kind: 'bivariate'; xTitle: Text; yTitle: Text; palette: Color[][]; xBreaks: number[]; yBreaks: number[] }
  | { kind: 'categories'; title: Text; items: { color: Color; label: string }[] }
  | { kind: 'size'; title: Text; unit?: string; stops: { value: number; px: number }[] }
export type Picked = { layer: LayerId; code?: GeoCode; key?: string; lngLat: [number, number]; fields: Record<string, unknown> }
```

State model:

- The **document** is the single source of truth for everything shareable. Edits are pure functions `MapDocument → MapDocument`; keep an undo stack of documents (structural sharing) capped at 100.
- **Ephemeral UI state** (hovered unit, selected rows, open panel, playing, brush) is not in the document. The camera and time cursor are mirrored to the query string (`?v=&t=&sel=`), not written to the document until the user saves.
- **Server state** via TanStack Query. Keys: `['resolve', docHashOfSourcesAndPin]`, `['frame', url]`, `['points', url]`, `['analysis', 'plan', requestHash]`, `['analysis', id]` (`staleTime: Infinity` once `ready`; refetch on `retry_after_s` while queued/running). Style, opacity, blend, order and visibility changes must not change any query key.
- Draft persistence: `localStorage['ldl:draft']` = the document; unsaved drafts are shareable as `/explore#d=…` when small.

## 8. Mock fixtures

Put fixtures under `web-v2/src/mocks/explore/` and serve them through the existing mock client so the swap to the real API is a base-URL change.

| Fixture | Shape | Content |
|---|---|---|
| `map.flood-prices.json` | `MapDocument` | the example of [10 §7](10-map-composition.md#7-worked-example--flood-exposure-vs-property-prices-western-province), verbatim |
| `map.flood-prices.resolved.json` | `ResolvedMap` | with mock URLs pointing at the fixtures below |
| `frame.exposure.adm4.lk1.json` | `Frame` | ~2,500 GN codes of Western Province, one column, synthetic values skewed toward 0 |
| `frame.popdensity.adm2.series.json` | `Frame` | 25 districts × 13 annual periods, for the time slider |
| `analysis.relate.result.json` | `AnalysisResult<RelateEstimates>` | the example of [11 §5.2](11-analysis-engine.md#52-result--ldl-analysis-result1), with outputs `bivariate`, `local_lee`, `residuals`, `scatter` |
| `analysis.relate.blocked.json` | `AnalysisResult` | `state: 'blocked'`, guard `MIN_N` |
| `analysis.relate.queued.json` | `AnalysisResult` | `state: 'queued'`, `retry_after_s: 2`, then the ready fixture |
| `analysis.group_compare.result.json` | `AnalysisResult<GroupCompareEstimates>` | inside / 0–250 m / 250–500 m |
| `analysis.plan.json` | `AnalysisPlan` | with an `ECOLOGICAL` guard and a point-level alternative |
| `points.listings.json` | columnar points (JSON fallback) | 5,000 synthetic listings in Western Province with `price_per_perch`, `land_use` |
| value tiles | PNG | a handful of `ldl-v16` tiles generated by a script from a synthetic gradient, to exercise the shader |
| `methods.json` | `GET /v2/analysis/methods` | ids, roles, parameter JSON Schemas for `relate`, `group_compare`, `hotspots` |

All mock price data is labelled synthetic in its fixture (`synthetic: true`, guard `SYNTHETIC_INPUT`) so the UI path for that warning is exercised from the first day.

## 9. Acceptance for FE-1 and FE-4 (against mocks)

- Load `map.flood-prices.json`; the panel lists layers top-first with groups; reorder by drag, toggle, opacity, blend and rename round-trip to a document that validates against these types and re-loads identically.
- Changing ramp, classes, opacity, blend, order or visibility issues **zero** network requests (assert in a test with a fetch spy).
- Time slider on the series fixture steps without a request and keeps one set of class breaks across periods by default.
- The resolved document's attribution and notices are visible on the map and in the exported PNG.
- The stats card renders the ready, queued and blocked fixtures; guards appear above estimates; the blocked fixture shows no number.
- Brushing the scatter highlights units on the map and the reverse.
- A layer with an unknown `x-` type and a source with `access: 'denied'` each render as a placeholder row without breaking the map.
