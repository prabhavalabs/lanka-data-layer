// Mock of the three data delivery shapes of 10-map-composition §3 (frames, value grids, features)
// and of the mappable catalogue. Values come from ./db (district tables) and ./synth (surfaces,
// divisions, listings): artificial but internally consistent.
import type { FeatureCollection, Point } from 'geojson'
import type { LicenceStatus } from '@/api/types'
import type { Theme } from '@/lib/hooks'
import type { BlendMode, Cql, FieldBinding, LayerType, Source, Style } from '@/lib/map-spec'
import type { RampName } from '@/lib/palettes'
import { RAMP_OF } from '@/lib/palettes'
import { INDICATORS, valueFor } from './db'
import * as synth from './synth'

/* ---------------- registry: indicators, grids, feature sets ---------------- */
export type DataClass = 'official' | 'estimated' | 'synthetic'
export type IndicatorMeta = {
  id: string
  label: string
  short: string
  unit: string
  ramp: RampName
  periods: string[]
  levels: number[]
  description: string
  dataset: string
  class: DataClass
  /** levels 3 and 4 exist for Western Province only in this mockup */
  scope?: string
}
const range = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => String(a + i))
export const INDICATOR_META: IndicatorMeta[] = [
  ...INDICATORS.map((i): IndicatorMeta => ({ id: i.id, label: i.label, short: i.short, unit: i.unit, ramp: RAMP_OF[i.ramp], periods: i.years.map(String), levels: [1, 2], description: i.description, dataset: i.dataset, class: 'official' })),
  { id: 'exp.flood_pop_share', label: 'Population in the flood zone', short: 'Flood exposure', unit: '%', ramp: 'ldl.seq.blue', periods: ['2026'], levels: [1, 2, 3, 4], description: 'Share of residents living where the modelled 1-in-100-year river flood is deeper than 0.15 m.', dataset: 'flood-exposure', class: 'estimated' },
  { id: '@demo/re.asking_price_per_perch_median', label: 'Median asking price per perch', short: 'Asking price', unit: 'LKR/perch', ramp: 'ldl.seq.orange', periods: range(2022, 2026), levels: [1, 2, 3, 4], description: 'Median asking price of residential land in a synthetic sample of listings. Modelled where a unit has fewer than three.', dataset: 'property-asking-prices-sample', class: 'synthetic' },
  { id: 'est.pop_density', label: 'Population density (grid estimate)', short: 'Density', unit: 'per km²', ramp: 'ldl.seq.violet', periods: ['2026'], levels: [3, 4], description: 'Residents per square kilometre, aggregated from the population grid.', dataset: 'pop-grid-100m', class: 'estimated', scope: 'LK1' },
  { id: 'acc.travel_time_city', label: 'Drive time to Colombo', short: 'To Colombo', unit: 'min', ramp: 'ldl.seq.teal', periods: ['2026'], levels: [3, 4], description: 'Typical drive time from the unit to Colombo Fort.', dataset: 'access-health', class: 'estimated', scope: 'LK1' },
]
export const indicatorMeta = (id: string) => INDICATOR_META.find((i) => i.id === id)
export const LEVELS: Record<number, { name: string; plural: string; code: string }> = {
  1: { name: 'Province', plural: 'provinces', code: 'ADM1' },
  2: { name: 'District', plural: 'districts', code: 'ADM2' },
  3: { name: 'DS division', plural: 'DS divisions', code: 'ADM3' },
  4: { name: 'GN division', plural: 'GN divisions', code: 'ADM4' },
}

