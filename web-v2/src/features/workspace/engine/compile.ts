// `toMapLibre(style)`: one layer of the document + the data it resolved to → MapLibre sources and
// layers, a legend model and a tooltip. Pure: the engine diffs what comes out against what is installed.
import type { Feature, FeatureCollection } from 'geojson'
import type { BoundariesPayload, DemPayload, FeaturesPayload, FieldMeta, FrameColumn, FramePayload, GridPayload, LayerData, ValuesPayload } from '@/api/workspace'
import { WORLDCOVER } from '@/api/real/registry'
import { registerPaint, valueAt } from '@/api/real/valueTiles'
import type { AdminTiles, TileRef } from '@/mocks/layers'
import type { Theme } from '@/lib/hooks'
import { clausesOf, dashKind, matches, resolvePeriod, type BlendMode, type Cql, type FieldBinding, type Fill, type Layer, type Scale, type Source, type TimeConfig } from '@/lib/map-spec'
import { bivariateColors, categorical, hexToRgb, paperColor, rampAt, rampClasses, rampStops, toneColor } from '@/lib/palettes'
import { classBreaks, classIndex, extent, tercileBreaks } from '@/lib/stats'
import { sample, type Frame as GridFrame, type Grid } from '@/mocks/synth'
import { fmtWith } from '../format'
import { gridSample, paintHillshade, paintRaster } from './raster'

export type Env = { scheme: Theme; time: TimeConfig | null; glyphs: boolean; font: string[]; relief: (exaggeration: number) => Grid | undefined; /** the map's zoom now, for reading values out of tiles */ zoom?: () => number }
type Expr = unknown[]
export type MlLayer = {
  id: string
  type: 'fill' | 'line' | 'circle' | 'symbol' | 'heatmap' | 'raster' | 'hillshade'
  source: string
  sourceLayer?: string
  paint: Record<string, unknown>
  layout: Record<string, unknown>
  filter?: Expr
  /** paint properties that carry the layer's opacity, with their value at 100 % */
  opacity: Record<string, number | Expr>
  hit?: boolean
  /** the zoom range of this part (within the document layer's own) */
  minzoom?: number
  maxzoom?: number
}
export type MlSource =
  | { id: string; kind: 'geojson'; data: FeatureCollection; options?: Record<string, unknown> }
  | { id: string; kind: 'canvas'; frame: GridFrame; key: string; paint: (canvas: HTMLCanvasElement) => void }
  | { id: string; kind: 'vector'; url?: string; tiles?: string[]; minzoom?: number; maxzoom?: number; promoteId?: string; tileSize?: number }
  | { id: string; kind: 'raster'; tiles: string[]; minzoom: number; maxzoom: number }
  | { id: string; kind: 'raster-dem'; tiles: string[]; minzoom: number; maxzoom: number; encoding: 'terrarium' | 'mapbox' }
export type LegendItem = { label: string; color: string; shape: 'square' | 'dot' | 'line' | 'ring' | 'dash'; width?: number }
export type LegendModel = (
  | { kind: 'classes'; colors: string[]; breaks: number[]; unit: string; hatch?: boolean }
  | { kind: 'ramp'; stops: string[]; domain: [number, number]; unit: string }
  | { kind: 'categories'; items: LegendItem[] }
  | { kind: 'bivariate'; colors: string[]; x: string; y: string }
  | { kind: 'heat'; stops: string[] }
  | { kind: 'relief' }
) & { note?: string; period?: string }
export type Tip = { title: string; code?: string; swatch?: string; rows: { label: string; value: string }[]; note?: string }
export type Compiled = {
  layers: MlLayer[]
  sources: MlSource[]
  /** fills an area: the coastline mask may sit above it */
  areal: boolean
  /** its own marks overlap, so a reduced opacity must apply to the layer as one image */
  overlap: boolean
  legend: LegendModel | null
  tip?: (properties: Record<string, unknown>, lngLat: [number, number]) => Tip | null
  /** the whole geometry of a hit feature (a rendered feature is clipped to its tile) */
  lookup?: (properties: Record<string, unknown>) => Feature | null
  /** raster value under the cursor */
  probe?: (lngLat: [number, number]) => Tip | null
  problem?: string
  /** per-unit state for a frame drawn on the boundary tiles: feature-state keyed by p-code */
  states?: { source: string; sourceLayer: string; key: string; values: [string, Record<string, unknown>][] }[]
  /** hover and selection outline through feature-state (`hl`, `sel`) on this tiled source */
  tiled?: { source: string; sourceLayer: string }
  /** draw through the compositor even at normal blend (a hillshade has no opacity of its own) */
  isolate?: boolean
}

const EMPTY: Compiled = { layers: [], sources: [], areal: false, overlap: false, legend: null }
export const sid = (layerId: string, part = '') => `ws:${layerId}${part ? `:${part}` : ''}`
export const isBinding = (v: unknown): v is FieldBinding => typeof v === 'object' && v !== null && 'field' in v
export const isBivariate = (v: unknown): v is Extract<Fill, { bivariate: unknown }> => typeof v === 'object' && v !== null && 'bivariate' in v
const NODATA: Record<Theme, string> = { dark: '#1b222b', light: '#e3e6e5' }
const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const mul = (k: number, e: number | Expr): number | Expr => (typeof e === 'number' ? +(k * e).toFixed(3) : ['*', k, e])
/** Scale a size with zoom around the zoom the map is usually read at. */
const zoomed = (e: number | Expr, lo = 0.6, hi = 1.7): Expr => ['interpolate', ['linear'], ['zoom'], 6, mul(lo, e), 9.5, e, 13, mul(hi, e)]
const get = (field: string): Expr => ['to-number', ['get', field], 0]

/* ---------------- scales ---------------- */
export type Resolved =
  | { kind: 'classes'; breaks: number[]; colors: string[] }
  | { kind: 'continuous'; stops: string[]; domain: [number, number]; transform: 'linear' | 'sqrt' | 'log' }
  | { kind: 'categorical'; keys: string[]; colors: string[]; other: string }

