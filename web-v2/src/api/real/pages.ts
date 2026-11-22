// Adapters: live v2 payloads -> the page-level types in ../types.
//
// Every adapter either returns a complete value or throws; `client.get` then falls back to the bundled mocks, so the
// pages never see a half-built object. Anything the API cannot serve yet is filled from the mocks and named in the
// result's `sample` list, which the pages render as a "sample data" mark.
import * as db from '@/mocks/db'
import { SITE } from '@/lib/site'
import type { Area, AreaProfile, ChildRow, Dataset, DatasetDetail, Headline, LicenceStatus, Meta, Provenance, PyramidRow, RankItem, SampleKeys, Stats, ThemeId } from '../types'
import { getEnvelope, getJson, type Meta as ApiMeta } from '../http'

/* ------------------------------ plumbing ------------------------------ */
const memo = new Map<string, Promise<unknown>>()
/** One in-flight/settled promise per key: profiles of sibling areas share their frames and dataset records. */
function once<T>(key: string, fn: () => Promise<T>): Promise<T> {
  let p = memo.get(key) as Promise<T> | undefined
  if (!p) {
    p = fn()
    memo.set(key, p)
    p.catch(() => memo.delete(key))
  }
  return p
}
const orNull = <T>(p: Promise<T>): Promise<T | null> => p.catch(() => null)

const licStatus = (s: string): LicenceStatus => (s === 'open' || s === 'attribution' ? s : 'unclear')
const worst = (xs: LicenceStatus[]): LicenceStatus => (xs.includes('unclear') ? 'unclear' : xs.includes('attribution') ? 'attribution' : 'open')
const day = (iso: string | null | undefined) => (iso ?? '').slice(0, 10)
const round = (v: number, d = 2) => Math.round(v * 10 ** d) / 10 ** d

/* ------------------------------ raw shapes ------------------------------ */
type RawSource = { id: string; publisher: string; title: string; homepage: string | null; licence_id: string | null; cadence: string | null; status: string }
type RawLicence = { id: string; status: string; url: string | null; spdx: string | null }
type RawIndicator = { id: string; label: string; unit: string; unit_label: string; dims: string[]; geo_levels: string[] }
type RawDataset = {
  id: string
  kind: 'indicators' | 'features' | 'boundaries'
  title: string
  description: string
  method: string | null
  limits: string[] | null
  current: { dsv: string; vintage: string | null; row_count: number | null; published_at: string | null } | null
  sources: RawSource[]
  licences: RawLicence[]
  indicators: RawIndicator[]
}
type CatalogEntry = { id: string; title: string; description: string; kind: RawDataset['kind']; themes: string[]; published: boolean }
type RawFrame = {
  support: { n: number; level: number }
  codes: string[]
  columns: { indicator: string; dims: Record<string, string> | null; period: string; unit: string; values: (number | null)[] }[]
}
type Obs = { obs_id: number; dims: string; value: number | null; basis: string; status: string }
/** `completed_from`: a value the engine completed from another indicator (ADM1 census counts summed from the districts) */
type AreaInd = { indicator_id: string; label: string; unit: string; dataset_id: string; period: string; values: Obs[]; class?: string; completed_from?: string }
type RawRef = { pcode: string; level: number; name_en: string; name_si: string | null; name_ta: string | null }
type RawArea = RawRef & { area_km2: number | null; centroid: [number, number]; ancestors: RawRef[]; children: RawRef[]; indicators: AreaInd[] }

const frame = (indicators: string, level: number, extra: { parent?: string; dims?: string; from?: string; to?: string } = {}) =>
  once(`frame|${indicators}|${level}|${extra.parent ?? ''}|${extra.dims ?? ''}|${extra.from ?? ''}`, () => getJson<RawFrame>('/v2/frame', { indicators, level, ...extra }))
const col = (f: RawFrame | null, pick: (c: RawFrame['columns'][number]) => boolean) => f?.columns.find(pick)

/* ------------------------------ stats ------------------------------ */
/** The canonical grid (0.001 degree) over the island has 11.34 M cells; see docs/architecture.md section 1. */
const GRID_CELLS = 11_340_000

