// TanStack Query option factories — the only place components get data from.
import { keepPreviousData, queryOptions } from '@tanstack/react-query'
import { get } from './client'
import type { Area, AreaProfile, Dataset, DatasetDetail, Indicator, IndicatorMap, Landmark, Provenance, SearchHit, Stats, ThemeInfo } from './types'

const HOUR = 60 * 60 * 1000

export const statsQuery = () => queryOptions({ queryKey: ['status'], queryFn: () => get<Stats>('/v2/status'), staleTime: HOUR })
export const themesQuery = () => queryOptions({ queryKey: ['themes'], queryFn: () => get<ThemeInfo[]>('/v2/themes'), staleTime: HOUR })
export const landmarksQuery = () => queryOptions({ queryKey: ['landmarks'], queryFn: () => get<Landmark[]>('/v2/landmarks'), staleTime: HOUR })
export const areasQuery = (level?: number) => queryOptions({ queryKey: ['areas', level ?? 'all'], queryFn: () => get<Area[]>('/v2/areas', { level }), staleTime: HOUR })
export const areaProfileQuery = (pcode: string) => queryOptions({ queryKey: ['area', pcode], queryFn: () => get<AreaProfile>(`/v2/areas/${pcode}`), staleTime: HOUR })
export const indicatorsQuery = () => queryOptions({ queryKey: ['indicators'], queryFn: () => get<Indicator[]>('/v2/indicators'), staleTime: HOUR })
export const indicatorMapQuery = (id: string, level: 1 | 2, period: number) =>
  queryOptions({
    queryKey: ['indicator-map', id, level, period],
    queryFn: () => get<IndicatorMap>(`/v2/indicators/${id}/map`, { level, period }),
    staleTime: HOUR,
    placeholderData: keepPreviousData,
  })
export const indicatorSeriesQuery = (id: string, geo: string[]) =>
  queryOptions({
    queryKey: ['indicator-series', id, geo.join(',')],
    queryFn: () => get<{ pcode: string; points: { year: number; value: number }[] }[]>(`/v2/indicators/${id}/series`, { geo: geo.join(',') }),
    staleTime: HOUR,
    enabled: geo.length > 0,
  })
export const datasetsQuery = () => queryOptions({ queryKey: ['datasets'], queryFn: () => get<Dataset[]>('/v2/catalog'), staleTime: HOUR })
export const datasetQuery = (id: string) => queryOptions({ queryKey: ['dataset', id], queryFn: () => get<DatasetDetail>(`/v2/datasets/${id}`), staleTime: HOUR })
export const searchQuery = (q: string) => queryOptions({ queryKey: ['search', q], queryFn: () => get<SearchHit[]>('/v2/search', { q }), staleTime: HOUR, placeholderData: keepPreviousData })
export const provenanceQuery = (observationId: string | null) =>
  queryOptions({ queryKey: ['provenance', observationId], queryFn: () => get<Provenance>(`/v2/observations/${observationId}/provenance`), staleTime: HOUR, enabled: !!observationId })
