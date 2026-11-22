// The map behind the workspace: owns the MapLibre instance and keeps it equal to the document.
//
// React hands it the layer tree and the data each layer resolved to; `sync()` compiles what changed,
// diffs it against what is installed, and touches only that. Things that change every frame of a
// drag (opacity) bypass React and the diff entirely: `previewOpacity` writes one paint property or
// one compositor uniform.
import type { Feature, FeatureCollection } from 'geojson'
import {
  AttributionControl,
  Map as MapLibreMap,
  setWorkerUrl,
  type CanvasSource,
  type GeoJSONSource,
  type LayerSpecification,
  type LngLatBoundsLike,
  type MapGeoJSONFeature,
  type PaddingOptions,
  type StyleSpecification,
} from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import type { LayerData } from '@/api/workspace'
import { LK_BOUNDS } from '@/lib/geo'
import type { Theme } from '@/lib/hooks'
import { isGroup, isTimeBound, type Layer, type LayerNode, type MapDocument, type Source, type TimeConfig } from '@/lib/map-spec'
import { paperColor, toneColor } from '@/lib/palettes'
import type { Grid } from '@/mocks/synth'
import { compileLayer, type Compiled, type Env, type LegendModel, type MlLayer, type MlSource, type Tip } from './compile'
import { Compositor, isoId } from './compositor'

setWorkerUrl(workerUrl)

