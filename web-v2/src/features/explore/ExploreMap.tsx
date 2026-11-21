import type { Feature, FeatureCollection } from 'geojson'
import {
  AttributionControl,
  Map as MapLibreMap,
  type ExpressionSpecification,
  type FilterSpecification,
  type GeoJSONSource,
  type LngLatBoundsLike,
  type PaddingOptions,
  setWorkerUrl,
  type StyleSpecification,
} from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'
// MapLibre 6 ships its worker as a separate ES module. Vite's dep pre-bundling does not carry it
// along, so hand MapLibre a URL that Vite serves (dev) and bundles (build) as a module worker.
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import { useEffect, useImperativeHandle, useRef, type Ref, type RefObject } from 'react'
import { LK_BOUNDS, type AdminCollection, type Place, type Polyline } from '@/lib/geo'
import type { Theme } from '@/lib/hooks'
import { ramp } from '@/lib/palettes'
import { NODATA_HEX, SELECT_HEX, type LayerState } from './model'

export type MapHandle = {
  zoomIn: () => void
  zoomOut: () => void
  reset: () => void
  focus: (pcode: string) => void
}

export type Overlays = {
  rail?: Polyline[]
  rivers?: Polyline[]
  protectedAreas?: FeatureCollection
  places?: Place[]
}

type Props = {
  theme: Theme
  /** undefined while loading, null when the basemap could not be fetched (offline). */
  basemap: StyleSpecification | null | undefined
  collection: AdminCollection | undefined
  provinces: AdminCollection | undefined
  /** pcode -> fill colour; null while the first payload loads. */
  colors: Map<string, string> | null
  dimmed: string[]
  selected: string | null
  layers: LayerState
  overlays: Overlays
  padding: PaddingOptions
  reducedMotion: boolean
  tooltipRef: RefObject<HTMLDivElement | null>
  onHover: (pcode: string | null) => void
  onSelect: (pcode: string | null) => void
  handle: Ref<MapHandle>
}

setWorkerUrl(workerUrl)

const MAX_BOUNDS: LngLatBoundsLike = [
  [73.4, 3.1],
  [88.1, 12.5],
]
const FIT_PAD = 14
const easeOut = (t: number) => 1 - (1 - t) ** 4
const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }
const NONE: FilterSpecification = ['==', ['get', 'pcode'], '']
const byPcode = (pcode: string | null): FilterSpecification => (pcode ? ['==', ['get', 'pcode'], pcode] : NONE)

