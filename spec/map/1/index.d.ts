// @lanka-data-layer/map-spec — TypeScript types of ldl-map/1.
// GENERATED from map.schema.json by `lanka map types`; do not edit.
// The schema is normative; the reference validator (`lanka map validate`)
// also checks limits and references these types cannot express.

export declare const MAP_SPEC: 'ldl-map/1'

/** A map is one JSON document that names data and never holds it (10-map-composition.md §2). Readers MUST reject an unknown major `spec`, MUST ignore unknown `x-*` keys and MUST preserve them on save. Limits beyond this schema (32 layers, 8 groups, group depth 3, 256 KB canonical JSON, references between sources/layers/analyses/panels, layer type ↔ source kind) are checked by the reference validator (`lanka map validate`). Licence: CC BY 4.0. */
export type MapDocument = {
  analyses?: Record<string, AnalysisRequest>
  basemap: {
    hillshade?: {
      exaggeration?: number
      on: boolean
      /** Extensions: readers ignore them and preserve them on save. */
      [x: `x-${string}`]: unknown
    }
    labels?: "top" | "bottom" | "off"
    /** `ldl-auto` follows the viewer's colour scheme; a URL to a MapLibre style is for self-hosts. */
    style: "ldl-auto" | "ldl-light" | "ldl-dark" | "ldl-mono" | "none" | string
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  /** CommonMark subset: paragraphs, emphasis, links. No HTML, no images. */
  description?: string
  extent?: null | {
    geo: GeoCode
  } | {
    bbox: BBox
  }
  /** `m_…` saved map, `ms_…` snapshot; absent in an unsaved document. Not part of the content hash. */
  id?: string
  layers: LayerNode[]
  meta?: {
    author?: string
    created_at?: string
    forked_from?: string | null
    generator?: string
    licence?: string
    updated_at?: string
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  panels?: Panel[]
  pin: "latest" | {
    release: Release
  }
  sources: Record<string, Source>
  spec: "ldl-map/1"
  time?: null | {
    cursor: PeriodCode
    freq: Freq
    playback?: {
      fps: number
      loop?: boolean
      /** Extensions: readers ignore them and preserve them on save. */
      [x: `x-${string}`]: unknown
    }
    range: [PeriodCode, PeriodCode]
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  title: Text
  view: {
    bearing?: number
    bounds?: BBox
    center: [number, number]
    pitch?: number
    zoom: number
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** An `ldl-analysis/1` request (spec/analysis/1, WP-29). The request is stored, never the result; `result` is written when the document is frozen. */
export type AnalysisRequest = {
  method: string
  result?: string
  spec: "ldl-analysis/1"
  [k: string]: unknown
}

export type AnalysisSource = {
  kind: "analysis"
  output: string
  ref: LocalId
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** [west, south, east, north], WGS84. */
export type BBox = [number, number, number, number]

/** A visual channel bound to a field through a scale. */
export type Binding = {
  field: string
  scale: Scale
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type BivariateFill = {
  bivariate: {
    palette: RampRef
    x: Binding
    y: Binding
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** `overlay` is non-separable: a renderer without a backdrop pass draws it as `normal` and says so in the layer panel. */
export type Blend = "normal" | "multiply" | "screen" | "darken" | "lighten" | "add" | "overlay"

export type BoundariesSource = {
  kind: "boundaries"
  level: number
  parent?: GeoCode
  scheme: string
  version?: VersionRef
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type ChartPanel = {
  analysis?: LocalId
  chart: "scatter" | "histogram" | "series" | "groups"
  id: LocalId
  linked?: LocalId
  options?: {
    brush?: boolean
    density?: "points" | "hexbin"
    fit?: ("ols" | "robust")[]
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  source?: LocalId
  type: "chart"
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** `#rrggbb`, `#rrggbbaa`, `transparent`, `none`, or a named tone `ldl.tone.<name>` from ramps.json `tones`, which follows the basemap's colour scheme. */
export type Color = string

export type ColorChannel = Color | Binding

/** CQL2-JSON subset (10 §2.4). Arity per operator is checked by the validator. */
export type Cql2 = {
  args: (Cql2 | {
    property: string
  } | string | number | boolean | (string | number)[])[]
  op: "=" | "<>" | "<" | "<=" | ">" | ">=" | "and" | "or" | "not" | "in" | "between" | "isNull" | "like"
}

/** A public id (`census24.pop_total`, `dcs-census-2024-population`) or a workspace id (`@acme/re.asking_price_median`). */
export type DataRef = string

/** Dimension filter: dimension id → code (codelist-backed, 02 §3). */
export type Dims = Record<string, string>

export type Dsv = string

/** Fetched by the browser only; the server never requests it. */
export type ExternalSource = {
  attribution: string
  kind: "external"
  type: "geojson" | "pmtiles" | "raster-xyz"
  url: string
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type FeaturesSource = {
  dataset: DataRef
  featureKind?: string
  fields?: string[]
  kind: "features"
  /** Binds a date field of the features to the document time cursor. */
  time?: {
    field: string
    match?: "exact" | "asof"
  }
  version?: VersionRef
  within?: GeoCode
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type Freq = "A" | "F" | "H" | "S" | "Q" | "M" | "W" | "D" | "P"

/** A p-code (`LK1103005`) or `scheme:code` (02 §2). */
export type GeoCode = string

export type GridSource = {
  dims?: Dims
  grid: DataRef
  kind: "grid"
  period: PeriodRef
  version?: VersionRef
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** Groups nest to depth 3 and have no blend mode. */
export type LayerGroup = {
  children: LayerNode[]
  collapsed?: boolean
  id: LocalId
  locked?: boolean
  name: string
  opacity: number
  type: "group"
  visible: boolean
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** A document-local identifier (source, layer, group, panel, analysis key). */
export type LocalId = string

export type IndicatorSource = {
  dims?: Dims
  indicator?: DataRef
  indicators?: DataRef[]
  kind: "indicator"
  level: LevelRef
  parent?: GeoCode
  period: PeriodRef
  scheme?: string
  version?: VersionRef
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** Client-only annotations, ≤ 100 KB of canonical JSON. */
export type InlineSource = {
  geojson: {
    features: unknown[]
    type: "FeatureCollection"
    [k: string]: unknown
  }
  kind: "inline"
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type Label = {
  halo?: Color
  priority?: string
  size?: number
  text: string
  zoom?: ZoomRange
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type Layer = {
  blend?: Blend
  clip?: {
    geo: GeoCode
  } | {
    bbox: BBox
  }
  /** CQL2-JSON subset; null = none. */
  filter?: null | Cql2
  id: LocalId
  interaction?: {
    hover?: boolean
    select?: boolean
    tooltip?: string[]
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  legend?: {
    show: boolean
    title?: Text | null
    /** Extensions: readers ignore them and preserve them on save. */
    [x: `x-${string}`]: unknown
  }
  locked?: boolean
  name: string
  /** 0..1; default 1. */
  opacity?: number
  source: LocalId
  style: Style
  type: "choropleth" | "bivariate" | "symbol" | "point" | "cluster" | "heatmap" | "bins" | "line" | "polygon" | "raster" | "hillshade" | "label" | "annotation" | string
  visible: boolean
  zoom?: ZoomRange
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type LegendPanel = {
  id: LocalId
  type: "legend"
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** A scheme-local level, or levels by zoom ([[zoom, level], …], zooms ascending). */
export type LevelRef = number | {
  byZoom: ([number, number])[]
}

export type LayerNode = LayerGroup | Layer

export type NumberChannel = number | Binding

export type Panel = LegendPanel | ChartPanel | StatsPanel | TextPanel

/** A period code of 02 §5: `2024`, `2024-Q2`, `2026-08`, `2024-Maha`, `2026-11-20`, `2012/2024`. */
export type PeriodCode = string

export type PeriodRef = "latest" | PeriodCode | PeriodCode[] | {
  from: PeriodCode
  to: PeriodCode
} | {
  bind: "time"
  match?: "exact" | "asof"
}

/** A ramp name from ramps.json, or an explicit colour array. */
export type RampRef = string | Color[]

/** A release data_version, e.g. `20261122.1`. */
export type Release = string

export type Scale = {
  across?: "all_periods" | "period"
  breaks?: number[]
  /** Absolute [lo, hi] (null = the data's own end) or percentiles ["p", 2, 98]. */
  clamp?: [number | null, number | null] | ["p", number, number]
  classes?: number
  domain?: "extent" | "data" | "national"
  /** categorical: code → colour (colour channels) or number (width/size channels). */
  mapping?: Record<string, Color | number>
  mid?: number | "mean" | "median"
  nodata?: Color
  ramp?: RampRef
  /** Output range in px for numeric channels (size, stroke width). */
  range?: [number, number]
  reverse?: boolean
  transform?: "linear" | "log" | "sqrt" | "symlog"
  type: "quantile" | "equal" | "jenks" | "stddev" | "headtail" | "pretty" | "manual" | "continuous" | "categorical" | "diverging" | "area" | "radius" | "graduated"
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type Source = IndicatorSource | BoundariesSource | FeaturesSource | GridSource | AnalysisSource | InlineSource | ExternalSource

export type StatsPanel = {
  analysis: LocalId
  id: LocalId
  type: "stats"
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type Stroke = {
  color?: ColorChannel
  dash?: number[]
  width?: NumberChannel
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

export type Style = {
  aggregate?: "count" | {
    sum: string
  } | {
    mean: string
  }
  cell?: number
  elevation?: NumberChannel
  exaggeration?: number
  fill?: Color | BivariateFill | Binding
  highlight?: Color
  icon?: string
  intensity?: number
  label?: Label
  radius?: number
  ramp?: RampRef
  /** raster: mask values outside [lo, hi] (null = open). */
  range?: [number | null, number | null]
  resample?: "nearest" | "linear"
  reverse?: boolean
  shadow?: Color
  shape?: "circle" | "square" | "bar"
  size?: NumberChannel
  stroke?: Stroke
  uncertainty?: "hatch" | "fade" | "off"
  weight?: string | null
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** Plain text, or per-language text (en required). */
export type Text = string | {
  en: string
  si?: string
  ta?: string
}

export type TextPanel = {
  id: LocalId
  text: string
  type: "text"
  /** Extensions: readers ignore them and preserve them on save. */
  [x: `x-${string}`]: unknown
}

/** Omitted: inherit the document `pin`. */
export type VersionRef = "latest" | {
  dsv: Dsv
}

export type ZoomRange = [number, number]
