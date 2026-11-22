// Layer data from the engine: the renderer never builds a data URL (ui-contract §4). It asks the app plane
// to resolve the document's sources (`POST /app/v1/maps/resolve`) and loads what each one resolved to:
// a frame joined to the admin tileset, value tiles, a raster-dem, GeoJSON, columnar points or vector tiles.
// A source the engine does not serve resolves to `unavailable`/`denied`, and the caller falls back to the
// bundled sample data for that one source.
import type { Feature, FeatureCollection, Geometry } from 'geojson'
import { absoluteApiUrl, ApiError, apiAvailable, getBytes, getEnvelope, postJson, type Attribution } from '@/api/http'
import { featureMeta, gridMeta, indicatorMeta, type DataClass, type FeaturesPayload, type FieldMeta, type FrameColumn, type FramePayload, type LayerData } from '@/mocks/layers'
import type { MapDocument, Source } from '@/lib/map-spec'
import { adminShapes, SHAPE_GATE } from './adminShapes'
import { POINT_KINDS } from './registry'
import { overviewSample } from './valueTiles'

export type FieldInfo = { id: string; label: string | { en: string }; type: 'number' | 'category' | 'string' | 'date'; unit?: string; measure_type?: string }
/** One source of a resolved document (ui-contract §4, with the WP-27/28 additions). */
export type ResolvedSource = {
  access: 'public' | 'granted' | 'denied'
  shape: 'frame' | 'boundaries' | 'values' | 'dem' | 'points' | 'tiles' | 'geojson' | 'inline' | 'external' | 'unavailable'
  class?: string
  dsv?: string
  versions?: { dataset: string; dsv: string }[]
  // frame / boundaries
  frame?: string
  by_zoom?: { frame: string; level: number; source_layer: string; zoom: number }[]
  tiles?: string
  tilejson?: string
  source_layer?: string
  centroids_layer?: string
  minzoom?: number
  maxzoom?: number
  // values
  values?: string
  layer?: string
  period?: string
  scale?: number
  offset?: number
  nodata?: number
  unit?: string
  reducer?: string
  stats?: { min: number; max: number; mean: number; n: number }
  categories?: Record<string, string>
  encoding?: string
  // features
  url?: string
  points?: string
  count?: number
  source_layers?: string[]
  fields?: FieldInfo[]
}
export type Notice = { code: string; text: string }
export type ResolveProblem = { path: string; code: string; hint?: Record<string, unknown> }
export type Resolved = { data_version: string | null; sources: Record<string, ResolvedSource>; attribution: Attribution[]; notices: Notice[]; problems: ResolveProblem[] }

/** Does this resolved source carry data the client can draw? */
export const servable = (r: ResolvedSource | undefined): r is ResolvedSource => !!r && r.access !== 'denied' && r.shape !== 'unavailable' && r.shape !== 'inline' && r.shape !== 'external'

/* ---------------- resolve ---------------- */
const LAYER_FOR: Record<string, string> = { indicator: 'choropleth', boundaries: 'line', features: 'point', grid: 'raster' }
/** The part of a document the resolver needs: its data sources, each with a layer of a kind it accepts. */
function resolvable(sources: Record<string, Source>, pin: MapDocument['pin']) {
  const keep = Object.entries(sources).filter(([, s]) => s.kind !== 'analysis')
  const doc = {
    spec: 'ldl-map/1',
    title: 'resolve',
    pin,
    view: { center: [80.7, 7.8], zoom: 7 },
    basemap: { style: 'ldl-auto' },
    sources: Object.fromEntries(keep),
    layers: keep.map(([id, s]) => ({ id: `r_${id}`, type: s.kind === 'grid' && s.grid === 'dem' ? 'hillshade' : LAYER_FOR[s.kind], name: id, source: id, visible: true, style: {} })),
  }
  return { doc, ids: keep.map(([id]) => id) }
}
const EMPTY_RESOLVED: Resolved = { data_version: null, sources: {}, attribution: [], notices: [], problems: [] }
async function post(doc: unknown): Promise<Resolved> {
  const { payload } = await postJson<{ resolved: Resolved }>('/app/v1/maps/resolve', doc)
  return payload.resolved
}
/**
 * Resolves the document's sources; null when the engine is unreachable (everything is then sample data).
 * One bad source must not cost the others: when the whole document is refused, each source is resolved alone.
 */
