// What the client knows about the engine's datasets: labels, units, ramps and how each is best drawn.
//
// The numbers, geometry and attribution come from the API at run time (resolve → frames, value tiles,
// points, vector tiles); this file only names things and picks defaults. Ids are the engine's own
// (`haz.flood_area_share` + dims, grid layer ids, feature dataset ids), so a document written against
// it resolves on any deployment of the engine.
import type { LicenceStatus } from '@/api/types'
import type { Theme } from '@/lib/hooks'
import type { Cql, FieldBinding, Source, Style } from '@/lib/map-spec'
import type { CatalogueItem, FeatureSetMeta, GridMeta, IndicatorMeta, Preset } from '@/mocks/layers'

/* ---------------- indicators on admin units ---------------- */
const ind = (m: Omit<IndicatorMeta, 'short' | 'real'> & { short?: string }): IndicatorMeta => ({ short: m.label, ...m, real: true })
export const REAL_INDICATORS: IndicatorMeta[] = [
  ind({ id: 'haz.flood_area_share:rp100', indicator: 'haz.flood_area_share', dims: { return_period: 'rp100' }, label: 'Flood area share, 1-in-100-year', short: 'Flood area', unit: '%', ramp: 'ldl.seq.blue', periods: ['2024'], levels: [1, 2, 3, 4], dataset: 'jrc-river-flood-hazard', class: 'estimated', description: 'Share of the unit’s area inside the modelled 1-in-100-year river flood extent (JRC global river flood hazard, about 90 m). River flooding only: no rain or coastal flooding, no flood defences.' }),
  ind({ id: 'haz.flood_area_share:rp10', indicator: 'haz.flood_area_share', dims: { return_period: 'rp10' }, label: 'Flood area share, 1-in-10-year', short: 'Flood area (10 y)', unit: '%', ramp: 'ldl.seq.blue', periods: ['2024'], levels: [1, 2, 3, 4], dataset: 'jrc-river-flood-hazard', class: 'estimated', description: 'Share of the unit’s area inside the modelled 1-in-10-year river flood extent (JRC).' }),
  ind({ id: 'derived.per_km2:pop', indicator: 'derived.per_km2', dims: { source_indicator: 'census24.pop_total' }, label: 'Population density', short: 'Density', unit: 'per km²', ramp: 'ldl.seq.violet', periods: ['2024'], levels: [1, 2, 3], dataset: 'derived-per-capita-indicators', class: 'official', description: 'Enumerated population (Census 2024) per square kilometre of the unit’s geodesic area.' }),
  ind({ id: 'census24.pop_total', indicator: 'census24.pop_total', dims: { sex: 'total' }, label: 'Population, Census 2024', short: 'Population', unit: 'people', ramp: 'ldl.seq.violet', periods: ['2024'], levels: [2, 3], dataset: 'dcs-census-2024-population', class: 'official', description: 'Enumerated population on census night 2024 (Department of Census and Statistics).' }),
  ind({ id: 'census24.sex_ratio', indicator: 'census24.sex_ratio', label: 'Males per 100 females', short: 'Sex ratio', unit: '', ramp: 'ldl.seq.teal', periods: ['2024'], levels: [2, 3], dataset: 'dcs-census-2024-population', class: 'official', description: 'Males per 100 females, Census 2024.' }),
  ind({ id: 'pop.grid_total', indicator: 'pop.grid_total', label: 'Population, modelled (WorldPop 2025)', short: 'Modelled population', unit: 'people', ramp: 'ldl.seq.violet', periods: ['2025'], levels: [1, 2, 3, 4], dataset: 'worldpop-pop-grid', class: 'estimated', description: 'Sum of the WorldPop 100 m constrained population grid over the unit.' }),
  ind({ id: 'lc.class_share:50', indicator: 'lc.class_share', dims: { lc_class: '50' }, label: 'Built-up share of land', short: 'Built-up', unit: '%', ramp: 'ldl.seq.orange', periods: ['2021'], levels: [1, 2, 3, 4], dataset: 'worldcover-land-cover', class: 'estimated', description: 'Share of the unit classed as built-up in ESA WorldCover 2021 (10 m).' }),
  ind({ id: 'lc.class_share:10', indicator: 'lc.class_share', dims: { lc_class: '10' }, label: 'Tree cover share of land', short: 'Tree cover', unit: '%', ramp: 'ldl.seq.green', periods: ['2021'], levels: [1, 2, 3, 4], dataset: 'worldcover-land-cover', class: 'estimated', description: 'Share of the unit classed as tree cover in ESA WorldCover 2021 (10 m).' }),
  ind({ id: 'terrain.elevation_mean', indicator: 'terrain.elevation_mean', label: 'Mean elevation', short: 'Elevation', unit: 'm', ramp: 'ldl.seq.green', periods: ['2015'], levels: [1, 2, 3, 4], dataset: 'copernicus-dem-terrain', class: 'official', description: 'Area-weighted mean height above sea level (Copernicus DEM GLO-30).' }),
  ind({ id: 'terrain.slope_mean', indicator: 'terrain.slope_mean', label: 'Mean slope', short: 'Slope', unit: '°', ramp: 'ldl.seq.teal', periods: ['2015'], levels: [1, 2, 3, 4], dataset: 'copernicus-dem-terrain', class: 'official', description: 'Area-weighted mean slope in degrees (Copernicus DEM GLO-30).' }),
  ind({ id: 'clim.wc_bio:BIO12', indicator: 'clim.wc_bio', dims: { bio_var: 'BIO12' }, label: 'Annual rainfall, 1970–2000 normal', short: 'Rainfall', unit: 'mm', ramp: 'ldl.seq.blue', periods: ['1970'], levels: [1, 2, 3, 4], dataset: 'worldclim-climate-normals', class: 'estimated', description: 'Mean annual precipitation of the 1970–2000 climate normal (WorldClim 2.1).' }),
]

