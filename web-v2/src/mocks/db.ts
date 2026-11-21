// Mock "database". Geometry and trilingual names are real (derived from the
// foundry artifacts in /public/geo); every VALUE in here is artificial but
// plausible. Nothing in this file should be mistaken for published statistics.
import { geoCentroid, geoContains } from 'd3-geo'
import { geo, type AdminFeature } from '@/lib/geo'
import { quantileBreaks } from '@/lib/palettes'
import type { Area, AreaProfile, ChildRow, Indicator, IndicatorMap, Landmark, Meta, Provenance, SearchHit, Stats, ThemeInfo } from '@/api/types'
import { DATASETS, datasetDetail } from './datasets'

/* ---------- deterministic randomness ---------- */
export function hash(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}
export function rng(seed: string) {
  let a = hash(seed)
  return () => {
    a |= 0
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

export const DATA_VERSION = '20261118.1'

/* ---------- district base table (plausible, not authoritative) ---------- */
// pcode: [pop2012, pop2024, areaKm2, rain mm, tree cover %, hospital drive min, built-up %, 65+ %]
const D: Record<string, [number, number, number, number, number, number, number, number]> = {
  LK11: [2324349, 2374461, 699, 2400, 18, 9, 46, 11.8],
  LK12: [2304833, 2433685, 1387, 2100, 28, 12, 31, 11.2],
  LK13: [1221948, 1305552, 1598, 3300, 52, 17, 14, 11.9],
  LK21: [1375382, 1461269, 1940, 1900, 48, 16, 12, 12.4],
  LK22: [484531, 532108, 1993, 1750, 58, 24, 5, 11.1],
  LK23: [711644, 706310, 1741, 2300, 61, 27, 4, 9.6],
  LK31: [1063334, 1096585, 1652, 2500, 49, 16, 12, 13.1],
  LK32: [814048, 837412, 1283, 2300, 46, 17, 11, 13.4],
  LK33: [599903, 671037, 2609, 1100, 41, 26, 5, 10.2],
  LK41: [583882, 594333, 1025, 1250, 14, 15, 15, 13.6],
  LK42: [99570, 124110, 1996, 1000, 55, 38, 2, 7.4],
  LK43: [172115, 172789, 1967, 1400, 62, 33, 2, 8.8],
  LK44: [92238, 122542, 2617, 1400, 71, 44, 1, 7.1],
  LK45: [113510, 136434, 1279, 1300, 48, 36, 2, 7.9],
  LK51: [526567, 595435, 2854, 1700, 38, 28, 5, 7.7],
  LK52: [649402, 744150, 4415, 1750, 47, 31, 4, 7.9],
  LK53: [379541, 442465, 2727, 1650, 56, 30, 3, 7.3],
  LK61: [1618465, 1760829, 4816, 1750, 34, 22, 7, 12.2],
  LK62: [762396, 820470, 3072, 1200, 37, 27, 5, 9.4],
  LK71: [860575, 950102, 7179, 1300, 52, 34, 3, 9.1],
  LK72: [406088, 430511, 3293, 1650, 55, 32, 3, 9.6],
  LK81: [815405, 837250, 2861, 1850, 51, 25, 5, 10.9],
  LK82: [451058, 520940, 5639, 1550, 63, 39, 2, 8.6],
  LK91: [1088007, 1145138, 3275, 3500, 59, 23, 6, 11.6],
  LK92: [840648, 860017, 1693, 2700, 55, 20, 8, 13.0],
}

export const INDICATORS: Indicator[] = [
  { id: 'pop.total', label: 'Population', short: 'Population', unit: 'people', theme: 'people', ramp: 'people', years: range(2012, 2024), description: 'Usual resident population, census counts with intercensal estimates between.', dataset: 'census-2024', higherIs: 'more' },
  { id: 'pop.density', label: 'Population density', short: 'Density', unit: 'per km²', theme: 'people', ramp: 'people', years: range(2012, 2024), description: 'Residents per square kilometre of land area.', dataset: 'census-2024', higherIs: 'more' },
  { id: 'age.65plus', label: 'Residents aged 65 and over', short: 'Aged 65+', unit: '%', theme: 'people', ramp: 'people', years: range(2012, 2024), description: 'Share of usual residents aged 65 or older.', dataset: 'census-2024', higherIs: 'more' },
  { id: 'access.hospital.drive_min', label: 'Drive time to nearest hospital', short: 'Hospital access', unit: 'min', theme: 'access', ramp: 'access', years: range(2018, 2026), description: 'Population-weighted mean drive time to the nearest base hospital or above.', dataset: 'access-health', higherIs: 'worse' },
  { id: 'built.share', label: 'Built-up land', short: 'Built-up', unit: '%', theme: 'land', ramp: 'built', years: range(2012, 2025), description: 'Share of land area classed as built-up surface.', dataset: 'ghsl-built', higherIs: 'more' },
  { id: 'veg.tree_cover', label: 'Tree cover', short: 'Tree cover', unit: '%', theme: 'land', ramp: 'vegetation', years: range(2015, 2025), description: 'Share of land area under tree canopy taller than 5 m.', dataset: 'landcover-10m', higherIs: 'more' },
  { id: 'rain.annual', label: 'Annual rainfall', short: 'Rainfall', unit: 'mm', theme: 'climate', ramp: 'water', years: range(2012, 2025), description: 'Total annual precipitation, area mean.', dataset: 'rain-chirps', higherIs: 'more' },
]

function range(a: number, b: number) {
  return Array.from({ length: b - a + 1 }, (_, i) => a + i)
}

/** Value of an indicator for a district in a year. Smooth trend + small noise. */
function districtValue(ind: string, pcode: string, year: number): number {
  const d = D[pcode]
  if (!d) return 0
  const r = rng(`${ind}:${pcode}:${year}`)
  const wob = (amp: number) => 1 + (r() - 0.5) * amp
  const t = (year - 2012) / 12
  switch (ind) {
    case 'pop.total':
      return Math.round(d[0] + (d[1] - d[0]) * (t + Math.sin(t * Math.PI) * 0.04))
    case 'pop.density':
      return (d[0] + (d[1] - d[0]) * t) / d[2]
    case 'age.65plus':
      return d[7] * (0.72 + 0.28 * t) * wob(0.02)
    case 'access.hospital.drive_min':
      return d[5] * (1.22 - 0.22 * ((year - 2018) / 8)) * wob(0.03)
    case 'built.share':
      return d[6] * (0.78 + 0.22 * ((year - 2012) / 13)) * wob(0.015)
    case 'veg.tree_cover':
      return d[4] * (1.05 - 0.05 * ((year - 2015) / 10)) * wob(0.02)
    case 'rain.annual':
      return d[3] * wob(0.34) * (1 + 0.1 * Math.sin(year * 1.7 + hash(pcode) % 7))
    default:
      return 0
  }
}

/* ---------- areas ---------- */
let areasPromise: Promise<{ list: Area[]; byCode: Map<string, Area>; features: Map<string, AdminFeature> }> | null = null

export function loadAreas() {
  areasPromise ??= Promise.all([geo.adm1(), geo.adm2()]).then(([a1, a2]) => {
    const list: Area[] = []
    const features = new Map<string, AdminFeature>()
    const clean = (s: string) => s.replace(/\s+(District|Province)$/i, '')
    // drop the trailing "district" / "province" word in Sinhala and Tamil too
    const dropLast = (s: string) => {
      const w = s.trim().split(/\s+/)
      return w.length > 1 && /^(දිස්|පළාත|மாவட்|மாகாண)/u.test(w[w.length - 1]) ? w.slice(0, -1).join(' ') : s.trim()
    }
    const cleanSi = dropLast
    const cleanTa = dropLast
    for (const f of [...a1.features, ...a2.features]) {
      const p = f.properties
      const level = p.level as 1 | 2
      const c = geoCentroid(f)
      features.set(p.pcode, f)
      list.push({
        pcode: p.pcode,
        level,
        name: { en: clean(p.name_en), si: cleanSi(p.name_si), ta: cleanTa(p.name_ta) },
        parent: p.parent_pcode,
        centroid: [c[0], c[1]],
        areaKm2: 0,
      })
    }
    list.unshift({ pcode: 'LK', level: 0, name: { en: 'Sri Lanka', si: 'ශ්‍රී ලංකාව', ta: 'இலங்கை' }, parent: null, centroid: [80.7, 7.87], areaKm2: 0 })
    const byCode = new Map(list.map((a) => [a.pcode, a]))
    for (const a of list) a.areaKm2 = districtsOf(a.pcode, list).reduce((s, p) => s + (D[p]?.[2] ?? 0), 0)
    return { list, byCode, features }
  })
  return areasPromise
}

function districtsOf(pcode: string, list?: Area[]): string[] {
  if (pcode === 'LK') return Object.keys(D)
  if (D[pcode]) return [pcode]
  if (pcode.length === 3) return Object.keys(D).filter((k) => k.startsWith(pcode))
  void list
  return []
}

const SUM = new Set(['pop.total'])
/** Aggregate an indicator to any ADM0–2 unit (sum or population-weighted mean). */
export function valueFor(ind: string, pcode: string, year: number): number {
  const ds = districtsOf(pcode)
  if (ds.length === 0) return 0
  if (ds.length === 1 && ds[0] === pcode) return districtValue(ind, pcode, year)
  if (SUM.has(ind)) return ds.reduce((s, p) => s + districtValue(ind, p, year), 0)
  if (ind === 'pop.density') return ds.reduce((s, p) => s + districtValue('pop.total', p, year), 0) / ds.reduce((s, p) => s + D[p][2], 0)
  const w = ds.map((p) => (ind === 'rain.annual' || ind === 'veg.tree_cover' || ind === 'built.share' ? D[p][2] : districtValue('pop.total', p, year)))
  const tot = w.reduce((s, x) => s + x, 0)
  return ds.reduce((s, p, i) => s + districtValue(ind, p, year) * w[i], 0) / tot
}

const clampYear = (ind: Indicator, year: number) => Math.min(ind.years[ind.years.length - 1], Math.max(ind.years[0], year))

export const META: Meta = {
  data_version: DATA_VERSION,
  source: [{ publisher: 'Department of Census and Statistics', title: 'Census of Population and Housing 2024 — preliminary report', url: 'https://www.statistics.gov.lk/' }],
  licences: [{ id: 'lk-gov-unstated', status: 'unclear', attribution: 'Department of Census and Statistics, Sri Lanka' }],
  completeness: { status: 'partial', note: 'GN level not published for this table' },
}

export async function indicatorMap(id: string, level: 1 | 2, year: number): Promise<IndicatorMap> {
  const ind = INDICATORS.find((i) => i.id === id) ?? INDICATORS[0]
  const { list } = await loadAreas()
  const y = clampYear(ind, year)
  const units = list.filter((a) => a.level === level)
  const values = units.map((a) => valueFor(ind.id, a.pcode, y))
  // breaks are fixed across years so the time slider shows change, not re-ranking
  const all = ind.years.flatMap((yy) => units.map((a) => valueFor(ind.id, a.pcode, yy)))
  return { indicator: ind.id, level, year: y, pcodes: units.map((a) => a.pcode), values, breaks: quantileBreaks(all, 7), national: valueFor(ind.id, 'LK', y), meta: META }
}

/* ---------- area profile ---------- */
const AGE_BANDS = ['0–4', '5–9', '10–14', '15–19', '20–24', '25–29', '30–34', '35–39', '40–44', '45–49', '50–54', '55–59', '60–64', '65–69', '70–74', '75–79', '80+']
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const COLOMBO_DS = ['Colombo', 'Kolonnawa', 'Kaduwela', 'Homagama', 'Seethawaka', 'Padukka', 'Maharagama', 'Sri Jayawardenepura Kotte', 'Thimbirigasyaya', 'Dehiwala', 'Ratmalana', 'Moratuwa', 'Kesbewa']

export async function areaProfile(pcodeRaw: string): Promise<AreaProfile> {
  const { list, byCode, features } = await loadAreas()
  const pcode = byCode.has(pcodeRaw.toUpperCase()) ? pcodeRaw.toUpperCase() : 'LK11'
  const area = byCode.get(pcode)!
  const r = rng('profile:' + pcode)
  const breadcrumb: Area[] = []
  for (let a: Area | undefined = area; a; a = a.parent ? byCode.get(a.parent) : undefined) breadcrumb.unshift(a)

  const pop24 = valueFor('pop.total', pcode, 2024)
  const pop12 = valueFor('pop.total', pcode, 2012)
  const hh = Math.round(pop24 / (3.5 + r() * 0.5))
  const obs = (k: string) => `obs_${hash(k + pcode).toString(16).padStart(8, '0')}`
  const headline = [
    { id: 'pop.total', label: 'Population', value: pop24, unit: 'people', year: 2024, status: 'provisional' as const, delta: { value: ((pop24 - pop12) / pop12) * 100, since: 2012 }, observationId: obs('pop') },
    { id: 'pop.density', label: 'Density', value: pop24 / area.areaKm2, unit: 'per km²', year: 2024, status: 'provisional' as const, observationId: obs('den') },
    { id: 'area.land', label: 'Land area', value: area.areaKm2, unit: 'km²', year: 2023, status: 'final' as const, observationId: obs('area') },
    { id: 'hh.total', label: 'Households', value: hh, unit: 'households', year: 2024, status: 'provisional' as const, delta: { value: 9.5 + r() * 6, since: 2012 }, observationId: obs('hh') },
  ]

  const sibLevel = area.level === 0 ? 1 : area.level
  const sibs = list.filter((a) => a.level === sibLevel)
  const ranks = (['pop.density', 'age.65plus', 'access.hospital.drive_min', 'built.share', 'veg.tree_cover', 'rain.annual'] as const).map((id) => {
    const ind = INDICATORS.find((i) => i.id === id)!
    const y = ind.years[ind.years.length - 1]
    const siblings = sibs.map((s) => ({ pcode: s.pcode, name: s.name.en, value: valueFor(id, s.pcode, y) }))
    const value = valueFor(id, pcode, y)
    const rank = siblings.filter((s) => s.value > value).length + 1
    return { indicator: id, label: ind.label, unit: ind.unit, value, rank, of: siblings.length, national: valueFor(id, 'LK', y), siblings }
  })

  const old = valueFor('age.65plus', pcode, 2024) / 11
  const shape = (i: number, ageing: number) => {
    const base = [7.6, 8.2, 8.4, 7.9, 7.2, 7.0, 7.3, 7.4, 6.9, 6.4, 6.0, 5.5, 4.8, 3.7, 2.6, 1.7, 1.4][i]
    return base * (i < 4 ? 1.12 - ageing * 0.2 : i > 11 ? 0.72 + ageing * 0.42 : 1)
  }
  const norm = (ageing: number) => AGE_BANDS.reduce((s, _, i) => s + shape(i, ageing), 0)
  const pyramid = AGE_BANDS.map((age, i) => {
    const now = (shape(i, old) / norm(old)) * 100
    const then = (shape(i, old * 0.62) / norm(old * 0.62)) * 100
    const fShare = 0.5 + (i > 11 ? 0.035 + (i - 11) * 0.012 : 0.006)
    return { age, m: now * (1 - fShare), f: now * fShare, m0: then * (1 - fShare + 0.004), f0: then * (fShare - 0.004) }
  })

  const hospital = valueFor('access.hospital.drive_min', pcode, 2026)
  const access = [
    { id: 'hospital', label: 'Hospital (base or above)', minutes: hospital, national: valueFor('access.hospital.drive_min', 'LK', 2026), within30: Math.max(22, Math.min(99, 118 - hospital * 2.1)) },
    { id: 'school', label: 'Secondary school', minutes: hospital * 0.42 + 2, national: 11.4, within30: Math.max(60, Math.min(99.6, 104 - hospital * 0.5)) },
    { id: 'station', label: 'Railway station', minutes: hospital * 1.3 + 6 + r() * 8, national: 34.2, within30: Math.max(8, Math.min(96, 110 - hospital * 2.6)) },
    { id: 'market', label: 'Town centre', minutes: hospital * 0.7 + 3, national: 17.9, within30: Math.max(40, Math.min(99, 112 - hospital * 1.3)) },
  ]

  const tree = valueFor('veg.tree_cover', pcode, 2025)
  const built = valueFor('built.share', pcode, 2025)
  const crop = Math.max(6, (100 - tree - built) * (0.52 + r() * 0.1))
  const water = 2 + r() * 5
  const land = [
    { id: 'tree', label: 'Tree cover', share: tree },
    { id: 'crop', label: 'Cropland', share: crop },
    { id: 'built', label: 'Built-up', share: built },
    { id: 'water', label: 'Water', share: water },
    { id: 'other', label: 'Grass, shrub, bare', share: Math.max(1, 100 - tree - built - crop - water) },
  ]

  const rain = valueFor('rain.annual', pcode, 2020)
  const wet = Math.min(1, Math.max(0, (rain - 1000) / 2400))
  const climate = MONTHS.map((month, i) => {
    const sw = Math.exp(-((i - 4.6) ** 2) / 3.2) * wet
    const ne = Math.exp(-((i - 10.2) ** 2) / 2.6) + Math.exp(-((i + 1.8) ** 2) / 2.6)
    const share = 0.035 + sw * 0.13 + ne * (0.15 - wet * 0.06)
    const v = rain * share * 1.25
    return { month, rain: v * (0.86 + r() * 0.3), lo: v * 0.72, hi: v * 1.3, temp: 27.4 - wet * 1.6 + Math.sin(((i - 3) / 12) * Math.PI * 2) * 1.4 }
  })

  const e = [0.44 + r() * 0.1, 0.27 + r() * 0.08, 0.1 + r() * 0.05, 0.05 + r() * 0.03]
  const rest = Math.max(0.03, 1 - e.reduce((s, x) => s + x, 0))
  const tot = e.reduce((s, x) => s + x, 0) + rest
  const votes = pop24 * 0.52
  const elections = ['Party A', 'Party B', 'Party C', 'Party D', 'Others'].map((party, i) => {
    const share = ((i < 4 ? e[i] : rest) / tot) * 100
    return { party, share, votes: Math.round((votes * share) / 100) }
  })

  // children: real units for ADM0/ADM1, plausible DS divisions for a district
  let children: ChildRow[]
  if (area.level < 2) {
    children = list
      .filter((a) => a.parent === pcode)
      .map((c) => childRow(c.pcode, c.name, valueFor('pop.total', c.pcode, 2024), c.areaKm2, valueFor('access.hospital.drive_min', c.pcode, 2026), (y) => valueFor('pop.total', c.pcode, y)))
  } else {
    const places = await geo.places()
    const f = features.get(pcode)!
    const inside = places.filter((p) => geoContains(f, [p.lon, p.lat]))
    const names: { en: string; si: string; ta: string }[] =
      pcode === 'LK11'
        ? COLOMBO_DS.map((en) => {
            const p = places.find((x) => x.name === en)
            return { en, si: p?.si ?? '', ta: p?.ta ?? '' }
          })
        : inside.slice(0, 14).map((p) => ({ en: p.name, si: p.si ?? '', ta: p.ta ?? '' }))
    while (names.length < 7) names.push({ en: `${area.name.en} ${['North', 'South', 'East', 'West', 'Central', 'Town', 'Rural'][names.length % 7]}`, si: '', ta: '' })
    const weights = names.map((_, i) => 1 / (i + 1.6) + r() * 0.12)
    const wsum = weights.reduce((s, x) => s + x, 0)
    children = names.map((name, i) => {
      const share = weights[i] / wsum
      const p = pop24 * share
      const km = area.areaKm2 * (0.4 / names.length + (0.6 * (1 - share)) / (names.length - 1))
      const g = 0.02 + (r() - 0.35) * 0.16
      return childRow(`${pcode}${String(i * 3 + 3).padStart(2, '0')}`, name, p, km, hospital * (0.5 + r() * 1.1), (y) => p * (1 - g * ((2024 - y) / 12)))
    })
  }
  children.sort((a, b) => b.population - a.population)

  const neighbours = sibs
    .filter((s) => s.pcode !== pcode)
    .map((s) => ({ s, d: Math.hypot(s.centroid[0] - area.centroid[0], s.centroid[1] - area.centroid[1]) }))
    .sort((a, b) => a.d - b.d)
    .slice(0, 5)
    .map(({ s }) => ({ pcode: s.pcode, name: s.name.en }))

  const postal = children.slice(0, 8).map((c, i) => ({ code: String(Math.floor(hash(pcode) % 70000) + 10000 + i * 30).padStart(5, '0'), name: c.name.en }))

  return {
    area,
    breadcrumb,
    headline,
    ranks,
    pyramid,
    popSeries: range(2012, 2024).map((year) => ({ year, value: valueFor('pop.total', pcode, year) })),
    access,
    land,
    built: [1975, 1990, 2000, 2010, 2015, 2020, 2025, 2030].map((year) => ({ year, share: built * (0.28 + 0.72 * Math.pow((year - 1975) / 50, 1.35)) })),
    climate,
    elections,
    children,
    postal,
    neighbours,
    meta: META,
  }
}

function childRow(pcode: string, name: ChildRow['name'], population: number, km: number, hospitalMin: number, series: (y: number) => number): ChildRow {
  const spark = [2012, 2014, 2016, 2018, 2020, 2022, 2024].map(series)
  return { pcode, name, population: Math.round(population), density: population / Math.max(1, km), change: ((spark[6] - spark[0]) / spark[0]) * 100, hospitalMin, spark }
}

/* ---------- provenance ---------- */
export async function provenance(observationId: string): Promise<Provenance> {
  const r = rng(observationId)
  const page = 18 + Math.floor(r() * 60)
  const districts = ['Colombo', 'Gampaha', 'Kalutara', 'Kandy', 'Matale']
  const cells = districts.map((d) => {
    const total = Math.round((D[`LK${districts.indexOf(d) < 3 ? '1' + (districts.indexOf(d) + 1) : '2' + (districts.indexOf(d) - 2)}`] ?? D.LK11)[1])
    const m = Math.round(total * 0.484)
    return [total.toLocaleString('en'), m.toLocaleString('en'), (total - m).toLocaleString('en')]
  })
  return {
    observationId,
    indicator: 'Population',
    area: 'Colombo District (LK11)',
    value: '2,374,461',
    verbatim: '2,374,461',
    document: { title: 'Census of Population and Housing 2024 — Preliminary Report', publisher: 'Department of Census and Statistics', url: 'https://www.statistics.gov.lk/', published: '2025-04-07', pages: 112 },
    locator: { page, table: 'Table 2.1', row: 'Colombo', column: 'Total population' },
    sha256: Array.from({ length: 64 }, () => '0123456789abcdef'[Math.floor(r() * 16)]).join(''),
    retrieved: '2026-11-17T02:14:09Z',
    licence: { id: 'lk-gov-unstated', label: 'No stated licence — reuse permission unclear', status: 'unclear' },
    review: { method: 'Table extraction, two-pass, reconciled to printed column totals', reviewer: 'maintainer', date: '2026-11-17' },
    crop: { header: ['District', 'Total', 'Male', 'Female'], rows: districts.map((d, i) => ({ label: d, cells: cells[i], hit: i === 0 ? 0 : undefined })) },
  }
}

/* ---------- catalogue-wide ---------- */
export const THEMES: ThemeInfo[] = [
  { id: 'people', label: 'People', blurb: 'Census counts, age and sex, households, down to GN division.', datasets: 4, indicators: 148, ramp: 'people' },
  { id: 'boundaries', label: 'Boundaries', blurb: 'Official p-coded units ADM0–ADM4, postal and electoral geographies.', datasets: 5, indicators: 0, ramp: 'water' },
  { id: 'access', label: 'Access', blurb: 'Travel time to hospitals, schools and stations for every grid cell.', datasets: 3, indicators: 36, ramp: 'access' },
  { id: 'land', label: 'Land and built environment', blurb: 'Land cover at 10 m, built-up surface since 1975, protected areas.', datasets: 4, indicators: 41, ramp: 'vegetation' },
  { id: 'climate', label: 'Climate and hazard', blurb: 'Rainfall normals and anomalies, flood exposure by division.', datasets: 3, indicators: 29, ramp: 'water' },
  { id: 'economy', label: 'Economy', blurb: 'Night lights, labour force and poverty where published.', datasets: 3, indicators: 54, ramp: 'built' },
  { id: 'elections', label: 'Elections', blurb: 'Results by polling division, 1982 to the latest count.', datasets: 2, indicators: 22, ramp: 'people' },
  { id: 'transport', label: 'Transport', blurb: 'Rail lines and stations, the road network, ports and airports.', datasets: 3, indicators: 12, ramp: 'built' },
]

export const STATS: Stats = { datasets: 27, areas: 14417, cells: 6_561_204, sources: 143, observations: 2_184_930, dataVersion: DATA_VERSION, lastRelease: '2026-11-18' }

export const LANDMARKS: Landmark[] = [
  { id: 'sigiriya', name: { en: 'Sigiriya', si: 'සීගිරිය', ta: 'சிகிரியா' }, kind: 'Rock fortress · 5th c.', lon: 80.7603, lat: 7.957, pcode: 'LK2207', district: 'Matale', facts: [{ label: 'DS population', value: '74,870' }, { label: 'Tree cover', value: '61.4%' }, { label: 'Hospital', value: '21 min' }] },
  { id: 'sri-pada', name: { en: 'Sri Pada', si: 'ශ්‍රී පාදය', ta: 'சிவனொளிபாதமலை' }, kind: "Adam's Peak · 2,243 m", lon: 80.4994, lat: 6.8096, pcode: 'LK9115', district: 'Ratnapura', facts: [{ label: 'Annual rain', value: '4,870 mm' }, { label: 'Tree cover', value: '83.0%' }, { label: 'Density', value: '112 /km²' }] },
  { id: 'lotus-tower', name: { en: 'Lotus Tower', si: 'නෙළුම් කුළුණ', ta: 'தாமரைக் கோபுரம்' }, kind: 'Colombo · 351.5 m', lon: 79.8583, lat: 6.9271, pcode: 'LK1103', district: 'Colombo', facts: [{ label: 'DS population', value: '312,640' }, { label: 'Density', value: '17,344 /km²' }, { label: 'Built-up', value: '93.2%' }] },
  { id: 'ruwanwelisaya', name: { en: 'Ruwanwelisaya', si: 'රුවන්වැලිසෑය', ta: 'ருவன்வெலிசாய' }, kind: 'Stupa · Anuradhapura', lon: 80.3964, lat: 8.35, pcode: 'LK7127', district: 'Anuradhapura', facts: [{ label: 'DS population', value: '98,210' }, { label: 'Annual rain', value: '1,285 mm' }, { label: 'Hospital', value: '11 min' }] },
  { id: 'temple-tooth', name: { en: 'Temple of the Tooth', si: 'ශ්‍රී දළදා මාළිගාව', ta: 'தலதா மாளிகை' }, kind: 'Kandy · World Heritage', lon: 80.6413, lat: 7.2936, pcode: 'LK2127', district: 'Kandy', facts: [{ label: 'DS population', value: '158,420' }, { label: 'Density', value: '2,640 /km²' }, { label: 'Elevation', value: '500 m' }] },
  { id: 'galle-fort', name: { en: 'Galle Fort', si: 'ගාලු කොටුව', ta: 'காலி கோட்டை' }, kind: 'Lighthouse · 1939', lon: 80.217, lat: 6.0247, pcode: 'LK3127', district: 'Galle', facts: [{ label: 'DS population', value: '101,750' }, { label: 'Built-up', value: '38.9%' }, { label: 'Station', value: '4 min' }] },
  { id: 'nine-arch', name: { en: 'Nine Arch Bridge', si: 'ආරුක්කු නවයේ පාලම', ta: 'ஒன்பது வளைவுப் பாலம்' }, kind: 'Ella · railway viaduct', lon: 81.0608, lat: 6.8768, pcode: 'LK8124', district: 'Badulla', facts: [{ label: 'DS population', value: '47,930' }, { label: 'Tree cover', value: '57.8%' }, { label: 'Elevation', value: '1,041 m' }] },
  { id: 'nallur', name: { en: 'Nallur Kandaswamy Kovil', si: 'නල්ලූර් කන්දස්වාමි කෝවිල', ta: 'நல்லூர் கந்தசுவாமி கோவில்' }, kind: 'Jaffna · gopuram', lon: 80.0295, lat: 9.6745, pcode: 'LK4133', district: 'Jaffna', facts: [{ label: 'DS population', value: '68,340' }, { label: 'Density', value: '4,010 /km²' }, { label: 'Annual rain', value: '1,240 mm' }] },
  { id: 'trinco', name: { en: 'Trincomalee Harbour', si: 'ත්‍රිකුණාමලය වරාය', ta: 'திருகோணமலை துறைமுகம்' }, kind: 'Deep-water harbour', lon: 81.2152, lat: 8.565, pcode: 'LK5324', district: 'Trincomalee', facts: [{ label: 'DS population', value: '108,420' }, { label: 'Harbour area', value: '1,630 ha' }, { label: 'Hospital', value: '8 min' }] },
]

export async function search(q: string): Promise<SearchHit[]> {
  const { list } = await loadAreas()
  const places = await geo.places()
  const needle = q.trim().toLowerCase()
  const hits: SearchHit[] = []
  const levelName = ['Country', 'Province', 'District', 'DS division']
  for (const a of list) {
    if (!needle || a.name.en.toLowerCase().includes(needle) || a.pcode.toLowerCase().includes(needle) || a.name.si.includes(q) || a.name.ta.includes(q))
      hits.push({ kind: 'area', id: a.pcode, title: a.name.en, subtitle: `${levelName[a.level]} · ${a.name.si} · ${a.name.ta}`, href: `/area/${a.pcode}`, code: a.pcode })
  }
  for (const d of DATASETS) if (!needle || d.title.toLowerCase().includes(needle) || d.id.includes(needle)) hits.push({ kind: 'dataset', id: d.id, title: d.title, subtitle: `${d.publisher} · ${d.vintage}`, href: `/data/${d.id}` })
  for (const i of INDICATORS) if (!needle || i.label.toLowerCase().includes(needle) || i.id.includes(needle)) hits.push({ kind: 'indicator', id: i.id, title: i.label, subtitle: `Indicator · ${i.unit}`, href: `/explore?indicator=${i.id}`, code: i.id })
  if (needle.length >= 2) for (const p of places) if (p.name.toLowerCase().includes(needle)) hits.push({ kind: 'place', id: p.name, title: p.name, subtitle: `${p.kind === 'city' ? 'City' : 'Town'} · ${p.si ?? ''}`, href: `/explore?lon=${p.lon}&lat=${p.lat}` })
  return hits.slice(0, needle ? 24 : 60)
}

export { DATASETS, datasetDetail }
