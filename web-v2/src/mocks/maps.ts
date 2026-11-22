// Mock of the app plane's map store (10-map-composition §5, §6.1): starter maps that ship with the
// product, and the visitor's own maps kept in localStorage in place of `app.maps`.
import { blankDocument, GENERATOR, instantiate, uid, type Group, type Layer, type LayerNode, type LayerTemplate, type MapDocument, type Source, type TimeConfig } from '@/lib/map-spec'
import type { Theme } from '@/lib/hooks'
import { PRESET } from './layers'

const KEY = 'ldl-maps/1'
const TIME = (cursor: string, from: string, to: string): TimeConfig => ({ cursor, range: [from, to], freq: 'A', playback: { fps: 1.1, loop: true } })

/** Collects sources as layers are added, so a starter reads top-down like the layer panel does. */
function builder(title: string, description: string) {
  const doc = blankDocument(title)
  doc.description = description
  doc.meta = { ...doc.meta, created_at: '2026-11-18T04:12:00Z', updated_at: '2026-11-18T04:12:00Z', author: '@lanka', generator: GENERATOR }
  // ids are stable from one issue of a starter to the next, so a selection survives a theme change
  const taken = new Set<string>()
  const idFor = (prefix: string, name: string) => {
    const base = `${prefix}_${name.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '').slice(0, 28)}`
    let id = base
    for (let n = 2; taken.has(id); n++) id = `${base}_${n}`
    taken.add(id)
    return id
  }
  const add = (t: LayerTemplate, patch: Partial<Layer> = {}, sourceId?: string): Layer => {
    const made = instantiate(t, sourceId ?? idFor('s', patch.name ?? t.name))
    doc.sources[made.sourceId] = made.source
    const layer = { ...made.layer, ...patch }
    return patch.id ? layer : { ...layer, id: idFor('l', layer.name) }
  }
  const source = (id: string, s: Source) => ((doc.sources[id] = s), id)
  const group = (name: string, children: LayerNode[], patch: Partial<Group> = {}): Group => ({ id: idFor('g', name), type: 'group', name, visible: true, opacity: 1, collapsed: false, children: children.reverse(), ...patch })
  /** `nodes` are given top → bottom, as they appear in the panel; the document stores bottom → top. */
  const done = (id: string, nodes: LayerNode[]): MapDocument => ({ ...doc, id, layers: nodes.reverse() })
  return { doc, add, source, group, done }
}