export async function stats(): Promise<Stats> {
  const [st, ds, tiles] = await Promise.all([
    getJson<{ release: { data_version: string; cut_at: string } | null; sources: { schedulable: number } }>('/v2/status'),
    getJson<{ state: string }[]>('/v2/status/datasets'),
    orNull(getJson<{ id: string; layers: { id: string; features: number }[] }[]>('/v2/tiles')),
  ])
  const adm = tiles?.find((t) => t.id === 'admin')?.layers.filter((l) => /^adm\d$/.test(l.id))
  return {
    datasets: ds.filter((d) => d.state === 'published').length,
    areas: adm?.length ? adm.reduce((s, l) => s + l.features, 0) : db.STATS.areas,
    cells: GRID_CELLS,
    sources: st.sources.schedulable,
    observations: db.STATS.observations,
    dataVersion: st.release?.data_version ?? db.STATS.dataVersion,
    lastRelease: day(st.release?.cut_at) || db.STATS.lastRelease,
    sample: ['observations', ...(adm?.length ? [] : ['areas'])],
  }
}

/* ------------------------------ catalogue ------------------------------ */
const detail = (id: string) => once(`dataset|${id}`, () => getEnvelope<RawDataset>(`/v2/datasets/${id}`))

function themeOf(e: CatalogEntry): ThemeId {
  const t = `${e.id} ${e.title}`.toLowerCase()
  if (e.kind === 'boundaries') return 'boundaries'
  if (/flood|climate|\brain|temperature|hazard/.test(t)) return 'climate'
  if (/election|polling/.test(t)) return 'elections'
  if (/rail|road|transport|trail|cycl|airport|port\b/.test(t)) return 'transport'
  if (/poi|amenit|health|school|hospital|facilit/.test(t)) return 'access'
  if (/census|population|ethnic|religion|pop-grid|demograph/.test(t)) return 'people'
  if (/wdi|gdp|econom|per-capita|metrics|night/.test(t)) return 'economy'
  if (/land|cover|park|green|protected|water|terrain|elevation|dem\b|tourism|environment/.test(t)) return 'land'
  if (e.themes.includes('ECON')) return 'economy'
  if (e.themes.includes('SOCI')) return 'people'
  if (e.themes.includes('ENVI')) return 'land'
  return 'boundaries'
}

function publisherType(publisher: string, id: string): Dataset['publisherType'] {
  if (/^derived|lanka data layer/i.test(id) || /lanka data layer/i.test(publisher)) return 'derived'
  if (/openstreetmap|osm|nuuuwan|community/i.test(publisher)) return 'community'
  if (/world bank|esa\b|copernicus|jrc|united nations|ocha|geoboundaries|asian development|worldpop|international/i.test(publisher)) return 'multilateral'
  if (/department|ministry|commission|survey|authority|government|board|bank of ceylon|central bank/i.test(publisher)) return 'government'
  return 'research'
}

const CADENCE: Record<string, string> = { one_off: 'One-off', annual: 'Annual', monthly: 'Monthly', quarterly: 'Quarterly', weekly: 'Weekly', daily: 'Daily', irregular: 'Irregular', decennial: 'Decennial', biannual: 'Twice a year' }
const cadenceLabel = (c: string | null | undefined) => (c ? (CADENCE[c] ?? c.charAt(0).toUpperCase() + c.slice(1).replace(/_/g, ' ')) : 'Irregular')

function levelsOf(e: CatalogEntry, raw: RawDataset | null): string[] {
  if (e.kind === 'boundaries') return ['ADM0–ADM4']
  if (e.kind === 'features') {
    const g = [/poi|tourism|amenit/.test(e.id) && 'Point', /rail|road|trail|waterway/.test(e.id) && 'Line', /park|protected|waterbod/.test(e.id) && 'Polygon'].filter((x): x is string => !!x)
    return g.length ? g : ['Point']
  }
  const nums = new Set<number>()
  for (const i of raw?.indicators ?? []) for (const l of i.geo_levels) nums.add(l === 'national' ? 0 : Number(l.replace('adm', '')))
  const xs = [...nums].filter((n) => Number.isFinite(n)).sort((a, b) => a - b)
  if (!xs.length) return []
  return [xs[0] === xs[xs.length - 1] ? `ADM${xs[0]}` : `ADM${xs[0]}–ADM${xs[xs.length - 1]}`]
}