const cssVar = (name: string, fallback: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback

function fallbackStyle(theme: Theme): StyleSpecification {
  return {
    version: 8,
    sources: {},
    layers: [{ id: 'bg', type: 'background', paint: { 'background-color': cssVar('--bg', theme === 'dark' ? '#0b0e12' : '#fbfbfa') } }],
  }
}

const lines = (polylines: Polyline[] | undefined): FeatureCollection =>
  polylines ? { type: 'FeatureCollection', features: [{ type: 'Feature', properties: {}, geometry: { type: 'MultiLineString', coordinates: polylines } }] } : EMPTY

const points = (places: Place[] | undefined): FeatureCollection =>
  places
    ? {
        type: 'FeatureCollection',
        features: [...places]
          .sort((a, b) => b.pop - a.pop)
          .slice(0, 25)
          .map((p, i): Feature => ({ type: 'Feature', properties: { name: p.name, order: i }, geometry: { type: 'Point', coordinates: [p.lon, p.lat] } })),
      }
    : EMPTY

function bboxOf(f: Feature): [[number, number], [number, number]] | null {
  let w = Infinity
  let s = Infinity
  let e = -Infinity
  let n = -Infinity
  const walk = (c: unknown) => {
    if (!Array.isArray(c)) return
    if (typeof c[0] === 'number') {
      const [x, y] = c as [number, number]
      if (x < w) w = x
      if (x > e) e = x
      if (y < s) s = y
      if (y > n) n = y
    } else c.forEach(walk)
  }
  if (f.geometry.type !== 'GeometryCollection') walk(f.geometry.coordinates)
  return Number.isFinite(w)
    ? [
        [w, s],
        [e, n],
      ]
    : null
}

const fillColor = (colors: Map<string, string> | null, nodata: string): ExpressionSpecification | string =>
  colors && colors.size ? (['match', ['get', 'pcode'], ...[...colors].flat(), nodata] as unknown as ExpressionSpecification) : nodata

const fillOpacity = (dimmed: string[]): ExpressionSpecification | number =>
  dimmed.length ? (['match', ['get', 'pcode'], dimmed, 0.14, 0.88] as unknown as ExpressionSpecification) : 0.88

export function ExploreMap(props: Props) {
  const { theme, basemap, collection, provinces, colors, dimmed, selected, layers, overlays, reducedMotion, handle } = props
  const container = useRef<HTMLDivElement | null>(null)
  const mapRef = useRef<MapLibreMap | null>(null)
  const ready = useRef(false)
  const hovered = useRef<string | null>(null)
  const latest = useRef(props)
  latest.current = props

  /** (Re)build every layer this page owns on top of whatever basemap just loaded. */
  const install = (map: MapLibreMap) => {
    if (map.getSource('adm')) return
    const p = latest.current
    const dark = p.theme === 'dark'
    const style = map.getStyle()
    const base = style.layers ?? []
    const hasGlyphs = Boolean(style.glyphs)

    const srcLayer = (l: (typeof base)[number]) => ('source-layer' in l ? l['source-layer'] : undefined)
    // The page draws its own place labels and admin boundaries; the basemap's would compete with the data.
    for (const l of base) {
      const sl = srcLayer(l)
      if ((l.type === 'symbol' && (sl === 'place' || sl === 'transportation_name')) || (sl === 'boundary' && !l.id.includes('country'))) map.setLayoutProperty(l.id, 'visibility', 'none')
    }
    // Stack: basemap land + roads < choropleth < basemap water < overlays < labels.
    // Lifting the water above the choropleth lets lakes, lagoons and the true coastline cut through it.
    const underLabels = base.find((l) => l.type === 'symbol' && (srcLayer(l) === 'transportation_name' || srcLayer(l) === 'place'))?.id
    const waterIds = base.filter((l) => l.type === 'fill' && srcLayer(l) === 'water').map((l) => l.id)
    for (const id of waterIds) map.moveLayer(id, underLabels)
    const underWater = waterIds[0] ?? underLabels

    const ink = dark ? '#e8edf2' : '#14181c'
    const paper = dark ? '#0b0e12' : '#ffffff'
    // reference overlays take their hue from the palette ramps, never from the accent
    const river = ramp('water', p.theme)[dark ? 5 : 4]
    const park = ramp('vegetation', p.theme)[dark ? 5 : 5]
    const vis = (on: boolean) => (on ? 'visible' : 'none') as 'visible' | 'none'

    map.addSource('adm', { type: 'geojson', data: p.collection ?? EMPTY, promoteId: 'pcode' })
    map.addSource('adm1', { type: 'geojson', data: p.provinces ?? EMPTY })
    map.addSource('rail', { type: 'geojson', data: lines(p.overlays.rail) })
    map.addSource('rivers', { type: 'geojson', data: lines(p.overlays.rivers) })
    map.addSource('protected', { type: 'geojson', data: p.overlays.protectedAreas ?? EMPTY })
    map.addSource('places', { type: 'geojson', data: points(p.overlays.places) })

    map.addLayer(
      { id: 'adm-fill', type: 'fill', source: 'adm', paint: { 'fill-color': fillColor(p.colors, NODATA_HEX[p.theme]), 'fill-opacity': fillOpacity(p.dimmed), 'fill-antialias': true } },
      underWater,
    )
    map.addLayer(
      {
        id: 'adm-line',
        type: 'line',
        source: 'adm',
        layout: { 'line-join': 'round' },
        paint: { 'line-color': dark ? '#0b0e12' : '#14181c', 'line-opacity': dark ? 0.75 : 0.32, 'line-width': ['interpolate', ['linear'], ['zoom'], 6, 0.5, 10, 1.1] },
      },
      underWater,
    )
    map.addLayer(
      {
        id: 'adm1-line',
        type: 'line',
        source: 'adm1',
        layout: { 'line-join': 'round' },
        paint: { 'line-color': dark ? '#0b0e12' : '#14181c', 'line-opacity': dark ? 0.9 : 0.5, 'line-width': ['interpolate', ['linear'], ['zoom'], 6, 0.9, 10, 1.8] },
      },
      underWater,
    )
    map.addLayer(
      {
        id: 'protected-fill',
        type: 'fill',
        source: 'protected',
        layout: { visibility: vis(p.layers.protected) },
        paint: { 'fill-color': park, 'fill-opacity': dark ? 0.22 : 0.3 },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'protected-line',
        type: 'line',
        source: 'protected',
        layout: { visibility: vis(p.layers.protected), 'line-join': 'round' },
        paint: { 'line-color': park, 'line-opacity': 0.95, 'line-width': 1, 'line-dasharray': [2, 1.5] },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'rivers-line',
        type: 'line',
        source: 'rivers',
        layout: { visibility: vis(p.layers.rivers), 'line-join': 'round', 'line-cap': 'round' },
        paint: { 'line-color': river, 'line-opacity': 0.95, 'line-width': ['interpolate', ['linear'], ['zoom'], 6, 1, 10, 2.2] },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'rail-casing',
        type: 'line',
        source: 'rail',
        layout: { visibility: vis(p.layers.rail), 'line-join': 'round' },
        paint: { 'line-color': paper, 'line-opacity': 0.55, 'line-width': ['interpolate', ['linear'], ['zoom'], 6, 1.8, 10, 3.4] },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'rail-line',
        type: 'line',
        source: 'rail',
        layout: { visibility: vis(p.layers.rail), 'line-join': 'round' },
        paint: { 'line-color': ink, 'line-opacity': 0.9, 'line-width': ['interpolate', ['linear'], ['zoom'], 6, 0.7, 10, 1.4], 'line-dasharray': [5, 2.5] },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'adm-hover',
        type: 'line',
        source: 'adm',
        filter: byPcode(hovered.current),
        layout: { 'line-join': 'round' },
        paint: { 'line-color': dark ? '#ffffff' : '#14181c', 'line-width': 1.5, 'line-opacity': 0.95 },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'adm-selected-casing',
        type: 'line',
        source: 'adm',
        filter: byPcode(p.selected),
        layout: { 'line-join': 'round' },
        paint: { 'line-color': paper, 'line-width': 4.5, 'line-opacity': dark ? 0.7 : 0.9 },
      },
      underLabels,
    )
    map.addLayer(
      {
        id: 'adm-selected',
        type: 'line',
        source: 'adm',
        filter: byPcode(p.selected),
        layout: { 'line-join': 'round' },
        paint: { 'line-color': SELECT_HEX[p.theme], 'line-width': 2 },
      },
      underLabels,
    )
    map.addLayer({
      id: 'places-dot',
      type: 'circle',
      source: 'places',
      layout: { visibility: vis(p.layers.places) },
      paint: { 'circle-radius': ['interpolate', ['linear'], ['zoom'], 6, 1.8, 10, 3.2], 'circle-color': ink, 'circle-stroke-color': paper, 'circle-stroke-width': 1.2 },
    })
    if (hasGlyphs) {
      // borrow the basemap's own town-label face so the page's labels sit in the same typographic system
      const fonts = base.flatMap((l) => (l.type === 'symbol' && srcLayer(l) === 'place' && Array.isArray(l.layout?.['text-font']) ? [l.layout['text-font'] as string[]] : []))
      const font = fonts.find((f) => !/italic/i.test(f.join())) ?? fonts[0] ?? ['Noto Sans Regular']
      map.addLayer({
        id: 'places-label',
        type: 'symbol',
        source: 'places',
        layout: {
          visibility: vis(p.layers.places),
          'text-field': ['get', 'name'],
          'text-font': font,
          'text-size': ['interpolate', ['linear'], ['zoom'], 6, 10.5, 10, 13],
          'text-anchor': 'top',
          'text-offset': [0, 0.45],
          'text-padding': 4,
          'text-letter-spacing': 0.01,
          'symbol-sort-key': ['get', 'order'],
        },
        paint: { 'text-color': ink, 'text-halo-color': paper, 'text-halo-width': 1.4, 'text-halo-blur': 0.4 },
      })
    }
  }

  /* ---- map lifecycle ---- */
  useEffect(() => {
    if (!container.current) return
    const p = latest.current
    const map = new MapLibreMap({
      container: container.current,
      style: p.basemap ?? fallbackStyle(p.theme),
      center: [80.75, 7.85],
      zoom: 6.6,
      maxBounds: MAX_BOUNDS,
      minZoom: 6.2,
      maxZoom: 13,
      attributionControl: false,
      dragRotate: false,
      pitchWithRotate: false,
      touchPitch: false,
      renderWorldCopies: false,
      fadeDuration: 120,
    })
    map.touchZoomRotate.disableRotation()
    map.keyboard.disableRotation()
    map.addControl(new AttributionControl({ compact: true }), 'bottom-right')
    // panels are persistent camera padding, so "centre" always means the visible map area
    map.setPadding(p.padding)
    map.fitBounds(LK_BOUNDS, { padding: FIT_PAD, duration: 0 })
    mapRef.current = map

    map.on('style.load', () => {
      ready.current = true
      install(map)
    })

    const fine = window.matchMedia('(hover: hover) and (pointer: fine)')
    const featureAt = (point: { x: number; y: number }) => {
      if (!ready.current || !map.getLayer('adm-fill')) return null
      const f = map.queryRenderedFeatures([point.x, point.y], { layers: ['adm-fill'] })[0]
      return (f?.properties?.pcode as string | undefined) ?? null
    }
    const setHover = (pcode: string | null) => {
      if (pcode === hovered.current) return
      hovered.current = pcode
      if (ready.current && map.getLayer('adm-hover')) map.setFilter('adm-hover', byPcode(pcode))
      map.getCanvas().style.cursor = pcode ? 'pointer' : ''
      latest.current.onHover(pcode)
    }
    map.on('mousemove', (e) => {
      if (!fine.matches) return
      setHover(featureAt(e.point))
      const tip = latest.current.tooltipRef.current
      if (!tip) return
      // position imperatively: no React work per mouse move
      const box = map.getContainer().getBoundingClientRect()
      const w = tip.offsetWidth
      const h = tip.offsetHeight
      // flip away from the right/bottom panels, not just the viewport edge
      const pad = latest.current.padding
      const x = e.point.x + 16 + w > box.width - (pad.right ?? 0) - 8 ? e.point.x - w - 14 : e.point.x + 16
      const y = e.point.y + 18 + h > box.height - (pad.bottom ?? 0) - 8 ? e.point.y - h - 12 : e.point.y + 18
      tip.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(y)}px, 0)`
    })
    map.on('mouseout', () => setHover(null))
    map.on('dragstart', () => setHover(null))
    map.on('click', (e) => latest.current.onSelect(featureAt(e.point)))

    const ro = new ResizeObserver(() => map.resize())
    ro.observe(container.current)
    return () => {
      ro.disconnect()
      ready.current = false
      mapRef.current = null
      map.remove()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  /* ---- basemap + theme: swap the style, layers are rebuilt on style.load ---- */
  const firstStyle = useRef(true)
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    if (firstStyle.current) {
      firstStyle.current = false
      return
    }
    ready.current = false
    map.setStyle(basemap ?? fallbackStyle(theme), { diff: false })
  }, [basemap, theme])

  /* ---- camera: panels are persistent padding, so the view stays centred in the free area ---- */
  const { padding } = props
  const pendingBox = useRef<{ box: [[number, number], [number, number]]; extra: number; maxZoom?: number } | null>(null)
  /** Fit a box inside the *target* padding, even while the padding itself is still easing. */
  const flushCamera = () => {
    const map = mapRef.current
    const job = pendingBox.current
    pendingBox.current = null
    if (!map || !job) return
    const target = latest.current.padding
    const cur = map.getPadding()
    const side = (k: 'top' | 'bottom' | 'left' | 'right') => (target[k] ?? 0) - (cur[k] ?? 0) + job.extra
    const cam = map.cameraForBounds(job.box, { padding: { top: side('top'), bottom: side('bottom'), left: side('left'), right: side('right') }, maxZoom: job.maxZoom })
    if (!cam || cam.zoom === undefined) return
    const [[w, s], [e, n]] = job.box
    const mercY = (lat: number) => Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))
    const midLat = (Math.atan(Math.exp((mercY(s) + mercY(n)) / 2)) * 360) / Math.PI - 90
    map.easeTo({ center: [(w + e) / 2, midLat], zoom: cam.zoom, padding: target, duration: latest.current.reducedMotion ? 0 : 650, easing: easeOut })
  }
  const firstPadding = useRef(true)
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    if (firstPadding.current) {
      firstPadding.current = false
      return
    }
    if (pendingBox.current) flushCamera()
    else map.easeTo({ padding, duration: reducedMotion ? 0 : 380, easing: easeOut })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [padding, reducedMotion])

  /* ---- data-only updates: never touch geometry when only colours change ---- */
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current) return
    ;(map.getSource('adm') as GeoJSONSource | undefined)?.setData(collection ?? EMPTY)
  }, [collection])
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current) return
    ;(map.getSource('adm1') as GeoJSONSource | undefined)?.setData(provinces ?? EMPTY)
  }, [provinces])
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current || !map.getLayer('adm-fill')) return
    map.setPaintProperty('adm-fill', 'fill-color', fillColor(colors, NODATA_HEX[theme]))
  }, [colors, theme])
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current || !map.getLayer('adm-fill')) return
    map.setPaintProperty('adm-fill', 'fill-opacity', fillOpacity(dimmed))
  }, [dimmed])
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current || !map.getLayer('adm-selected')) return
    map.setFilter('adm-selected', byPcode(selected))
    map.setFilter('adm-selected-casing', byPcode(selected))
  }, [selected])
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current) return
    const set = (ids: string[], on: boolean) => {
      for (const id of ids) if (map.getLayer(id)) map.setLayoutProperty(id, 'visibility', on ? 'visible' : 'none')
    }
    set(['rail-line', 'rail-casing'], layers.rail)
    set(['rivers-line'], layers.rivers)
    set(['protected-fill', 'protected-line'], layers.protected)
    set(['places-dot', 'places-label'], layers.places)
  }, [layers])
  useEffect(() => {
    const map = mapRef.current
    if (!map || !ready.current) return
    const src = (id: string) => map.getSource(id) as GeoJSONSource | undefined
    src('rail')?.setData(lines(overlays.rail))
    src('rivers')?.setData(lines(overlays.rivers))
    src('protected')?.setData(overlays.protectedAreas ?? EMPTY)
    src('places')?.setData(points(overlays.places))
  }, [overlays])

  useImperativeHandle(
    handle,
    () => {
      // deferred a frame so a selection made in the same click can change the padding first
      const fit = (box: [[number, number], [number, number]], extra: number, maxZoom?: number) => {
        pendingBox.current = { box, extra, maxZoom }
        requestAnimationFrame(() => pendingBox.current && flushCamera())
      }
      return {
        zoomIn: () => mapRef.current?.zoomIn({ duration: 220 }),
        zoomOut: () => mapRef.current?.zoomOut({ duration: 220 }),
        reset: () => fit(LK_BOUNDS, FIT_PAD),
        focus: (pcode) => {
          const f = latest.current.collection?.features.find((x) => x.properties.pcode === pcode)
          const box = f && bboxOf(f)
          if (box) fit(box, (mapRef.current?.getContainer().clientWidth ?? 1024) < 768 ? 20 : 56, 9.4)
        },
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  )

  // MapLibre forces position:relative on its container, so the sizing lives on a wrapper
  return (
    <div className="explore-map absolute inset-0">
      <div ref={container} className="size-full" role="application" aria-label="Choropleth map of Sri Lanka" />
    </div>
  )
}