function floodVsPrices(scheme: Theme): MapDocument {
  const b = builder('Flood exposure vs property prices', 'Where modelled 1-in-100-year river flooding coincides with lower asking prices in Western Province. Associations, not causes.')
  b.doc.extent = { geo: 'LK1' }
  b.doc.view = { center: [80.03, 6.85], zoom: 9.35, bounds: [79.8, 6.36, 80.3, 7.32] }
  b.source('exposure', { kind: 'indicator', indicator: 'exp.flood_pop_share', dims: { return_period: 'RP100' }, level: 4, parent: 'LK1', period: 'latest' })
  b.source('prices', { kind: 'indicator', indicator: '@demo/re.asking_price_per_perch_median', level: 4, parent: 'LK1', period: '2026' })
  b.source('flood', PRESET.floodRaster(scheme).source)
  b.source('listings', PRESET.listings().source)
  b.source('rel_biv', { kind: 'analysis', ref: 'rel', output: 'bivariate' })
  b.source('rel_lee', { kind: 'analysis', ref: 'rel', output: 'local_lee' })
  b.source('zone_pts', { kind: 'analysis', ref: 'zone', output: 'points' })
  b.source('dsd', { kind: 'boundaries', scheme: 'admin', level: 3, parent: 'LK1' })
  b.doc.analyses = {
    rel: {
      spec: 'ldl-analysis/1',
      method: 'relate',
      support: { kind: 'admin', level: 4, within: 'LK1' },
      variables: { x: { source: 'exposure' }, y: { source: 'prices', transform: 'log' }, controls: [{ indicator: 'est.pop_density', period: '2026', transform: 'log' }, { indicator: 'acc.travel_time_city', dims: { city: 'colombo', mode: 'car' } }] },
      params: { correlation: ['pearson', 'spearman'], regression: 'ols', robust: true, spatial: { weights: 'knn6', local: 'lee', permutations: 199 }, bivariate: { classes: 3, method: 'quantile' }, scale_sensitivity: [3, 4] },
    },
    zone: {
      spec: 'ldl-analysis/1',
      method: 'group_compare',
      support: { kind: 'points', source: 'listings' },
      variables: { y: { field: 'price_per_perch', transform: 'log' } },
      params: { zone: { source: 'flood', where: { op: '>=', args: [{ property: 'value' }, 0.15] } }, rings_m: [0, 250, 500], compare: 'inside_vs_ring', within_groups: { level: 3 }, tests: ['welch', 'mann_whitney'], bootstrap: 2000 },
    },
  }
  b.doc.panels = [
    { id: 'p_legend', type: 'legend' },
    { id: 'p_scatter', type: 'chart', chart: 'scatter', analysis: 'rel', linked: 'l_biv', options: { fit: ['ols', 'robust'], density: 'hexbin', brush: true } },
    { id: 'p_stats', type: 'stats', analysis: 'rel' },
    { id: 'p_zone', type: 'stats', analysis: 'zone' },
  ]
  const outline = { color: 'ldl.tone.paper', width: 0.3 }
  return b.done('m_flood_prices', [
    b.add(PRESET.boundaryLabels(3, 'LK1'), { name: 'DS division names' }, 'dsd'),
    b.add(PRESET.boundaryLines(3, 'LK1'), { name: 'DS divisions', opacity: 0.42 }, 'dsd'),
    b.add(PRESET.rivers(), { opacity: 0.8 }),
    b.add(PRESET.floodRaster(scheme), { id: 'l_flood', opacity: scheme === 'dark' ? 0.4 : 0.55 }, 'flood'),
    b.group('Exposure × price', [
      b.add(
        {
          type: 'polygon',
          name: 'Significant local association',
          source: { kind: 'analysis', ref: 'rel', output: 'local_lee' },
          style: {
            fill: 'none',
            stroke: { width: 1.5, color: { field: 'cluster', scale: { type: 'categorical', mapping: { HH: 'ldl.tone.orange', HL: 'ldl.tone.ink', LH: 'ldl.tone.green', LL: 'ldl.tone.blue' } } } },
          },
          filter: { op: '<=', args: [{ property: 'p_fdr' }, 0.05] },
        },
        { id: 'l_lee', visible: false },
        'rel_lee',
      ),
      b.add(
        {
          type: 'bivariate',
          name: 'Flood exposure × asking price',
          source: { kind: 'analysis', ref: 'rel', output: 'bivariate' },
          style: { fill: { bivariate: { x: { field: 'x', scale: { type: 'quantile', classes: 3 } }, y: { field: 'y', scale: { type: 'quantile', classes: 3 } }, palette: 'ldl.biv.blue-orange' } }, stroke: outline, uncertainty: 'hatch' },
          opacity: 0.9,
        },
        { id: 'l_biv' },
        'rel_biv',
      ),
    ]),
    b.add(
      {
        type: 'point',
        name: 'Listings inside and near the flood zone',
        source: { kind: 'analysis', ref: 'zone', output: 'points' },
        style: { size: 3.4, fill: { field: 'ring', scale: { type: 'categorical', mapping: { inside: 'ldl.tone.blue', '0-250': 'ldl.tone.orange', '250-500': 'ldl.tone.grey' } } }, stroke: { color: 'ldl.tone.paper', width: 0.7 } },
        visible: false,
        opacity: 0.95,
      },
      { id: 'l_zone' },
      'zone_pts',
    ),
    b.group(
      'Inputs',
      [
        b.add(PRESET.listings(), { visible: false }, 'listings'),
        b.add(PRESET.choropleth('exp.flood_pop_share', 4, 'LK1'), { name: 'Population in the flood zone', visible: false }, 'exposure'),
        b.add(PRESET.choropleth('@demo/re.asking_price_per_perch_median', 4, 'LK1'), { name: 'Median asking price per perch', visible: false }, 'prices'),
      ],
      { collapsed: true },
    ),
  ])
}

function hospitalAccess(_scheme: Theme): MapDocument {
  void _scheme
  const b = builder('Access to hospitals', 'Drive time to the nearest base hospital or above, with the facilities themselves and the roads that reach them.')
  return b.done('m_hospital_access', [
    b.add(PRESET.towns(), { opacity: 0.9 }),
    b.add(PRESET.hospitals()),
    b.add(PRESET.roads(), { opacity: 0.55 }),
    b.add(PRESET.boundaryLines(2), { opacity: 0.45 }),
    b.add(PRESET.driveRaster(), { opacity: 0.85 }),
    b.add(PRESET.choropleth('access.hospital.drive_min', 2), { name: 'Mean drive time by district', visible: false }),
    b.add(PRESET.hillshade(), { opacity: 0.5 }),
  ])
}

function populationAndRail(scheme: Theme): MapDocument {
  const b = builder('Population and rail', 'Where people live against the railway that serves them, 2012 to 2026.')
  b.doc.time = TIME('2024', '2012', '2026')
  return b.done('m_population_rail', [
    b.add(PRESET.towns(), { opacity: 0.9 }),
    b.add({ ...PRESET.rail(), style: { stroke: { color: 'ldl.tone.ink', width: { field: 'trains_per_day', scale: { type: 'continuous', transform: 'sqrt', range: [0.8, 3.6] } }, 'x-casing': true } } }, { name: 'Railway, by trains per day' }),
    b.add(PRESET.protectedAreas(scheme)),
    b.add(PRESET.boundaryLines(1), { opacity: 0.4 }),
    b.add({ ...PRESET.popRaster(), style: { ...PRESET.popRaster().style, 'x-relief': true } }, { opacity: 0.9 }),
  ])
}