function toDataset(e: CatalogEntry, raw: RawDataset | null): Dataset {
  const sources = raw?.sources ?? []
  const publishers = [...new Set(sources.map((s) => s.publisher))]
  const licences = raw?.licences ?? []
  const status = worst(licences.length ? licences.map((l) => licStatus(l.status)) : ['unclear'])
  const lic = licences.find((l) => licStatus(l.status) === status)
  const publisher = publishers.slice(0, 2).join(' · ') || 'Lanka Data Layer (derived)'
  const stale = sources.some((s) => s.status === 'failing') ? 'stale' : sources.some((s) => s.status === 'overdue') ? 'due' : 'fresh'
  const cur = raw?.current
  const sizeMb = 0.2 + (db.hash(`${e.id}:size`) % 480) / 100
  return {
    id: e.id,
    title: raw?.title ?? e.title,
    summary: raw?.description ?? e.description,
    theme: themeOf(e),
    publisher,
    publisherType: publisherType(publishers[0] ?? publisher, e.id),
    licence: { id: lic?.id ?? 'unknown', label: status === 'unclear' ? 'No stated licence' : (lic?.spdx ?? lic?.id ?? 'Open'), status },
    cadence: cadenceLabel(sources[0]?.cadence),
    levels: levelsOf(e, raw),
    rows: cur?.row_count ?? 0,
    indicators: raw?.indicators.length ?? 0,
    vintage: cur?.vintage ?? '',
    updated: day(cur?.published_at),
    freshness: stale,
    formats: e.kind === 'indicators' ? ['JSON'] : ['GeoJSON', 'JSON'],
    sizeMb: round(sizeMb, 1),
    spark: Array.from({ length: 12 }, (_, i) => 40 + ((db.hash(e.id + i) % 60) + i * (db.hash(e.id) % 7))),
    sample: ['spark', 'sizeMb'],
  }
}

export async function datasets(): Promise<Dataset[]> {
  const cat = await getJson<CatalogEntry[]>('/v2/catalog')
  const published = cat.filter((e) => e.published)
  if (!published.length) throw new Error('empty catalogue')
  return Promise.all(published.map(async (e) => toDataset(e, (await orNull(detail(e.id)))?.payload ?? null)))
}

/* ------------------------------ dataset page ------------------------------ */
type FeatureCollection = { features: { properties: { key: string; kind: string | null; name: string | null; unit: string | null } }[] }

async function preview(raw: RawDataset, d: Dataset): Promise<Pick<DatasetDetail, 'columns' | 'preview' | 'previewColumns' | 'previewNote'> | null> {
  if (raw.kind === 'indicators' && raw.indicators.length) {
    const ind = raw.indicators.find((i) => i.geo_levels.includes('adm2')) ?? raw.indicators[0]
    const level = ind.geo_levels.includes('adm2') ? 2 : ind.geo_levels.includes('adm1') ? 1 : 0
    const f = await getJson<RawFrame>('/v2/frame', { indicators: ind.id, level })
    const cols = f.columns.slice(0, 6)
    const n = Math.min(8, f.codes.length)
    if (!n || !cols.length) return null
    const head = (c: RawFrame['columns'][number]) => {
      const dims = Object.entries(c.dims ?? {}).map(([k, v]) => `${k}=${v}`).join(';')
      return `${c.indicator.split('.').pop()}${dims ? `[${dims}]` : ''}`
    }
    return {
      columns: [
        { name: 'pcode', type: 'text', note: 'Official p-code of the unit (join key)' },
        ...raw.indicators.map((i) => ({ name: i.id, type: 'real', note: `${i.label} · ${i.unit_label}${i.dims.length ? ` · by ${i.dims.join(', ')}` : ''}` })),
      ],
      previewColumns: ['pcode', ...cols.map(head)],
      preview: f.codes.slice(0, n).map((code, i) => [code, ...cols.map((c) => (c.values[i] === null || c.values[i] === undefined ? '—' : round(c.values[i] as number)))]),
      previewNote: `first ${n} of ${f.codes.length} units · ${ind.id} · ADM${level}`,
    }
  }
  if (raw.kind === 'features') {
    const fc = await getJson<FeatureCollection>(`/v2/features/${raw.id}`, { limit: 8 })
    if (!fc.features.length) return null
    return {
      columns: [
        { name: 'key', type: 'text', note: 'Stable feature id' },
        { name: 'kind', type: 'text', note: 'Source classification' },
        { name: 'name', type: 'text', note: 'Name as published (name_si, name_ta where mapped)' },
        { name: 'unit', type: 'text', note: 'Containing p-code (join key)' },
        { name: '…fields', type: 'mixed', note: 'Source attributes, one property each' },
      ],
      previewColumns: ['key', 'kind', 'name', 'unit'],
      preview: fc.features.map((f) => [f.properties.key, f.properties.kind ?? '', f.properties.name ?? '', f.properties.unit ?? '']),
      previewNote: `first ${fc.features.length} of ${fmtN(d.rows)} features`,
    }
  }
  return null
}
const fmtN = (n: number) => n.toLocaleString('en')

