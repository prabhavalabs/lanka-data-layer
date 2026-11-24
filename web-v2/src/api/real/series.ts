// Observations of one indicator, for the dataset chart: dataset detail (indicators, sources, version),
// the dimension values an indicator takes, the periods it has, a frame (one value per unit) and series
// (one line per unit). Everything is read from the live engine; there are no mock fallbacks, because a
// chart of invented numbers would be worse than no chart.
//
//   GET /v2/datasets/{id}                     indicators with dims, levels, unit; sources; current dsv
//   GET /v2/frame?indicators&level&dims&periods
//   GET /v2/indicators/{id}/series?geo&dims&from&to
import { keepPreviousData, queryOptions } from '@tanstack/react-query'
import { ApiError, getEnvelope, getJson } from '@/api/http'
import { units, type Unit } from './names'

const HOUR = 60 * 60 * 1000
export const NATIONAL = 'LK'

export type RawIndicator = {
  id: string
  dataset_id: string
  label: string
  unit: string
  unit_label: string
  unit_mult: number
  dims: string[]
  geo_levels: string[]
  period_type: string
  freq: string
  measure_type: string
  aggregation: string
  class: string
}
export type ChartSource = { id: string; publisher: string; title: string; homepage: string | null; licence_id: string | null; attribution?: string | null }
export type DatasetMeta = {
  id: string
  kind: 'indicators' | 'features' | 'boundaries'
  title: string
  current: { dsv: string; vintage: string | null; published_at: string | null; row_count: number | null } | null
  sources: ChartSource[]
  licences: { id: string; status: string }[]
  indicators: RawIndicator[]
  dataVersion: string | null
}

export const datasetMetaQuery = (id: string) =>
  queryOptions({
    queryKey: ['chart', 'dataset', id],
    queryFn: async (): Promise<DatasetMeta> => {
      const { payload, meta } = await getEnvelope<Omit<DatasetMeta, 'dataVersion'>>(`/v2/datasets/${id}`)
      return { ...payload, indicators: payload.indicators ?? [], sources: payload.sources ?? [], licences: payload.licences ?? [], dataVersion: meta?.data_version ?? null }
    },
    staleTime: HOUR,
    retry: false,
  })

/** The admin levels an indicator has values on (`national` is 0). Raster and point levels are not units of a table. */
export function levelsOf(ind: RawIndicator): number[] {
  const out = new Set<number>()
  for (const g of ind.geo_levels) {
    if (g === 'national') out.add(0)
    else if (/^adm[1-4]$/.test(g)) out.add(Number(g.slice(3)))
  }
  return [...out].sort((a, b) => a - b)
}

/* ------------------------------ units ------------------------------ */
export const unitsQuery = (level: number) => queryOptions({ queryKey: ['chart', 'units', level], queryFn: (): Promise<Unit[]> => units(level), staleTime: HOUR })

/* ------------------------------ dims ------------------------------ */
export type DimCombos = { values: Record<string, string[]>; combos: Record<string, string>[] }
/** "a=x;b=y" in the canonical (sorted) order the engine uses. */
export const dimsKey = (d: Record<string, string>) =>
  Object.keys(d)
    .sort()
    .map((k) => `${k}=${d[k]}`)
    .join(';')
const parseDims = (s: string): Record<string, string> => Object.fromEntries(s.split(';').filter(Boolean).map((p) => [p.slice(0, p.indexOf('=')), p.slice(p.indexOf('=') + 1)]))

/**
 * The dimension values an indicator takes. The engine has no endpoint for this: a frame request for a value
 * that cannot match answers `DIMS_NO_MATCH` with the valid values and every combination that exists, which
 * is what is read here. See the gaps in the track report.
 */
export const dimCombosQuery = (ind: RawIndicator, levels: number[]) =>
  queryOptions({
    queryKey: ['chart', 'dims', ind.id],
    queryFn: async (): Promise<DimCombos> => {
      for (const level of levels) {
        try {
          await getJson('/v2/frame', { indicators: ind.id, level, dims: `${ind.dims[0]}=__probe__` })
        } catch (e) {
          const p = e instanceof ApiError ? e.problem?.problem : undefined
          const combos = p?.combinations as string[] | undefined
          const values = p?.valid as Record<string, string[]> | undefined
          if (values && combos?.length) return { values, combos: combos.map(parseDims) }
        }
      }
      return { values: {}, combos: [] }
    },
    staleTime: HOUR,
    retry: false,
    enabled: ind.dims.length > 0,
  })

/* ------------------------------ series ------------------------------ */
export type SeriesPoint = { period: string; value: number | null; basis: string; status: string }
export type RawSeries = { geo: string; name: string; dims: string; points: SeriesPoint[] }

export const seriesQuery = (indicator: string, geos: string[], dims: string, enabled = true) =>
  queryOptions({
    queryKey: ['chart', 'series', indicator, geos.join(','), dims],
    queryFn: async () => {
      const { payload } = await getEnvelope<{ series: RawSeries[] }>(`/v2/indicators/${indicator}/series`, { geo: geos.join(','), dims })
      return payload.series
    },
    staleTime: HOUR,
    enabled: enabled && geos.length > 0,
    placeholderData: keepPreviousData,
    retry: false,
  })

/* ------------------------------ frame ------------------------------ */
export type FrameColumn = { indicator: string; dims: Record<string, string> | null; period: string; unit: string; unit_mult: number; basis: string; status: string; class: string; values: (number | null)[] }
export type Frame = { codes: string[]; columns: FrameColumn[]; sources: { id: string; publisher: string; title: string; url: string }[]; dsvs: { dataset: string; dsv: string; vintage?: string | null }[]; dataVersion: string | null }

export const frameQuery = (indicator: string, level: number, dims: string, period: string | undefined, enabled = true) =>
  queryOptions({
    queryKey: ['chart', 'frame', indicator, level, dims, period],
    queryFn: async (): Promise<Frame> => {
      const { payload, meta } = await getEnvelope<{ codes: string[]; columns: FrameColumn[] }>('/v2/frame', { indicators: indicator, level, dims, periods: period })
      return { codes: payload.codes, columns: payload.columns, sources: meta?.source ?? [], dsvs: meta?.dataset_versions ?? [], dataVersion: meta?.data_version ?? null }
    },
    staleTime: HOUR,
    enabled: enabled && !!period,
    placeholderData: keepPreviousData,
    retry: false,
  })