export type GridMeta = { id: string; label: string; unit: string; ramp: RampName; periods: string[]; dataset: string; class: DataClass; description: string }
export const GRID_META: GridMeta[] = [
  { id: 'haz.flood_depth', label: 'Flood depth, 1-in-100-year', unit: 'm', ramp: 'ldl.seq.blue', periods: ['2026'], dataset: 'flood-hazard', class: 'estimated', description: 'Modelled depth of standing water in a 1-in-100-year river flood.' },
  { id: 'pop.density', label: 'Population density', unit: 'per km²', ramp: 'ldl.seq.violet', periods: synth.POP_YEARS.map(String), dataset: 'pop-grid-100m', class: 'estimated', description: 'Residents per square kilometre on the population grid.' },
  { id: '@demo/re.asking_price_surface', label: 'Asking price surface', unit: 'LKR/perch', ramp: 'ldl.seq.orange', periods: ['2026'], dataset: 'property-asking-prices-sample', class: 'synthetic', description: 'Listing prices interpolated by inverse-distance weighting; blank where no listing is within 6.5 km.' },
  { id: 'acc.hospital_drive_time', label: 'Drive time to nearest hospital', unit: 'min', ramp: 'ldl.seq.teal', periods: ['2026'], dataset: 'access-health', class: 'estimated', description: 'Minutes by road to the nearest base hospital or above.' },
  { id: 'dem', label: 'Elevation', unit: 'm', ramp: 'ldl.seq.green', periods: ['2026'], dataset: 'dem', class: 'official', description: 'Height above sea level.' },
]
export const gridMeta = (id: string) => GRID_META.find((g) => g.id === id)

export type FieldMeta = { id: string; label: string; kind: 'number' | 'category'; unit?: string; domain?: [number, number]; categories?: string[] }
export type FeatureSetMeta = { id: string; label: string; geometry: 'point' | 'line' | 'polygon'; fields: FieldMeta[]; title?: string; class: DataClass }
export const FEATURE_META: FeatureSetMeta[] = [
  {
    id: '@demo/property-asking-prices-sample', label: 'Land listings (sample)', geometry: 'point', title: 'land_use', class: 'synthetic',
    fields: [
      { id: 'price_per_perch', label: 'Asking price', kind: 'number', unit: 'LKR/perch', domain: [100_000, 12_000_000] },
      { id: 'perches', label: 'Plot size', kind: 'number', unit: 'perches', domain: [6, 60] },
      { id: 'land_use', label: 'Land use', kind: 'category', categories: [...synth.LAND_USE] },
      { id: 'listed_year', label: 'Year listed', kind: 'number', domain: [2022, 2026] },
    ],
  },
  {
    id: 'health-facilities', label: 'Health facilities', geometry: 'point', title: 'name', class: 'official',
    fields: [
      { id: 'tier', label: 'Tier', kind: 'category', categories: ['Teaching', 'General', 'Base', 'Divisional'] },
      { id: 'beds', label: 'Beds', kind: 'number', unit: 'beds', domain: [20, 2200] },
      { id: 'ownership', label: 'Ownership', kind: 'category', categories: ['Government', 'Private'] },
    ],
  },
  {
    id: 'places', label: 'Towns and cities', geometry: 'point', title: 'name', class: 'official',
    fields: [
      { id: 'kind', label: 'Kind', kind: 'category', categories: ['City', 'Town'] },
      { id: 'population', label: 'Population', kind: 'number', unit: 'people', domain: [5_000, 760_000] },
    ],
  },
  { id: 'waterways', label: 'Rivers', geometry: 'line', title: 'class', class: 'official', fields: [{ id: 'class', label: 'Class', kind: 'category', categories: ['Main river', 'River', 'Stream'] }, { id: 'length_km', label: 'Length', kind: 'number', unit: 'km', domain: [0, 60] }] },
  { id: 'railways', label: 'Railway lines', geometry: 'line', title: 'class', class: 'official', fields: [{ id: 'class', label: 'Service', kind: 'category', categories: ['Suburban', 'Main line'] }, { id: 'trains_per_day', label: 'Trains per day', kind: 'number', unit: 'trains', domain: [8, 180] }] },
  { id: 'roads', label: 'Road network', geometry: 'line', title: 'class', class: 'official', fields: [{ id: 'class', label: 'Class', kind: 'category', categories: ['motorway', 'trunk', 'primary', 'secondary'] }] },
  { id: 'protected-areas', label: 'Protected areas', geometry: 'polygon', title: 'name', class: 'official', fields: [{ id: 'designation', label: 'Designation', kind: 'category', categories: ['National park', 'Nature reserve', 'Protected area'] }, { id: 'area_km2', label: 'Area', kind: 'number', unit: 'km²', domain: [0, 1300] }] },
]
export const featureMeta = (id: string) => FEATURE_META.find((f) => f.id === id)

