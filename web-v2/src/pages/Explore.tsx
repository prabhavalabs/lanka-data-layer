import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ListOrdered, LocateFixed, Minus, PanelLeftClose, PanelLeftOpen, Plus, SlidersHorizontal } from 'lucide-react'
import type { StyleSpecification } from 'maplibre-gl'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { areasQuery, datasetsQuery, indicatorMapQuery, indicatorsQuery, themesQuery } from '@/api/queries'
import type { Indicator, LicenceStatus } from '@/api/types'
import { PcodePlate } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { AreaDrawer } from '@/features/explore/AreaDrawer'
import { Controls, SearchField, type ControlsProps } from '@/features/explore/Controls'
import '@/features/explore/explore.css'
import { ExploreMap, type MapHandle } from '@/features/explore/ExploreMap'
import { Legend } from '@/features/explore/Legend'
import { MobileSheet, SHEET_PEEK } from '@/features/explore/MobileSheet'
import {
  buildModel,
  DEFAULT_INDICATOR,
  fmtBare,
  levelOfPcode,
  ordinal,
  rangeBounds,
  unitSuffix,
  type LayerKey,
  type LayerState,
  type Level,
  type LicenceFilter,
} from '@/features/explore/model'
import { RankedList } from '@/features/explore/RankedList'
import { geo } from '@/lib/geo'
import { useMediaQuery, useReducedMotion, useTheme, type Theme } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const BASEMAP: Record<Theme, string> = {
  dark: 'https://tiles.openfreemap.org/styles/dark',
  light: 'https://tiles.openfreemap.org/styles/positron',
}
/** Basemap style as data: resolves to null when unreachable so the map can fall back to a blank canvas. */
const basemapQuery = (theme: Theme) => ({
  queryKey: ['basemap', theme],
  queryFn: async (): Promise<StyleSpecification | null> => {
    try {
      const res = await fetch(BASEMAP[theme], { signal: AbortSignal.timeout(8000) })
      if (!res.ok) return null
      return (await res.json()) as StyleSpecification
    } catch {
      return null
    }
  },
  staleTime: Infinity,
  gcTime: Infinity,
  retry: false,
})
const geoQuery = <T,>(key: string, load: () => Promise<T>, enabled = true) => ({ queryKey: ['geo', key], queryFn: load, staleTime: Infinity, enabled })

type Patch = Partial<Record<'indicator' | 'level' | 'year' | 'area', string | number | null>>