export function resolveScale(values: number[], scale: Scale, scheme: Theme, categories?: string[]): Resolved {
  const ramp = scale.ramp ?? 'ldl.seq.blue'
  if (scale.type === 'categorical') {
    const keys = scale.mapping ? Object.keys(scale.mapping) : (categories ?? [])
    const palette = categorical(scheme)
    return { kind: 'categorical', keys, colors: keys.map((k, i) => (scale.mapping ? toneColor(String(scale.mapping[k]), scheme) : palette[Math.min(i, palette.length - 1)])), other: palette[palette.length - 1] }
  }
  if (scale.type === 'continuous') {
    const [lo, hi] = extent(values)
    return { kind: 'continuous', stops: rampStops(ramp, scheme, scale.reverse), domain: [scale.clamp?.[0] ?? lo, scale.clamp?.[1] ?? hi], transform: scale.transform ?? 'linear' }
  }
  const k = Math.max(2, Math.min(9, scale.classes ?? 5))
  const breaks = scale.type === 'manual' || scale.breaks ? [...(scale.breaks ?? [])].sort((a, b) => a - b) : classBreaks(values, scale.type === 'diverging' ? 'quantile' : scale.type, k)
  return { kind: 'classes', breaks, colors: rampClasses(scale.type === 'diverging' ? 'ldl.div.blue-orange' : ramp, scheme, breaks.length + 1, scale.reverse) }
}
const warp = (v: number, t: 'linear' | 'sqrt' | 'log') => (t === 'log' ? Math.log(Math.max(1e-9, v)) : t === 'sqrt' ? Math.sqrt(Math.max(0, v)) : v)
const unwarp = (v: number, t: 'linear' | 'sqrt' | 'log') => (t === 'log' ? Math.exp(v) : t === 'sqrt' ? v * v : v)
export function colorAt(r: Resolved, v: number | string): string {
  if (r.kind === 'categorical') return r.colors[r.keys.indexOf(String(v))] ?? r.other
  if (r.kind === 'classes') return r.colors[classIndex(Number(v), r.breaks)]
  const t = (warp(Number(v), r.transform) - warp(r.domain[0], r.transform)) / (warp(r.domain[1], r.transform) - warp(r.domain[0], r.transform) || 1)
  return r.stops[Math.round(Math.min(1, Math.max(0, t)) * (r.stops.length - 1))]
}
/** A paint expression for a colour bound to a feature property. */
function colorExpr(r: Resolved, field: string): string | Expr {
  if (r.kind === 'categorical') return r.keys.length ? ['match', ['to-string', ['get', field]], ...r.keys.flatMap((k, i) => [k, r.colors[i]]), r.other] : r.other
  if (r.kind === 'classes') return r.breaks.length ? ['step', get(field), r.colors[0], ...r.breaks.flatMap((b, i) => [b, r.colors[i + 1]])] : r.colors[0]
  const a = warp(r.domain[0], r.transform), b = warp(r.domain[1], r.transform)
  const stops: unknown[] = []
  let last = -Infinity
  r.stops.forEach((c, i) => {
    const v = unwarp(a + ((b - a) * i) / (r.stops.length - 1), r.transform)
    if (v > last) stops.push(v, c)
    last = v
  })
  return stops.length >= 4 ? ['interpolate', ['linear'], get(field), ...stops] : r.stops[r.stops.length - 1]
}
/** A size or width: a constant, a category → number mapping, or a continuous scale onto a pixel range. */
function sizeExpr(v: number | FieldBinding | undefined, fallback: number, fields: FieldMeta[], values: (f: string) => number[]): number | Expr {
  if (v === undefined) return fallback
  if (typeof v === 'number') return v
  const s = v.scale
  if (s.type === 'categorical' && s.mapping) return ['match', ['to-string', ['get', v.field]], ...Object.entries(s.mapping).flatMap(([k, n]) => [k, Number(n) || fallback]), fallback]
  const meta = fields.find((f) => f.id === v.field)
  const [lo, hi] = meta?.domain ?? extent(values(v.field))
  const d0 = s.clamp?.[0] ?? lo, d1 = s.clamp?.[1] ?? hi
  const [r0, r1] = s.range ?? [2, 12]
  const t = s.transform ?? 'linear'
  const input: Expr = t === 'sqrt' ? ['sqrt', ['max', 0, get(v.field)]] : t === 'log' ? ['ln', ['max', 0.000001, get(v.field)]] : get(v.field)
  const a = warp(d0, t), b = warp(d1, t)
  return b > a ? ['interpolate', ['linear'], input, a, r0, b, r1] : r0
}

/* ---------------- filters ---------------- */
export function cqlToMl(f: Cql | null | undefined): Expr | undefined {
  if (!f) return undefined
  const prop = (p: { property: string }, sample: unknown): Expr => (typeof sample === 'number' ? ['to-number', ['get', p.property], 0] : ['to-string', ['get', p.property]])
  switch (f.op) {
    case 'and':
      return ['all', ...f.args.map((a) => cqlToMl(a) ?? true)]
    case 'or':
      return ['any', ...f.args.map((a) => cqlToMl(a) ?? true)]
    case 'not':
      return ['!', cqlToMl(f.args[0]) ?? true]
    case 'isNull':
      return ['!', ['has', f.args[0].property]]
    case 'between':
      return ['all', ['>=', prop(f.args[0], 0), f.args[1]], ['<=', prop(f.args[0], 0), f.args[2]]]
    case 'in':
      return ['in', ['to-string', ['get', f.args[0].property]], ['literal', f.args[1].map(String)]]
    default:
      return [f.op === '=' ? '==' : f.op === '<>' ? '!=' : f.op, prop(f.args[0], f.args[1]), f.args[1]]
  }
}
const all = (...fs: (Expr | undefined)[]): Expr | undefined => {
  const list = fs.filter((f): f is Expr => !!f)
  return list.length === 0 ? undefined : list.length === 1 ? list[0] : ['all', ...list]
}

/* ---------------- shared pieces ---------------- */
const textField = (template: string): Expr | string => {
  const parts = template.split(/(\{[^}]+\})/).filter(Boolean)
  const exprs = parts.map((p) => (p.startsWith('{') ? (['to-string', ['get', p.slice(1, -1)]] as Expr) : p))
  return exprs.length === 1 ? (typeof exprs[0] === 'string' ? exprs[0] : exprs[0]) : ['concat', ...exprs]
}
function labelLayer(layer: Layer, source: string, env: Env, sortField?: string, offset = 0, tiled?: { sourceLayer: string; filter?: Expr }): MlLayer[] {
  const l = layer.style.label
  if (!l || !env.glyphs) return []
  // the boundary tiles name units `name_en`; the template's `{name}` means that there
  const text = tiled ? l.text.replace(/\{name\}/g, '{name_en}') : l.text
  const ink = toneColor('ink', env.scheme)
  return [
    {
      id: sid(layer.id, 'label'),
      type: 'symbol',
      source,
      paint: { 'text-color': ink, 'text-halo-color': l.halo && l.halo !== 'none' ? toneColor(l.halo, env.scheme) : paperColor(env.scheme), 'text-halo-width': l.halo === 'none' ? 0 : 1.4, 'text-halo-blur': 0.4 },
      layout: {
        'text-field': textField(text),
        'text-font': env.font,
        'text-size': ['interpolate', ['linear'], ['zoom'], 6, l.size * 0.9, 11, l.size * 1.15],
        'text-anchor': offset ? 'top' : 'center',
        'text-offset': [0, offset],
        'text-padding': 4,
        'text-letter-spacing': 0.01,
        'text-max-width': 7,
        ...(sortField ? { 'symbol-sort-key': ['to-number', ['get', sortField], 0] } : {}),
      },
      ...(tiled ? { sourceLayer: tiled.sourceLayer, filter: tiled.filter } : {}),
      opacity: { 'text-opacity': 1 },
    },
  ]
}
const strokeColor = (layer: Layer, env: Env, fallback: 'ink' | 'paper' = 'ink') => {
  const c = layer.style.stroke?.color
  return typeof c === 'string' ? toneColor(c, env.scheme) : toneColor(fallback, env.scheme)
}
const titleCase = (s: string) => s.replace(/^./, (c) => c.toUpperCase())