/* ---------------- grids drawn as value tiles ---------------- */
export const REAL_GRIDS: GridMeta[] = [
  { id: 'haz.flood_depth.rp100', label: 'Flood depth, 1-in-100-year', unit: 'm', ramp: 'ldl.seq.blue', periods: ['2024'], dataset: 'jrc-river-flood-hazard', class: 'estimated', description: 'Modelled depth of a 1-in-100-year river flood (JRC, about 90 m; the deepest pixel in each grid cell).', real: true },
  { id: 'haz.flood_depth.rp10', label: 'Flood depth, 1-in-10-year', unit: 'm', ramp: 'ldl.seq.blue', periods: ['2024'], dataset: 'jrc-river-flood-hazard', class: 'estimated', description: 'Modelled depth of a 1-in-10-year river flood (JRC).', real: true },
  { id: 'haz.flood_depth.rp500', label: 'Flood depth, 1-in-500-year', unit: 'm', ramp: 'ldl.seq.blue', periods: ['2024'], dataset: 'jrc-river-flood-hazard', class: 'estimated', description: 'Modelled depth of a 1-in-500-year river flood (JRC).', real: true },
  { id: 'worldpop.population', label: 'People per grid cell (WorldPop 2025)', unit: 'people', ramp: 'ldl.seq.violet', periods: ['2025'], dataset: 'worldpop-pop-grid', class: 'estimated', description: 'Modelled residents per 0.001° cell (about 110 m), constrained to mapped settlement.', real: true },
  { id: 'terrain.elevation', label: 'Elevation', unit: 'm', ramp: 'ldl.seq.green', periods: ['2015'], dataset: 'copernicus-dem-terrain', class: 'official', description: 'Height above sea level (Copernicus DEM GLO-30, resampled to the 0.001° grid).', real: true },
  { id: 'terrain.slope', label: 'Slope', unit: '°', ramp: 'ldl.seq.teal', periods: ['2015'], dataset: 'copernicus-dem-terrain', class: 'official', description: 'Mean slope in degrees.', real: true },
  { id: 'clim.bio12', label: 'Annual rainfall, 1970–2000', unit: 'mm', ramp: 'ldl.seq.blue', periods: ['1970'], dataset: 'worldclim-climate-normals', class: 'estimated', description: 'Mean annual precipitation, 1970–2000 normal (WorldClim 2.1, about 1 km).', real: true },
  { id: 'worldcover.class', label: 'Land cover class (ESA WorldCover 2021)', unit: '', ramp: 'ldl.cat.default', periods: ['2021'], dataset: 'worldcover-land-cover', class: 'estimated', description: 'The most common WorldCover class in each grid cell.', real: true },
]
/** ESA WorldCover legend (its own published colours). */
export const WORLDCOVER: Record<string, { label: string; color: string }> = {
  '10': { label: 'Tree cover', color: '#2f7d32' },
  '20': { label: 'Shrubland', color: '#c9a227' },
  '30': { label: 'Grassland', color: '#d8d45c' },
  '40': { label: 'Cropland', color: '#d98fe0' },
  '50': { label: 'Built-up', color: '#d6453d' },
  '60': { label: 'Bare', color: '#a8a8a8' },
  '80': { label: 'Water', color: '#3f7fc7' },
  '90': { label: 'Wetland', color: '#2c9aa3' },
  '95': { label: 'Mangroves', color: '#33b27a' },
}