export async function dataset(id: string): Promise<DatasetDetail> {
  const { payload: raw, meta } = await detail(id)
  const base = toDataset({ id: raw.id, title: raw.title, description: raw.description, kind: raw.kind, themes: [], published: true }, raw)
  const pv = await orNull(preview(raw, base))
  const fallback = pv ? null : db.datasetDetail('admin-boundaries')
  const dataVersion = meta?.data_version ?? db.DATA_VERSION
  const steps = (raw.method ?? '').split(/(?<=[.;])\s+/).filter(Boolean)
  const method = steps.length > 8 ? [...steps.slice(0, 7), steps.slice(7).join(' ')] : steps
  const year = (base.vintage.match(/\d{4}/)?.[0] ?? base.updated.slice(0, 4)) || String(new Date().getFullYear())
  const lic = (id: string | null) => raw.licences.find((l) => l.id === id)
  const sample = ['downloads', 'sizeMb', ...(pv ? [] : ['preview'])]
  return {
    ...base,
    sample,
    description: raw.description,
    contains: [
      `${fmtN(base.rows)} rows${base.levels.length ? ` across ${base.levels.join(', ')}` : ''}`,
      raw.indicators.length ? `${raw.indicators.length} indicator${raw.indicators.length === 1 ? '' : 's'}: ${raw.indicators.slice(0, 4).map((i) => i.label).join('; ')}${raw.indicators.length > 4 ? '…' : ''}` : 'Geometry and identifiers only, no statistical indicators',
      'An observation id on every value, resolving to document, locator and file hash',
      `Vintage ${base.vintage || 'not stated'}; refresh cadence: ${base.cadence.toLowerCase()}`,
    ],
    method: method.length ? method : ['Fetched on schedule, parsed into rows, joined to p-codes by code and validated before release.'],
    limits: raw.limits?.length ? raw.limits : ['No limits have been recorded for this dataset yet.'],
    sources: raw.sources.map((s, i) => {
      const l = lic(s.licence_id)
      return { n: i + 1, title: s.title, publisher: s.publisher, url: s.homepage ?? SITE, licence: l ? (licStatus(l.status) === 'unclear' ? 'No stated licence' : (l.spdx ?? l.id)) : 'Not recorded', status: licStatus(l?.status ?? 'unclear'), retrieved: '', sha256: '' }
    }),
    columns: pv?.columns ?? fallback!.columns,
    preview: pv?.preview ?? fallback!.preview,
    previewColumns: pv?.previewColumns ?? fallback!.columns.map((c) => c.name),
    previewNote: pv?.previewNote,
    downloads: base.formats.map((f) => ({ format: f, label: `${id}.${f === 'GeoJSON' ? 'geojson' : 'json'}`, size: `${base.sizeMb.toFixed(2)} MB`, href: '#' })),
    versions: raw.current ? [{ dsv: raw.current.dsv, date: day(raw.current.published_at), note: 'Current release', added: raw.current.row_count ?? 0, changed: 0, removed: 0 }] : [],
    usedIn: [
      { label: 'Area profiles (all levels)', href: '/area/LK11' },
      { label: 'Explore map', href: '/explore' },
    ],
    citation: `${raw.sources[0]?.publisher ?? 'Lanka Data Layer'} (${year}). ${raw.title}. Retrieved ${base.updated} via Lanka Data Layer, data version ${dataVersion}, dataset ${raw.id}.`,
    dataVersion,
  }
}

/* ------------------------------ area profile ------------------------------ */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const sum = (xs: number[]) => xs.reduce((s, x) => s + x, 0)
const num = (v: number | null | undefined): v is number => typeof v === 'number' && Number.isFinite(v)

const pick = (a: RawArea, id: string) => a.indicators.find((i) => i.indicator_id === id)
const obsOf = (i: AreaInd | undefined, dims = '') => i?.values.find((v) => v.dims === dims)
/** A figure the engine completed from the units below (ADM1 census counts from the districts) says so, quietly. */
const basisOf = (i: AreaInd | undefined): Headline['basis'] => (i?.completed_from ? { label: 'computed', hint: 'Summed from the districts: the census publishes this table by district and DS division' } : undefined)

const tri = (r: RawRef, fb?: Area): Area['name'] => ({ en: r.name_en, si: r.name_si ?? fb?.name.si ?? '', ta: r.name_ta ?? fb?.name.ta ?? '' })

/** Sibling stat helper: value of `code` in a frame column, plus the full vector for ranking. */
type Vec = { codes: string[]; values: (number | null)[] }
const vec = (f: RawFrame | null, pickCol: (c: RawFrame['columns'][number]) => boolean): Vec | null => {
  const c = col(f, pickCol)
  return f && c ? { codes: f.codes, values: c.values } : null
}