/* ---------------- frames: choropleth and bivariate ---------------- */
/** The columns of one variable of a frame, all periods. `value` means the source's own indicator. */
export function columnsOf(data: FramePayload, field: string, source?: Source): FrameColumn[] {
  const want = field === 'value' ? (source?.kind === 'indicator' ? source.indicator : data.columns[0]?.indicator) : field
  const hit = data.columns.filter((c) => c.id === field || c.indicator === want)
  return hit.length ? hit : data.columns.filter((c) => c.indicator === data.columns[0]?.indicator)
}
export function columnAt(data: FramePayload, field: string, source: Source | undefined, time: TimeConfig | null) {
  const cols = columnsOf(data, field, source)
  const period = resolvePeriod(source?.kind === 'indicator' ? source.period : 'latest', time, cols.map((c) => c.period))
  return { cols, col: cols.find((c) => c.period === period) ?? cols[cols.length - 1] }
}
export const DEFAULT_FILL: FieldBinding = { field: 'value', scale: { type: 'quantile', classes: 5, ramp: 'ldl.seq.blue' } }

/** Only the units of the frame's parent, on a tileset that holds the whole country. */
const parentFilter = (t: AdminTiles): Expr | undefined => (t.parent ? ['==', ['index-of', t.parent, ['to-string', ['get', 'pcode']]], 0] : undefined)
/** Hover and selection outlines, switched per unit by feature-state: no geometry has to be looked up. */
function outlineLayers(layer: Layer, src: string, sourceLayer: string, env: Env, filter?: Expr): MlLayer[] {
  const on = (k: string) => ['boolean', ['feature-state', k], false]
  const sel = env.scheme === 'dark' ? '#d8446b' : '#c22f5a'
  return [
    { id: sid(layer.id, 'hl-casing'), type: 'line', source: src, sourceLayer, filter, paint: { 'line-color': paperColor(env.scheme), 'line-width': ['case', on('sel'), 4.5, on('hl'), 4, 0], 'line-opacity': ['case', on('sel'), 0.85, 0.7] }, layout: { 'line-join': 'round' }, opacity: {} },
    { id: sid(layer.id, 'hl-line'), type: 'line', source: src, sourceLayer, filter, paint: { 'line-color': ['case', on('sel'), sel, toneColor('ink', env.scheme)], 'line-width': ['case', on('sel'), 2, on('hl'), 1.6, 0] }, layout: { 'line-join': 'round' }, opacity: {} },
  ]
}
/** A frame on the engine's boundary tiles: colours, dimming and the estimate flag travel as feature-state. */
function frameTiled(layer: Layer, data: FramePayload, t: AdminTiles, env: Env, colors: (string | null)[], dimmed: string[], estimated: string[]): Pick<Compiled, 'layers' | 'sources' | 'states' | 'tiled'> {
  const src = sid(layer.id)
  const base = parentFilter(t)
  const stroke = layer.style.stroke
  const w = typeof stroke?.width === 'number' ? stroke.width : 0.5
  const dim = new Set(dimmed)
  const values: [string, Record<string, unknown>][] = data.codes.map((c, i) => [c, { c: colors[i] ?? null, d: dim.has(c) }])
  const layers: MlLayer[] = [
    {
      id: sid(layer.id, 'fill'),
      type: 'fill',
      source: src,
      sourceLayer: t.sourceLayer,
      filter: base,
      paint: { 'fill-color': ['to-color', ['coalesce', ['feature-state', 'c'], NODATA[env.scheme]]], 'fill-antialias': true },
      layout: {},
      opacity: { 'fill-opacity': dimmed.length ? ['case', ['boolean', ['feature-state', 'd'], false], 0.1, 1] : 1 },
      hit: true,
    },
  ]
  if (layer.style.uncertainty === 'hatch' && estimated.length)
    layers.push({ id: sid(layer.id, 'hatch'), type: 'fill', source: src, sourceLayer: t.sourceLayer, paint: { 'fill-pattern': 'ws-hatch' }, layout: {}, filter: all(base, ['in', ['get', 'pcode'], ['literal', estimated]]), opacity: { 'fill-opacity': 0.6 } })
  if (w > 0)
    layers.push({
      id: sid(layer.id, 'line'),
      type: 'line',
      source: src,
      sourceLayer: t.sourceLayer,
      filter: base,
      paint: { 'line-color': strokeColor(layer, env, 'paper'), 'line-width': zoomed(w, 0.5, 2.2) },
      layout: { 'line-join': 'round' },
      opacity: { 'line-opacity': w < 0.45 ? (env.scheme === 'dark' ? 0.5 : 0.7) : env.scheme === 'dark' ? 0.8 : 0.6 },
    })
  // the source's zoom range is the tileset's for this level: no tile is asked for below the level's minzoom or above the maxzoom
  const sources: MlSource[] = [{ id: src, kind: 'vector', tiles: t.tiles, minzoom: t.minzoom, maxzoom: t.maxzoom, promoteId: 'pcode' }]
  const states = [{ source: src, sourceLayer: t.sourceLayer }]
  layers.push(...outlineLayers(layer, src, t.sourceLayer, env, base))
  if (layer.style.label && env.glyphs) layers.push(...labelLayer(layer, src, env, undefined, 0, { sourceLayer: t.centroidsLayer, filter: all(['==', ['get', 'level'], t.level], base) }))
  // the key only has to change when a state does
  let key = ''
  for (let i = 0; i < colors.length; i++) key += `${colors[i] ?? '-'}${dim.has(data.codes[i]) ? '*' : ''},`
  return { layers, sources, states: states.map((st) => ({ ...st, key, values })), tiled: { source: src, sourceLayer: t.sourceLayer } }
}

function frameBase(layer: Layer, data: FramePayload, env: Env, colors: (string | null)[], dimmed: string[], estimated: string[]): Pick<Compiled, 'layers' | 'sources' | 'states' | 'tiled'> {
  if (data.tiles) return frameTiled(layer, data, data.tiles, env, colors, dimmed, estimated)
  const src = sid(layer.id)
  const pairs: unknown[] = []
  data.codes.forEach((c, i) => colors[i] && pairs.push(c, colors[i]))
  const stroke = layer.style.stroke
  const w = typeof stroke?.width === 'number' ? stroke.width : 0.5
  const layers: MlLayer[] = [
    {
      id: sid(layer.id, 'fill'),
      type: 'fill',
      source: src,
      paint: { 'fill-color': pairs.length ? ['match', ['get', 'pcode'], ...pairs, NODATA[env.scheme]] : NODATA[env.scheme], 'fill-antialias': true },
      layout: {},
      opacity: { 'fill-opacity': dimmed.length ? ['match', ['get', 'pcode'], dimmed, 0.1, 1] : 1 },
      hit: true,
    },
  ]
  if (layer.style.uncertainty === 'hatch' && estimated.length)
    layers.push({ id: sid(layer.id, 'hatch'), type: 'fill', source: src, paint: { 'fill-pattern': 'ws-hatch' }, layout: {}, filter: ['in', ['get', 'pcode'], ['literal', estimated]], opacity: { 'fill-opacity': 0.6 } })
  if (w > 0)
    layers.push({
      id: sid(layer.id, 'line'),
      type: 'line',
      source: src,
      paint: { 'line-color': strokeColor(layer, env, 'paper'), 'line-width': zoomed(w, 0.5, 2.2) },
      layout: { 'line-join': 'round' },
      opacity: { 'line-opacity': w < 0.45 ? (env.scheme === 'dark' ? 0.5 : 0.7) : env.scheme === 'dark' ? 0.8 : 0.6 },
    })
  const sources: MlSource[] = [{ id: src, kind: 'geojson', data: data.geometry, options: { promoteId: 'pcode' } }]
  if (layer.style.label && env.glyphs) {
    sources.push({ id: sid(layer.id, 'pt'), kind: 'geojson', data: { type: 'FeatureCollection', features: data.codes.map((c, i) => ({ type: 'Feature', properties: { pcode: c, name: data.names[i] }, geometry: { type: 'Point', coordinates: data.centroids[i] } })) } })
    layers.push(...labelLayer(layer, sid(layer.id, 'pt'), env))
  }
  return { layers, sources }
}
const frameLookup = (data: FramePayload) => {
  let index: Map<string, Feature> | null = null
  return (p: Record<string, unknown>) => {
    index ??= new Map(data.geometry.features.map((f) => [String(f.properties?.pcode), f]))
    return index.get(String(p.pcode)) ?? null
  }
}