/* ---------------- OpenStreetMap feature sets ---------------- */
const cats = (...k: string[]) => ({ id: 'kind', label: 'Kind', kind: 'category' as const, categories: k })
export const REAL_FEATURES: FeatureSetMeta[] = [
  { id: 'osm-pois-amenities', label: 'Facilities and amenities', geometry: 'point', title: 'name', class: 'official', real: true, fields: [cats('hospital', 'clinic', 'school', 'college', 'university', 'place_of_worship', 'bank', 'government_office', 'aerodrome'), { id: 'beds', label: 'Beds', kind: 'number', unit: 'beds', domain: [0, 2000] }] },
  { id: 'osm-rail-network', label: 'Railway lines and stations', geometry: 'line', title: 'name', class: 'official', real: true, fields: [cats('rail', 'station', 'halt')] },
  { id: 'osm-road-network', label: 'Road network', geometry: 'line', title: 'name', class: 'official', real: true, fields: [cats('motorway', 'trunk', 'primary', 'secondary', 'tertiary', 'unclassified', 'residential')] },
  { id: 'osm-waterways-waterbodies', label: 'Rivers, canals, tanks and reservoirs', geometry: 'line', title: 'name', class: 'official', real: true, fields: [cats('river', 'stream', 'canal', 'reservoir', 'lake', 'lagoon', 'river_area')] },
  { id: 'osm-protected-areas', label: 'Protected areas', geometry: 'polygon', title: 'name', class: 'official', real: true, fields: [cats('national_park', 'nature_reserve', 'protected_area')] },
  { id: 'osm-parks-greenspace', label: 'Parks and green space', geometry: 'polygon', title: 'name', class: 'official', real: true, fields: [cats('park', 'garden', 'playground', 'recreation_ground')] },
  { id: 'osm-tourism-attractions', label: 'Attractions and viewpoints', geometry: 'point', title: 'name', class: 'official', real: true, fields: [cats('heritage', 'waterfall', 'beach', 'viewpoint', 'museum', 'theme_park', 'nature', 'religious', 'park', 'other')] },
  { id: 'osm-trails-cycling', label: 'Walking trails and cycle routes', geometry: 'line', title: 'name', class: 'official', real: true, fields: [cats('path', 'hiking_route', 'bicycle_route', 'cycleway', 'footway', 'bridleway')] },
]
/** A kind of a dataset can have its own geometry: stations are points of the rail network. */
export const POINT_KINDS = new Set(['station', 'halt', 'hospital', 'clinic', 'school', 'college', 'university', 'place_of_worship', 'bank', 'government_office', 'aerodrome'])