async function ranksFor(a: RawArea, names: (code: string) => string): Promise<RankItem[] | null> {
  const L = a.level === 0 ? 1 : a.level
  if (L > 3) return null
  const [fArea, fDen, fAge, fLc, fRain, fFlood] = await Promise.all([
    orNull(frame('admin.area_km2', L)),
    orNull(frame('derived.per_km2', L)),
    orNull(frame('census24.pop_age_sex', L)),
    orNull(frame('lc.class_share', L)),
    orNull(frame('clim.wc_bio', L, { dims: 'bio_var=BIO12' })),
    orNull(frame('haz.flood_area_share', L, { dims: 'return_period=rp100' })),
  ])
  const area = vec(fArea, () => true)
  const asRatioWeightedByArea = (v: Vec | null) => v && area && v.codes.length === area.codes.length ? v : null
  const items: RankItem[] = []
  const push = (indicator: string, label: string, unit: string, v: Vec | null, national: number | null) => {
    if (!v || national === null) return
    const own = a.level === 0 ? national : v.values[v.codes.indexOf(a.pcode)]
    if (!num(own)) return
    const sibs = v.codes.map((code, i) => ({ pcode: code, name: names(code), value: v.values[i] })).filter((s): s is { pcode: string; name: string; value: number } => num(s.value))
    if (sibs.length < 2) return
    items.push({ indicator, label, unit, value: own, rank: sibs.filter((s) => s.value > own).length + 1, of: sibs.length, national, siblings: sibs })
  }
  const areaWeighted = (v: Vec | null) => {
    const w = asRatioWeightedByArea(v)
    if (!w || !area) return null
    let num_ = 0
    let den = 0
    w.values.forEach((x, i) => {
      const k = area.values[i]
      if (num(x) && num(k)) {
        num_ += x * k
        den += k
      }
    })
    return den ? num_ / den : null
  }
  // density: residents per km2, national = total residents over total area
  const den = vec(fDen, (c) => (c.dims?.source_indicator ?? '').endsWith('census24.pop_total'))
  push('pop.density', 'Population density', 'per km²', den, areaWeighted(den))
  // 65+ share from the age groups of the same census table
  const total = fAge?.columns.filter((c) => (c.dims?.sex ?? 'total') === 'total')
  const old = total?.find((c) => c.dims?.age_group === '65+')
  if (fAge && total?.length && old) {
    const share = fAge.codes.map((_, i) => {
      const t = sum(total.map((c) => c.values[i] ?? 0))
      return t > 0 && num(old.values[i]) ? ((old.values[i] as number) / t) * 100 : null
    })
    const T = sum(total.flatMap((c) => c.values.filter(num)))
    push('age.65plus', 'Residents aged 65 and over', '%', { codes: fAge.codes, values: share }, T > 0 ? (sum(old.values.filter(num)) / T) * 100 : null)
  }
  const lc = (cls: string) => vec(fLc, (c) => c.dims?.lc_class === cls)
  const tree = lc('10')
  const built = lc('50')
  push('veg.tree_cover', 'Tree cover', '%', tree, areaWeighted(tree))
  push('built.share', 'Built-up land', '%', built, areaWeighted(built))
  const rain = vec(fRain, () => true)
  push('rain.annual', 'Annual rainfall', 'mm', rain, areaWeighted(rain))
  const flood = vec(fFlood, () => true)
  push('haz.flood_area_share', 'Flood-prone land, 1-in-100 year', '%', flood, areaWeighted(flood))
  return items.length >= 3 ? items : null
}

async function climateFor(a: RawArea): Promise<AreaProfile['climate'] | null> {
  if (a.level === 0) return null
  const parent = a.level >= 3 ? a.ancestors[a.ancestors.length - 1]?.pcode : undefined
  const f = await frame('clim.wc_prec,clim.wc_tavg', a.level, { parent, from: '1970-01', to: '1970-12' })
  const i = f.codes.indexOf(a.pcode)
  if (i < 0) return null
  const month = (indicator: string, m: number) => f.columns.find((c) => c.indicator === indicator && c.period.endsWith(`-${String(m + 1).padStart(2, '0')}`))?.values[i]
  const out = MONTHS.map((name, m) => ({ month: name, rain: month('clim.wc_prec', m), temp: month('clim.wc_tavg', m) }))
  if (!out.every((r) => num(r.rain) && num(r.temp))) return null
  // WorldClim ships normals only, so there is no observed-versus-normal band: low and high collapse onto the normal
  return out.map((r) => ({ month: r.month, rain: r.rain as number, lo: r.rain as number, hi: r.rain as number, temp: r.temp as number }))
}