function choropleth(layer: Layer, source: Source | undefined, data: FramePayload, env: Env): Compiled {
  const binding = isBinding(layer.style.fill) ? layer.style.fill : DEFAULT_FILL
  const { cols, col } = columnAt(data, binding.field, source, env.time)
  if (!col) return { ...EMPTY, problem: 'This source has no values at this level.' }
  const pool = (binding.scale.across ?? 'all_periods') === 'all_periods' ? cols.flatMap((c) => c.values) : col.values
  const scale = resolveScale(pool.filter(num), binding.scale, env.scheme)
  const at = new Map(data.codes.map((c, i) => [c, i]))
  const dimmed: string[] = [], estimated: string[] = []
  const colors = data.codes.map((code, i) => {
    const v = col.values[i]
    if (!num(v)) return null
    if (!matches(layer.filter, { value: v, name: data.names[i], pcode: code })) dimmed.push(code)
    if (col.basis?.[i] === 'estimated' && col.class !== 'estimated') estimated.push(code)
    return colorAt(scale, v)
  })
  return {
    ...frameBase(layer, data, env, colors, dimmed, estimated),
    areal: true,
    overlap: false,
    legend: scale.kind === 'classes' ? { kind: 'classes', colors: scale.colors, breaks: scale.breaks, unit: col.unit, hatch: estimated.length > 0 && layer.style.uncertainty === 'hatch', period: cols.length > 1 ? col.period : undefined } : scale.kind === 'continuous' ? { kind: 'ramp', stops: scale.stops, domain: scale.domain, unit: col.unit, period: cols.length > 1 ? col.period : undefined } : null,
    lookup: data.tiles ? undefined : frameLookup(data),
    tip: (p) => {
      const i = at.get(String(p.pcode))
      if (i === undefined) return null
      const v = col.values[i]
      const rows = [{ label: cols.length > 1 ? `${col.label} · ${col.period}` : col.label, value: fmtWith(v, col.unit) }]
      if (col.n) rows.push({ label: 'Listings', value: String(col.n[i]) })
      return { title: String(p.name_en ?? data.names[i] ?? data.codes[i]), code: data.codes[i], swatch: colors[i] ?? NODATA[env.scheme], rows, note: col.basis?.[i] === 'estimated' && col.n ? 'Modelled: fewer than three listings' : undefined }
    },
  }
}

export function bivariateBreaks(layer: Layer, data: FramePayload, source: Source | undefined, time: TimeConfig | null) {
  const f = isBivariate(layer.style.fill) ? layer.style.fill.bivariate : null
  if (!f) return null
  const x = columnAt(data, f.x.field, source, time).col, y = columnAt(data, f.y.field, source, time).col
  if (!x || !y) return null
  // explicit breaks first, then the engine's own (an analysis output), then thirds of the data
  const bx = f.x.scale.breaks?.length === 2 ? f.x.scale.breaks : x.breaks?.length === 2 ? x.breaks : tercileBreaks(x.values.filter(num))
  const by = f.y.scale.breaks?.length === 2 ? f.y.scale.breaks : y.breaks?.length === 2 ? y.breaks : tercileBreaks(y.values.filter(num))
  return { x, y, bx, by, palette: f.palette }
}
function bivariate(layer: Layer, source: Source | undefined, data: FramePayload, env: Env): Compiled {
  const b = bivariateBreaks(layer, data, source, env.time)
  if (!b) return { ...EMPTY, problem: 'A bivariate layer needs two variables on one support.' }
  const palette = bivariateColors(b.palette, env.scheme)
  const at = new Map(data.codes.map((c, i) => [c, i]))
  const estimated: string[] = [], dimmed: string[] = []
  const colors = data.codes.map((code, i) => {
    const vx = b.x.values[i], vy = b.y.values[i]
    if (!num(vx) || !num(vy)) return null
    if (b.y.basis?.[i] === 'estimated') estimated.push(code)
    if (!matches(layer.filter, { x: vx, y: vy, name: data.names[i], pcode: code })) dimmed.push(code)
    return palette[classIndex(vy, b.by) * 3 + classIndex(vx, b.bx)]
  })
  const short = (c: FrameColumn) => c.label.replace(/^Median /, '').replace(/^Population in the flood zone$/, 'Flood exposure')
  return {
    ...frameBase(layer, data, env, colors, dimmed, estimated),
    areal: true,
    overlap: false,
    legend: { kind: 'bivariate', colors: palette, x: titleCase(short(b.x)), y: titleCase(short(b.y)) },
    lookup: data.tiles ? undefined : frameLookup(data),
    tip: (p) => {
      const i = at.get(String(p.pcode))
      if (i === undefined) return null
      const rows = [
        { label: short(b.x), value: fmtWith(b.x.values[i], b.x.unit) },
        { label: short(b.y), value: fmtWith(b.y.values[i], b.y.unit) },
      ]
      if (b.y.n) rows.push({ label: 'Listings', value: String(b.y.n[i]) })
      return { title: String(p.name_en ?? data.names[i] ?? data.codes[i]), code: data.codes[i], swatch: colors[i] ?? NODATA[env.scheme], rows, note: b.y.basis?.[i] === 'estimated' && b.y.n ? 'Price modelled: fewer than three listings' : undefined }
    },
  }
}

/* ---------------- features: polygons, lines, points, clusters, heat ---------------- */
type Vector = { collection: FeatureCollection; fields: FieldMeta[]; title?: string; tiles?: FeaturesPayload['tiles'] }
const adminVector = (t: AdminTiles, collection: FeatureCollection): Vector => ({ collection, fields: [], title: 'name_en', tiles: { tiles: t.tiles, minzoom: t.minzoom, maxzoom: t.maxzoom, sourceLayer: t.sourceLayer, filter: parentFilter(t) } })
const vectorOf = (data: LayerData): Vector | null =>
  data.kind === 'features' ? data : data.kind === 'boundaries' ? (data.tiles ? adminVector(data.tiles, data.collection) : { collection: data.collection, fields: [], title: 'name' }) : data.kind === 'frame' ? (data.tiles ? adminVector(data.tiles, data.geometry) : { collection: data.geometry, fields: [], title: 'name' }) : null