export default function Explore() {
  const [theme] = useTheme()
  const desktop = useMediaQuery('(min-width: 768px)')
  const reducedMotion = useReducedMotion()
  const queryClient = useQueryClient()
  const [params, setParams] = useSearchParams()

  /* ---------------- data ---------------- */
  const { data: indicators } = useQuery(indicatorsQuery())
  const { data: themes } = useQuery(themesQuery())
  const { data: datasets } = useQuery(datasetsQuery())
  const { data: areaList } = useQuery(areasQuery())
  const areas = useMemo(() => areaList && new Map(areaList.map((a) => [a.pcode, a])), [areaList])
  const { data: adm1 } = useQuery(geoQuery('adm1', geo.adm1))
  const { data: adm2 } = useQuery(geoQuery('adm2', geo.adm2))
  const { data: basemap } = useQuery(basemapQuery(theme))
  useEffect(() => {
    void queryClient.prefetchQuery(basemapQuery(theme === 'dark' ? 'light' : 'dark'))
  }, [queryClient, theme])

  /* ---------------- URL state: ?indicator=&level=&year=&area= ---------------- */
  const areaParam = params.get('area')?.toUpperCase() ?? null
  const levelParam = params.get('level')
  const level: Level = levelParam === '1' ? 1 : levelParam === '2' ? 2 : ((areaParam ? levelOfPcode(areaParam) : null) ?? 2)
  const selected = areaParam && levelOfPcode(areaParam) === level ? areaParam : null
  const indicator = indicators?.find((i) => i.id === params.get('indicator')) ?? indicators?.find((i) => i.id === DEFAULT_INDICATOR) ?? indicators?.[0]
  const yearParam = Number(params.get('year'))
  const year = indicator ? (indicator.years.includes(yearParam) ? yearParam : indicator.years[indicator.years.length - 1]) : yearParam || 2024

  const patch = useCallback(
    (p: Patch) =>
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev)
          for (const [k, v] of Object.entries(p)) {
            if (v === null || v === undefined) next.delete(k)
            else next.set(k, String(v))
          }
          return next
        },
        { replace: true },
      ),
    [setParams],
  )
  const patchRef = useRef(patch)
  patchRef.current = patch

  /* ---------------- choropleth ---------------- */
  const mapQuery = useQuery({ ...indicatorMapQuery(indicator?.id ?? '', level, year), enabled: !!indicator })
  const mapData = mapQuery.data
  // Draw what the payload describes (it may be the previous one while the next loads).
  const shown = mapData && indicators?.find((i) => i.id === mapData.indicator)
  const model = useMemo(() => (shown && mapData ? buildModel(shown, mapData, theme) : null), [shown, mapData, theme])
  const shownLevel = model?.data.level ?? level
  const collection = shownLevel === 1 ? adm1 : adm2

  const [range, setRange] = useState<[number, number]>([0, 100])
  const bounds = useMemo(() => rangeBounds(model, range), [model, range])
  const colors = useMemo(() => (model ? new Map([...model.byPcode].map(([p, v]) => [p, v.color])) : null), [model])
  const dimmed = useMemo(() => (model ? model.data.pcodes.filter((_, i) => model.data.values[i] < bounds[0] || model.data.values[i] > bounds[1]) : []), [model, bounds])

  /* ---------------- panel state ---------------- */
  const [licences, setLicences] = useState<LicenceFilter>({ open: true, attribution: true, unclear: true })
  const [layers, setLayers] = useState<LayerState>({ rail: true, rivers: false, protected: false, places: true })
  const [panelOpen, setPanelOpen] = useState(true)
  const [tab, setTab] = useState('controls')
  const [playing, setPlaying] = useState(false)
  const [hover, setHover] = useState<string | null>(null)
  const lastHover = useRef<string | null>(null)
  if (hover) lastHover.current = hover
  const tooltipRef = useRef<HTMLDivElement | null>(null)
  const mapHandle = useRef<MapHandle | null>(null)

  const { data: rail } = useQuery(geoQuery('rail', geo.rail, layers.rail))
  const { data: rivers } = useQuery(geoQuery('rivers', geo.rivers, layers.rivers))
  const { data: protectedAreas } = useQuery(geoQuery('protected', geo.protectedAreas, layers.protected))
  const { data: places } = useQuery(geoQuery('places', geo.places, layers.places))
  const overlays = useMemo(() => ({ rail, rivers, protectedAreas, places }), [rail, rivers, protectedAreas, places])

  const datasetOf = useCallback((ind: Indicator) => datasets?.find((d) => d.id === ind.dataset), [datasets])
  const licenceOf = useCallback((ind: Indicator): LicenceStatus | undefined => datasetOf(ind)?.licence.status, [datasetOf])

  /* ---------------- playback ---------------- */
  const yearRef = useRef(year)
  yearRef.current = year
  const years = indicator?.years
  useEffect(() => {
    if (!playing || !years || years.length < 2) return
    const id = window.setInterval(() => {
      const i = years.indexOf(yearRef.current)
      patchRef.current({ year: years[(i + 1) % years.length] })
    }, 900)
    return () => window.clearInterval(id)
  }, [playing, years])

  /* ---------------- handlers ---------------- */
  const onIndicator = (id: string) => {
    const next = indicators?.find((i) => i.id === id)
    if (!next) return
    setRange([0, 100])
    patch({ indicator: id, year: next.years.includes(year) ? year : next.years[next.years.length - 1] })
  }
  const onLevel = (l: Level) => {
    if (l === level) return
    setRange([0, 100])
    patch({ level: l, area: l === 1 && selected ? selected.slice(0, 3) : null })
  }
  const onYear = useCallback((y: number) => {
    setPlaying(false)
    patchRef.current({ year: y })
  }, [])
  const onPlay = () => {
    if (!years || years.length < 2) return
    // reduced motion: step once instead of auto-animating the colours
    if (reducedMotion) patch({ year: years[(years.indexOf(year) + 1) % years.length] })
    else setPlaying((v) => !v)
  }
  const onLicence = (status: LicenceStatus, on: boolean) => {
    const next = { ...licences, [status]: on }
    setLicences(next)
    if (indicator && indicators && !next[licenceOf(indicator) ?? 'open']) {
      const first = indicators.find((i) => next[licenceOf(i) ?? 'open'])
      if (first) onIndicator(first.id)
    }
  }
  const onLayer = (key: LayerKey, on: boolean) => setLayers((s) => ({ ...s, [key]: on }))
  const onSelect = useCallback((pcode: string | null) => patchRef.current({ area: pcode }), [])
  const onClose = useCallback(() => patchRef.current({ area: null }), [])
  const onPick = (pcode: string) => {
    patch({ area: pcode })
    mapHandle.current?.focus(pcode)
  }

  // On a phone the sheet covers most of the map, so bring the selected area into the strip that is left.
  const hasGeometry = !!collection
  useEffect(() => {
    if (!desktop && selected && hasGeometry) mapHandle.current?.focus(selected)
  }, [desktop, selected, hasGeometry])

  /* ---------------- camera padding: keep the island clear of the panels ---------------- */
  const padding = useMemo(
    () =>
      desktop
        ? { top: 72, bottom: 24, left: panelOpen ? 364 : 16, right: selected ? 404 : 16 }
        : { top: 60, bottom: selected ? Math.round(window.innerHeight * 0.62) + 4 : SHEET_PEEK + 8, left: 8, right: 8 },
    [desktop, panelOpen, selected],
  )

  const controls: ControlsProps = {
    indicators,
    themes,
    licenceOf,
    licences,
    onLicence,
    indicator,
    onIndicator,
    level,
    onLevel,
    year,
    onYear,
    playing,
    onPlay,
    reducedMotion,
    model,
    range,
    onRange: setRange,
    layers,
    onLayer,
  }

  const tabs = (
    <Tabs value={tab} onValueChange={setTab} className="min-h-0 flex-1 gap-0">
      <div className="px-3 pb-3 max-md:pt-3">
        <TabsList className="flex w-full max-md:h-11">
          <TabsTrigger value="controls" className="flex-1">
            <SlidersHorizontal />
            Controls
          </TabsTrigger>
          <TabsTrigger value="ranking" className="flex-1">
            <ListOrdered />
            Ranking
          </TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="controls" className="scroll-thin min-h-0 flex-1 border-t border-glass-line md:overflow-y-auto">
        <Controls {...controls} />
      </TabsContent>
      <TabsContent value="ranking" className="scroll-thin min-h-0 flex-1 border-t border-glass-line md:overflow-y-auto">
        <RankedList model={model} areas={areas} selected={selected} bounds={bounds} onPick={onPick} />
      </TabsContent>
    </Tabs>
  )

  const tip = hover ?? lastHover.current
  const tipData = tip ? model?.byPcode.get(tip) : undefined

  return (
    <div
      className="relative h-[100dvh] w-full overflow-hidden bg-bg"
      style={{ '--inset-r': desktop && selected ? '396px' : '0px', '--inset-b': desktop ? '0px' : `calc(${SHEET_PEEK}px + env(safe-area-inset-bottom))` } as React.CSSProperties}
    >
      <h1 className="sr-only">Explore the map</h1>
      <ExploreMap
        theme={theme}
        basemap={basemap}
        collection={collection}
        provinces={adm1}
        colors={colors}
        dimmed={dimmed}
        selected={selected}
        layers={layers}
        overlays={overlays}
        padding={padding}
        reducedMotion={reducedMotion}
        tooltipRef={tooltipRef}
        onHover={setHover}
        onSelect={onSelect}
        handle={mapHandle}
      />

      {/* keeps the floating nav legible over any basemap */}
      <div className="pointer-events-none absolute inset-x-0 top-0 z-10 h-24 bg-gradient-to-b from-bg/80 to-transparent" aria-hidden />

      {/* hover tooltip: positioned imperatively by the map, content only changes per area */}
      <div ref={tooltipRef} className={cn('glass pointer-events-none absolute top-0 left-0 z-20 min-w-[176px] rounded-xl px-3 py-2.5 transition-opacity duration-150 will-change-transform', hover ? 'opacity-100' : 'opacity-0')} aria-hidden>
        {tip && (
          <>
            <div className="flex items-center justify-between gap-4">
              <span className="text-[13px] font-semibold tracking-[-0.01em] text-ink">{areas?.get(tip)?.name.en ?? tip}</span>
              <PcodePlate code={tip} size="sm" />
            </div>
            <div className="mt-2 flex items-baseline justify-between gap-4">
              {tipData && model ? (
                <>
                  <span className="t-num font-mono text-[15px] font-medium tracking-[-0.02em] text-ink">
                    {fmtBare(tipData.value, model.indicator.unit)}
                    <span className="ml-1 font-sans text-[11.5px] font-normal text-ink2">{unitSuffix(model.indicator.unit)}</span>
                  </span>
                  <span className="t-label t-num">
                    {ordinal(tipData.rank)} of {model.data.pcodes.length}
                  </span>
                </>
              ) : (
                <span className="text-[12px] text-ink3">No data</span>
              )}
            </div>
          </>
        )}
      </div>

      {desktop ? (
        <>
          <aside
            aria-label="Map controls"
            inert={!panelOpen}
            className={cn(
              'glass absolute top-[68px] bottom-4 left-4 z-30 flex w-[340px] flex-col overflow-hidden rounded-2xl transition-[transform,opacity] duration-[340ms] ease-[var(--ease-drawer)] motion-reduce:transition-none',
              !panelOpen && 'pointer-events-none -translate-x-[calc(100%+24px)] opacity-0',
            )}
          >
            <div className="flex items-center gap-1.5 p-3">
              <SearchField />
              <Button variant="ghost" size="icon" onClick={() => setPanelOpen(false)} aria-label="Hide controls">
                <PanelLeftClose />
              </Button>
            </div>
            {tabs}
            <Legend docked model={model} dataset={shown ? datasetOf(shown) : undefined} loading={mapQuery.isPlaceholderData} />
          </aside>
          <Button
            variant="glass"
            size="icon"
            onClick={() => setPanelOpen(true)}
            aria-label="Show controls"
            inert={panelOpen}
            className={cn('absolute top-[68px] left-4 z-20 size-10 rounded-xl transition-opacity duration-200', panelOpen ? 'pointer-events-none opacity-0' : 'opacity-100 delay-150')}
          >
            <PanelLeftOpen />
          </Button>
          <Legend
            model={model}
            dataset={shown ? datasetOf(shown) : undefined}
            loading={mapQuery.isPlaceholderData}
            className={cn('absolute bottom-4 left-4 z-20 transition-opacity duration-200', panelOpen ? 'pointer-events-none opacity-0' : 'opacity-100 delay-150')}
          />
        </>
      ) : (
        <MobileSheet model={model} year={year} playing={playing} onPlay={onPlay} reducedMotion={reducedMotion}>
          <div className="flex px-3 pt-3">
            <SearchField />
          </div>
          {tabs}
        </MobileSheet>
      )}

      {/* map controls */}
      <div
        className="glass absolute right-4 z-20 flex flex-col overflow-hidden rounded-xl transition-transform duration-[380ms] ease-[var(--ease-drawer)] motion-reduce:transition-none max-md:top-[68px] max-md:right-3 md:bottom-12"
        style={{ transform: desktop ? 'translateX(calc(-1 * var(--inset-r)))' : undefined }}
      >
        {desktop && (
          <>
            <button type="button" onClick={() => mapHandle.current?.zoomIn()} aria-label="Zoom in" className="pressable grid size-9 place-items-center text-ink2 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3/70 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
              <Plus className="size-4" />
            </button>
            <button type="button" onClick={() => mapHandle.current?.zoomOut()} aria-label="Zoom out" className="pressable grid size-9 place-items-center border-t border-glass-line text-ink2 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3/70 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
              <Minus className="size-4" />
            </button>
          </>
        )}
        <button
          type="button"
          onClick={() => mapHandle.current?.reset()}
          aria-label="Reset view to Sri Lanka"
          className="pressable grid size-9 place-items-center text-ink2 max-md:size-11 md:border-t md:border-glass-line [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3/70 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink"
        >
          <LocateFixed className="size-4" />
        </button>
      </div>

      <AreaDrawer pcode={selected} model={model} areas={areas} theme={theme} year={year} mobile={!desktop} onYear={onYear} onClose={onClose} />
    </div>
  )
}