/* ---------------- payloads ---------------- */
export type FrameColumn = { id: string; indicator: string; period: string; unit: string; label: string; class: DataClass; values: (number | null)[]; n?: number[]; basis?: ('observed' | 'estimated')[] }
export type FramePayload = {
  kind: 'frame'
  support: { scheme: 'admin'; level: number; parent?: string; n: number }
  codes: string[]
  names: string[]
  columns: FrameColumn[]
  /** the boundary tileset, stood in for by GeoJSON */
  geometry: FeatureCollection
  centroids: [number, number][]
  national?: Record<string, number>
  fields: FieldMeta[]
}
export type GridPayload = { kind: 'grid'; grid: synth.Grid; id: string; unit: string; label: string; period: string; domain: [number, number]; class: DataClass }
/** `tiles`: the set is too large for GeoJSON and comes as vector tiles (here: the basemap's own). */
export type FeaturesPayload = { kind: 'features'; geometry: 'point' | 'line' | 'polygon'; collection: FeatureCollection; fields: FieldMeta[]; title?: string; class: DataClass; tiles?: { url: string; sourceLayer: string; filter: unknown[] } }
export type BoundariesPayload = { kind: 'boundaries'; level: number; collection: FeatureCollection; centroids: FeatureCollection<Point> }
export type LayerData = FramePayload | GridPayload | FeaturesPayload | BoundariesPayload

const finite = (a: ArrayLike<number>): [number, number] => {
  let lo = Infinity, hi = -Infinity
  for (let i = 0; i < a.length; i++) {
    const v = a[i]
    if (!Number.isFinite(v)) continue
    if (v < lo) lo = v
    if (v > hi) hi = v
  }
  return [lo, hi]
}

/** `GET /v2/frame?indicators=&level=&parent=&from=&to=`: one code array, one value array per indicator and period. */
export async function frame(indicators: string[], level: number, parent?: string): Promise<FramePayload> {
  const metas = indicators.map((id) => indicatorMeta(id) ?? INDICATOR_META[0])
  const first = metas[0]
  const lvl = first.levels.includes(level) ? level : first.levels[first.levels.length - 1]
  const within = lvl >= 3 ? 'LK1' : parent
  const all = await synth.units(lvl)
  const keep = all.codes.map((c, i) => (!within || c.startsWith(within) ? i : -1)).filter((i) => i >= 0)
  const pick = <T,>(a: ArrayLike<T>) => keep.map((i) => a[i])
  const needStats = metas.some((m) => !INDICATORS.some((i) => i.id === m.id))
  const S = needStats ? await synth.unitStats(String(lvl)) : null
  const columns: FrameColumn[] = []
  for (const meta of metas) {
    const legacy = INDICATORS.some((i) => i.id === meta.id)
    for (const period of meta.periods) {
      const base = { id: `c${columns.length}`, indicator: meta.id, period, unit: meta.unit, label: meta.label, class: meta.class }
      if (!meta.levels.includes(lvl)) columns.push({ ...base, values: keep.map(() => null) })
      else if (legacy) columns.push({ ...base, values: keep.map((i) => valueFor(meta.id, all.codes[i], Number(period))) })
      else if (meta.id === 'exp.flood_pop_share') columns.push({ ...base, values: pick(S!.floodShare).map((v) => +v.toFixed(2)), basis: keep.map(() => 'estimated') })
      else if (meta.id === 'est.pop_density') columns.push({ ...base, values: pick(S!.popDensity).map((v) => Math.round(v)) })
      else if (meta.id === 'acc.travel_time_city') columns.push({ ...base, values: pick(S!.travelMin).map((v) => +v.toFixed(1)) })
      else {
        const growth = Math.pow(1.07, Number(period) - 2026)
        columns.push({ ...base, values: pick(S!.price).map((v) => Math.round((v * growth) / 1000) * 1000), n: pick(S!.nListings), basis: pick(S!.nListings).map((n) => (n >= 3 ? 'observed' : 'estimated')) })
      }
    }
  }
  const national = INDICATORS.some((i) => i.id === first.id) ? Object.fromEntries(first.periods.map((p) => [p, valueFor(first.id, 'LK', Number(p))])) : undefined
  return {
    kind: 'frame',
    support: { scheme: 'admin', level: lvl, parent: within, n: keep.length },
    codes: pick(all.codes),
    names: pick(all.names),
    columns,
    geometry: { type: 'FeatureCollection', features: pick(all.features) },
    centroids: pick(all.centroids),
    national,
    fields: metas.length === 1 ? [{ id: 'value', label: first.label, kind: 'number', unit: first.unit }] : metas.map((m) => ({ id: m.id, label: m.label, kind: 'number' as const, unit: m.unit })),
  }
}