const valuesOf = (v: Vector) => (field: string) => v.collection.features.map((f) => Number(f.properties?.[field])).filter(Number.isFinite)
const categoriesOf = (v: Vector, field: string) => v.fields.find((f) => f.id === field)?.categories ?? [...new Set(v.collection.features.map((f) => String(f.properties?.[field])))].sort()
function boundColor(v: Vector, c: string | FieldBinding | undefined, env: Env, fallback: 'ink' | 'paper' | 'grey' = 'ink'): { paint: string | Expr; scale: Resolved | null; field?: string } {
  if (!isBinding(c)) return { paint: toneColor(typeof c === 'string' && c !== 'none' ? c : fallback, env.scheme), scale: null }
  const scale = resolveScale(valuesOf(v)(c.field), c.scale, env.scheme, categoriesOf(v, c.field))
  return { paint: colorExpr(scale, c.field), scale, field: c.field }
}
function boundLegend(v: Vector, b: ReturnType<typeof boundColor>, shape: LegendItem['shape'], constantLabel: string, width?: number): LegendModel | null {
  const meta = v.fields.find((f) => f.id === b.field)
  if (!b.scale) return { kind: 'categories', items: [{ label: constantLabel, color: b.paint as string, shape, width }] }
  if (b.scale.kind === 'categorical') return { kind: 'categories', items: b.scale.keys.map((k, i) => ({ label: k.replace(/_/g, ' '), color: (b.scale as Extract<Resolved, { kind: 'categorical' }>).colors[i], shape, width })) }
  if (b.scale.kind === 'classes') return { kind: 'classes', colors: b.scale.colors, breaks: b.scale.breaks, unit: meta?.unit ?? '', note: meta?.label }
  return { kind: 'ramp', stops: b.scale.stops, domain: b.scale.domain, unit: meta?.unit ?? '', note: meta?.label }
}
const featureTip = (v: Vector, layerName: string, env: Env, colorOf?: (p: Record<string, unknown>) => string | undefined) => (p: Record<string, unknown>): Tip | null => {
  const title = v.title && p[v.title] != null ? String(p[v.title]) : layerName
  const rows = v.fields
    .filter((f) => f.id !== v.title && p[f.id] != null)
    .slice(0, 4)
    .map((f) => ({ label: f.label, value: f.kind === 'number' ? fmtWith(Number(p[f.id]), f.unit) : String(p[f.id]) }))
  void env
  return { title: titleCase(title), code: typeof p.pcode === 'string' ? p.pcode : undefined, rows, swatch: colorOf?.(p) }
}
/** A time-bound feature source shows what existed by the cursor. */
const timeFilter = (source: Source | undefined, env: Env): Expr | undefined => {
  const t = source?.kind === 'features' ? source.time : undefined
  if (!t || !env.time) return undefined
  return [t.match === 'exact' ? '==' : '<=', get(t.field), Number(env.time.cursor)]
}
const isTileRef = (t: FeaturesPayload['tiles']): t is TileRef => !!t && 'tiles' in t
const vectorSource = (layer: Layer, v: Vector, options?: Record<string, unknown>): { id: string; source: MlSource; sourceLayer?: string; base?: Expr } =>
  isTileRef(v.tiles)
    ? // the engine's tiles: one source per layer, with the zoom range of the archive so nothing above it is requested
      { id: sid(layer.id), source: { id: sid(layer.id), kind: 'vector', tiles: v.tiles.tiles, minzoom: v.tiles.minzoom, maxzoom: v.tiles.maxzoom, ...(v.title === 'name_en' ? { promoteId: 'pcode' } : {}) }, sourceLayer: v.tiles.sourceLayer, base: v.tiles.filter as Expr | undefined }
    : v.tiles
      ? { id: 'ws-tiles', source: { id: 'ws-tiles', kind: 'vector', url: v.tiles.url }, sourceLayer: v.tiles.sourceLayer, base: v.tiles.filter as Expr }
      : { id: sid(layer.id), source: { id: sid(layer.id), kind: 'geojson', data: v.collection, options } }

function polygon(layer: Layer, source: Source | undefined, v: Vector, env: Env): Compiled {
  const s = vectorSource(layer, v)
  const filter = all(s.base, cqlToMl(layer.filter), timeFilter(source, env))
  const fill = layer.style.fill
  const none = fill === 'none' || fill === undefined
  const f = boundColor(v, none ? undefined : (fill as string | FieldBinding), env, 'grey')
  const stroke = layer.style.stroke
  const sc = boundColor(v, stroke?.color, env, 'ink')
  const w = sizeExpr(stroke?.width, stroke ? 1 : 0, v.fields, valuesOf(v))
  const layers: MlLayer[] = []
  if (!none) layers.push({ id: sid(layer.id, 'fill'), type: 'fill', source: s.id, sourceLayer: s.sourceLayer, paint: { 'fill-color': f.paint, 'fill-antialias': true }, layout: {}, filter, opacity: { 'fill-opacity': 1 }, hit: true })
  if (stroke && w !== 0)
    layers.push({
      id: sid(layer.id, 'line'),
      type: 'line',
      source: s.id,
      sourceLayer: s.sourceLayer,
      paint: { 'line-color': sc.paint, 'line-width': zoomed(w, 0.65, 1.6), ...(stroke.dash?.length ? { 'line-dasharray': stroke.dash } : {}) },
      layout: { 'line-join': 'round' },
      filter,
      opacity: { 'line-opacity': 1 },
      hit: none,
    })
  const byId = () => {
    let index: Map<string, Feature> | null = null
    return (p: Record<string, unknown>) => {
      index ??= new Map(v.collection.features.map((ft) => [String(ft.properties?.pcode ?? ft.properties?.id), ft]))
      return index.get(String(p.pcode ?? p.id)) ?? null
    }
  }
  const admin = isTileRef(v.tiles) && v.title === 'name_en'
  if (admin) layers.push(...outlineLayers(layer, s.id, s.sourceLayer!, env, s.base))
  return {
    layers,
    sources: [s.source],
    areal: !none,
    overlap: false,
    legend: none ? boundLegend(v, sc, 'ring', layer.name) : boundLegend(v, f, 'square', layer.name),
    lookup: isTileRef(v.tiles) ? undefined : byId(),
    tip: featureTip(v, layer.name, env),
    ...(admin ? { tiled: { source: s.id, sourceLayer: s.sourceLayer! } } : {}),
  }
}