/* ---------------- presets: how "Add layer" offers each of them ---------------- */
const TIME = 'latest' as const
const OUTLINE: Style['stroke'] = { color: 'ldl.tone.paper', width: 0.3 }
const fill = (ramp: string, classes = 5, type: 'quantile' | 'jenks' = 'quantile'): FieldBinding => ({ field: 'value', scale: { type, classes, ramp } })
const LEVEL = ['', 'province', 'district', 'DS division', 'GN division']
const LEVEL_CODE = ['ADM0', 'ADM1', 'ADM2', 'ADM3', 'ADM4']
export const metaOf = (key: string) => REAL_INDICATORS.find((m) => m.id === key)!
export function realChoropleth(key: string, level: number, opts: { ramp?: string; type?: 'quantile' | 'jenks'; parent?: string } = {}): Preset {
  const m = metaOf(key)
  const source: Source = { kind: 'indicator', indicator: m.indicator!, ...(m.dims ? { dims: { ...m.dims } } : {}), level, ...(opts.parent ? { parent: opts.parent } : {}), period: TIME }
  return { id: `${key}:${level}`, label: `By ${LEVEL[level]}`, hint: `${LEVEL_CODE[level]} choropleth`, type: 'choropleth', name: m.label, source, style: { fill: fill(opts.ramp ?? m.ramp, 5, opts.type), stroke: level >= 4 ? { color: 'ldl.tone.paper', width: 0 } : OUTLINE, uncertainty: 'off' }, opacity: 0.9 }
}
const grid = (id: string): Source => ({ kind: 'grid', grid: id, period: TIME })
const tint = (scheme: Theme) => (scheme === 'dark' ? ('screen' as const) : ('multiply' as const))
const inKinds = (...k: string[]): Cql => ({ op: 'in', args: [{ property: 'kind' }, k] })