/** `GET /v2/grids/{id}/values/{z}/{x}/{y}.png`: here the whole grid at once. */
export async function grid(id: string, period: string): Promise<GridPayload> {
  const meta = gridMeta(id) ?? GRID_META[0]
  const year = meta.periods.includes(period) ? period : (meta.periods.filter((p) => p <= period).pop() ?? meta.periods[meta.periods.length - 1])
  const g = await (meta.id === 'haz.flood_depth'
    ? synth.floodGrid()
    : meta.id === 'pop.density'
      ? synth.popGrid(year)
      : meta.id === '@demo/re.asking_price_surface'
        ? synth.priceGrid()
        : meta.id === 'acc.hospital_drive_time'
          ? synth.driveTimeGrid()
          : synth.elevationGrid())
  return { kind: 'grid', grid: g, id: meta.id, unit: meta.unit, label: meta.label, period: year, domain: finite(g.data), class: meta.class }
}
export const relief = (exaggeration: number) => synth.reliefGrid(String(exaggeration))

/** `GET /v2/features/{dataset}`. */
export async function features(dataset: string): Promise<FeaturesPayload> {
  const meta = featureMeta(dataset) ?? FEATURE_META[0]
  const base = { kind: 'features' as const, geometry: meta.geometry, fields: meta.fields, title: meta.title, class: meta.class }
  const empty: FeatureCollection = { type: 'FeatureCollection', features: [] }
  switch (meta.id) {
    case 'health-facilities':
      return { ...base, collection: await synth.hospitalFeatures() }
    case 'places':
      return { ...base, collection: await synth.placeFeatures() }
    case 'waterways':
      return { ...base, collection: await synth.riverFeatures() }
    case 'railways':
      return { ...base, collection: await synth.railFeatures() }
    case 'protected-areas':
      return { ...base, collection: await synth.protectedFeatures() }
    case 'roads':
      return { ...base, collection: empty, tiles: { url: 'https://tiles.openfreemap.org/planet', sourceLayer: 'transportation', filter: ['all', ['match', ['geometry-type'], ['LineString', 'MultiLineString'], true, false], ['match', ['get', 'class'], ['motorway', 'trunk', 'primary', 'secondary'], true, false], ['!', ['has', 'brunnel']]] } }
    default:
      return { ...base, collection: await synth.listingFeatures() }
  }
}

export async function boundaries(level: number, parent?: string): Promise<BoundariesPayload> {
  const all = await synth.units(level)
  const within = level >= 3 ? 'LK1' : parent
  const keep = all.codes.map((c, i) => (!within || c.startsWith(within) ? i : -1)).filter((i) => i >= 0)
  return {
    kind: 'boundaries',
    level,
    collection: { type: 'FeatureCollection', features: keep.map((i) => all.features[i]) },
    centroids: { type: 'FeatureCollection', features: keep.map((i) => ({ type: 'Feature', properties: { ...all.features[i].properties, rank: -all.areas[i] }, geometry: { type: 'Point', coordinates: all.centroids[i] } })) },
  }
}