function line(layer: Layer, source: Source | undefined, v: Vector, env: Env): Compiled {
  const s = vectorSource(layer, v)
  const filter = all(s.base, cqlToMl(layer.filter), timeFilter(source, env))
  const stroke = layer.style.stroke ?? {}
  const c = boundColor(v, stroke.color, env, 'ink')
  const w = sizeExpr(stroke.width, 1, v.fields, valuesOf(v))
  const dash = stroke.dash?.length ? stroke.dash : undefined
  const layers: MlLayer[] = []
  if (stroke['x-casing'])
    layers.push({ id: sid(layer.id, 'casing'), type: 'line', source: s.id, sourceLayer: s.sourceLayer, paint: { 'line-color': paperColor(env.scheme), 'line-width': zoomed(mul(2.5, w), 0.7, 1.5) }, layout: { 'line-join': 'round', 'line-cap': 'round' }, filter, opacity: { 'line-opacity': 0.6 } })
  layers.push({
    id: sid(layer.id, 'line'),
    type: 'line',
    source: s.id,
    sourceLayer: s.sourceLayer,
    paint: { 'line-color': c.paint, 'line-width': zoomed(w, 0.62, 1.6), ...(dash ? { 'line-dasharray': dash } : {}) },
    layout: { 'line-join': 'round', 'line-cap': dashKind(stroke.dash) === 'dot' ? 'round' : 'butt' },
    filter,
    opacity: { 'line-opacity': 1 },
    hit: v.fields.length > 0,
  })
  const widthMeta = isBinding(stroke.width) ? v.fields.find((f) => f.id === (stroke.width as FieldBinding).field) : undefined
  const legend = boundLegend(v, c, dashKind(stroke.dash) === 'solid' ? 'line' : 'dash', layer.name)
  return { layers, sources: [s.source], areal: false, overlap: false, legend: legend && widthMeta ? { ...legend, note: `Width: ${widthMeta.label.toLowerCase()}` } : legend, tip: v.fields.length ? featureTip(v, layer.name, env) : undefined }
}

function point(layer: Layer, source: Source | undefined, v: Vector, env: Env): Compiled {
  const src = sid(layer.id)
  const filter = all(cqlToMl(layer.filter), timeFilter(source, env))
  const fill = boundColor(v, layer.style.fill as string | FieldBinding | undefined, env, 'ink')
  const r = sizeExpr(layer.style.size, 4, v.fields, valuesOf(v))
  const stroke = layer.style.stroke
  const sizeField = isBinding(layer.style.size) ? layer.style.size.field : undefined
  const sizeMeta = v.fields.find((f) => f.id === sizeField)
  const layers: MlLayer[] = [
    {
      id: sid(layer.id, 'circle'),
      type: 'circle',
      source: src,
      paint: {
        'circle-color': fill.paint,
        'circle-radius': zoomed(r, 0.62, 1.55),
        'circle-stroke-color': typeof stroke?.color === 'string' ? toneColor(stroke.color, env.scheme) : paperColor(env.scheme),
        'circle-stroke-width': typeof stroke?.width === 'number' ? stroke.width : 0,
      },
      // large marks at the back, so a small one is never buried under a big one
      layout: sizeField ? { 'circle-sort-key': ['-', 0, get(sizeField)] } : {},
      filter,
      opacity: { 'circle-opacity': 1, 'circle-stroke-opacity': 1 },
      hit: true,
    },
    ...labelLayer(layer, src, env, 'rank', 0.5).map((l) => ({ ...l, filter })),
  ]
  const legend = boundLegend(v, fill, 'dot', layer.name)
  const scale = fill.scale
  return {
    layers,
    sources: [{ id: src, kind: 'geojson', data: v.collection }],
    areal: false,
    overlap: true,
    legend: legend && sizeMeta ? { ...legend, note: [legend.note, `Size: ${sizeMeta.label.toLowerCase()}`].filter(Boolean).join(' · ') } : legend,
    tip: featureTip(v, layer.name, env, scale && fill.field ? (p) => colorAt(scale, p[fill.field!] as number | string) : () => fill.paint as string),
  }
}

function cluster(layer: Layer, v: Vector, env: Env): Compiled {
  const src = sid(layer.id)
  const color = toneColor(typeof layer.style.fill === 'string' ? layer.style.fill : 'ldl.tone.teal', env.scheme)
  const paper = paperColor(env.scheme)
  const layers: MlLayer[] = [
    {
      id: sid(layer.id, 'clusters'),
      type: 'circle',
      source: src,
      filter: ['has', 'point_count'],
      paint: { 'circle-color': color, 'circle-radius': ['step', ['get', 'point_count'], 12, 5, 15, 12, 19, 30, 24], 'circle-stroke-color': paper, 'circle-stroke-width': 1.5 },
      layout: {},
      opacity: { 'circle-opacity': 0.92, 'circle-stroke-opacity': 1 },
    },
    { id: sid(layer.id, 'circle'), type: 'circle', source: src, filter: ['!', ['has', 'point_count']], paint: { 'circle-color': color, 'circle-radius': 4, 'circle-stroke-color': paper, 'circle-stroke-width': 1.2 }, layout: {}, opacity: { 'circle-opacity': 1, 'circle-stroke-opacity': 1 }, hit: true },
  ]
  if (env.glyphs)
    layers.push({
      id: sid(layer.id, 'count'),
      type: 'symbol',
      source: src,
      filter: ['has', 'point_count'],
      paint: { 'text-color': env.scheme === 'dark' ? '#0b0e12' : '#ffffff' },
      layout: { 'text-field': ['to-string', ['get', 'point_count']], 'text-font': env.font, 'text-size': 11.5, 'text-allow-overlap': true, 'text-ignore-placement': true },
      opacity: { 'text-opacity': 1 },
    })
  return {
    layers,
    sources: [{ id: src, kind: 'geojson', data: v.collection, options: { cluster: true, clusterRadius: layer.style.radius ?? 44, clusterMaxZoom: 11 } }],
    areal: false,
    overlap: true,
    legend: { kind: 'categories', items: [{ label: 'Count in the cluster', color, shape: 'dot' }] },
    tip: featureTip(v, layer.name, env, () => color),
  }
}

function heatmap(layer: Layer, source: Source | undefined, v: Vector, env: Env): Compiled {
  const stops = rampStops(layer.style.ramp ?? 'ldl.seq.orange', env.scheme, layer.style.reverse)
  const radius = layer.style.radius ?? 22
  const intensity = layer.style.intensity ?? 1
  const w = layer.style.weight
  const meta = v.fields.find((f) => f.id === w)
  const [lo, hi] = meta?.domain ?? (w ? extent(valuesOf(v)(w)) : [0, 1])
  const density: unknown[] = [0, 'rgba(0,0,0,0)']
  stops.forEach((c, i) => density.push(0.08 + (0.92 * i) / (stops.length - 1), c))
  return {
    layers: [
      {
        id: sid(layer.id, 'heat'),
        type: 'heatmap',
        source: sid(layer.id),
        paint: {
          'heatmap-weight': w ? ['interpolate', ['linear'], get(w), lo, 0.05, hi, 1] : 1,
          'heatmap-radius': ['interpolate', ['exponential', 1.6], ['zoom'], 6, radius * 0.42, 9.5, radius, 13, radius * 2.6],
          'heatmap-intensity': ['interpolate', ['linear'], ['zoom'], 6, intensity * 0.28, 9.5, intensity * 0.55, 13, intensity * 1.6],
          'heatmap-color': ['interpolate', ['linear'], ['heatmap-density'], ...density],
        },
        layout: {},
        filter: all(cqlToMl(layer.filter), timeFilter(source, env)),
        opacity: { 'heatmap-opacity': 1 },
      },
    ],
    sources: [{ id: sid(layer.id), kind: 'geojson', data: v.collection }],
    areal: false,
    overlap: false,
    legend: { kind: 'heat', stops, note: meta ? `Weighted by ${meta.label.toLowerCase()}` : 'Density of points' },
  }
}