export async function resolveSources(sources: Record<string, Source>, pin: MapDocument['pin']): Promise<Resolved | null> {
  if (!(await apiAvailable())) return null
  const { doc, ids } = resolvable(sources, pin)
  if (!ids.length) return EMPTY_RESOLVED
  try {
    return await post(doc)
  } catch (e) {
    if (!(e instanceof ApiError) || e.status >= 500) return null
    const parts = await Promise.all(ids.map((id) => post(resolvable({ [id]: sources[id] }, pin).doc).catch(() => null)))
    const out: Resolved = { ...EMPTY_RESOLVED, sources: {}, attribution: [], notices: [], problems: [] }
    for (const p of parts) {
      if (!p) continue
      out.data_version ??= p.data_version
      Object.assign(out.sources, p.sources)
      for (const a of p.attribution) if (!out.attribution.some((b) => b.source_id === a.source_id)) out.attribution.push(a)
      for (const n of p.notices) if (!out.notices.some((m) => m.code === n.code)) out.notices.push(n)
      out.problems.push(...p.problems)
    }
    return out
  }
}
/** A stable key for the sources that resolution depends on. */
export const resolveKey = (sources: Record<string, Source>, pin: MapDocument['pin']) =>
  JSON.stringify([pin, Object.entries(sources).filter(([, s]) => s.kind !== 'analysis').sort(([a], [b]) => a.localeCompare(b))])

/* ---------------- loaders ---------------- */
const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }
const classOf = (c: string | undefined, fallback: DataClass): DataClass => (c === 'official' ? 'official' : c === 'estimated' || c === 'derived' || c === 'third_party' ? (fallback === 'official' ? 'official' : 'estimated') : fallback)
type ServerColumn = { id: string; indicator: string; dims: Record<string, string>; period: string; unit: string; class: string; values: (number | null)[]; basis: string | number[] }
type ServerFrame = { support: { level: number; parent: string | null; n: number }; codes: string[]; columns: ServerColumn[] }

/** Keeps one series per period: the one the source asks for, else the "total" one. */
function pickSeries(columns: ServerColumn[], dims: Record<string, string> | undefined): ServerColumn[] {
  const byPeriod = new Map<string, ServerColumn[]>()
  for (const c of columns) (byPeriod.get(c.period) ?? byPeriod.set(c.period, []).get(c.period)!).push(c)
  const out: ServerColumn[] = []
  const want = (c: ServerColumn) => Object.entries(dims ?? {}).every(([k, v]) => String(c.dims[k] ?? '').toLowerCase() === String(v).toLowerCase())
  for (const cols of byPeriod.values()) out.push(cols.find(want) ?? cols.find((c) => Object.values(c.dims).some((v) => /^(total|all)$/i.test(v))) ?? cols[0])
  return out.sort((a, b) => a.period.localeCompare(b.period))
}

async function frame(source: Extract<Source, { kind: 'indicator' }>, r: ResolvedSource): Promise<FramePayload> {
  const level0 = typeof source.level === 'number' ? source.level : 0
  const [{ payload: f }, shapes] = await Promise.all([getEnvelope<ServerFrame>(r.frame!), adminShapes(level0)])
  const ids = source.indicators?.length ? source.indicators : [source.indicator]
  const metas = ids.map((id) => indicatorMeta(id, source.dims))
  const columns: FrameColumn[] = []
  ids.forEach((id, k) => {
    const m = metas[k]
    for (const c of pickSeries(f.columns.filter((c) => c.indicator === id), source.dims)) {
      const estimated = c.basis === 'estimated' || c.basis === 'forecast'
      columns.push({ id: c.id, indicator: id, period: c.period, unit: m?.unit ?? c.unit, label: m?.label ?? id, class: classOf(c.class, m?.class ?? 'official'), values: c.values, ...(estimated ? { basis: c.values.map(() => 'estimated' as const) } : {}) })
    }
  })
  const level = f.support.level
  return {
    kind: 'frame',
    origin: 'api',
    support: { scheme: 'admin', level, parent: f.support.parent ?? undefined, n: f.codes.length },
    codes: f.codes,
    // names come with the boundary tiles (`name_en`); the analysis panel looks them up when it needs them
    names: [],
    columns,
    geometry: shapes,
    centroids: [],
    fields: metas.length === 1 ? [{ id: 'value', label: metas[0]?.label ?? ids[0], kind: 'number', unit: metas[0]?.unit }] : ids.map((id, k) => ({ id, label: metas[k]?.label ?? id, kind: 'number' as const, unit: metas[k]?.unit })),
    tiles: { tiles: [absoluteApiUrl(r.tiles!)], minzoom: 0, maxzoom: r.maxzoom ?? 12, sourceLayer: r.source_layer ?? `adm${level}`, centroidsLayer: r.centroids_layer ?? 'centroids', level, parent: source.parent, gate: SHAPE_GATE[level] },
  }
}