const MAX_BOUNDS: LngLatBoundsLike = [
  [73.4, 3.1],
  [88.1, 12.5],
]
const GLYPHS = 'https://tiles.openfreemap.org/fonts/{fontstack}/{range}.pbf'
const easeOut = (t: number) => 1 - (1 - t) ** 4
const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }
const same = (a: unknown, b: unknown) => a === b || JSON.stringify(a) === JSON.stringify(b)
// property names and values are produced by the style compiler, not written by hand: the typed overloads add nothing here
const setPaint = (map: MapLibreMap, id: string, prop: string, value: unknown) => (map.setPaintProperty as (id: string, prop: string, value: unknown) => unknown).call(map, id, prop, value)
const setLayout = (map: MapLibreMap, id: string, prop: string, value: unknown) => (map.setLayoutProperty as (id: string, prop: string, value: unknown) => unknown).call(map, id, prop, value)
const cssVar = (name: string, fallback: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback
export const SELECT_HEX: Record<Theme, string> = { dark: '#d8446b', light: '#c22f5a' }

export function blankStyle(scheme: Theme): StyleSpecification {
  return { version: 8, glyphs: GLYPHS, sources: {}, layers: [{ id: 'bg', type: 'background', paint: { 'background-color': cssVar('--bg', scheme === 'dark' ? '#0b0e12' : '#fbfbfa') } }] }
}

export type Hover = { layerId: string; tip: Tip; code?: string }
export type EngineState = {
  layers: LayerNode[]
  sources: Record<string, Source>
  data: Map<string, LayerData | undefined>
  scheme: Theme
  time: TimeConfig | null
  basemap: MapDocument['basemap']
  solo: string | null
  relief: Map<number, Grid>
}
export type EngineEvents = {
  onHover: (hover: Hover | null) => void
  onClick: (hit: { layerId: string; code?: string; properties: Record<string, unknown> } | null) => void
  onView: (view: { center: [number, number]; zoom: number }) => void
  /** legends and problems changed */
  onCompiled: () => void
  tooltip: () => HTMLElement | null
}
type Installed = { spec: MlLayer; opacity: string }
type InstalledSource = { kind: MlSource['kind']; options: string; data: unknown; key: string; canvas?: HTMLCanvasElement }

function bboxOf(fc: FeatureCollection | Feature): [[number, number], [number, number]] | null {
  let w = Infinity, s = Infinity, e = -Infinity, n = -Infinity
  const visit = (c: unknown) => {
    if (!Array.isArray(c)) return
    if (typeof c[0] === 'number') {
      const [x, y] = c as [number, number]
      if (x < w) w = x
      if (x > e) e = x
      if (y < s) s = y
      if (y > n) n = y
    } else c.forEach(visit)
  }
  for (const f of 'features' in fc ? fc.features : [fc]) if (f.geometry && f.geometry.type !== 'GeometryCollection') visit(f.geometry.coordinates)
  return Number.isFinite(w) ? [[w, s], [e, n]] : null
}

export class MapEngine {
  readonly map: MapLibreMap
  private ready = false
  private state: EngineState | null = null
  private padding: PaddingOptions
  private reducedMotion: boolean
  private readonly events: EngineEvents
  private readonly compositor = new Compositor()

  private installed = new Map<string, Installed>()
  private sources = new Map<string, InstalledSource>()
  private isoInstalled = new Set<string>()
  private compiled = new Map<string, { inputs: unknown[]; out: Compiled }>()
  private owner = new Map<string, string>()
  private order = ''
  private staleCanvas = new Set<string>()
  /** opacity being dragged, by node id: wins over the document until the drag is committed */
  private preview = new Map<string, number>()
  private forceIso = new Set<string>()
  private isoNow = new Set<string>()
  private hitLayers: string[] = []
  private rasterTop: string[] = []

  private firstSymbol: string | undefined
  private symbolIds: string[] = []
  private waterIds: string[] = []
  private waterHome: string | undefined
  private font: string[] = ['Noto Sans Regular']
  private glyphs = true

  private pendingBox: { box: [[number, number], [number, number]]; extra: number; maxZoom?: number } | null = null
  private pointer: { x: number; y: number; lng: number; lat: number } | null = null
  private pickRaf = 0
  private hoverKey = ''
  private selected: Feature | null = null
  private disposed = false

  constructor(container: HTMLElement, opts: { scheme: Theme; basemap: StyleSpecification | null | undefined; padding: PaddingOptions; reducedMotion: boolean; view: MapDocument['view'] }, events: EngineEvents) {
    this.padding = opts.padding
    this.reducedMotion = opts.reducedMotion
    this.events = events
    const map = new MapLibreMap({
      container,
      style: opts.basemap ?? blankStyle(opts.scheme),
      center: opts.view.center,
      zoom: opts.view.zoom,
      maxBounds: MAX_BOUNDS,
      minZoom: 5.6,
      maxZoom: 14,
      attributionControl: false,
      dragRotate: false,
      pitchWithRotate: false,
      touchPitch: false,
      renderWorldCopies: false,
      fadeDuration: 120,
    })
    this.map = map
    map.touchZoomRotate.disableRotation()
    map.keyboard.disableRotation()
    map.addControl(new AttributionControl({ compact: true }), 'bottom-right')
    map.setPadding(opts.padding)
    this.applyView(opts.view, 0)
    this.compositor.attach(map)

    map.on('style.load', () => {
      this.ready = true
      this.onStyle()
      this.sync()
    })
    map.on('moveend', () => {
      const c = map.getCenter()
      events.onView({ center: [+c.lng.toFixed(4), +c.lat.toFixed(4)], zoom: +map.getZoom().toFixed(2) })
      this.flushCanvases()
    })
    const fine = window.matchMedia('(hover: hover) and (pointer: fine)')
    map.on('mousemove', (e) => {
      if (!fine.matches) return
      this.pointer = { x: e.point.x, y: e.point.y, lng: e.lngLat.lng, lat: e.lngLat.lat }
      this.placeTooltip()
      if (!this.pickRaf) this.pickRaf = requestAnimationFrame(() => this.pick())
    })
    map.on('mouseout', () => {
      this.pointer = null
      this.setHover(null, '')
    })
    map.on('dragstart', () => this.setHover(null, ''))
    map.on('click', (e) => {
      const hit = this.hitAt(e.point.x, e.point.y, e.lngLat.lng, e.lngLat.lat)
      events.onClick(hit ? { layerId: hit.layerId, code: hit.code, properties: hit.properties } : null)
    })
    const ro = new ResizeObserver(() => map.resize())
    ro.observe(container)
    this.dispose = () => {
      this.disposed = true
      ro.disconnect()
      cancelAnimationFrame(this.pickRaf)
      this.ready = false
      map.remove()
    }
  }
  dispose: () => void

  /* ---------------- basemap ---------------- */
  /** Remember where the basemap keeps its water and its labels, and quiet what the workspace draws itself. */
  private onStyle() {
    const map = this.map
    this.installed.clear()
    this.sources.clear()
    this.isoInstalled.clear()
    this.order = ''
    const style = map.getStyle()
    const base = style.layers ?? []
    const sl = (l: (typeof base)[number]) => ('source-layer' in l ? l['source-layer'] : undefined)
    this.glyphs = Boolean(style.glyphs)
    this.symbolIds = base.filter((l) => l.type === 'symbol').map((l) => l.id)
    // data goes over the basemap's land, water and roads, and under its road and place names
    this.firstSymbol = base.find((l) => l.type === 'symbol' && (sl(l) === 'place' || sl(l) === 'transportation_name'))?.id
    this.waterIds = base.filter((l) => l.type === 'fill' && sl(l) === 'water').map((l) => l.id)
    const lastWater = base.findIndex((l) => l.id === this.waterIds[this.waterIds.length - 1])
    this.waterHome = lastWater >= 0 ? base[lastWater + 1]?.id : undefined
    for (const l of base) if (sl(l) === 'boundary' && !l.id.includes('country')) map.setLayoutProperty(l.id, 'visibility', 'none')
    const fonts = base.flatMap((l) => (l.type === 'symbol' && sl(l) === 'place' && Array.isArray(l.layout?.['text-font']) ? [l.layout['text-font'] as string[]] : []))
    this.font = fonts.find((f) => !/italic/i.test(f.join())) ?? fonts[0] ?? ['Noto Sans Regular']
    this.addHatch()

    const scheme = this.state?.scheme ?? 'dark'
    const ink = toneColor('ink', scheme), paper = paperColor(scheme)
    map.addSource('ws-hl', { type: 'geojson', data: EMPTY })
    map.addSource('ws-sel', { type: 'geojson', data: this.selected ?? EMPTY })
    const add = (spec: LayerSpecification) => map.addLayer(spec, this.firstSymbol)
    add({ id: 'ws-hl-casing', type: 'line', source: 'ws-hl', layout: { 'line-join': 'round' }, paint: { 'line-color': paper, 'line-width': 4, 'line-opacity': 0.7 } })
    add({ id: 'ws-hl-line', type: 'line', source: 'ws-hl', layout: { 'line-join': 'round' }, paint: { 'line-color': ink, 'line-width': 1.6 } })
    add({ id: 'ws-hl-ring', type: 'circle', source: 'ws-hl', filter: ['==', ['geometry-type'], 'Point'], paint: { 'circle-radius': 8, 'circle-color': 'rgba(0,0,0,0)', 'circle-stroke-color': ink, 'circle-stroke-width': 1.6 } })
    add({ id: 'ws-sel-casing', type: 'line', source: 'ws-sel', layout: { 'line-join': 'round' }, paint: { 'line-color': paper, 'line-width': 4.5, 'line-opacity': 0.85 } })
    add({ id: 'ws-sel-line', type: 'line', source: 'ws-sel', layout: { 'line-join': 'round' }, paint: { 'line-color': SELECT_HEX[scheme], 'line-width': 2 } })
  }
  private addHatch() {
    const scheme = this.state?.scheme ?? 'dark'
    const c = document.createElement('canvas')
    c.width = c.height = 16
    const ctx = c.getContext('2d')!
    // lines that contrast with any class colour: light on a dark map, dark on a light one
    ctx.strokeStyle = scheme === 'dark' ? 'rgba(236,240,244,0.72)' : 'rgba(20,24,28,0.6)'
    ctx.lineWidth = 1.5
    for (const o of [-16, 0, 16]) {
      ctx.beginPath()
      ctx.moveTo(o, 16)
      ctx.lineTo(o + 16, 0)
      ctx.stroke()
    }
    if (this.map.hasImage('ws-hatch')) this.map.removeImage('ws-hatch')
    this.map.addImage('ws-hatch', ctx.getImageData(0, 0, 16, 16), { pixelRatio: 2 })
  }
  setBasemap(style: StyleSpecification | null | undefined, scheme: Theme) {
    this.ready = false
    this.map.setStyle(style ?? blankStyle(scheme), { diff: false })
  }

  /* ---------------- document → map ---------------- */
  setState(state: EngineState) {
    this.state = state
    this.sync()
  }
  private env(): Env {
    const s = this.state!
    return { scheme: s.scheme, time: s.time, glyphs: this.glyphs, font: this.font, relief: (z) => s.relief.get(z) }
  }
  private visible(node: LayerNode, chain: LayerNode[]): boolean {
    const s = this.state!
    if (!node.visible || chain.some((g) => !g.visible)) return false
    if (!s.solo) return true
    // solo: the node itself, what is inside it, and the groups that lead to it
    return node.id === s.solo || chain.some((g) => g.id === s.solo) || (isGroup(node) && this.contains(node, s.solo))
  }
  private contains(node: LayerNode, id: string): boolean {
    return node.id === id || (isGroup(node) && node.children.some((c) => this.contains(c, id)))
  }

  sync() {
    if (!this.ready || !this.state || this.disposed) return
    const map = this.map
    const s = this.state
    const env = this.env()
    const want = new Map<string, MlLayer>()
    const wantSources = new Map<string, MlSource>()
    const ops: string[] = []
    const iso = new Set<string>()
    const live = new Set<string>()
    const owner = new Map<string, string>()
    const hits: string[] = []
    const rasters: string[] = []
    let changed = false
    /** index in `ops` just above the unbroken run of area layers at the bottom of the stack */
    let coast = 0
    let coastOpen = true

    /** Emits a node's render ops; says whether it drew anything, and whether all of it was plain area fill. */
    const emit = (node: LayerNode, chain: LayerNode[]): { drew: boolean; areal: boolean } => {
      if (!this.visible(node, chain)) return { drew: false, areal: true }
      if (isGroup(node)) {
        const isolate = node.opacity < 0.999 || this.forceIso.has(node.id) || this.preview.has(node.id)
        const start = ops.length
        if (isolate) ops.push(isoId(node.id, false))
        let drew = false, areal = !isolate
        for (const c of node.children) {
          const r = emit(c, [...chain, node])
          drew ||= r.drew
          if (r.drew && !r.areal) areal = false
        }
        if (isolate) {
          if (ops.length === start + 1) ops.pop()
          else {
            ops.push(isoId(node.id, true))
            iso.add(node.id)
            this.compositor.params.set(node.id, { blend: 'normal', opacity: this.preview.get(node.id) ?? node.opacity })
          }
        }
        return { drew, areal }
      }
      live.add(node.id)
      const source = s.sources[node.source]
      const data = s.data.get(node.id)
      const inputs = [node.style, node.filter, node.type, node.name, node.blend, source, data, env.scheme, isTimeBound(source) ? env.time?.cursor : '', env.glyphs, node.style['x-relief'] ? s.relief.get(1) : node.type === 'hillshade' ? s.relief.get(node.style.exaggeration ?? 1) : null]
      let c = this.compiled.get(node.id)
      if (!c || c.inputs.some((v, i) => v !== inputs[i])) {
        c = { inputs, out: compileLayer(node, source, data, env) }
        this.compiled.set(node.id, c)
        changed = true
      }
      if (!c.out.layers.length) return { drew: false, areal: true }
      const isolate = node.blend !== 'normal' || this.forceIso.has(node.id)
      if (isolate) {
        ops.push(isoId(node.id, false))
        iso.add(node.id)
        this.compositor.params.set(node.id, { blend: node.blend, opacity: this.preview.get(node.id) ?? node.opacity })
      }
      for (const src of c.out.sources) wantSources.set(src.id, src)
      for (const l of c.out.layers) {
        want.set(l.id, l)
        owner.set(l.id, node.id)
        ops.push(l.id)
        if (l.hit && !node.locked && node.interaction?.hover !== false) hits.push(l.id)
      }
      if (c.out.probe && !node.locked) rasters.push(node.id)
      if (isolate) ops.push(isoId(node.id, true))
      return { drew: true, areal: c.out.areal && !isolate }
    }
    for (const node of s.layers) {
      const r = emit(node, [])
      if (!r.drew) continue
      if (coastOpen && r.areal) coast = ops.length
      else coastOpen = false
    }
    for (const id of [...this.compiled.keys()]) if (!live.has(id)) this.compiled.delete(id), (changed = true)

    // 1. remove what is no longer wanted (layers before the sources they read)
    for (const id of [...this.installed.keys()]) {
      const w = want.get(id)
      const cur = this.installed.get(id)!.spec
      if (!w || w.type !== cur.type || w.source !== cur.source || w.sourceLayer !== cur.sourceLayer) {
        if (map.getLayer(id)) map.removeLayer(id)
        this.installed.delete(id)
      }
    }
    for (const id of [...this.isoInstalled]) {
      if (iso.has(id)) continue
      for (const end of [false, true]) if (map.getLayer(isoId(id, end))) map.removeLayer(isoId(id, end))
      this.isoInstalled.delete(id)
      this.compositor.params.delete(id)
    }
    for (const [id, cur] of [...this.sources]) {
      const w = wantSources.get(id)
      const options = w?.kind === 'geojson' ? JSON.stringify(w.options ?? {}) : ''
      if (w && w.kind === cur.kind && options === cur.options) continue
      for (const [lid, l] of [...this.installed]) {
        if (l.spec.source !== id) continue
        if (map.getLayer(lid)) map.removeLayer(lid)
        this.installed.delete(lid)
      }
      if (map.getSource(id)) map.removeSource(id)
      this.sources.delete(id)
    }

    // 2. sources: add, or push new data into what is there
    for (const [id, w] of wantSources) {
      const cur = this.sources.get(id)
      if (w.kind === 'geojson') {
        if (!cur) {
          map.addSource(id, { type: 'geojson', data: w.data, ...(w.options ?? {}) })
          this.sources.set(id, { kind: 'geojson', options: JSON.stringify(w.options ?? {}), data: w.data, key: '' })
        } else if (cur.data !== w.data) {
          ;(map.getSource(id) as GeoJSONSource | undefined)?.setData(w.data)
          cur.data = w.data
        }
      } else if (w.kind === 'vector') {
        if (!cur) {
          map.addSource(id, { type: 'vector', url: w.url })
          this.sources.set(id, { kind: 'vector', options: '', data: null, key: '' })
        }
      } else {
        const f = w.frame
        if (!cur) {
          const canvas = document.createElement('canvas')
          w.paint(canvas)
          map.addSource(id, { type: 'canvas', canvas, animate: false, coordinates: [[f.west, f.north], [f.east, f.north], [f.east, f.south], [f.west, f.south]] })
          this.sources.set(id, { kind: 'canvas', options: '', data: null, key: w.key, canvas })
        } else if (cur.key !== w.key && cur.canvas) {
          w.paint(cur.canvas)
          cur.key = w.key
          this.staleCanvas.add(id)
        }
      }
    }
    this.flushCanvases()

    // 3. layers: add, or change only the properties that differ
    const anchor = s.basemap.labels === 'bottom' ? undefined : this.firstTop()
    for (const [id, w] of want) {
      const layer = this.docLayer(owner.get(id)!)
      const zoom = layer?.zoom
      const cur = this.installed.get(id)
      if (!cur) {
        map.addLayer(
          { id, type: w.type, source: w.source, ...(w.sourceLayer ? { 'source-layer': w.sourceLayer } : {}), ...(w.filter ? { filter: w.filter } : {}), ...(zoom ? { minzoom: zoom[0], maxzoom: zoom[1] } : {}), paint: w.paint, layout: w.layout } as LayerSpecification,
          anchor,
        )
        this.installed.set(id, { spec: w, opacity: '' })
        continue
      }
      const old = cur.spec
      if (old === w) continue
      for (const k of new Set([...Object.keys(old.paint), ...Object.keys(w.paint)])) if (!same(old.paint[k], w.paint[k])) setPaint(map, id, k, w.paint[k])
      for (const k of new Set([...Object.keys(old.layout), ...Object.keys(w.layout)])) if (!same(old.layout[k], w.layout[k])) setLayout(map, id, k, w.layout[k])
      if (!same(old.filter, w.filter)) map.setFilter(id, (w.filter ?? null) as never)
      if (!same(old.opacity, w.opacity)) cur.opacity = ''
      cur.spec = w
    }
    for (const [id] of want) {
      const zoom = this.docLayer(owner.get(id)!)?.zoom
      const l = map.getLayer(id)
      if (l && (l.minzoom !== (zoom?.[0] ?? 0) || l.maxzoom !== (zoom?.[1] ?? 24))) map.setLayerZoomRange(id, zoom?.[0] ?? 0, zoom?.[1] ?? 24)
    }
    for (const id of iso) {
      if (this.isoInstalled.has(id)) continue
      map.addLayer(this.compositor.begin(id), anchor)
      map.addLayer(this.compositor.end(id), anchor)
      this.isoInstalled.add(id)
    }

    // 4. order, with the basemap's water lifted over the area layers at the bottom so the true coast cuts them
    const lift = s.basemap['x-coast'] !== false && coast > 0
    const signature = `${ops.join('|')}#${anchor ?? ''}#${lift ? coast : -1}#${s.basemap.labels}`
    if (signature !== this.order) {
      this.order = signature
      let before: string | undefined = map.getLayer('ws-hl-casing') ? 'ws-hl-casing' : anchor
      for (let i = ops.length - 1; i >= 0; i--) {
        if (!map.getLayer(ops[i])) continue
        map.moveLayer(ops[i], before)
        before = ops[i]
      }
      for (const id of ['ws-hl-casing', 'ws-hl-line', 'ws-hl-ring', 'ws-sel-casing', 'ws-sel-line']) if (map.getLayer(id)) map.moveLayer(id, anchor)
      const waterBefore = lift ? (ops.slice(coast).find((id) => map.getLayer(id)) ?? 'ws-hl-casing') : (this.waterHome && map.getLayer(this.waterHome) ? this.waterHome : ops.find((id) => map.getLayer(id)))
      for (const id of this.waterIds) if (map.getLayer(id)) map.moveLayer(id, waterBefore)
      for (const id of this.symbolIds) if (map.getLayer(id)) map.setLayoutProperty(id, 'visibility', s.basemap.labels === 'off' ? 'none' : 'visible')
    }

    this.owner = owner
    this.isoNow = iso
    this.hitLayers = hits
    this.rasterTop = rasters.reverse()
    this.applyOpacity()
    if (changed) this.events.onCompiled()
  }
  private firstTop(): string | undefined {
    return this.firstSymbol && this.map.getLayer(this.firstSymbol) ? this.firstSymbol : undefined
  }
  private docLayer(id: string): Layer | null {
    const find = (nodes: LayerNode[]): Layer | null => {
      for (const n of nodes) {
        if (isGroup(n)) {
          const hit = find(n.children)
          if (hit) return hit
        } else if (n.id === id) return n
      }
      return null
    }
    return this.state ? find(this.state.layers) : null
  }
  /** After a canvas is repainted its texture must be re-uploaded; that only happens while the source has tiles on screen. */
  private flushCanvases() {
    for (const id of [...this.staleCanvas]) {
      const src = this.map.getSource(id) as (CanvasSource & { tiles?: Record<string, unknown> }) | undefined
      if (!src) {
        this.staleCanvas.delete(id)
        continue
      }
      if (!src.tiles || Object.keys(src.tiles).length === 0) continue
      src.play()
      src.pause()
      this.staleCanvas.delete(id)
    }
    this.map.triggerRepaint()
  }

  /* ---------------- opacity: the fast path ---------------- */
  private applyOpacity(only?: string) {
    const map = this.map
    for (const [id, inst] of this.installed) {
      const layerId = this.owner.get(id)
      if (!layerId || (only && layerId !== only)) continue
      const layer = this.docLayer(layerId)
      if (!layer) continue
      // an isolated layer is drawn at full strength and faded as one image by the compositor
      const f = this.isoNow.has(layerId) ? 1 : (this.preview.get(layerId) ?? layer.opacity)
      const key = String(f)
      if (inst.opacity === key) continue
      inst.opacity = key
      for (const [prop, base] of Object.entries(inst.spec.opacity)) {
        // never exactly 1: an opaque fill would be drawn in MapLibre's opaque pass, ahead of the isolation that should contain it
        const cap = prop === 'fill-opacity' ? 0.999 : 1
        setPaint(map, id, prop, typeof base === 'number' ? Math.min(cap, base * f) : f === 1 ? base : ['*', f, base])
      }
    }
  }
  /** While a slider is dragged: no document change, no React, one property (or one uniform) per frame. */
  previewOpacity(nodeId: string, value: number | null) {
    if (value === null) {
      this.preview.delete(nodeId)
      this.forceIso.delete(nodeId)
      return
    }
    this.preview.set(nodeId, value)
    const p = this.compositor.params.get(nodeId)
    if (this.isoNow.has(nodeId) && p) {
      p.opacity = value
      this.map.triggerRepaint()
    } else if (this.docLayer(nodeId)) this.applyOpacity(nodeId)
    else {
      // a group that was at 100 %: isolate it now, so the drag already shows the real result
      this.forceIso.add(nodeId)
      this.sync()
    }
  }

  /* ---------------- pointer ---------------- */
  private hitAt(x: number, y: number, lng: number, lat: number): { layerId: string; code?: string; properties: Record<string, unknown>; feature: MapGeoJSONFeature | null } | null {
    if (!this.ready) return null
    const layers = this.hitLayers.filter((id) => this.map.getLayer(id))
    const f = layers.length ? this.map.queryRenderedFeatures([[x - 3, y - 3], [x + 3, y + 3]], { layers })[0] : undefined
    if (f) return { layerId: this.owner.get(f.layer.id)!, code: typeof f.properties?.pcode === 'string' ? f.properties.pcode : undefined, properties: f.properties ?? {}, feature: f }
    for (const id of this.rasterTop) if (this.compiled.get(id)?.out.probe?.([lng, lat])) return { layerId: id, properties: {}, feature: null }
    return null
  }
  private pick() {
    this.pickRaf = 0
    const p = this.pointer
    if (!p || this.map.isMoving()) return
    const hit = this.hitAt(p.x, p.y, p.lng, p.lat)
    if (!hit) return this.setHover(null, '')
    const c = this.compiled.get(hit.layerId)?.out
    const tip = hit.feature ? c?.tip?.(hit.properties, [p.lng, p.lat]) : c?.probe?.([p.lng, p.lat])
    if (!tip) return this.setHover(null, '')
    const key = hit.feature ? `${hit.layerId}:${String(hit.properties.pcode ?? hit.properties.id ?? hit.feature.id ?? tip.title)}` : `${hit.layerId}:${tip.rows[0]?.value}`
    if (key === this.hoverKey) return
    const geometry = hit.feature ? (c?.lookup?.(hit.properties) ?? (hit.feature.geometry.type === 'Point' ? ({ type: 'Feature', properties: {}, geometry: hit.feature.geometry } as Feature) : null)) : null
    this.setHover({ layerId: hit.layerId, tip, code: hit.code }, key, geometry)
  }
  private setHover(hover: Hover | null, key: string, geometry: Feature | null = null) {
    if (key === this.hoverKey && !hover) return
    this.hoverKey = key
    if (this.ready) (this.map.getSource('ws-hl') as GeoJSONSource | undefined)?.setData(geometry ?? EMPTY)
    this.map.getCanvas().style.cursor = hover ? 'pointer' : ''
    this.events.onHover(hover)
    requestAnimationFrame(() => this.placeTooltip())
  }
  private placeTooltip() {
    const tip = this.events.tooltip()
    const p = this.pointer
    if (!tip || !p) return
    const box = this.map.getContainer().getBoundingClientRect()
    const w = tip.offsetWidth, h = tip.offsetHeight
    const pad = this.padding
    const x = p.x + 16 + w > box.width - (pad.right ?? 0) - 8 ? p.x - w - 14 : p.x + 16
    const y = p.y + 18 + h > box.height - (pad.bottom ?? 0) - 8 ? p.y - h - 12 : p.y + 18
    tip.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(y)}px, 0)`
  }
  /** Outline a unit from outside the map (a point hovered in the scatter). */
  highlight(code: string | null) {
    if (!this.ready) return
    let feature: Feature | null = null
    if (code) for (const { out } of this.compiled.values()) if ((feature = out.lookup?.({ pcode: code }) ?? null)) break
    ;(this.map.getSource('ws-hl') as GeoJSONSource | undefined)?.setData(feature ?? EMPTY)
  }
  select(code: string | null) {
    let feature: Feature | null = null
    if (code) for (const { out } of this.compiled.values()) if ((feature = out.lookup?.({ pcode: code }) ?? null)) break
    this.selected = feature
    if (this.ready) (this.map.getSource('ws-sel') as GeoJSONSource | undefined)?.setData(feature ?? EMPTY)
  }

  /* ---------------- what the panels read ---------------- */
  legend(layerId: string): LegendModel | null {
    return this.compiled.get(layerId)?.out.legend ?? null
  }
  problem(layerId: string): string | undefined {
    return this.compiled.get(layerId)?.out.problem
  }

  /* ---------------- camera ---------------- */
  private applyView(view: MapDocument['view'], duration: number) {
    if (view.bounds) {
      const [w, s, e, n] = view.bounds
      this.fit([[w, s], [e, n]], 18, undefined, duration)
    } else this.map.easeTo({ center: view.center, zoom: view.zoom, duration })
  }
  setView(view: MapDocument['view']) {
    this.applyView(view, this.reducedMotion ? 0 : 700)
  }
  setPadding(padding: PaddingOptions, reducedMotion: boolean) {
    this.padding = padding
    this.reducedMotion = reducedMotion
    if (this.pendingBox) this.flushCamera()
    else this.map.easeTo({ padding, duration: reducedMotion ? 0 : 380, easing: easeOut })
  }
  /** Fit a box inside the *target* padding, even while the padding itself is still easing. */
  private flushCamera(duration?: number) {
    const map = this.map
    const job = this.pendingBox
    this.pendingBox = null
    if (!job) return
    const target = this.padding
    const cur = map.getPadding()
    const side = (k: 'top' | 'bottom' | 'left' | 'right') => (target[k] ?? 0) - (cur[k] ?? 0) + job.extra
    const cam = map.cameraForBounds(job.box, { padding: { top: side('top'), bottom: side('bottom'), left: side('left'), right: side('right') }, ...(job.maxZoom !== undefined ? { maxZoom: job.maxZoom } : {}) })
    if (!cam || cam.zoom === undefined) return
    const [[w, s], [e, n]] = job.box
    const mercY = (lat: number) => Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))
    const midLat = (Math.atan(Math.exp((mercY(s) + mercY(n)) / 2)) * 360) / Math.PI - 90
    map.easeTo({ center: [(w + e) / 2, midLat], zoom: cam.zoom, padding: target, duration: duration ?? (this.reducedMotion ? 0 : 650), easing: easeOut })
  }
  private fit(box: [[number, number], [number, number]], extra: number, maxZoom?: number, duration?: number) {
    this.pendingBox = { box, extra, maxZoom }
    if (duration === 0) this.flushCamera(0)
    else requestAnimationFrame(() => this.pendingBox && this.flushCamera(duration))
  }
  zoomIn() {
    this.map.zoomIn({ duration: 220 })
  }
  zoomOut() {
    this.map.zoomOut({ duration: 220 })
  }
  reset(view?: MapDocument['view']) {
    if (view?.bounds) this.fit([[view.bounds[0], view.bounds[1]], [view.bounds[2], view.bounds[3]]], 18)
    else this.fit(LK_BOUNDS, 14)
  }
  /** Frame everything a layer draws. */
  fitLayer(layerId: string) {
    const data = this.state?.data.get(layerId)
    const box = !data ? null : data.kind === 'grid' ? ([[data.grid.west, data.grid.south], [data.grid.east, data.grid.north]] as [[number, number], [number, number]]) : bboxOf(data.kind === 'frame' ? data.geometry : data.collection)
    if (box) this.fit(data?.kind === 'grid' || (data?.kind === 'features' && data.tiles) ? LK_BOUNDS : box, 28, 12)
  }
  focus(code: string) {
    let feature: Feature | null = null
    for (const { out } of this.compiled.values()) if ((feature = out.lookup?.({ pcode: code }) ?? null)) break
    const box = feature && bboxOf(feature)
    if (box) this.fit(box, this.map.getContainer().clientWidth < 768 ? 20 : 56, 10.5)
  }

  /* ---------------- export ---------------- */
  /** The map as drawn right now, at device resolution. */
  snapshot(): Promise<HTMLCanvasElement> {
    return new Promise((resolve) => {
      this.map.once('render', () => {
        const src = this.map.getCanvas()
        const out = document.createElement('canvas')
        out.width = src.width
        out.height = src.height
        out.getContext('2d')!.drawImage(src, 0, 0)
        resolve(out)
      })
      this.map.triggerRepaint()
    })
  }
}