/* ---------------- the mappable catalogue: what "Add layer" lists ---------------- */
export type Preset = {
  id: string
  label: string
  hint: string
  type: LayerType
  name: string
  source: Source
  style: Style
  opacity?: number
  blend?: BlendMode
  filter?: Cql
  zoom?: [number, number]
}
export type CatalogueTheme = 'Hazard' | 'Property' | 'People' | 'Access' | 'Transport' | 'Environment' | 'Boundaries' | 'Terrain'
export type CatalogueItem = {
  id: string
  title: string
  summary: string
  theme: CatalogueTheme
  publisher: string
  licence: { label: string; status: LicenceStatus }
  coverage: string
  updated: string
  shape: 'Areas' | 'Grid' | 'Points' | 'Lines' | 'Polygons'
  class: DataClass
  /** the dataset page, where one exists */
  dataset?: string
  presets: Preset[]
}
const TIME = { bind: 'time' as const, match: 'asof' as const }
/** A tint laid over other layers: it darkens on a light basemap and lightens on a dark one. */
export const tint = (scheme: Theme): BlendMode => (scheme === 'dark' ? 'screen' : 'multiply')
const seq = (ramp: RampName, classes = 5): FieldBinding => ({ field: 'value', scale: { type: 'quantile', classes, ramp } })
const OUTLINE: Style['stroke'] = { color: 'ldl.tone.paper', width: 0.5 }
const choropleth = (indicator: string, level: number, parent?: string): Preset => {
  const m = indicatorMeta(indicator)!
  return {
    id: `${indicator}:${level}`,
    label: `By ${LEVELS[level].name.toLowerCase()}`,
    hint: `${LEVELS[level].code} choropleth`,
    type: 'choropleth',
    name: m.label,
    source: { kind: 'indicator', indicator, level, ...(parent ? { parent } : {}), period: m.periods.length > 1 ? TIME : 'latest' },
    style: { fill: seq(m.ramp), stroke: OUTLINE, uncertainty: m.class === 'official' ? 'off' : 'hatch' },
    opacity: 0.9,
  }
}
export const PRESET = {
  floodRaster: (scheme: Theme = 'light'): Preset => ({
    id: 'flood:raster', label: 'Depth surface', hint: 'Raster, classed at 0.15 / 0.5 / 1 / 2 m', type: 'raster', name: 'Flood depth, 1-in-100-year',
    source: { kind: 'grid', grid: 'haz.flood_depth', dims: { return_period: 'RP100' }, period: 'latest' },
    style: { fill: { field: 'value', scale: { type: 'manual', breaks: [0.15, 0.5, 1, 2], ramp: 'ldl.seq.blue' } }, range: [0.15, null], resample: 'linear' },
    opacity: 0.9, blend: tint(scheme),
  }),
  popRaster: (): Preset => ({
    id: 'pop:raster', label: 'Density surface', hint: 'Raster, continuous', type: 'raster', name: 'Population density',
    source: { kind: 'grid', grid: 'pop.density', period: TIME },
    style: { fill: { field: 'value', scale: { type: 'continuous', ramp: 'ldl.seq.violet', transform: 'sqrt', clamp: [40, 9000] } }, range: [40, null], resample: 'linear', 'x-relief': false },
    opacity: 0.85,
  }),
  priceRaster: (): Preset => ({
    id: 'price:raster', label: 'Interpolated surface', hint: 'IDW raster from the listings', type: 'raster', name: 'Asking price surface',
    source: { kind: 'grid', grid: '@demo/re.asking_price_surface', period: 'latest' },
    style: { fill: { field: 'value', scale: { type: 'continuous', ramp: 'ldl.seq.orange', transform: 'log', clamp: [250_000, 9_000_000] } }, range: [null, null], resample: 'linear' },
    opacity: 0.8,
  }),
  driveRaster: (): Preset => ({
    id: 'drive:raster', label: 'Isochrone bands', hint: '15 / 30 / 45 / 60 minutes', type: 'raster', name: 'Drive time to nearest hospital',
    source: { kind: 'grid', grid: 'acc.hospital_drive_time', period: 'latest' },
    style: { fill: { field: 'value', scale: { type: 'manual', breaks: [15, 30, 45], ramp: 'ldl.seq.teal', reverse: true } }, range: [null, 60], resample: 'linear' },
    opacity: 0.8,
  }),
  hillshade: (): Preset => ({ id: 'dem:hillshade', label: 'Shaded relief', hint: 'Hillshade, lit from the north-west', type: 'hillshade', name: 'Terrain relief', source: { kind: 'grid', grid: 'dem', period: 'latest' }, style: { exaggeration: 1 }, opacity: 0.7, blend: 'multiply' }),
  listings: (): Preset => ({
    id: 'listings:point', label: 'Listings', hint: 'Points coloured by price', type: 'point', name: 'Land listings',
    source: { kind: 'features', dataset: '@demo/property-asking-prices-sample', fields: ['price_per_perch', 'land_use', 'listed_year'] },
    style: { size: 3.2, fill: { field: 'price_per_perch', scale: { type: 'quantile', classes: 5, ramp: 'ldl.seq.orange' } }, stroke: { color: 'ldl.tone.paper', width: 0.6 } },
    opacity: 0.9,
  }),
  listingsHeat: (): Preset => ({
    id: 'listings:heat', label: 'Listing density', hint: 'Kernel density of the points', type: 'heatmap', name: 'Listing density',
    source: { kind: 'features', dataset: '@demo/property-asking-prices-sample' }, style: { radius: 22, intensity: 1, weight: null, ramp: 'ldl.seq.orange' }, opacity: 0.85,
  }),
  hospitals: (): Preset => ({
    id: 'hosp:point', label: 'Facilities', hint: 'Sized by beds, coloured by tier', type: 'point', name: 'Health facilities',
    source: { kind: 'features', dataset: 'health-facilities', fields: ['tier', 'beds', 'ownership'] },
    style: { size: { field: 'beds', scale: { type: 'continuous', transform: 'sqrt', range: [2.5, 13] } }, fill: { field: 'tier', scale: { type: 'categorical', ramp: 'ldl.cat.default' } }, stroke: { color: 'ldl.tone.paper', width: 1 } },
    opacity: 0.95,
  }),
  hospitalClusters: (): Preset => ({ id: 'hosp:cluster', label: 'Clustered', hint: 'Counts that split as you zoom', type: 'cluster', name: 'Health facilities (clustered)', source: { kind: 'features', dataset: 'health-facilities' }, style: { radius: 44, fill: 'ldl.tone.teal', aggregate: 'count' }, opacity: 0.95 }),
  towns: (): Preset => ({
    id: 'places:point', label: 'Towns', hint: 'Dots with names', type: 'point', name: 'Towns and cities', source: { kind: 'features', dataset: 'places' },
    style: { size: { field: 'population', scale: { type: 'continuous', transform: 'sqrt', range: [1.8, 5.5] } }, fill: 'ldl.tone.ink', stroke: { color: 'ldl.tone.paper', width: 1.2 }, label: { text: '{name}', size: 11.5 } },
  }),
  rivers: (): Preset => ({ id: 'rivers:line', label: 'Rivers', hint: 'Width by class', type: 'line', name: 'Rivers', source: { kind: 'features', dataset: 'waterways' }, style: { stroke: { color: 'ldl.tone.blue', width: { field: 'class', scale: { type: 'categorical', mapping: { 'Main river': '2.2', River: '1.4', Stream: '0.8' } } } } }, opacity: 0.9 }),
  rail: (): Preset => ({ id: 'rail:line', label: 'Lines', hint: 'Dashed, with a casing', type: 'line', name: 'Railway lines', source: { kind: 'features', dataset: 'railways' }, style: { stroke: { color: 'ldl.tone.ink', width: 1.2, dash: [4.5, 2.4], 'x-casing': true } }, opacity: 0.9 }),
  roads: (): Preset => ({ id: 'roads:line', label: 'Roads', hint: 'Width and colour by class', type: 'line', name: 'Main roads', source: { kind: 'features', dataset: 'roads' }, style: { stroke: { color: 'ldl.tone.grey', width: { field: 'class', scale: { type: 'categorical', mapping: { motorway: '2.4', trunk: '1.9', primary: '1.3', secondary: '0.8' } } } } }, opacity: 0.8 }),
  protectedAreas: (scheme: Theme = 'light'): Preset => ({
    id: 'protected:polygon', label: 'Areas', hint: 'Filled by designation', type: 'polygon', name: 'Protected areas', source: { kind: 'features', dataset: 'protected-areas' },
    style: { fill: { field: 'designation', scale: { type: 'categorical', mapping: { 'National park': 'ldl.tone.green', 'Nature reserve': 'ldl.tone.teal', 'Protected area': 'ldl.tone.grey' } } }, stroke: { color: 'ldl.tone.green', width: 0.8, dash: [4.5, 2.4] } },
    opacity: scheme === 'dark' ? 0.6 : 0.45, blend: tint(scheme),
  }),
  boundaryLines: (level: number, parent?: string): Preset => ({ id: `adm:${level}`, label: LEVELS[level].plural.replace(/^./, (c) => c.toUpperCase()), hint: `${LEVELS[level].code} outlines`, type: 'line', name: LEVELS[level].plural.replace(/^./, (c) => c.toUpperCase()), source: { kind: 'boundaries', scheme: 'admin', level, ...(parent ? { parent } : {}) }, style: { stroke: { color: 'ldl.tone.ink', width: level === 1 ? 1.4 : level === 2 ? 1 : level === 3 ? 0.8 : 0.4 } }, opacity: level >= 3 ? 0.7 : 0.6 }),
  boundaryLabels: (level: number, parent?: string): Preset => ({ id: `adm:${level}:label`, label: 'Names', hint: `${LEVELS[level].name} names`, type: 'label', name: `${LEVELS[level].name} names`, source: { kind: 'boundaries', scheme: 'admin', level, ...(parent ? { parent } : {}) }, style: { label: { text: '{name}', size: level >= 3 ? 10.5 : 12 } }, zoom: level >= 3 ? [8.8, 22] : [5, 22] }),
  choropleth,
}