/** The engine nests declared attributes under `attrs` and names in three languages: one flat record per feature. */
function flatten(f: Feature): Feature {
  const p = (f.properties ?? {}) as Record<string, unknown>
  const attrs = (p.attrs ?? {}) as Record<string, unknown>
  return { type: 'Feature', id: (p.key as string) ?? f.id, geometry: f.geometry, properties: { key: p.key, kind: p.kind, name: p.name_en ?? p.name ?? null, unit: p.unit, ...attrs } }
}
const geometryOf = (dataset: string, kind: string | undefined, sample?: Geometry | null): 'point' | 'line' | 'polygon' => {
  if (kind && POINT_KINDS.has(kind)) return 'point'
  const t = sample?.type
  if (t) return t.includes('Point') ? 'point' : t.includes('Line') ? 'line' : 'polygon'
  return featureMeta(dataset)?.geometry ?? 'point'
}
function fieldsOf(dataset: string, info: FieldInfo[] | undefined, collection?: FeatureCollection): FieldMeta[] {
  const meta = featureMeta(dataset)
  const out: FieldMeta[] = []
  const kind = meta?.fields.find((f) => f.id === 'kind')
  const seen = collection ? [...new Set(collection.features.map((f) => String(f.properties?.kind ?? '')).filter(Boolean))].sort() : null
  out.push({ id: 'kind', label: 'Kind', kind: 'category', categories: seen?.length ? (kind?.categories?.filter((c) => seen.includes(c)).concat(seen.filter((c) => !kind?.categories?.includes(c))) ?? seen) : kind?.categories })
  for (const f of info ?? []) {
    if (f.id === 'kind' || f.id === 'name') continue
    const label = typeof f.label === 'string' ? f.label : f.label.en
    if (f.type === 'number') {
      const vals = collection?.features.map((x) => Number(x.properties?.[f.id])).filter(Number.isFinite) ?? []
      if (collection && !vals.length) continue
      out.push({ id: f.id, label: label.replace(/^./, (c) => c.toUpperCase()), kind: 'number', unit: f.unit && !f.unit.startsWith('{') ? f.unit : undefined, domain: vals.length ? [Math.min(...vals), Math.max(...vals)] : undefined })
    } else if (f.type === 'category' && collection) {
      const c = [...new Set(collection.features.map((x) => x.properties?.[f.id]).filter((v): v is string => typeof v === 'string'))].sort()
      if (c.length > 1 && c.length <= 12) out.push({ id: f.id, label: label.replace(/^./, (x) => x.toUpperCase()), kind: 'category', categories: c })
    }
  }
  return out
}

async function geojson(source: Extract<Source, { kind: 'features' }>, r: ResolvedSource): Promise<FeaturesPayload> {
  // `format=geojson` is a bare FeatureCollection, not an envelope
  const fc = (await (await getBytes(r.url!)).json()) as FeatureCollection
  const collection: FeatureCollection = { type: 'FeatureCollection', features: (fc.features ?? []).map(flatten) }
  const meta = featureMeta(source.dataset)
  return { kind: 'features', origin: 'api', geometry: geometryOf(source.dataset, source.featureKind, collection.features[0]?.geometry), collection, fields: fieldsOf(source.dataset, r.fields, collection), title: 'name', class: meta?.class ?? 'official', count: r.count }
}

/** `…/points?format=arrow`: lon/lat Float32, key Utf8, then the fields. Apache Arrow loads with the first such layer. */
async function points(source: Extract<Source, { kind: 'features' }>, r: ResolvedSource): Promise<FeaturesPayload> {
  const [{ tableFromIPC }, res] = await Promise.all([import('apache-arrow'), getBytes(r.points!)])
  const table = tableFromIPC(new Uint8Array(await res.arrayBuffer()))
  const lon = table.getChild('lon')!.toArray() as Float32Array
  const lat = table.getChild('lat')!.toArray() as Float32Array
  const key = table.getChild('key')!
  const names = table.schema.fields.map((f) => f.name).filter((n) => n !== 'lon' && n !== 'lat' && n !== 'key')
  const cols = names.map((n) => [n, table.getChild(n)!] as const)
  const features: Feature[] = new Array(table.numRows)
  for (let i = 0; i < table.numRows; i++) {
    const properties: Record<string, unknown> = { key: key.get(i) }
    for (const [n, c] of cols) properties[n] = c.get(i)
    features[i] = { type: 'Feature', id: String(properties.key), properties, geometry: { type: 'Point', coordinates: [+lon[i].toFixed(6), +lat[i].toFixed(6)] } }
  }
  const collection: FeatureCollection = { type: 'FeatureCollection', features }
  const meta = featureMeta(source.dataset)
  return { kind: 'features', origin: 'api', geometry: 'point', collection, fields: fieldsOf(source.dataset, r.fields, collection), title: 'name', class: meta?.class ?? 'official', count: r.count ?? features.length }
}