export const REAL_PRESET = {
  floodDepth: (scheme: Theme = 'light', rp: 10 | 100 | 500 = 100): Preset => ({
    id: `flood:rp${rp}`, label: `1-in-${rp}-year depth`, hint: 'Value tiles, classed at 0.15 / 0.5 / 1 / 2 m', type: 'raster', name: `Flood depth, 1-in-${rp}-year`,
    source: grid(`haz.flood_depth.rp${rp}`), style: { fill: { field: 'value', scale: { type: 'manual', breaks: [0.5, 1, 2, 4], ramp: 'ldl.seq.blue' } }, range: [0.15, null], resample: 'nearest' },
    opacity: 0.85, blend: tint(scheme),
  }),
  population: (): Preset => ({
    id: 'worldpop:raster', label: 'Population surface', hint: 'People per 110 m cell, continuous', type: 'raster', name: 'Population (WorldPop 2025)',
    source: grid('worldpop.population'), style: { fill: { field: 'value', scale: { type: 'continuous', ramp: 'ldl.seq.violet', transform: 'sqrt', clamp: [0.5, 120] } }, range: [0.5, null], resample: 'linear' },
    opacity: 0.85,
  }),
  elevation: (): Preset => ({ id: 'dem:elev', label: 'Elevation surface', hint: 'Continuous, 0–2,000 m', type: 'raster', name: 'Elevation', source: grid('terrain.elevation'), style: { fill: { field: 'value', scale: { type: 'continuous', ramp: 'ldl.seq.green', transform: 'sqrt', clamp: [0, 2200] } }, range: [1, null], resample: 'linear' }, opacity: 0.8 }),
  slope: (): Preset => ({ id: 'dem:slope', label: 'Slope surface', hint: 'Classed at 5 / 10 / 20 / 30°', type: 'raster', name: 'Slope', source: grid('terrain.slope'), style: { fill: { field: 'value', scale: { type: 'manual', breaks: [5, 10, 20, 30], ramp: 'ldl.seq.teal' } }, range: [2, null], resample: 'nearest' }, opacity: 0.8 }),
  rainfall: (): Preset => ({ id: 'clim:rain', label: 'Rainfall surface', hint: 'Annual mean, continuous', type: 'raster', name: 'Annual rainfall, 1970–2000', source: grid('clim.bio12'), style: { fill: { field: 'value', scale: { type: 'continuous', ramp: 'ldl.seq.blue', clamp: [900, 4900] } }, range: [null, null], resample: 'linear' }, opacity: 0.8 }),
  landCover: (): Preset => ({
    id: 'wc:class', label: 'Land cover classes', hint: 'Most common class per cell', type: 'raster', name: 'Land cover (WorldCover 2021)', source: grid('worldcover.class'),
    style: { fill: { field: 'value', scale: { type: 'categorical', mapping: Object.fromEntries(Object.entries(WORLDCOVER).map(([k, v]) => [k, v.color])) } }, range: [null, null], resample: 'nearest' }, opacity: 0.75,
  }),
  hillshade: (): Preset => ({ id: 'dem:hillshade', label: 'Shaded relief', hint: 'Hillshade from the DEM tiles, lit from the north-west', type: 'hillshade', name: 'Terrain relief', source: { kind: 'grid', grid: 'dem', period: TIME }, style: { exaggeration: 0.6 }, opacity: 0.7, blend: 'multiply' }),
  hospitals: (): Preset => ({ id: 'osm:hospital', label: 'Hospitals', hint: 'OpenStreetMap, 891 points', type: 'point', name: 'Hospitals', source: { kind: 'features', dataset: 'osm-pois-amenities', featureKind: 'hospital' }, style: { size: 3.6, fill: 'ldl.tone.orange', stroke: { color: 'ldl.tone.paper', width: 1 } }, opacity: 0.95 }),
  hospitalClusters: (): Preset => ({ id: 'osm:hospital:cluster', label: 'Hospitals, clustered', hint: 'Counts that split as you zoom', type: 'cluster', name: 'Hospitals (clustered)', source: { kind: 'features', dataset: 'osm-pois-amenities', featureKind: 'hospital' }, style: { radius: 44, fill: 'ldl.tone.orange', aggregate: 'count' }, opacity: 0.95 }),
  clinics: (): Preset => ({ id: 'osm:clinic', label: 'Clinics', hint: 'OpenStreetMap points', type: 'point', name: 'Clinics', source: { kind: 'features', dataset: 'osm-pois-amenities', featureKind: 'clinic' }, style: { size: 2.6, fill: 'ldl.tone.teal', stroke: { color: 'ldl.tone.paper', width: 0.8 } }, opacity: 0.9 }),
  schools: (): Preset => ({ id: 'osm:school', label: 'Schools', hint: '5,051 points, columnar (Arrow)', type: 'point', name: 'Schools', source: { kind: 'features', dataset: 'osm-pois-amenities', featureKind: 'school' }, style: { size: 2.2, fill: 'ldl.tone.violet', stroke: { color: 'ldl.tone.paper', width: 0.6 } }, opacity: 0.9 }),
  amenityHeat: (): Preset => ({ id: 'osm:amenity:heat', label: 'Amenity density', hint: '24,888 points as a heatmap', type: 'heatmap', name: 'Facilities and amenities, density', source: { kind: 'features', dataset: 'osm-pois-amenities' }, style: { radius: 18, intensity: 1, weight: null, ramp: 'ldl.seq.orange' }, opacity: 0.85 }),
  rail: (): Preset => ({ id: 'osm:rail', label: 'Railway lines', hint: 'Dashed, with a casing', type: 'line', name: 'Railway', source: { kind: 'features', dataset: 'osm-rail-network' }, filter: inKinds('rail'), style: { stroke: { color: 'ldl.tone.ink', width: 1.3, dash: [4.5, 2.4], 'x-casing': true } }, opacity: 0.9 }),
  stations: (): Preset => ({ id: 'osm:station', label: 'Stations', hint: 'Points with names', type: 'point', name: 'Railway stations', source: { kind: 'features', dataset: 'osm-rail-network', featureKind: 'station' }, style: { size: 2.6, fill: 'ldl.tone.ink', stroke: { color: 'ldl.tone.paper', width: 1.2 } }, opacity: 0.95 }),
  roads: (): Preset => ({ id: 'osm:roads', label: 'Main roads', hint: 'Vector tiles, width by class', type: 'line', name: 'Main roads', source: { kind: 'features', dataset: 'osm-road-network' }, filter: inKinds('motorway', 'trunk', 'primary', 'secondary'), style: { stroke: { color: 'ldl.tone.grey', width: { field: 'kind', scale: { type: 'categorical', mapping: { motorway: 2.4, trunk: 1.8, primary: 1.2, secondary: 0.8 } } } } }, opacity: 0.8 }),
  rivers: (): Preset => ({ id: 'osm:rivers', label: 'Rivers and canals', hint: 'Vector tiles, width by kind', type: 'line', name: 'Rivers and canals', source: { kind: 'features', dataset: 'osm-waterways-waterbodies' }, filter: inKinds('river', 'canal'), style: { stroke: { color: 'ldl.tone.blue', width: { field: 'kind', scale: { type: 'categorical', mapping: { river: 1.3, canal: 0.6 } } } } }, opacity: 0.85 }),
  waterBodies: (scheme: Theme = 'light'): Preset => ({ id: 'osm:tanks', label: 'Tanks and reservoirs', hint: 'Polygons', type: 'polygon', name: 'Tanks, reservoirs and lagoons', source: { kind: 'features', dataset: 'osm-waterways-waterbodies' }, filter: inKinds('reservoir', 'lake', 'lagoon'), style: { fill: 'ldl.tone.blue', stroke: { color: 'ldl.tone.blue', width: 0.4 } }, opacity: scheme === 'dark' ? 0.55 : 0.45 }),
  protectedAreas: (scheme: Theme = 'light'): Preset => ({
    id: 'osm:protected', label: 'Areas', hint: 'Filled by designation', type: 'polygon', name: 'Protected areas', source: { kind: 'features', dataset: 'osm-protected-areas' },
    style: { fill: { field: 'kind', scale: { type: 'categorical', mapping: { national_park: 'ldl.tone.green', nature_reserve: 'ldl.tone.teal', protected_area: 'ldl.tone.grey' } } }, stroke: { color: 'ldl.tone.green', width: 0.8, dash: [4.5, 2.4] } },
    opacity: scheme === 'dark' ? 0.6 : 0.45, blend: tint(scheme),
  }),
  parks: (): Preset => ({ id: 'osm:parks', label: 'Parks', hint: 'Vector tiles from zoom 9', type: 'polygon', name: 'Parks and green space', source: { kind: 'features', dataset: 'osm-parks-greenspace' }, style: { fill: 'ldl.tone.green', stroke: { color: 'ldl.tone.green', width: 0.5 } }, opacity: 0.5 }),
  attractions: (): Preset => ({ id: 'osm:attractions', label: 'Attractions', hint: 'Columnar points, coloured by kind', type: 'point', name: 'Attractions', source: { kind: 'features', dataset: 'osm-tourism-attractions' }, style: { size: 2.6, fill: { field: 'kind', scale: { type: 'categorical', ramp: 'ldl.cat.default' } }, stroke: { color: 'ldl.tone.paper', width: 0.6 } }, opacity: 0.9 }),
  trails: (): Preset => ({ id: 'osm:trails', label: 'Trails', hint: 'Vector tiles, dotted', type: 'line', name: 'Walking trails and cycle routes', source: { kind: 'features', dataset: 'osm-trails-cycling' }, style: { stroke: { color: 'ldl.tone.green', width: 0.9, dash: [0.4, 2] } }, opacity: 0.85 }),
  boundaryLines: (level: number): Preset => {
    const plural = ['Country', 'Provinces', 'Districts', 'DS divisions', 'GN divisions'][level]
    return { id: `adm:${level}`, label: plural, hint: `${LEVEL_CODE[level]} outlines from the boundary tiles`, type: 'line', name: plural, source: { kind: 'boundaries', scheme: 'admin', level }, style: { stroke: { color: 'ldl.tone.ink', width: level === 1 ? 1.4 : level === 2 ? 1 : level === 3 ? 0.6 : 0.3 } }, opacity: level >= 3 ? 0.55 : 0.6 }
  },
  boundaryLabels: (level: number): Preset => ({ id: `adm:${level}:label`, label: 'Names', hint: `${LEVEL[level]} names`, type: 'label', name: `${LEVEL[level].replace(/^./, (c) => c.toUpperCase())} names`, source: { kind: 'boundaries', scheme: 'admin', level }, style: { label: { text: '{name}', size: level >= 3 ? 10.5 : 12 } }, zoom: level >= 3 ? [8.6, 22] : [5, 22] }),
}