const LDL = 'Lanka Data Layer (derived)'
const CCBY = { label: 'CC BY 4.0', status: 'attribution' as LicenceStatus }
const ODBL = { label: 'ODbL 1.0', status: 'attribution' as LicenceStatus }
const UNSTATED = { label: 'No stated licence', status: 'unclear' as LicenceStatus }
export const catalogue = (scheme: Theme): CatalogueItem[] => [
  { id: 'flood-hazard', title: 'River flood hazard', summary: 'Modelled depth of a 1-in-100-year river flood on the 500 m grid.', theme: 'Hazard', publisher: LDL, licence: CCBY, coverage: 'Sri Lanka', updated: '2026-10-02', shape: 'Grid', class: 'estimated', presets: [PRESET.floodRaster(scheme)] },
  { id: 'flood-exposure', title: 'Flood exposure', summary: 'Share of residents inside the 1-in-100-year flood extent, for every admin unit.', theme: 'Hazard', publisher: LDL, licence: CCBY, coverage: 'Districts island-wide · DS and GN in Western Province', updated: '2026-10-02', shape: 'Areas', class: 'estimated', dataset: 'flood-exposure', presets: [choropleth('exp.flood_pop_share', 2), choropleth('exp.flood_pop_share', 3, 'LK1'), choropleth('exp.flood_pop_share', 4, 'LK1')] },
  { id: 'property-asking-prices-sample', title: 'Land asking prices (synthetic sample)', summary: 'A demonstration sample of 22,000 land listings with asking price per perch. Not market data.', theme: 'Property', publisher: '@demo workspace', licence: { label: 'Demo data', status: 'unclear' }, coverage: 'Sri Lanka, denser in Western Province', updated: '2026-11-01', shape: 'Points', class: 'synthetic', presets: [PRESET.listings(), PRESET.listingsHeat(), PRESET.priceRaster(), choropleth('@demo/re.asking_price_per_perch_median', 2), choropleth('@demo/re.asking_price_per_perch_median', 3, 'LK1'), choropleth('@demo/re.asking_price_per_perch_median', 4, 'LK1')] },
  { id: 'pop-grid-100m', title: 'Population grid', summary: 'Residents per grid cell, constrained to mapped buildings and reconciled to census totals.', theme: 'People', publisher: LDL, licence: CCBY, coverage: 'Sri Lanka · 2012–2026', updated: '2026-11-18', shape: 'Grid', class: 'estimated', dataset: 'pop-grid-100m', presets: [PRESET.popRaster()] },
  { id: 'census-2024', title: 'Census of Population and Housing', summary: 'Population, density and age structure by province and district.', theme: 'People', publisher: 'Department of Census and Statistics', licence: UNSTATED, coverage: 'Sri Lanka · 2012–2024', updated: '2026-08-30', shape: 'Areas', class: 'official', dataset: 'census-2024', presets: [choropleth('pop.density', 2), choropleth('pop.total', 2), choropleth('age.65plus', 2), choropleth('pop.density', 1)] },
  { id: 'access-health', title: 'Travel time to health facilities', summary: 'Drive time to the nearest base hospital or above, as a surface and by district.', theme: 'Access', publisher: LDL, licence: CCBY, coverage: 'Sri Lanka', updated: '2026-09-14', shape: 'Grid', class: 'estimated', dataset: 'access-health', presets: [PRESET.driveRaster(), choropleth('access.hospital.drive_min', 2)] },
  { id: 'health-facilities', title: 'Health facilities', summary: 'Hospitals by tier and ownership, with bed counts where published.', theme: 'Access', publisher: 'Ministry of Health · OpenStreetMap', licence: ODBL, coverage: 'Sri Lanka', updated: '2026-10-20', shape: 'Points', class: 'official', dataset: 'health-facilities', presets: [PRESET.hospitals(), PRESET.hospitalClusters()] },
  { id: 'railways', title: 'Railway lines', summary: 'The rail network, with a service class and trains per day.', theme: 'Transport', publisher: 'OpenStreetMap contributors', licence: ODBL, coverage: 'Sri Lanka', updated: '2026-11-10', shape: 'Lines', class: 'official', dataset: 'railways', presets: [PRESET.rail()] },
  { id: 'roads', title: 'Road network', summary: 'Motorways, trunk, primary and secondary roads.', theme: 'Transport', publisher: 'OpenStreetMap contributors', licence: ODBL, coverage: 'Sri Lanka', updated: '2026-11-10', shape: 'Lines', class: 'official', dataset: 'roads', presets: [PRESET.roads()] },
  { id: 'waterways', title: 'Rivers', summary: 'Named rivers and streams with a size class.', theme: 'Environment', publisher: 'OpenStreetMap contributors', licence: ODBL, coverage: 'Sri Lanka', updated: '2026-11-10', shape: 'Lines', class: 'official', dataset: 'waterways', presets: [PRESET.rivers()] },
  { id: 'protected-areas', title: 'Protected areas', summary: 'National parks, nature reserves and sanctuaries.', theme: 'Environment', publisher: 'Department of Wildlife Conservation · OpenStreetMap', licence: ODBL, coverage: 'Sri Lanka', updated: '2026-07-05', shape: 'Polygons', class: 'official', dataset: 'protected-areas', presets: [PRESET.protectedAreas(scheme)] },
  { id: 'landcover-10m', title: 'Land cover and built-up surface', summary: 'Tree cover and built-up share by district.', theme: 'Environment', publisher: 'ESA WorldCover · GHSL', licence: CCBY, coverage: 'Sri Lanka · 2012–2025', updated: '2026-06-12', shape: 'Areas', class: 'official', dataset: 'landcover-10m', presets: [choropleth('veg.tree_cover', 2), choropleth('built.share', 2), choropleth('rain.annual', 2)] },
  { id: 'places', title: 'Towns and cities', summary: 'Populated places with a population estimate.', theme: 'Boundaries', publisher: 'OpenStreetMap contributors', licence: ODBL, coverage: 'Sri Lanka', updated: '2026-11-10', shape: 'Points', class: 'official', presets: [PRESET.towns()] },
  { id: 'admin-boundaries', title: 'Administrative boundaries', summary: 'Provinces and districts island-wide; DS and GN divisions for Western Province.', theme: 'Boundaries', publisher: 'Survey Department · OCHA COD', licence: UNSTATED, coverage: 'Sri Lanka', updated: '2026-03-01', shape: 'Lines', class: 'official', dataset: 'admin-boundaries', presets: [PRESET.boundaryLines(2), PRESET.boundaryLines(1), PRESET.boundaryLines(3, 'LK1'), PRESET.boundaryLines(4, 'LK1'), PRESET.boundaryLabels(2), PRESET.boundaryLabels(3, 'LK1')] },
  { id: 'dem', title: 'Terrain', summary: 'Shaded relief from the elevation model.', theme: 'Terrain', publisher: 'Copernicus DEM', licence: CCBY, coverage: 'Sri Lanka', updated: '2025-12-01', shape: 'Grid', class: 'official', presets: [PRESET.hillshade()] },
]