/** The single-indicator map the explorer used to be: the landing point of links from area pages. */
export function glance(indicator = 'pop.density', level = 2, _scheme: Theme = 'dark'): MapDocument {
  void _scheme
  const b = builder('Sri Lanka at a glance', 'One indicator by district, year by year.')
  b.doc.time = TIME('2024', '2012', '2026')
  return b.done('m_glance', [
    b.add(PRESET.towns(), { opacity: 0.9 }),
    b.add(PRESET.rail(), { opacity: 0.7 }),
    b.add(PRESET.boundaryLines(1), { opacity: 0.55 }),
    b.add(PRESET.choropleth(indicator, level), { id: 'l_indicator' }),
  ])
}

export const STARTERS: ((scheme: Theme) => MapDocument)[] = [floodVsPrices, hospitalAccess, populationAndRail, (scheme) => glance(undefined, undefined, scheme)]
/** Starters are written for the scheme they open in: a tint multiplies on a light map and screens on a dark one. */
const starterCache = new Map<string, MapDocument>()
const starter = (id: string, scheme: Theme): MapDocument | undefined => {
  const key = `${scheme}:${id}`
  if (!starterCache.has(`${scheme}:`)) {
    starterCache.set(`${scheme}:`, blankDocument())
    for (const make of STARTERS) {
      const d = make(scheme)
      starterCache.set(`${scheme}:${d.id}`, d)
    }
  }
  return starterCache.get(key)
}
export const isStarter = (id: string | undefined) => !!id && !!starter(id, 'dark')
export const DEFAULT_MAP = 'm_flood_prices'

/* ---------------- the visitor's own maps ---------------- */
type Store = Record<string, MapDocument>
function read(): Store {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? '{}') as Store
  } catch {
    return {}
  }
}
function write(store: Store) {
  try {
    localStorage.setItem(KEY, JSON.stringify(store))
  } catch {
    /* private mode or full: the map stays in memory and in its link */
  }
}
const countLayers = (nodes: LayerNode[]): number => nodes.reduce((s, n) => s + (n.type === 'group' ? countLayers(n.children) : 1), 0)
export type MapSummary = { id: string; title: string; description?: string; updated_at: string; layers: number; starter: boolean; analyses: number; forked_from?: string | null; /** the top layers, for a row of thumbnails */ preview: Layer[] }
const topLayers = (nodes: LayerNode[], out: Layer[] = []): Layer[] => {
  for (let i = nodes.length - 1; i >= 0 && out.length < 6; i--) {
    const n = nodes[i]
    if (n.type === 'group') topLayers(n.children, out)
    else if (n.type !== 'label') out.push(n)
  }
  return out
}
const summary = (d: MapDocument, isStart: boolean): MapSummary => ({ id: d.id!, title: d.title, description: d.description, updated_at: d.meta.updated_at, layers: countLayers(d.layers), starter: isStart, analyses: Object.keys(d.analyses).length, forked_from: d.meta.forked_from, preview: topLayers(d.layers) })

/** `GET /app/v1/workspaces/{ws}/maps` */
export function listMaps(): MapSummary[] {
  const own = Object.values(read()).sort((a, b) => b.meta.updated_at.localeCompare(a.meta.updated_at))
  return [...STARTERS.map((make) => summary(starter(make('dark').id!, 'dark')!, true)), ...own.map((d) => summary(d, false))]
}
/** `GET /app/v1/maps/{id}` */
export function getMap(id: string, scheme: Theme): MapDocument | null {
  const own = read()[id]
  if (own) return own
  const s = starter(id, scheme)
  return s ? structuredClone(s) : null
}
/** `POST …/maps` for a document without an id (or a starter, which is forked), `PUT /app/v1/maps/{id}` otherwise. */
export function saveMap(doc: MapDocument): MapDocument {
  const now = new Date().toISOString()
  const fork = !doc.id || isStarter(doc.id)
  const saved: MapDocument = { ...doc, id: fork ? uid('m', 12) : doc.id, meta: { ...doc.meta, updated_at: now, ...(fork ? { created_at: now, forked_from: doc.id ?? null, author: '@you' } : {}) } }
  write({ ...read(), [saved.id!]: saved })
  return saved
}
/** `POST /app/v1/maps/{id}/fork` */
export function forkMap(id: string, scheme: Theme): MapDocument | null {
  const src = getMap(id, scheme)
  if (!src) return null
  const now = new Date().toISOString()
  const copy: MapDocument = { ...structuredClone(src), id: uid('m', 12), title: `${src.title} copy`, meta: { ...src.meta, created_at: now, updated_at: now, forked_from: id, author: '@you' } }
  write({ ...read(), [copy.id!]: copy })
  return copy
}
/** `DELETE /app/v1/maps/{id}` */
export function deleteMap(id: string) {
  const store = read()
  delete store[id]
  write(store)
}
