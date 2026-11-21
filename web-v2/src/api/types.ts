// Shapes mirror the planned REST v2 payloads (docs: revamp plan §6.1).
import type { RampId } from '@/lib/palettes'

export type Trilingual = { en: string; si: string; ta: string }
export type LicenceStatus = 'open' | 'attribution' | 'unclear'

export type Area = {
  pcode: string
  level: 0 | 1 | 2 | 3
  name: Trilingual
  parent: string | null
  centroid: [number, number]
  areaKm2: number
}

export type Indicator = {
  id: string
  label: string
  short: string
  unit: string
  theme: ThemeId
  ramp: RampId
  years: number[]
  description: string
  dataset: string
  higherIs: 'more' | 'better' | 'worse'
}

export type IndicatorMap = {
  indicator: string
  level: 1 | 2
  year: number
  pcodes: string[]
  values: number[]
  breaks: number[]
  national: number
  meta: Meta
}

export type Meta = {
  data_version: string
  source: { publisher: string; title: string; url: string }[]
  licences: { id: string; status: LicenceStatus; attribution: string }[]
  completeness: { status: 'complete' | 'partial'; note?: string }
}

export type Headline = {
  id: string
  label: string
  value: number
  unit: string
  year: number
  status: 'final' | 'provisional'
  delta?: { value: number; since: number }
  observationId: string
}

export type RankItem = {
  indicator: string
  label: string
  unit: string
  value: number
  rank: number
  of: number
  national: number
  siblings: { pcode: string; name: string; value: number }[]
}

export type PyramidRow = { age: string; m: number; f: number; m0: number; f0: number }
export type ChildRow = { pcode: string; name: Trilingual; population: number; density: number; change: number; hospitalMin: number; spark: number[] }

export type AreaProfile = {
  area: Area
  breadcrumb: Area[]
  headline: Headline[]
  ranks: RankItem[]
  pyramid: PyramidRow[]
  popSeries: { year: number; value: number }[]
  access: { id: string; label: string; minutes: number; national: number; within30: number }[]
  land: { id: string; label: string; share: number }[]
  built: { year: number; share: number }[]
  climate: { month: string; rain: number; lo: number; hi: number; temp: number }[]
  elections: { party: string; votes: number; share: number }[]
  children: ChildRow[]
  postal: { code: string; name: string }[]
  neighbours: { pcode: string; name: string }[]
  meta: Meta
}

export type Provenance = {
  observationId: string
  indicator: string
  area: string
  value: string
  verbatim: string
  document: { title: string; publisher: string; url: string; published: string; pages: number }
  locator: { page: number; table: string; row: string; column: string }
  sha256: string
  retrieved: string
  licence: { id: string; label: string; status: LicenceStatus }
  review: { method: string; reviewer: string; date: string }
  crop: { rows: { label: string; cells: string[]; hit?: number }[]; header: string[] }
}

export type ThemeId = 'people' | 'boundaries' | 'access' | 'land' | 'climate' | 'economy' | 'elections' | 'transport'
export type ThemeInfo = { id: ThemeId; label: string; blurb: string; datasets: number; indicators: number; ramp: RampId }

export type Dataset = {
  id: string
  title: string
  summary: string
  theme: ThemeId
  publisher: string
  publisherType: 'government' | 'multilateral' | 'community' | 'research' | 'derived'
  licence: { id: string; label: string; status: LicenceStatus }
  cadence: string
  levels: string[]
  rows: number
  indicators: number
  vintage: string
  updated: string
  freshness: 'fresh' | 'due' | 'stale'
  formats: string[]
  sizeMb: number
  spark: number[]
}

export type DatasetDetail = Dataset & {
  description: string
  contains: string[]
  method: string[]
  limits: string[]
  sources: { n: number; title: string; publisher: string; url: string; licence: string; status: LicenceStatus; retrieved: string; sha256: string }[]
  columns: { name: string; type: string; note: string }[]
  preview: (string | number)[][]
  downloads: { format: string; label: string; size: string; href: string }[]
  versions: { dsv: string; date: string; note: string; added: number; changed: number; removed: number }[]
  usedIn: { label: string; href: string }[]
  citation: string
}

export type SearchHit = { kind: 'area' | 'dataset' | 'indicator' | 'place'; id: string; title: string; subtitle: string; href: string; code?: string }

export type Stats = { datasets: number; areas: number; cells: number; sources: number; observations: number; dataVersion: string; lastRelease: string }

export type Landmark = {
  id: string
  name: Trilingual
  kind: string
  lon: number
  lat: number
  pcode: string
  district: string
  facts: { label: string; value: string }[]
}
