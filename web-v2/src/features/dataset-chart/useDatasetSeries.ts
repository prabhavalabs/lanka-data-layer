// One dataset's observations, ready to draw: the indicator, admin level, dimension values and period the
// reader has chosen, and the frame / series / composition they give. Reusable wherever a dataset is shown
// (the dataset page now, the map workspace's inspector later); it holds no markup.
import { useQuery } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { dimCombosQuery, dimsKey, datasetMetaQuery, frameQuery, levelsOf, NATIONAL, seriesQuery, unitsQuery, type DimCombos, type Frame, type RawIndicator, type RawSeries } from '@/api/real/series'
import { fillPeriods, medianOf, TOTALS, type Ranked } from './format'

export type ChartView = 'ranking' | 'trend' | 'spread' | 'makeup' | 'number'
export const MAX_COMPARE = 5

export type Segment = { id: string; label: string; value: number | null }
export type Makeup = { id: string; name: string; segments: Segment[]; total: number }

const ADDITIVE = new Set(['count', 'sum', 'share'])

/** The combination to start on: the one with the most "total" values, then the first listed. */
function startDims(c: DimCombos): Record<string, string> {
  let best = c.combos[0] ?? {}
  let score = -1
  for (const combo of c.combos) {
    const s = Object.values(combo).filter((v) => TOTALS.has(v)).length
    if (s > score) (best = combo), (score = s)
  }
  return best
}

const defaultLevel = (levels: number[]) => (levels.includes(2) ? 2 : (levels.find((l) => l > 0) ?? 0))