/* ---------------- the catalogue rows ---------------- */
const CCBY = { label: 'CC BY 4.0', status: 'attribution' as LicenceStatus }
const ODBL = { label: 'ODbL 1.0', status: 'attribution' as LicenceStatus }
const IGO = { label: 'CC BY-IGO 3.0', status: 'attribution' as LicenceStatus }
const UNCLEAR = { label: 'Permission unclear', status: 'unclear' as LicenceStatus }
const OSM = 'OpenStreetMap contributors (Geofabrik)'
/** `updated` is filled from `/v2/status/datasets` when the catalogue loads. */
export const realCatalogue = (scheme: Theme): CatalogueItem[] => [
  { id: 'jrc-river-flood-hazard', title: 'River flood hazard', summary: 'Modelled flood depth for 10- to 500-year river floods, and the share of each unit’s area inside the flood extent.', theme: 'Hazard', publisher: 'European Commission Joint Research Centre', licence: CCBY, coverage: 'Sri Lanka · ADM1–ADM4 and a 0.001° grid', updated: '', shape: 'Grid', class: 'estimated', dataset: 'jrc-river-flood-hazard', presets: [REAL_PRESET.floodDepth(scheme, 100), REAL_PRESET.floodDepth(scheme, 10), REAL_PRESET.floodDepth(scheme, 500), realChoropleth('haz.flood_area_share:rp100', 3), realChoropleth('haz.flood_area_share:rp100', 2), realChoropleth('haz.flood_area_share:rp100', 4), realChoropleth('haz.flood_area_share:rp10', 3)] },
  { id: 'dcs-census-2024-population', title: 'Census 2024: population', summary: 'Enumerated population and its density, by district and DS division.', theme: 'People', publisher: 'Department of Census and Statistics', licence: UNCLEAR, coverage: 'Sri Lanka · ADM1–ADM3', updated: '', shape: 'Areas', class: 'official', dataset: 'dcs-census-2024-population', presets: [realChoropleth('derived.per_km2:pop', 3), realChoropleth('derived.per_km2:pop', 2), realChoropleth('derived.per_km2:pop', 1), realChoropleth('census24.pop_total', 3), realChoropleth('census24.sex_ratio', 3)] },
  { id: 'worldpop-pop-grid', title: 'Population grid (WorldPop)', summary: 'Modelled residents per 100 m cell, constrained to settlement, 2025.', theme: 'People', publisher: 'WorldPop, University of Southampton', licence: CCBY, coverage: 'Sri Lanka · 0.001° grid and ADM1–ADM4', updated: '', shape: 'Grid', class: 'estimated', dataset: 'worldpop-pop-grid', presets: [REAL_PRESET.population(), realChoropleth('pop.grid_total', 3)] },
  { id: 'worldcover-land-cover', title: 'Land cover (ESA WorldCover)', summary: 'Eleven land-cover classes at 10 m, and their shares per admin unit.', theme: 'Environment', publisher: 'European Space Agency', licence: CCBY, coverage: 'Sri Lanka · 2021', updated: '', shape: 'Grid', class: 'estimated', dataset: 'worldcover-land-cover', presets: [REAL_PRESET.landCover(), realChoropleth('lc.class_share:50', 3, { type: 'jenks' }), realChoropleth('lc.class_share:10', 3)] },
  { id: 'copernicus-dem-terrain', title: 'Terrain (Copernicus DEM)', summary: 'Shaded relief, elevation and slope from the 30 m Copernicus DEM.', theme: 'Terrain', publisher: 'European Space Agency / Airbus', licence: { label: 'Copernicus DEM licence', status: 'attribution' }, coverage: 'Sri Lanka', updated: '', shape: 'Grid', class: 'official', dataset: 'copernicus-dem-terrain', presets: [REAL_PRESET.hillshade(), REAL_PRESET.elevation(), REAL_PRESET.slope(), realChoropleth('terrain.elevation_mean', 3), realChoropleth('terrain.slope_mean', 3)] },
  { id: 'worldclim-climate-normals', title: 'Climate normals (WorldClim)', summary: 'Rainfall and temperature normals, 1970–2000.', theme: 'Environment', publisher: 'WorldClim', licence: CCBY, coverage: 'Sri Lanka · about 1 km', updated: '', shape: 'Grid', class: 'estimated', dataset: 'worldclim-climate-normals', presets: [REAL_PRESET.rainfall(), realChoropleth('clim.wc_bio:BIO12', 3)] },
  { id: 'osm-pois-amenities', title: 'Facilities and amenities', summary: 'Hospitals, clinics, schools, banks, places of worship and more from OpenStreetMap.', theme: 'Access', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 24,888 points', updated: '', shape: 'Points', class: 'official', dataset: 'osm-pois-amenities', presets: [REAL_PRESET.hospitals(), REAL_PRESET.hospitalClusters(), REAL_PRESET.clinics(), REAL_PRESET.schools(), REAL_PRESET.amenityHeat()] },
  { id: 'osm-rail-network', title: 'Railway lines and stations', summary: 'The rail network with its stations and halts.', theme: 'Transport', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 1,601 features', updated: '', shape: 'Lines', class: 'official', dataset: 'osm-rail-network', presets: [REAL_PRESET.rail(), REAL_PRESET.stations()] },
  { id: 'osm-road-network', title: 'Road network', summary: 'Every highway from expressways to tracks, served as vector tiles.', theme: 'Transport', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 237,699 features', updated: '', shape: 'Lines', class: 'official', dataset: 'osm-road-network', presets: [REAL_PRESET.roads()] },
  { id: 'osm-waterways-waterbodies', title: 'Rivers, canals and tanks', summary: 'Waterways and water bodies, including the ancient irrigation tanks.', theme: 'Environment', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 15,581 features', updated: '', shape: 'Lines', class: 'official', dataset: 'osm-waterways-waterbodies', presets: [REAL_PRESET.rivers(), REAL_PRESET.waterBodies(scheme)] },
  { id: 'osm-protected-areas', title: 'Protected areas', summary: 'National parks, nature reserves and sanctuaries mapped in OpenStreetMap.', theme: 'Environment', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 91 areas', updated: '', shape: 'Polygons', class: 'official', dataset: 'osm-protected-areas', presets: [REAL_PRESET.protectedAreas(scheme)] },
  { id: 'osm-parks-greenspace', title: 'Parks and green space', summary: 'Parks, gardens, playgrounds and recreation grounds.', theme: 'Environment', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 2,378 areas', updated: '', shape: 'Polygons', class: 'official', dataset: 'osm-parks-greenspace', presets: [REAL_PRESET.parks()] },
  { id: 'osm-tourism-attractions', title: 'Attractions and viewpoints', summary: 'Heritage sites, waterfalls, beaches, viewpoints and museums.', theme: 'Environment', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 2,537 points', updated: '', shape: 'Points', class: 'official', dataset: 'osm-tourism-attractions', presets: [REAL_PRESET.attractions()] },
  { id: 'osm-trails-cycling', title: 'Trails and cycle routes', summary: 'Hiking paths, footways and cycleways.', theme: 'Transport', publisher: OSM, licence: ODBL, coverage: 'Sri Lanka · 25,831 features', updated: '', shape: 'Lines', class: 'official', dataset: 'osm-trails-cycling', presets: [REAL_PRESET.trails()] },
  { id: 'cod-ab-lka-boundaries', title: 'Administrative boundaries', summary: 'Provinces, districts, DS and GN divisions with OCHA p-codes, as vector tiles.', theme: 'Boundaries', publisher: 'UN OCHA · Survey Department · geoBoundaries', licence: IGO, coverage: 'Sri Lanka · ADM0–ADM4', updated: '', shape: 'Lines', class: 'official', dataset: 'cod-ab-lka-boundaries', presets: [REAL_PRESET.boundaryLines(2), REAL_PRESET.boundaryLines(1), REAL_PRESET.boundaryLines(3), REAL_PRESET.boundaryLines(4), REAL_PRESET.boundaryLabels(2), REAL_PRESET.boundaryLabels(3)] },
]