function vectorTiles(source: Extract<Source, { kind: 'features' }>, r: ResolvedSource): FeaturesPayload {
  const meta = featureMeta(source.dataset)
  return {
    kind: 'features',
    origin: 'api',
    geometry: geometryOf(source.dataset, source.featureKind),
    collection: EMPTY,
    fields: fieldsOf(source.dataset, r.fields),
    title: 'name',
    class: meta?.class ?? 'official',
    count: r.count,
    tiles: { tiles: [absoluteApiUrl(r.tiles!)], minzoom: r.minzoom ?? 0, maxzoom: r.maxzoom ?? 14, sourceLayer: r.source_layers?.[0] ?? source.dataset, ...(source.featureKind ? { filter: ['==', ['get', 'kind'], source.featureKind] } : {}) },
  }
}

/** Loads what a source resolved to, in the payload shape the compiler takes. */
export async function loadResolved(source: Source, r: ResolvedSource): Promise<LayerData> {
  switch (r.shape) {
    case 'frame':
      if (source.kind !== 'indicator') break
      return frame(source, r.frame ? r : { ...r, frame: r.by_zoom?.[r.by_zoom.length - 1]?.frame, source_layer: r.by_zoom?.[r.by_zoom.length - 1]?.source_layer })
    case 'boundaries':
      if (source.kind !== 'boundaries') break
      return { kind: 'boundaries', origin: 'api', level: source.level, collection: EMPTY, centroids: { type: 'FeatureCollection', features: [] }, tiles: { tiles: [absoluteApiUrl(r.tiles!)], minzoom: 0, maxzoom: r.maxzoom ?? 12, sourceLayer: r.source_layer ?? `adm${source.level}`, centroidsLayer: 'centroids', level: source.level, parent: source.parent } }
    case 'values': {
      if (source.kind !== 'grid') break
      const codec = { scale: r.scale ?? 1, offset: r.offset ?? 0, nodata: r.nodata ?? 65535 }
      const template = absoluteApiUrl(r.values!)
      const sample = await overviewSample(template, codec)
      const meta = gridMeta(source.grid) ?? gridMeta(r.layer ?? '')
      let lo = Infinity, hi = -Infinity
      for (const v of sample) {
        if (v < lo) lo = v
        if (v > hi) hi = v
      }
      return {
        kind: 'values',
        origin: 'api',
        id: r.layer ?? source.grid,
        unit: meta?.unit ?? r.unit ?? '',
        label: meta?.label ?? r.layer ?? source.grid,
        period: r.period ?? 'latest',
        domain: Number.isFinite(lo) ? [lo, hi] : [r.stats?.min ?? 0, r.stats?.max ?? 1],
        class: meta?.class ?? 'estimated',
        values: { template, ...codec, minzoom: r.minzoom ?? 5, maxzoom: r.maxzoom ?? 11, reducer: r.reducer, categories: r.categories },
        sample,
      }
    }
    case 'dem':
      return { kind: 'dem', origin: 'api', tiles: [absoluteApiUrl(r.tiles!)], minzoom: r.minzoom ?? 5, maxzoom: r.maxzoom ?? 11, encoding: r.encoding === 'mapbox' ? 'mapbox' : 'terrarium' }
    case 'geojson':
      if (source.kind === 'features') return geojson(source, r)
      break
    case 'points':
      if (source.kind === 'features') return points(source, r)
      break
    case 'tiles':
      if (source.kind === 'features') return vectorTiles(source, r)
      break
  }
  throw new Error(`Cannot draw a ${r.shape} source of kind ${source.kind}`)
}
/** What identifies the bytes of a resolved source: its URLs, not its style. */
export const resolvedKey = (r: ResolvedSource) => [r.shape, r.frame ?? r.values ?? r.points ?? r.url ?? r.tiles ?? '', r.source_layer ?? r.source_layers?.join() ?? '', r.dsv ?? '']