type ChildStats = { pop: Map<string, number>; density: Map<string, number> }
async function childStats(a: RawArea): Promise<ChildStats | null> {
  const kid = a.children[0]
  if (!kid) return null
  const parent = a.level === 0 ? undefined : a.pcode
  const [fTot, fAge, fDen] = await Promise.all([
    orNull(frame('census24.pop_total', kid.level, { parent, dims: 'sex=total' })),
    orNull(frame('census24.pop_age_sex', kid.level, { parent })),
    orNull(frame('derived.per_km2', kid.level, { parent })),
  ])
  const pop = new Map<string, number>()
  const density = new Map<string, number>()
  const tot = fTot?.columns[0]
  fTot?.codes.forEach((c, i) => num(tot?.values[i]) && pop.set(c, tot!.values[i] as number))
  if (fAge) {
    const cols = fAge.columns.filter((c) => (c.dims?.sex ?? 'total') === 'total')
    fAge.codes.forEach((c, i) => {
      if (!pop.has(c) && cols.length) pop.set(c, sum(cols.map((x) => x.values[i] ?? 0)))
    })
  }
  const den = col(fDen, (c) => (c.dims?.source_indicator ?? '').endsWith('census24.pop_total'))
  fDen?.codes.forEach((c, i) => num(den?.values[i]) && density.set(c, den!.values[i] as number))
  return pop.size ? { pop, density } : null
}

/** Land-cover classes folded into the five bars the page draws. */
function landFrom(i: AreaInd | undefined): AreaProfile['land'] | null {
  if (!i) return null
  const by = new Map<string, number>()
  for (const v of i.values) if (num(v.value)) by.set(v.dims.replace('lc_class=', ''), v.value)
  if (!by.size) return null
  const g = (...k: string[]) => sum(k.map((x) => by.get(x) ?? 0))
  const tree = g('10')
  const crop = g('40')
  const built = g('50')
  const water = g('80', '90')
  const other = Math.max(0, g(...[...by.keys()]) - tree - crop - built - water)
  return [
    { id: 'tree', label: 'Tree cover', share: tree },
    { id: 'crop', label: 'Cropland', share: crop },
    { id: 'built', label: 'Built-up', share: built },
    { id: 'water', label: 'Water and wetland', share: water },
    { id: 'other', label: 'Grass, shrub, bare', share: other },
  ]
}

/** Scale the sample pyramid so its four census age groups match the real counts; the sex split and the 2012 outline stay sample. */
function calibratePyramid(rows: PyramidRow[], groups: { g0: number; g15: number; g60: number; g65: number } | null): PyramidRow[] {
  if (!groups) return rows
  const bands: [number, number, number][] = [
    [0, 3, groups.g0],
    [3, 12, groups.g15],
    [12, 13, groups.g60],
    [13, rows.length, groups.g65],
  ]
  const out = rows.map((r) => ({ ...r }))
  for (const [from, to, target] of bands) {
    const cur = sum(rows.slice(from, to).map((r) => r.m + r.f))
    if (cur <= 0) continue
    for (let i = from; i < to; i++) {
      out[i].m *= target / cur
      out[i].f *= target / cur
    }
  }
  return out
}

/** Sources and licences as the envelope names them: each source with its licence, each licence with the attribution it requires. */
function metaFor(env: ApiMeta | undefined): Meta {
  return {
    data_version: env?.data_version ?? db.DATA_VERSION,
    source: (env?.source ?? []).map((s) => ({ publisher: s.publisher, title: s.title, url: s.url || SITE })),
    licences: (env?.licences ?? []).map((l) => ({ id: l.id, status: licStatus(l.status), attribution: l.attribution })),
    completeness: { status: 'complete' },
  }
}

