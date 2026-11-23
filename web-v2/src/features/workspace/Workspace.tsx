// The map workspace: a layered map document on the canvas, the layer stack on the left, the
// inspector and the analysis on the right. On a phone the two docks fold into one bottom sheet.
import { useQuery } from '@tanstack/react-query'
import { ChartScatter, Layers, ListTree, LocateFixed, Minus, PanelLeftClose, PanelLeftOpen, PanelRightClose, PanelRightOpen, Plus, SlidersHorizontal } from 'lucide-react'
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react'
import { areasQuery, indicatorMapQuery, indicatorsQuery } from '@/api/queries'
import { PcodePlate } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { AreaDrawer } from '@/features/explore/AreaDrawer'
import '@/features/explore/explore.css'
import { buildModel } from '@/features/explore/model'
import { useMediaQuery, useReducedMotion, useTheme } from '@/lib/hooks'
import { allLayers, findNode, isGroup, resolvePeriod, type MapDocument } from '@/lib/map-spec'
import { cn } from '@/lib/utils'
import { shownLayers, useBasemap, useLayerData, WorkspaceContext, type WorkspaceCtx } from './context'
import { DocBar, MapsDialog, type DocActions } from './DocBar'
import type { Hover, MapEngine } from './engine/MapEngine'
import { exportPng } from './exporting'
import { Inspector } from './Inspector'
import { LayersPanel } from './LayersPanel'
import { LegendCard, legendEntries, LegendList } from './LegendCard'
import { MapCanvas } from './MapCanvas'
import { actions, useWs } from './store'
import { Timeline } from './Timeline'
import { SHEET_PEEK, WorkspaceSheet } from './WorkspaceSheet'
import { setMapZoom } from './zoom'
import './workspace.css'

// the catalogue and the analysis (its charts and the statistics behind them) load when first opened
const AddLayerDialog = lazy(() => import('./AddLayerDialog').then((m) => ({ default: m.AddLayerDialog })))
const RelatePanel = lazy(() => import('./relate/RelatePanel').then((m) => ({ default: m.RelatePanel })))

const UI_KEY = 'ldl-ws-ui/1'
type Ui = { left: number; right: number; leftOpen: boolean; rightOpen: boolean }
const DEFAULT_UI: Ui = { left: 312, right: 384, leftOpen: true, rightOpen: true }
const LIMIT = { left: [272, 440], right: [340, 520] } as const
const readUi = (): Ui => {
  try {
    return { ...DEFAULT_UI, ...(JSON.parse(localStorage.getItem(UI_KEY) ?? '{}') as Partial<Ui>) }
  } catch {
    return DEFAULT_UI
  }
}
const ctrl = 'pressable grid size-9 place-items-center text-ink2 outline-none [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3/70 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink'
const PanelFallback = () => (
  <div className="space-y-3 p-4">
    <div className="skeleton h-9 w-full" />
    <div className="skeleton h-9 w-full" />
    <div className="skeleton h-40 w-full" />
  </div>
)

export type WorkspaceProps = {
  doc: MapDocument
  docActions: Omit<DocActions, 'exportPng'>
  /** the camera to open at: a link's `?v=`, or the document's own view */
  initialView: MapDocument['view']
  /** changes when another document is opened, so the camera goes to its view */
  docKey: string
  onView: (view: { center: [number, number]; zoom: number }) => void
  area: string | null
  onArea: (pcode: string | null) => void
}

