import type { PaddingOptions, StyleSpecification } from 'maplibre-gl'
import { useEffect, useRef, type RefObject } from 'react'
import type { LayerData } from '@/api/workspace'
import type { Theme } from '@/lib/hooks'
import type { MapDocument } from '@/lib/map-spec'
import type { Grid } from '@/mocks/synth'
import { MapEngine, type EngineEvents } from './engine/MapEngine'

type Props = {
  doc: MapDocument
  data: Map<string, LayerData | undefined>
  relief: Map<number, Grid>
  scheme: Theme
  /** undefined while loading, null when there is none */
  basemap: StyleSpecification | null | undefined
  solo: string | null
  padding: PaddingOptions
  reducedMotion: boolean
  engine: RefObject<MapEngine | null>
  tooltipRef: RefObject<HTMLDivElement | null>
  onHover: EngineEvents['onHover']
  onClick: EngineEvents['onClick']
  onView: EngineEvents['onView']
  onCompiled: () => void
  /** the view to open at: a link's `?v=`, or the document's own */
  initialView: MapDocument['view']
}

/** The MapLibre canvas. All drawing is the engine's; this component only feeds it. */
export function MapCanvas(p: Props) {
  const container = useRef<HTMLDivElement | null>(null)
  const latest = useRef(p)
  latest.current = p

  useEffect(() => {
    if (!container.current) return
    const l = latest.current
    const engine = new MapEngine(
      container.current,
      { scheme: l.scheme, basemap: l.basemap, padding: l.padding, reducedMotion: l.reducedMotion, view: l.initialView },
      {
        onHover: (h) => latest.current.onHover(h),
        onClick: (h) => latest.current.onClick(h),
        onView: (v) => latest.current.onView(v),
        onCompiled: () => latest.current.onCompiled(),
        tooltip: () => latest.current.tooltipRef.current,
      },
    )
    l.engine.current = engine
    // a handle for debugging in development builds only
    if (import.meta.env.DEV) (window as unknown as { __ldl?: MapEngine }).__ldl = engine
    return () => {
      l.engine.current = null
      engine.dispose()
    }
  }, [])

  const { doc, data, relief, scheme, basemap, solo, padding, reducedMotion } = p
  // the state goes in before a style swap, so the layers rebuilt on style.load are already the new scheme's
  useEffect(() => {
    p.engine.current?.setState({ layers: doc.layers, sources: doc.sources, data, scheme, time: doc.time, basemap: doc.basemap, solo, relief })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc.layers, doc.sources, doc.time, doc.basemap, data, scheme, solo, relief])
  const first = useRef(true)
  useEffect(() => {
    if (first.current) {
      first.current = false
      return
    }
    p.engine.current?.setBasemap(basemap, scheme)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [basemap, scheme])
  const firstPad = useRef(true)
  useEffect(() => {
    if (firstPad.current) {
      firstPad.current = false
      return
    }
    p.engine.current?.setPadding(padding, reducedMotion)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [padding, reducedMotion])

  // MapLibre forces position:relative on its container, so the sizing lives on a wrapper
  return (
    <div className="ws-map absolute inset-0">
      <div ref={container} className="size-full" role="application" aria-label={`Map: ${doc.title}`} />
    </div>
  )
}