export async function areaProfile(pcodeRaw: string): Promise<AreaProfile> {
  const { payload: a, meta: env } = await getEnvelope<RawArea>(`/v2/areas/${pcodeRaw.toUpperCase()}`)
  const mock = await db.loadAreas()
  const names = (code: string) => mock.byCode.get(code)?.name.en ?? code
  const chain = [...a.ancestors].reverse().map((r) => r.pcode)
  const baseCode = [a.pcode, ...chain].find((c) => mock.byCode.has(c)) ?? 'LK'
  const [base, ranks, climate, kids, meta] = await Promise.all([db.areaProfile(baseCode), orNull(ranksFor(a, names)), orNull(climateFor(a)), orNull(childStats(a)), metaFor(env)])
  const sample: SampleKeys = ['access', 'elections', 'postal', 'pyramid', 'popSeries', 'built']

  /* ---- area ---- */
  const own = mock.byCode.get(a.pcode)
  const parent = a.ancestors[a.ancestors.length - 1]
  const area: Area = {
    pcode: a.pcode,
    level: a.level as Area['level'],
    name: tri(a, own),
    parent: parent?.pcode ?? null,
    centroid: [a.centroid[1], a.centroid[0]], // the API is [lat, lon]
    areaKm2: a.area_km2 ?? obsOf(pick(a, 'admin.area_km2'))?.value ?? 0,
  }
  const breadcrumb: Area[] = [...a.ancestors, a].map((r) => ({ pcode: r.pcode, level: r.level as Area['level'], name: tri(r, mock.byCode.get(r.pcode)), parent: null, centroid: [0, 0], areaKm2: 0 }))

  /* ---- headline ---- */
  const popInd = pick(a, 'census24.pop_total')
  const ageInd = pick(a, 'census24.pop_age_sex')
  const age = (g: string) => obsOf(ageInd, `age_group=${g};sex=total`)
  const ageObs = ['0-14', '15-59', '60-64', '65+'].map(age)
  const popObs = obsOf(popInd, 'sex=total')
  const popValue = popObs?.value ?? (ageObs.every((o) => num(o?.value)) ? sum(ageObs.map((o) => o!.value as number)) : null)
  // density from the census where it counts the unit; a GN division has no census count, so its density is WorldPop's model
  const censusDen = a.indicators.find((i) => i.indicator_id === 'derived.per_km2')?.values.find((v) => v.dims.endsWith('census24.pop_total'))
  const denObs = censusDen ?? obsOf(pick(a, 'pop.grid_density'))
  const denBasis: Headline['basis'] = !censusDen && denObs ? { label: 'modelled (WorldPop)', hint: 'WorldPop 2025, 100 m constrained population grid, summed over the unit and divided by its area' } : undefined
  const areaObs = obsOf(pick(a, 'admin.area_km2'))
  const sexObs = obsOf(pick(a, 'census24.sex_ratio'))
  const status = (o: Obs): Headline['status'] => (o.status === 'normal' ? 'final' : 'provisional')
  const headline: Headline[] = []
  const year = (i: AreaInd | undefined) => Number.parseInt(i?.period ?? '', 10) || 2024
  if (num(popValue)) headline.push({ id: 'pop.total', label: 'Population', value: popValue, unit: 'people', year: year(popInd ?? ageInd), status: popObs ? status(popObs) : 'provisional', observationId: String((popObs ?? ageObs.find(Boolean))?.obs_id ?? ''), basis: basisOf(popInd ?? ageInd) })
  if (denObs && num(denObs.value)) headline.push({ id: 'pop.density', label: 'Density', value: denObs.value, unit: 'per km²', year: censusDen ? 2024 : year(pick(a, 'pop.grid_density')), status: status(denObs), observationId: String(denObs.obs_id), basis: denBasis })
  if (areaObs && num(areaObs.value)) headline.push({ id: 'area.land', label: 'Land area', value: areaObs.value, unit: 'km²', year: 2024, status: status(areaObs), observationId: String(areaObs.obs_id) })
  if (sexObs && num(sexObs.value)) headline.push({ id: 'sex.ratio', label: 'Sex ratio', value: sexObs.value, unit: 'men per 100 women', year: 2024, status: status(sexObs), observationId: String(sexObs.obs_id), basis: basisOf(pick(a, 'census24.sex_ratio')) })
  if (!headline.length) throw new Error('profile has no headline data')

  /* ---- people ---- */
  const real65 = ageObs.every((o) => num(o?.value)) ? ageObs.map((o) => o!.value as number) : null
  const tot = real65 ? sum(real65) : 0
  const groups = real65 && tot > 0 ? { g0: (real65[0] / tot) * 100, g15: (real65[1] / tot) * 100, g60: (real65[2] / tot) * 100, g65: (real65[3] / tot) * 100 } : null
  const baseLast = base.popSeries[base.popSeries.length - 1].value
  const popFactor = num(popValue) && baseLast > 0 ? popValue / baseLast : 1

  /* ---- land and built ---- */
  const land = landFrom(a.indicators.find((i) => i.indicator_id === 'lc.class_share'))
  if (!land) sample.push('land')
  const builtShare = land?.find((l) => l.id === 'built')?.share
  const b25 = base.built.find((d) => d.year === 2025)?.share ?? 0
  const builtFactor = builtShare !== undefined && b25 > 0 ? builtShare / b25 : 1

  /* ---- children ---- */
  const syn = (code: string, popGuess: number): ChildRow => {
    const r = db.rng(`child:${code}`)
    const change = 2 + (r() - 0.35) * 14
    const spark = [0, 1, 2, 3, 4, 5, 6].map((k) => popGuess * (1 - (change / 100) * ((6 - k) / 6)) * (1 + (r() - 0.5) * 0.015))
    return { pcode: code, name: { en: '', si: '', ta: '' }, population: 0, density: 0, change, hospitalMin: 8 + r() * 30, spark }
  }
  const children: ChildRow[] = a.children
    .map((c) => {
      const row = syn(c.pcode, kids?.pop.get(c.pcode) ?? 1000)
      const r = db.rng(`childpop:${c.pcode}`)
      const pop = kids?.pop.get(c.pcode) ?? Math.round((num(popValue) ? popValue : 50_000) * (0.4 / a.children.length) * (0.5 + r()))
      const den = kids?.density.get(c.pcode) ?? pop / Math.max(1, (area.areaKm2 || 100) / a.children.length)
      return { ...row, name: tri(c, mock.byCode.get(c.pcode)), population: Math.round(pop), density: den }
    })
    .sort((x, y) => y.population - x.population)
  if (a.children.length) sample.push('children')
  if (a.children.length && !kids) sample.push('children.values')

  /* ---- neighbours, postal ---- */
  let neighbours = base.neighbours
  if (baseCode !== a.pcode) {
    const siblings = parent ? await orNull(once(`area|${parent.pcode}`, () => getJson<RawArea>(`/v2/areas/${parent.pcode}`))) : null
    neighbours = (siblings?.children ?? []).filter((c) => c.pcode !== a.pcode).slice(0, 5).map((c) => ({ pcode: c.pcode, name: c.name_en }))
    sample.push('neighbours')
  }
  const postalBase = db.hash(a.pcode) % 70000
  const postal = (children.length ? children : base.children).slice(0, 8).map((c, i) => ({ code: String(postalBase + 10000 + i * 30).padStart(5, '0'), name: c.name.en }))

  if (!ranks) sample.push('ranks')
  if (!climate) sample.push('climate')
  if (sample.length > 0) meta.completeness = { status: 'partial', note: 'Sections marked “sample data” show illustrative values: the live API does not serve them yet.' }

  return {
    area,
    breadcrumb,
    headline,
    ranks: ranks ?? base.ranks,
    pyramid: calibratePyramid(base.pyramid, groups),
    popSeries: base.popSeries.map((p) => ({ year: p.year, value: p.value * popFactor })),
    access: base.access,
    land: land ?? base.land,
    built: base.built.map((b) => ({ year: b.year, share: b.share * builtFactor })),
    climate: climate ?? base.climate,
    elections: base.elections,
    children,
    postal,
    neighbours,
    meta,
    real: true,
    sample,
  }
}