export function useDatasetSeries(datasetId: string, initial: { indicator?: string } = {}) {
  const meta = useQuery(datasetMetaQuery(datasetId))
  const indicators = useMemo(() => (meta.data?.indicators ?? []).filter((i) => levelsOf(i).length > 0), [meta.data])

  const [indId, setIndId] = useState<string | undefined>(initial.indicator)
  const ind: RawIndicator | undefined = indicators.find((i) => i.id === indId) ?? indicators[0]
  const levels = useMemo(() => (ind ? levelsOf(ind) : []), [ind])
  const [userLevel, setLevel] = useState<number | null>(null)
  const level = userLevel !== null && levels.includes(userLevel) ? userLevel : defaultLevel(levels)
  const hasNational = levels.includes(0)

  /* ------- dimensions ------- */
  const combosQ = useQuery(ind ? dimCombosQuery(ind, levels) : { queryKey: ['chart', 'dims', 'none'], queryFn: async (): Promise<DimCombos> => ({ values: {}, combos: [] }), enabled: false })
  const combos: DimCombos = combosQ.data ?? { values: {}, combos: [] }
  const [dimPick, setDimPick] = useState<{ ind: string; dims: Record<string, string> } | null>(null)
  const dimsReady = !ind || ind.dims.length === 0 || combosQ.isSuccess || combosQ.isError
  const dims: Record<string, string> = dimPick && dimPick.ind === ind?.id ? dimPick.dims : startDims(combos)
  const setDim = (key: string, value: string) => {
    if (!ind) return
    // keep the other choices where the combination exists, otherwise take the closest one that does
    let best: Record<string, string> | undefined
    let score = -1
    for (const c of combos.combos) {
      if (c[key] !== value) continue
      const s = Object.keys(c).filter((k) => k !== key && c[k] === dims[k]).length
      if (s > score) (best = c), (score = s)
    }
    setDimPick({ ind: ind.id, dims: best ?? { ...dims, [key]: value } })
  }
  const dimStr = dimsKey(dims)

  /* ------- units and periods ------- */
  const unitsQ = useQuery(unitsQuery(level))
  const names = useMemo(() => new Map((unitsQ.data ?? []).map((u) => [u.pcode, u.name_en])), [unitsQ.data])
  const periodGeos = level === 0 ? [NATIONAL] : (unitsQ.data ?? []).slice(0, 25).map((u) => u.pcode)
  const periodsQ = useQuery(seriesQuery(ind?.id ?? '', periodGeos, dimStr, !!ind && dimsReady))
  const periods = useMemo(() => [...new Set((periodsQ.data ?? []).flatMap((s) => s.points.filter((p) => p.value !== null).map((p) => p.period)))].sort(), [periodsQ.data])
  const [userPeriod, setPeriod] = useState<string | null>(null)
  const period = userPeriod && periods.includes(userPeriod) ? userPeriod : periods[periods.length - 1]

  /* ------- composition: the dimensions a stacked bar can split on ------- */
  const splitCandidates = useMemo(() => {
    if (!ind || !ADDITIVE.has(ind.measure_type)) return []
    return ind.dims.filter((d) => (combos.values[d] ?? []).filter((v) => !TOTALS.has(v)).length >= 2 && (combos.values[d] ?? []).length <= 24)
  }, [ind, combos])
  const [userSplit, setSplit] = useState<string | null>(null)
  const splitDim = userSplit && splitCandidates.includes(userSplit) ? userSplit : splitCandidates[0]

  /* ------- view ------- */
  const trendable = periods.length > 2
  const views: ChartView[] = useMemo(() => {
    const v: ChartView[] = []
    if (level > 0) v.push('ranking')
    if (trendable) v.push('trend')
    if (level > 0) v.push('spread')
    if (splitDim) v.push('makeup')
    if (level === 0 && !trendable) v.unshift('number')
    return v
  }, [level, trendable, splitDim])
  const [userView, setView] = useState<ChartView | null>(null)
  const view: ChartView | undefined = userView && views.includes(userView) ? userView : views[0]

  /* ------- selection for comparison ------- */
  const [picked, setPicked] = useState<{ level: number; ids: string[] }>({ level: 0, ids: [] })
  const selected = picked.level === level ? picked.ids : []
  const toggle = (id: string) =>
    setPicked((p) => {
      const cur = p.level === level ? p.ids : []
      return { level, ids: cur.includes(id) ? cur.filter((x) => x !== id) : cur.length >= MAX_COMPARE ? [...cur.slice(1), id] : [...cur, id] }
    })
  const clearSelected = () => setPicked({ level, ids: [] })

  /* ------- the frame: one value per unit, for the chosen period ------- */
  const wantFrame = !!ind && dimsReady && !!period && (view === 'ranking' || view === 'spread' || view === 'number' || view === 'makeup')
  const frameQ = useQuery(frameQuery(ind?.id ?? '', level, dimStr, period, wantFrame))
  const natQ = useQuery(frameQuery(ind?.id ?? '', 0, dimStr, period, wantFrame && level > 0 && hasNational))
  const pick = (f: Frame | undefined) => f?.columns.find((c) => dimsKey(c.dims ?? {}) === dimStr) ?? f?.columns[0]
  const col = pick(frameQ.data)
  const rows: Ranked[] = useMemo(() => (frameQ.data && col ? frameQ.data.codes.map((c, i) => ({ pcode: c, name: c === NATIONAL ? 'Sri Lanka' : (names.get(c) ?? c), value: col.values[i] ?? null })) : []), [frameQ.data, col, names])
  const sorted = useMemo(() => [...rows].sort((a, b) => (a.value === null ? 1 : b.value === null ? -1 : b.value - a.value)), [rows])
  const national = level === 0 ? (rows[0]?.value ?? null) : (pick(natQ.data)?.values[0] ?? null)
  const values = useMemo(() => rows.flatMap((r) => (r.value === null ? [] : [r.value])), [rows])
  const median = useMemo(() => (values.length ? medianOf([...values].sort((a, b) => a - b)) : null), [values])

  /* ------- the trend: one line per picked unit, the national one beside them ------- */
  const trendGeos = useMemo(() => {
    const g: string[] = []
    // a national total would flatten units' lines on one axis, so it stays beside them only for rates, means and shares
    const total = ind?.measure_type === 'count' || ind?.measure_type === 'sum'
    if (hasNational && (level === 0 || selected.length === 0 || !total)) g.push(NATIONAL)
    if (level > 0) g.push(...selected)
    if (level > 0 && !hasNational && selected.length === 0 && sorted[0]) g.push(sorted[0].pcode)
    return [...new Set(g)]
  }, [hasNational, level, selected, sorted, ind?.measure_type])
  const trendQ = useQuery(seriesQuery(ind?.id ?? '', trendGeos, dimStr, !!ind && dimsReady && view === 'trend'))
  const trend = useMemo(() => {
    const ser: RawSeries[] = trendQ.data ?? []
    const axis = fillPeriods([...new Set(ser.flatMap((s) => s.points.map((p) => p.period)))].sort(), ind?.period_type ?? '')
    return { axis, series: ser.map((s) => ({ ...s, name: s.geo === NATIONAL ? 'Sri Lanka' : (names.get(s.geo) ?? s.name), byPeriod: new Map(s.points.map((p) => [p.period, p])) })) }
  }, [trendQ.data, names, ind?.period_type])

  /* ------- composition: the split dimension's values, for the national figure and the picked units ------- */
  const partial = useMemo(() => dimsKey(Object.fromEntries(Object.entries(dims).filter(([k]) => k !== splitDim))), [dims, splitDim])
  const wantMakeup = view === 'makeup' && !!ind && !!splitDim && !!period
  const makeNatQ = useQuery(frameQuery(ind?.id ?? '', 0, partial, period, wantMakeup && hasNational))
  const makeUnitsQ = useQuery(frameQuery(ind?.id ?? '', level, partial, period, wantMakeup && level > 0))
  const makeup: Makeup[] = useMemo(() => {
    if (!splitDim) return []
    const build = (f: Frame | undefined, code: string, name: string): Makeup | null => {
      const i = f?.codes.indexOf(code) ?? -1
      if (!f || i < 0) return null
      const segments: Segment[] = f.columns
        .filter((c) => c.dims?.[splitDim] !== undefined && !TOTALS.has(c.dims[splitDim]))
        .map((c) => ({ id: c.dims![splitDim], label: c.dims![splitDim], value: c.values[i] ?? null }))
      const total = segments.reduce((s, x) => s + (x.value ?? 0), 0)
      return segments.length >= 2 ? { id: code, name, segments, total } : null
    }
    const out: Makeup[] = []
    if (hasNational) {
      const n = build(makeNatQ.data, NATIONAL, 'Sri Lanka')
      if (n) out.push(n)
    }
    const ids = selected.length ? selected : !hasNational && sorted[0] ? [sorted[0].pcode] : []
    for (const id of ids) {
      const u = build(makeUnitsQ.data, id, names.get(id) ?? id)
      if (u) out.push(u)
    }
    return out
  }, [splitDim, hasNational, makeNatQ.data, makeUnitsQ.data, selected, sorted, names])

  const sources = frameQ.data?.sources ?? natQ.data?.sources ?? makeNatQ.data?.sources ?? []
  const dataVersion = frameQ.data?.dataVersion ?? meta.data?.dataVersion ?? null
  /** how the figures were made, from the frame: anything other than reported is flagged on the chart */
  const basis = col?.basis ?? makeNatQ.data?.columns[0]?.basis ?? makeUnitsQ.data?.columns[0]?.basis
  const provisional = (col ?? makeNatQ.data?.columns[0])?.status === 'provisional'

  const loading = meta.isPending || (!!ind && !dimsReady) || periodsQ.isPending || (view === 'trend' ? trendQ.isPending : view === 'makeup' ? makeNatQ.isPending && makeUnitsQ.isPending && wantMakeup : wantFrame && frameQ.isPending)
  const error = meta.isError ? meta.error : unitsQ.isError ? unitsQ.error : periodsQ.isError ? periodsQ.error : frameQ.isError && view !== 'trend' && view !== 'makeup' ? frameQ.error : trendQ.isError && view === 'trend' ? trendQ.error : null

  return {
    meta: meta.data,
    metaError: meta.isError,
    loading,
    error,
    indicators,
    ind,
    setIndicator: (id: string) => (setIndId(id), setLevel(null), setPeriod(null), setView(null), setSplit(null), setDimPick(null)),
    levels,
    level,
    setLevel: (l: number) => (setLevel(l), setView(null), setPeriod(null)),
    dims,
    dimValues: combos.values,
    setDim,
    periods,
    period,
    setPeriod,
    views,
    view,
    setView,
    splitCandidates,
    splitDim,
    setSplit,
    selected,
    toggle,
    clearSelected,
    names,
    rows,
    sorted,
    values,
    median,
    national,
    trend,
    trendGeos,
    makeup,
    sources,
    dataVersion,
    basis,
    provisional,
    unitsLoading: unitsQ.isPending,
    fetching: frameQ.isFetching || trendQ.isFetching || makeNatQ.isFetching || makeUnitsQ.isFetching,
  }
}
export type DatasetSeries = ReturnType<typeof useDatasetSeries>