function label(layer: Layer, data: LayerData, env: Env): Compiled {
  const admin = data.kind === 'boundaries' || data.kind === 'frame' ? data.tiles : undefined
  if (admin) {
    const style = layer.style.label ?? { text: '{name}', size: 11 }
    const src = sid(layer.id)
    const layers = labelLayer({ ...layer, style: { ...layer.style, label: style } }, src, env, undefined, 0, { sourceLayer: admin.centroidsLayer, filter: all(['==', ['get', 'level'], admin.level], parentFilter(admin), cqlToMl(layer.filter)) })
    return { layers, sources: [{ id: src, kind: 'vector', tiles: admin.tiles, minzoom: admin.minzoom, maxzoom: admin.maxzoom }], areal: false, overlap: false, legend: null, problem: env.glyphs ? undefined : 'Labels need the basemap fonts, which are offline.' }
  }
  const pts: FeatureCollection | null = data.kind === 'boundaries' ? data.centroids : data.kind === 'features' && data.geometry === 'point' ? data.collection : data.kind === 'frame' ? { type: 'FeatureCollection', features: data.codes.map((c, i) => ({ type: 'Feature', properties: { pcode: c, name: data.names[i] }, geometry: { type: 'Point', coordinates: data.centroids[i] } })) } : null
  if (!pts) return { ...EMPTY, problem: 'Labels need points or areas.' }
  const style = layer.style.label ?? { text: '{name}', size: 11 }
  const layers = labelLayer({ ...layer, style: { ...layer.style, label: style } }, sid(layer.id), env, 'rank').map((l) => ({ ...l, filter: cqlToMl(layer.filter) }))
  return { layers, sources: [{ id: sid(layer.id), kind: 'geojson', data: pts }], areal: false, overlap: false, legend: null, problem: env.glyphs ? undefined : 'Labels need the basemap fonts, which are offline.' }
}

/* ---------------- grids ---------------- */
export function rasterScale(layer: Layer, data: GridPayload, scheme: Theme): { scale: Resolved; values: number[] } {
  const binding = isBinding(layer.style.fill) ? layer.style.fill : { field: 'value', scale: { type: 'continuous' as const, ramp: 'ldl.seq.blue' } }
  const values = gridSample(data.grid, layer.style.range)
  return { scale: resolveScale(values, binding.scale, scheme), values }
}
let gridSerial = 0
const gridIds = new WeakMap<Grid, number>()
const gridKey = (g: Grid | null | undefined) => {
  if (!g) return 0
  let id = gridIds.get(g)
  if (!id) gridIds.set(g, (id = ++gridSerial))
  return id
}
function raster(layer: Layer, data: GridPayload, env: Env): Compiled {
  const { scale } = rasterScale(layer, data, env.scheme)
  if (scale.kind === 'categorical') return { ...EMPTY, problem: 'A raster needs a numeric scale.' }
  const relief = layer.style['x-relief'] ? (env.relief(1) ?? null) : null
  const smooth = layer.style.resample !== 'nearest'
  const range = layer.style.range ?? [null, null]
  const paint = { colors: scale.kind === 'classes' ? scale.colors : scale.stops, breaks: scale.kind === 'classes' ? scale.breaks : null, domain: scale.kind === 'continuous' ? scale.domain : data.domain, transform: scale.kind === 'continuous' ? scale.transform : ('linear' as const), range, relief, smooth }
  const key = JSON.stringify([gridKey(data.grid), paint.colors, paint.breaks, paint.domain, paint.transform, range, gridKey(relief), smooth])
  return {
    layers: [{ id: sid(layer.id, 'raster'), type: 'raster', source: sid(layer.id), paint: { 'raster-fade-duration': 0, 'raster-resampling': smooth ? 'linear' : 'nearest' }, layout: {}, opacity: { 'raster-opacity': 1 } }],
    sources: [{ id: sid(layer.id), kind: 'canvas', frame: data.grid, key, paint: (c) => paintRaster(c, data.grid, paint) }],
    areal: true,
    overlap: false,
    legend: scale.kind === 'classes' ? { kind: 'classes', colors: scale.colors, breaks: scale.breaks, unit: data.unit } : { kind: 'ramp', stops: scale.stops, domain: scale.domain, unit: data.unit },
    probe: ([lon, lat]) => {
      const v = sample(data.grid, lon, lat)
      if (!Number.isFinite(v) || v < (range[0] ?? -Infinity) || v > (range[1] ?? Infinity)) return null
      return { title: layer.name, swatch: colorAt(scale, v), rows: [{ label: data.label, value: fmtWith(v, data.unit) }] }
    },
  }
}
function hillshade(layer: Layer, data: GridPayload, env: Env): Compiled {
  const z = layer.style.exaggeration ?? 1
  const relief = env.relief(z)
  if (!relief) return EMPTY
  return {
    layers: [{ id: sid(layer.id, 'raster'), type: 'raster', source: sid(layer.id), paint: { 'raster-fade-duration': 0, 'raster-resampling': 'linear' }, layout: {}, opacity: { 'raster-opacity': 1 } }],
    sources: [{ id: sid(layer.id), kind: 'canvas', frame: data.grid, key: `hill:${gridKey(relief)}:${env.scheme}`, paint: (c) => paintHillshade(c, relief, env.scheme === 'dark') }],
    areal: true,
    overlap: false,
    legend: { kind: 'relief' },
  }
}