/* ------------------------------ provenance ------------------------------ */
type RawProvenance = {
  obs_id: number
  indicator_id: string
  geo_code: string
  value: number | null
  verbatim: string | null
  artifact_sha256: string | null
  artifact_url: string | null
  fetched_at: string | null
  locator: Record<string, string> | null
  method: string
  review: string | null
  reviewed_by: string | null
  reviewed_at: string | null
}

export async function provenance(id: string): Promise<Provenance> {
  const { payload: p, meta } = await getEnvelope<RawProvenance>(`/v2/observations/${id}/provenance`)
  const src = meta?.source?.[0]
  const lic = meta?.licences?.find((l) => l.id === src?.licence) ?? meta?.licences?.[0]
  const loc = p.locator ?? {}
  const value = p.value === null ? '—' : p.value.toLocaleString('en', { maximumFractionDigits: 2 })
  const cell = loc.cell ? `${loc.sheet ?? loc.table ?? ''}!${loc.cell}` : (loc.table ?? loc.sheet ?? p.method)
  return {
    observationId: String(p.obs_id),
    indicator: p.indicator_id,
    area: p.geo_code,
    value,
    verbatim: p.verbatim ?? value,
    document: { title: src?.title ?? p.indicator_id, publisher: src?.publisher ?? '', url: p.artifact_url ?? src?.url ?? SITE, published: day(p.fetched_at), pages: 0 },
    locator: { page: 0, table: cell, row: loc.row ?? '', column: loc.column ?? '' },
    sha256: p.artifact_sha256 ?? '',
    retrieved: p.fetched_at ?? '',
    licence: { id: lic?.id ?? '', label: lic ? (licStatus(lic.status) === 'unclear' ? 'No stated licence' : lic.id) : 'Not recorded', status: licStatus(lic?.status ?? 'unclear') },
    review: { method: `Extraction: ${p.method}; review ${p.review ?? 'not recorded'}`, reviewer: p.reviewed_by ?? 'not recorded', date: day(p.reviewed_at) },
    crop: { header: [loc.column ?? 'Value'], rows: [{ label: loc.row ?? p.geo_code, cells: [p.verbatim ?? value], hit: 0 }] },
  }
}