export function Workspace({ doc, docActions, initialView, docKey, onView, area, onArea }: WorkspaceProps) {
  const [theme] = useTheme()
  const desktop = useMediaQuery('(min-width: 768px)')
  const reducedMotion = useReducedMotion()
  const selected = useWs((s) => s.selected)
  const solo = useWs((s) => s.solo)
  const engine = useRef<MapEngine | null>(null)
  const tooltipRef = useRef<HTMLDivElement | null>(null)
  const [tick, setTick] = useState(0)
  const [hover, setHover] = useState<Hover | null>(null)
  const lastHover = useRef<Hover | null>(null)
  if (hover) lastHover.current = hover
  const [ui, setUi] = useState(readUi)
  const [tab, setTab] = useState<'inspect' | 'relate'>(doc.analyses.rel ? 'relate' : 'inspect')
  const [sheetTab, setSheetTab] = useState('layers')
  const [sheetRest, setSheetRest] = useState(SHEET_PEEK)
  const [sheetSnap, setSheetSnap] = useState(0)
  const [addOpen, setAddOpen] = useState(false)
  const [mapsOpen, setMapsOpen] = useState(false)
  const { scheme, style } = useBasemap(doc, theme)
  const { data, failed, relief, loading, resolved, shown: onScreen } = useLayerData(doc, selected, solo)
  /* ---- attribution and notices of the resolved document, on the map and in exports ---- */
  const credits = useMemo(() => {
    const esc = (t: string) => t.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!)
    const notices = (resolved?.notices ?? []).filter((n) => n.code !== 'PRIVATE_DATA' || Object.values(resolved?.sources ?? {}).some((r) => r.access === 'denied'))
    const sample = allLayers(doc.layers).some((l) => onScreen.has(l.id) && data.get(l.id)?.origin === 'sample')
    const html = [
      ...notices.map((n) => `<strong>${esc(n.code === 'ESTIMATE' ? 'Estimates' : n.code === 'PRIVATE_DATA' ? 'Not served' : n.code.charAt(0) + n.code.slice(1).toLowerCase().replace(/_/g, ' '))}:</strong> ${esc(n.code === 'PRIVATE_DATA' ? 'Some layers name workspace data this map cannot read; they are drawn from bundled sample data.' : n.text)}`),
      ...(sample ? ['<strong>Sample data:</strong> layers marked SAMPLE are bundled demonstration data, not served by the engine.'] : []),
      ...[...new Set((resolved?.attribution ?? []).map((a) => a.text))].map(esc),
    ]
    const plain = [...new Set((resolved?.attribution ?? []).map((a) => a.publisher))]
    return { html, plain: resolved ? ['Lanka Data Layer', ...plain, ...(sample ? ['includes sample data'] : [])] : ['Lanka Data Layer · sample data'] }
  }, [resolved, doc.layers, data, onScreen])
  useEffect(() => {
    engine.current?.setAttribution(credits.html)
  }, [credits, tick])
  const ctx = useMemo<WorkspaceCtx>(() => ({ engine, data, failed, scheme, tick, desktop }), [data, failed, scheme, tick, desktop])
  const onCompiled = useCallback(() => setTick((t) => t + 1), [])
  useEffect(() => {
    try {
      localStorage.setItem(UI_KEY, JSON.stringify(ui))
    } catch {
      /* private mode */
    }
  }, [ui])

  /* ---- a newly opened document: its camera, and the panel that suits it ---- */
  const openedDoc = useRef(docKey)
  const sameDoc = useRef(docKey)
  useEffect(() => {
    if (openedDoc.current === docKey) return
    openedDoc.current = docKey
    engine.current?.setView(doc.view)
    setTab(doc.analyses.rel ? 'relate' : 'inspect')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docKey])
  // choosing a layer brings up its inspector; opening a document does not
  const lastSelected = useRef(selected)
  useEffect(() => {
    if (sameDoc.current === docKey && selected && selected !== lastSelected.current) setTab('inspect')
    sameDoc.current = docKey
    lastSelected.current = selected
  }, [selected, docKey])

  /* ---- the legacy single-indicator view: an area opens its drawer ---- */
  const areaLayer = useMemo(() => {
    const shown = shownLayers(doc.layers, solo)
    const cand = allLayers(doc.layers).filter((l) => l.type === 'choropleth' && shown.has(l.id))
    const pick = cand.find((l) => l.id === selected) ?? cand[cand.length - 1]
    const s = pick && doc.sources[pick.source]
    return s?.kind === 'indicator' && s.level <= 2 ? { layer: pick, source: s } : null
  }, [doc.layers, doc.sources, selected, solo])
  const { data: indicators } = useQuery({ ...indicatorsQuery(), enabled: !!area })
  const indicator = indicators?.find((i) => i.id === areaLayer?.source.indicator)
  const year = indicator && areaLayer ? Number(resolvePeriod(areaLayer.source.period, doc.time, indicator.years.map(String))) : 2024
  const areaMap = useQuery({ ...indicatorMapQuery(indicator?.id ?? '', (areaLayer?.source.level ?? 2) as 1 | 2, year), enabled: !!area && !!indicator })
  const { data: areaList } = useQuery({ ...areasQuery(), enabled: !!area })
  const areas = useMemo(() => areaList && new Map(areaList.map((a) => [a.pcode, a])), [areaList])
  const areaModel = useMemo(() => (indicator && areaMap.data ? buildModel(indicator, areaMap.data, scheme) : null), [indicator, areaMap.data, scheme])
  const areaShown = area && indicator && area.length === (areaLayer?.source.level === 1 ? 3 : 4) ? area : null
  useEffect(() => {
    engine.current?.select(areaShown)
  }, [areaShown, tick])

  /* ---- layout: docks are persistent camera padding ---- */
  const rightShown = ui.rightOpen && !areaShown
  const timeBound = useWs((s) => !!s.doc?.time)
  const padding = useMemo(
    () =>
      desktop
        ? { top: 72, bottom: timeBound ? 84 : 24, left: ui.leftOpen ? ui.left + 28 : 16, right: areaShown ? 404 : rightShown ? ui.right + 28 : 16 }
        : { top: 60, bottom: (areaShown ? Math.round(window.innerHeight * 0.62) : sheetRest) + (timeBound ? 64 : 8), left: 8, right: 8 },
    [desktop, ui.leftOpen, ui.left, ui.right, rightShown, areaShown, sheetRest, timeBound],
  )
  const resize = (side: 'left' | 'right') => (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    const grip = e.currentTarget
    const dock = grip.parentElement!
    const x0 = e.clientX, w0 = dock.offsetWidth
    const [min, max] = LIMIT[side]
    grip.setPointerCapture(e.pointerId)
    grip.setAttribute('data-active', '')
    let w = w0
    const move = (ev: PointerEvent) => {
      w = Math.max(min, Math.min(max, w0 + (side === 'left' ? ev.clientX - x0 : x0 - ev.clientX)))
      // straight to the element: the document and the map hear about it once, on release
      dock.style.width = `${w}px`
    }
    const up = () => {
      grip.removeAttribute('data-active')
      grip.removeEventListener('pointermove', move)
      grip.removeEventListener('pointerup', up)
      grip.removeEventListener('pointercancel', up)
      setUi((u) => ({ ...u, [side]: w }))
    }
    grip.addEventListener('pointermove', move)
    grip.addEventListener('pointerup', up)
    grip.addEventListener('pointercancel', up)
  }

  /* ---- document actions that need the map ---- */
  const onExportPng = useCallback(() => {
    const e = engine.current
    if (!e) return
    const entries = legendEntries(doc.layers, shownLayers(doc.layers, solo)).flatMap((l) => {
      const model = e.legend(l.id)
      return model ? [{ title: l.legend.title ?? l.name, model }] : []
    })
    void exportPng(e, doc, entries, { left: padding.left - 12, top: 0, right: padding.right - 12, bottom: 0 }, scheme === 'dark', credits.plain)
  }, [doc, solo, padding, scheme, credits])
  const all: DocActions = useMemo(() => ({ ...docActions, exportPng: onExportPng }), [docActions, onExportPng])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test((e.target as HTMLElement).tagName)
      if (mod && e.key.toLowerCase() === 's') {
        e.preventDefault()
        docActions.save()
      } else if (mod && e.key.toLowerCase() === 'z' && !typing) {
        e.preventDefault()
        if (e.shiftKey) actions.redo()
        else actions.undo()
      } else if (e.key === 'Escape' && !typing && solo) actions.solo(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [docActions, solo])

  const onClick = useCallback(
    (hit: { layerId: string; code?: string } | null) => {
      if (!hit) return
      actions.select(hit.layerId)
      const node = findNode(doc.layers, hit.layerId)
      const s = node && !isGroup(node) ? doc.sources[node.source] : undefined
      if (hit.code && node && !isGroup(node) && node.type === 'choropleth' && s?.kind === 'indicator' && s.level <= 2) {
        onArea(hit.code)
        engine.current?.focus(hit.code)
      }
    },
    [doc.layers, doc.sources, onArea],
  )
  const openRelate = useCallback(() => {
    setTab('relate')
    setUi((u) => ({ ...u, rightOpen: true }))
    setSheetTab('relate')
    setSheetSnap((n) => n + 1)
  }, [])
  const openAdd = useCallback(() => setAddOpen(true), [])
  const openMaps = useCallback(() => setMapsOpen(true), [])

  const tip = hover ?? lastHover.current
  const tipLayer = tip ? findNode(doc.layers, tip.layerId) : null
  const vars = { '--inset-r': desktop ? `${padding.right - 16}px` : '0px', '--inset-b': desktop ? '0px' : `${padding.bottom - 8}px`, '--pl': `${padding.left}px`, '--pr': `${padding.right}px` } as CSSProperties
  const relate = (
    <Suspense fallback={<PanelFallback />}>
      <RelatePanel />
    </Suspense>
  )

  // the zoom the layer panel and the inspector measure layers against (see ./zoom)
  useState(() => setMapZoom(initialView.zoom))
  const onViewAndZoom = useCallback(
    (v: { center: [number, number]; zoom: number }) => {
      setMapZoom(v.zoom)
      onView(v)
    },
    [onView],
  )
  return (
    <WorkspaceContext.Provider value={ctx}>
      <div className={cn('relative h-[100dvh] w-full overflow-hidden bg-bg', scheme !== theme && 'ws-mixed')} style={vars}>
        <h1 className="sr-only">{doc.title}</h1>
        <MapCanvas doc={doc} data={data} relief={relief} scheme={scheme} basemap={style} solo={solo} padding={padding} reducedMotion={reducedMotion} engine={engine} tooltipRef={tooltipRef} onHover={setHover} onClick={onClick} onView={onViewAndZoom} onCompiled={onCompiled} initialView={initialView} />
        {/* keeps the floating nav legible over any basemap */}
        <div className="pointer-events-none absolute inset-x-0 top-0 z-10 h-24 bg-gradient-to-b from-bg/80 to-transparent" aria-hidden />

        {/* hover tooltip: positioned imperatively by the engine, content only changes per feature */}
        <div ref={tooltipRef} className={cn('glass pointer-events-none absolute top-0 left-0 z-20 max-w-[260px] min-w-[168px] rounded-xl px-3 py-2.5 transition-opacity duration-150 will-change-transform', hover ? 'opacity-100' : 'opacity-0')} aria-hidden>
          {tip && (
            <>
              <div className="flex items-center gap-2">
                {tip.tip.swatch && <span className="size-2.5 shrink-0 rounded-[3px] ring-1 ring-[color-mix(in_oklab,var(--ink)_18%,transparent)]" style={{ background: tip.tip.swatch }} />}
                <span className="min-w-0 flex-1 truncate text-[13px] font-semibold tracking-[-0.01em] text-ink">{tip.tip.title}</span>
                {tip.tip.code && tip.tip.code.length <= 4 && <PcodePlate code={tip.tip.code} size="sm" />}
              </div>
              {tip.tip.rows.map((r) => (
                <div key={r.label} className="mt-1.5 flex items-baseline justify-between gap-4">
                  <span className="truncate text-[11.5px] text-ink2 first-letter:uppercase">{r.label}</span>
                  <span className="t-num shrink-0 font-mono text-[12.5px] font-medium tracking-[-0.01em] text-ink">{r.value}</span>
                </div>
              ))}
              {tip.tip.note && <div className="mt-1.5 text-[11px] leading-snug text-warn">{tip.tip.note}</div>}
              {tipLayer && tipLayer.name !== tip.tip.title && <div className="t-label mt-2 truncate border-t border-glass-line pt-1.5">{tipLayer.name}</div>}
            </>
          )}
        </div>

        {desktop ? (
          <>
            <aside aria-label="Layers" data-side="left" data-open={ui.leftOpen} inert={!ui.leftOpen} className="ws-dock glass absolute top-[68px] bottom-4 left-4 z-30 flex flex-col overflow-hidden rounded-2xl" style={{ width: ui.left }}>
              <div className="ws-grip right-0" onPointerDown={resize('left')} role="separator" aria-orientation="vertical" aria-label="Resize the layers panel" />
              <LayersPanel
                onAdd={openAdd}
                header={
                  <DocBar
                    doc={all}
                    onMaps={openMaps}
                    extra={
                      <button type="button" onClick={() => setUi((u) => ({ ...u, leftOpen: false }))} aria-label="Hide the layers panel" className="pressable grid size-7 shrink-0 place-items-center rounded-md text-ink3 outline-none [@media(hover:hover)_and_(pointer:fine)]:hover:bg-[color-mix(in_oklab,var(--ink)_9%,transparent)] [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
                        <PanelLeftClose className="size-4" />
                      </button>
                    }
                  />
                }
              />
            </aside>
            <Button variant="glass" size="icon" onClick={() => setUi((u) => ({ ...u, leftOpen: true }))} aria-label="Show the layers panel" inert={ui.leftOpen} className={cn('absolute top-[68px] left-4 z-20 size-10 rounded-xl transition-opacity duration-200', ui.leftOpen ? 'pointer-events-none opacity-0' : 'opacity-100 delay-150')}>
              <PanelLeftOpen />
            </Button>

            <aside aria-label="Inspector and analysis" data-side="right" data-open={rightShown} inert={!rightShown} className="ws-dock glass absolute top-[68px] right-4 bottom-4 z-30 flex flex-col overflow-hidden rounded-2xl" style={{ width: ui.right }}>
              <div className="ws-grip left-0" onPointerDown={resize('right')} role="separator" aria-orientation="vertical" aria-label="Resize the inspector" />
              <Tabs value={tab} onValueChange={(v) => setTab(v as typeof tab)} className="min-h-0 flex-1 gap-0">
                <div className="flex items-center gap-1.5 p-2.5">
                  <TabsList className="flex flex-1">
                    <TabsTrigger value="inspect" className="flex-1">
                      <SlidersHorizontal />
                      Inspect
                    </TabsTrigger>
                    <TabsTrigger value="relate" className="flex-1">
                      <ChartScatter />
                      Relate
                    </TabsTrigger>
                  </TabsList>
                  <Button variant="ghost" size="icon-sm" onClick={() => setUi((u) => ({ ...u, rightOpen: false }))} aria-label="Hide the inspector">
                    <PanelRightClose />
                  </Button>
                </div>
                <TabsContent value="inspect" className="scroll-thin min-h-0 flex-1 overflow-y-auto border-t border-glass-line">
                  <Inspector onRelate={openRelate} />
                </TabsContent>
                <TabsContent value="relate" className="scroll-thin min-h-0 flex-1 overflow-y-auto border-t border-glass-line">
                  {relate}
                </TabsContent>
              </Tabs>
            </aside>
            <Button variant="glass" size="icon" onClick={() => setUi((u) => ({ ...u, rightOpen: true }))} aria-label="Show the inspector" inert={rightShown || !!areaShown} className={cn('absolute top-[68px] right-4 z-20 size-10 rounded-xl transition-opacity duration-200', rightShown || areaShown ? 'pointer-events-none opacity-0' : 'opacity-100 delay-150')}>
              <PanelRightOpen />
            </Button>

            <LegendCard className={cn('ws-float absolute left-0 z-20', timeBound ? 'bottom-[76px]' : 'bottom-4')} style={{ transform: 'translateX(var(--pl))' }} />
            <div className="ws-float pointer-events-none absolute bottom-4 left-0 z-20 flex justify-center" style={{ transform: 'translateX(var(--pl))', width: 'calc(100% - var(--pl) - var(--pr))' }}>
              <Timeline reducedMotion={reducedMotion} className="pointer-events-auto" />
            </div>
          </>
        ) : (
          <>
            <div className="pointer-events-none fixed inset-x-0 z-20 flex justify-center px-3 transition-transform duration-[380ms] ease-[var(--ease-drawer)]" style={{ bottom: 0, transform: `translateY(calc(-${sheetRest}px - 8px))` }}>
              <Timeline reducedMotion={reducedMotion} className="pointer-events-auto" />
            </div>
            <WorkspaceSheet reducedMotion={reducedMotion} onRest={setSheetRest} onAdd={openAdd} snap={sheetSnap}>
              <Tabs value={sheetTab} onValueChange={setSheetTab} className="gap-0">
                <div className="px-3 py-3">
                  <TabsList className="flex h-11 w-full">
                    <TabsTrigger value="layers" className="flex-1 px-1.5">
                      <Layers />
                      Layers
                    </TabsTrigger>
                    <TabsTrigger value="inspect" className="flex-1 px-1.5">
                      <SlidersHorizontal />
                      Inspect
                    </TabsTrigger>
                    <TabsTrigger value="relate" className="flex-1 px-1.5">
                      <ChartScatter />
                      Relate
                    </TabsTrigger>
                    <TabsTrigger value="legend" className="flex-1 px-1.5">
                      <ListTree />
                      Key
                    </TabsTrigger>
                  </TabsList>
                </div>
                <TabsContent value="layers" className="border-t border-glass-line">
                  <LayersPanel onAdd={openAdd} header={<DocBar doc={all} onMaps={openMaps} compact />} />
                </TabsContent>
                <TabsContent value="inspect" className="border-t border-glass-line">
                  <Inspector onRelate={openRelate} />
                </TabsContent>
                <TabsContent value="relate" className="border-t border-glass-line">
                  {sheetTab === 'relate' && relate}
                </TabsContent>
                <TabsContent value="legend" className="border-t border-glass-line">
                  <LegendList />
                </TabsContent>
              </Tabs>
            </WorkspaceSheet>
          </>
        )}

        {/* map controls */}
        <div className={cn('glass ws-float absolute right-4 z-20 flex flex-col overflow-hidden rounded-xl max-md:top-[68px] max-md:right-3 md:bottom-12', loading && 'after:absolute after:inset-x-0 after:bottom-0 after:h-px after:animate-pulse after:bg-ink2')} style={{ transform: desktop ? 'translateX(calc(-1 * var(--inset-r)))' : undefined }}>
          {desktop && (
            <>
              <button type="button" onClick={() => engine.current?.zoomIn()} aria-label="Zoom in" className={ctrl}>
                <Plus className="size-4" />
              </button>
              <button type="button" onClick={() => engine.current?.zoomOut()} aria-label="Zoom out" className={cn(ctrl, 'border-t border-glass-line')}>
                <Minus className="size-4" />
              </button>
            </>
          )}
          <button type="button" onClick={() => engine.current?.reset(doc.view)} aria-label="Back to the map's own view" className={cn(ctrl, 'max-md:size-11 md:border-t md:border-glass-line')}>
            <LocateFixed className="size-4" />
          </button>
        </div>

        <AreaDrawer pcode={areaShown} model={areaModel} areas={areas} theme={scheme} year={year} mobile={!desktop} onYear={(y) => actions.setCursor(String(y))} onClose={() => onArea(null)} />
        {addOpen && (
          <Suspense fallback={null}>
            <AddLayerDialog open={addOpen} onOpenChange={setAddOpen} scheme={scheme} />
          </Suspense>
        )}
        <MapsDialog open={mapsOpen} onOpenChange={setMapsOpen} doc={all} scheme={scheme} />
      </div>
    </WorkspaceContext.Provider>
  )
}