/* ---------------- grids from the engine: value tiles and the terrain tileset ---------------- */
const rgbOf = (hex: string): [number, number, number] => hexToRgb(hex).map((c) => Math.round(c)) as [number, number, number]
function valuesScale(layer: Layer, data: ValuesPayload, scheme: Theme): Resolved {
  const binding = isBinding(layer.style.fill) ? layer.style.fill : { field: 'value', scale: { type: 'continuous' as const, ramp: gridRamp(data) } }
  const range = layer.style.range ?? [null, null]
  const lo = range[0] ?? -Infinity, hi = range[1] ?? Infinity
  const values: number[] = []
  for (const v of data.sample) if (v >= lo && v <= hi) values.push(v)
  return resolveScale(values, binding.scale, scheme, data.values.categories ? Object.keys(data.values.categories) : undefined)
}
const gridRamp = (data: ValuesPayload) => (data.unit === 'm' ? 'ldl.seq.blue' : 'ldl.seq.violet')
/** Value tiles coloured through the protocol: the style is a lookup table, so restyling refetches nothing. */
function valuesRaster(layer: Layer, data: ValuesPayload, env: Env): Compiled {
  const scale = valuesScale(layer, data, env.scheme)
  const range = layer.style.range ?? [null, null]
  const lo = range[0] ?? -Infinity, hi = range[1] ?? Infinity
  const stops = scale.kind === 'continuous' ? scale.stops : []
  const warpT = (v: number) => (scale.kind === 'continuous' ? (scale.transform === 'log' ? Math.log(Math.max(1e-9, v)) : scale.transform === 'sqrt' ? Math.sqrt(Math.max(0, v)) : v) : v)
  const d0 = scale.kind === 'continuous' ? warpT(scale.domain[0]) : 0, d1 = scale.kind === 'continuous' ? warpT(scale.domain[1]) : 1
  const classRgb = scale.kind === 'classes' ? scale.colors.map(rgbOf) : scale.kind === 'categorical' ? scale.colors.map(rgbOf) : []
  const otherRgb = scale.kind === 'categorical' ? rgbOf(scale.other) : null
  const color = (v: number): [number, number, number] | null => {
    if (!(v >= lo && v <= hi)) return null
    if (scale.kind === 'classes') return classRgb[classIndex(v, scale.breaks)]
    if (scale.kind === 'categorical') {
      const i = scale.keys.indexOf(String(Math.round(v)))
      return i >= 0 ? classRgb[i] : otherRgb
    }
    const t = Math.min(1, Math.max(0, (warpT(v) - d0) / (d1 - d0 || 1)))
    return rampAt(stops, t).map((c) => Math.round(c)) as [number, number, number]
  }
  const paintKey = JSON.stringify([scale, range])
  const url = registerPaint(data.values.template, data.values, paintKey, color)
  const unitLabel = data.values.categories && layer.style.fill && isBinding(layer.style.fill) && layer.style.fill.scale.type === 'categorical'
  return {
    layers: [{ id: sid(layer.id, 'raster'), type: 'raster', source: sid(layer.id), paint: { 'raster-fade-duration': 0, 'raster-resampling': layer.style.resample === 'nearest' ? 'nearest' : 'linear' }, layout: {}, opacity: { 'raster-opacity': 1 } }],
    sources: [{ id: sid(layer.id), kind: 'raster', tiles: [url], minzoom: data.values.minzoom, maxzoom: data.values.maxzoom }],
    areal: true,
    overlap: false,
    legend:
      scale.kind === 'classes'
        ? { kind: 'classes', colors: scale.colors, breaks: scale.breaks, unit: data.unit }
        : scale.kind === 'continuous'
          ? { kind: 'ramp', stops: scale.stops, domain: scale.domain, unit: data.unit }
          : { kind: 'categories', items: scale.keys.map((k, i) => ({ label: categoryLabel(layer, k), color: scale.colors[i], shape: 'square' as const })) },
    probe: ([lon, lat]) => {
      const v = valueAt(data.values.template, data.values, lon, lat, env.zoom?.() ?? 8, data.values.minzoom, data.values.maxzoom)
      if (v === null || v < lo || v > hi) return null
      const c = color(v)
      const swatch = c ? `rgb(${c.join(',')})` : undefined
      return { title: layer.name, swatch, rows: [{ label: data.label, value: unitLabel ? categoryLabel(layer, String(Math.round(v))) : fmtWith(v, data.unit) }] }
    },
  }
}
/** Class codes of a categorical grid, named (WorldCover is the one served today). */
const categoryLabel = (_layer: Layer, code: string) => WORLDCOVER[code]?.label ?? code
/** Shaded relief from the engine's raster-dem tiles, drawn by MapLibre; opacity goes through the compositor. */
function demHillshade(layer: Layer, data: DemPayload, env: Env): Compiled {
  const dark = env.scheme === 'dark'
  return {
    layers: [
      {
        id: sid(layer.id, 'hillshade'),
        type: 'hillshade',
        source: sid(layer.id),
        paint: {
          'hillshade-exaggeration': Math.min(1, Math.max(0, layer.style.exaggeration ?? 0.6)),
          'hillshade-illumination-direction': 315,
          'hillshade-shadow-color': dark ? '#000000' : '#263442',
          'hillshade-highlight-color': dark ? 'rgba(255,255,255,0.18)' : '#ffffff',
          'hillshade-accent-color': dark ? '#000000' : '#36424f',
        },
        layout: {},
        opacity: {},
      },
    ],
    sources: [{ id: sid(layer.id), kind: 'raster-dem', tiles: data.tiles, minzoom: data.minzoom, maxzoom: data.maxzoom, encoding: data.encoding }],
    areal: true,
    overlap: false,
    isolate: true,
    legend: { kind: 'relief' },
  }
}

/* ---------------- entry ---------------- */
/**
 * The colour scheme a layer draws in. A ramp is oriented so that "more" is its strongest end; under a
 * blend that can only darken, the strongest end is the dark one whatever the basemap, and under one
 * that can only lighten, the light one. So a multiplied hazard deepens where the value is high on a
 * dark map too, instead of blacking out where it is low.
 */
export const layerScheme = (blend: BlendMode | undefined, scheme: Theme): Theme => (blend === 'multiply' || blend === 'darken' ? 'light' : blend === 'screen' || blend === 'lighten' || blend === 'add' ? 'dark' : scheme)

export function compileLayer(layer: Layer, source: Source | undefined, data: LayerData | undefined, mapEnv: Env): Compiled {
  if (!data) return EMPTY
  const env = { ...mapEnv, scheme: layerScheme(layer.blend, mapEnv.scheme) }
  const v = vectorOf(data)
  const wrong = (needs: string): Compiled => ({ ...EMPTY, problem: `${titleCase(layer.type)} layers need ${needs}.` })
  switch (layer.type) {
    case 'choropleth':
      return data.kind === 'frame' ? choropleth(layer, source, data, env) : wrong('values on admin units')
    case 'bivariate':
      return data.kind === 'frame' ? bivariate(layer, source, data, env) : wrong('two variables on admin units')
    case 'raster':
      return data.kind === 'grid' ? raster(layer, data, env) : data.kind === 'values' ? valuesRaster(layer, data, env) : wrong('a value grid')
    case 'hillshade':
      return data.kind === 'grid' ? hillshade(layer, data, env) : data.kind === 'dem' ? demHillshade(layer, data, env) : wrong('an elevation grid')
    case 'polygon':
      return v && (data.kind !== 'features' || data.geometry === 'polygon') ? polygon(layer, source, v, env) : wrong('polygons')
    case 'line':
      return v && (data.kind !== 'features' || data.geometry !== 'point') ? line(layer, source, v, env) : wrong('lines or areas')
    case 'point':
      return data.kind === 'features' && data.geometry === 'point' ? point(layer, source, data, env) : wrong('points')
    case 'cluster':
      return data.kind === 'features' && data.geometry === 'point' ? cluster(layer, data, env) : wrong('points')
    case 'heatmap':
      return data.kind === 'features' && data.geometry === 'point' ? heatmap(layer, source, data, env) : wrong('points')
    case 'label':
      return label(layer, data, env)
  }
}
/** Fields a filter can be written on, for a layer's data. */
export function filterFields(layer: Layer, data: LayerData | undefined): FieldMeta[] {
  if (!data) return []
  if (data.kind === 'frame') return layer.type === 'bivariate' ? [] : [{ id: 'value', label: data.columns[0]?.label ?? 'Value', kind: 'number', unit: data.columns[0]?.unit, domain: extent(data.columns.flatMap((c) => c.values).filter(num)) }]
  if (data.kind === 'features') return data.fields
  return []
}
export const activeClauses = (layer: Layer) => clausesOf(layer.filter)
export type { BoundariesPayload }
