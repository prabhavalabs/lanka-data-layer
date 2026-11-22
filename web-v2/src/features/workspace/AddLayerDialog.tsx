// "Add layer": the mappable catalogue, searchable, with a real preview of the selected dataset drawn
// the way the chosen preset would draw it.
import { useQuery } from '@tanstack/react-query'
import { Check, CircleDot, Grid3x3, Hexagon, Plus, Search, Spline } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { paintTile, islandTiles } from '@/api/real/valueTiles'
import { catalogueQuery, layerDataQuery, reliefQuery, type CatalogueItem, type LayerData, type Preset } from '@/api/workspace'
import { LicenceLight } from '@/components/common/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { geo } from '@/lib/geo'
import type { Theme } from '@/lib/hooks'
import { instantiate, LAYER_TYPE_LABEL, type FieldBinding } from '@/lib/map-spec'
import { toneColor } from '@/lib/palettes'
import { cn } from '@/lib/utils'
import type { Grid } from '@/mocks/synth'
import { colorAt, columnAt, compileLayer, isBinding, layerScheme, resolveScale } from './engine/compile'
import { paintHillshade } from './engine/raster'
import { resolvedFor, useResolved } from './context'
import { actions } from './store'

const SHAPE_ICON = { Areas: Hexagon, Grid: Grid3x3, Points: CircleDot, Lines: Spline, Polygons: Hexagon }
const CLASS_BADGE: Record<string, { label: string; variant: 'neutral' | 'warn' | 'outline' }> = { official: { label: 'Published', variant: 'outline' }, estimated: { label: 'Estimate', variant: 'neutral' }, synthetic: { label: 'Synthetic', variant: 'warn' } }
const mercY = (lat: number) => Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))

/** The dataset in miniature: the same compile step the map uses, drawn onto a small canvas. */
function Preview({ preset, scheme: mapScheme }: { preset: Preset; scheme: Theme }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const sources = useMemo(() => ({ p: preset.source }), [preset.source])
  const resolution = useResolved({ sources, pin: 'latest' })
  const { data } = useQuery(layerDataQuery(preset.source, { sources: {}, analyses: {} }, null, resolvedFor('p', resolution)))
  const { data: country } = useQuery({ queryKey: ['geo', 'country'], queryFn: geo.country, staleTime: Infinity })
  // a frame on the boundary tiles is previewed on the bundled province and district outlines (same p-codes)
  const level = data?.kind === 'frame' && data.tiles ? data.support.level : 0
  const { data: outlines } = useQuery({ queryKey: ['geo', `adm${level}`], queryFn: () => (level === 1 ? geo.adm1() : geo.adm2()), staleTime: Infinity, enabled: level === 1 || level === 2 })
  // value tiles: the overview tiles, coloured exactly as the map will colour them
  const [tiles, setTiles] = useState<{ bitmap: ImageBitmap; west: number; east: number; north: number; south: number }[] | null>(null)
  useEffect(() => {
    setTiles(null)
    if (data?.kind !== 'values') return
    const src = compileLayer(instantiate(preset).layer, preset.source, data, { scheme: layerScheme(preset.blend, mapScheme), time: null, glyphs: false, font: [], relief: () => undefined }).sources[0]
    if (src?.kind !== 'raster') return
    let live = true
    void Promise.all(islandTiles().map(async (t) => ({ ...t, bitmap: await paintTile(src.tiles[0].replace('{z}', String(t.z)).replace('{x}', String(t.x)).replace('{y}', String(t.y))) }))).then((out) => live && setTiles(out))
    return () => {
      live = false
    }
  }, [data, preset, mapScheme])
  const { data: relief } = useQuery(reliefQuery(preset.style.exaggeration ?? 1, preset.type === 'hillshade' || !!preset.style['x-relief']))
  const scheme = layerScheme(preset.blend, mapScheme)
  useEffect(() => {
    const c = canvas.current
    if (!c || !country) return
    const dpr = Math.min(2, window.devicePixelRatio || 1)
    const W = c.clientWidth, H = c.clientHeight
    c.width = W * dpr
    c.height = H * dpr
    const ctx = c.getContext('2d')!
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, W, H)
    // frame: the data's own extent when it is local (Western Province), the island otherwise
    let [w, s, e, n] = [79.45, 5.85, 82.0, 9.9]
    const local = data && (data.kind === 'frame' || data.kind === 'boundaries') && (data.kind === 'frame' ? data.support.level : data.level) >= 3
    if (local) [w, s, e, n] = [79.74, 6.3, 80.36, 7.36]
    const k = Math.min((W - 20) / ((e - w) * (Math.PI / 180)), (H - 20) / (mercY(n) - mercY(s)))
    const X = (lon: number) => W / 2 + (lon - (w + e) / 2) * (Math.PI / 180) * k
    const Y = (lat: number) => H / 2 - (mercY(lat) - (mercY(n) + mercY(s)) / 2) * k
    const ring = (r: number[][]) => {
      r.forEach(([x, y], i) => (i ? ctx.lineTo(X(x), Y(y)) : ctx.moveTo(X(x), Y(y))))
      ctx.closePath()
    }
    const dark = mapScheme === 'dark'
    ctx.beginPath()
    country.rings.forEach(ring)
    ctx.fillStyle = dark ? '#161c25' : '#ffffff'
    ctx.fill()
    ctx.save()
    ctx.clip()
    if (tiles) {
      ctx.imageSmoothingEnabled = true
      for (const t of tiles) ctx.drawImage(t.bitmap, X(t.west), Y(t.north), X(t.east) - X(t.west), Y(t.south) - Y(t.north))
    } else if (data?.kind === 'frame' && data.tiles) {
      // one multipolygon per code, from the bundled outlines (provinces, districts) or the decoded shapes (DS divisions)
      const pool = (data.support.level <= 2 ? outlines?.features : data.geometry.features) ?? []
      const parts = new Map<string, number[][][][]>()
      for (const f of pool as GeoJSON.Feature[]) {
        const g = f.geometry
        const polys = g?.type === 'Polygon' ? [g.coordinates] : g?.type === 'MultiPolygon' ? g.coordinates : []
        const k = String(f.properties?.pcode)
        parts.set(k, [...(parts.get(k) ?? []), ...polys])
      }
      draw(ctx, preset, { ...data, geometry: { type: 'FeatureCollection', features: data.codes.map((c) => ({ type: 'Feature', properties: { pcode: c }, geometry: { type: 'MultiPolygon', coordinates: parts.get(c) ?? [] } })) } }, scheme, X, Y, ring, relief)
    }
    else if (data) draw(ctx, preset, data, scheme, X, Y, ring, relief)
    ctx.restore()
    ctx.beginPath()
    country.rings.forEach(ring)
    ctx.strokeStyle = dark ? 'rgba(232,237,242,0.28)' : 'rgba(20,24,28,0.3)'
    ctx.lineWidth = 0.8
    ctx.stroke()
  }, [data, country, relief, preset, scheme, mapScheme, tiles, outlines])
  const tiledNote = data && ((data.kind === 'features' && data.tiles) || (data.kind === 'boundaries' && data.tiles) || (data.kind === 'frame' && data.tiles && data.support.level >= 4)) ? (data.kind === 'features' ? 'drawn from vector tiles on the map' : 'drawn on the boundary tiles on the map') : data?.kind === 'dem' ? 'shaded from the terrain tiles on the map' : null
  return (
    <div className="relative overflow-hidden rounded-xl border border-line bg-bg3/50">
      <canvas ref={canvas} className="block h-[236px] w-full" role="img" aria-label={`Preview of ${preset.name}`} />
      {!data && <div className="skeleton absolute inset-0 rounded-none opacity-60" />}
      {tiledNote && <p className="absolute inset-x-0 bottom-2 text-center font-mono text-[10.5px] text-ink3">{tiledNote}</p>}
    </div>
  )
}
function draw(ctx: CanvasRenderingContext2D, preset: Preset, data: LayerData, scheme: Theme, X: (lon: number) => number, Y: (lat: number) => number, ring: (r: number[][]) => void, relief: Grid | undefined) {
  const layer = instantiate(preset).layer
  const env = { scheme, time: null, glyphs: false, font: [], relief: () => relief }
  if (data.kind === 'values' || data.kind === 'dem') return
  if (data.kind === 'grid') {
    const tmp = document.createElement('canvas')
    if (preset.type === 'hillshade') {
      if (!relief) return
      paintHillshade(tmp, relief, scheme === 'dark')
    } else {
      const src = compileLayer({ ...layer, style: { ...layer.style, resample: 'nearest' } }, preset.source, data, env).sources[0]
      if (src?.kind !== 'canvas') return
      src.paint(tmp)
    }
    const g = data.grid
    ctx.imageSmoothingEnabled = true
    ctx.globalAlpha = preset.type === 'hillshade' ? 0.9 : 1
    ctx.drawImage(tmp, X(g.west), Y(g.north), X(g.east) - X(g.west), Y(g.south) - Y(g.north))
    ctx.globalAlpha = 1
    return
  }
  const polys = (g: GeoJSON.Geometry | null) => (!g ? [] : g.type === 'Polygon' ? [g.coordinates] : g.type === 'MultiPolygon' ? g.coordinates : [])
  if (data.kind === 'frame') {
    const b = isBinding(preset.style.fill) ? preset.style.fill : null
    const { col, cols } = columnAt(data, b?.field ?? 'value', preset.source, null)
    const scale = b && col ? resolveScale(cols.flatMap((c) => c.values).filter((v): v is number => typeof v === 'number'), b.scale, scheme) : null
    data.geometry.features.forEach((f, i) => {
      ctx.beginPath()
      for (const p of polys(f.geometry)) p.forEach(ring)
      const v = col?.values[i]
      ctx.fillStyle = scale && typeof v === 'number' ? colorAt(scale, v) : 'transparent'
      ctx.fill()
      ctx.strokeStyle = scheme === 'dark' ? 'rgba(11,14,18,0.7)' : 'rgba(255,255,255,0.8)'
      ctx.lineWidth = 0.5
      ctx.stroke()
    })
    return
  }
  const features = data.kind === 'boundaries' ? data.collection.features : data.collection.features
  const fields = data.kind === 'features' ? data.fields : []
  const bound = (c: unknown): ((p: Record<string, unknown> | null) => string) => {
    if (!isBinding(c)) return () => toneColor(typeof c === 'string' && c !== 'none' ? c : 'ink', scheme)
    const b = c as FieldBinding
    const meta = fields.find((f) => f.id === b.field)
    const scale = resolveScale(features.map((f) => Number(f.properties?.[b.field])).filter(Number.isFinite), b.scale, scheme, meta?.categories)
    return (p) => colorAt(scale, p?.[b.field] as number | string)
  }
  if (preset.type === 'label' || preset.type === 'line' || preset.type === 'polygon') {
    const fill = preset.type === 'polygon' && preset.style.fill !== 'none' && preset.style.fill !== undefined ? bound(preset.style.fill) : null
    const stroke = bound(preset.style.stroke?.color)
    for (const f of features) {
      const g = f.geometry
      ctx.beginPath()
      if (g?.type === 'LineString') g.coordinates.forEach(([x, y], i) => (i ? ctx.lineTo(X(x), Y(y)) : ctx.moveTo(X(x), Y(y))))
      else for (const p of polys(g)) p.forEach(ring)
      if (fill) {
        ctx.globalAlpha = 0.75
        ctx.fillStyle = fill(f.properties)
        ctx.fill()
        ctx.globalAlpha = 1
      }
      ctx.strokeStyle = stroke(f.properties)
      ctx.lineWidth = preset.type === 'line' ? 1 : 0.7
      ctx.globalAlpha = preset.type === 'label' ? 0.35 : 0.95
      ctx.stroke()
      ctx.globalAlpha = 1
    }
    return
  }
  const heat = resolveScale([0, 1], { type: 'continuous', ramp: preset.style.ramp ?? 'ldl.seq.orange' }, scheme)
  const color = preset.type === 'heatmap' ? () => colorAt(heat, 0.7) : bound(preset.style.fill)
  ctx.globalAlpha = preset.type === 'heatmap' ? 0.1 : 0.85
  for (const f of features) {
    if (f.geometry?.type !== 'Point') continue
    const [x, y] = f.geometry.coordinates
    ctx.beginPath()
    ctx.arc(X(x), Y(y), preset.type === 'heatmap' ? 5 : preset.type === 'cluster' ? 2.2 : 1.5, 0, Math.PI * 2)
    ctx.fillStyle = color(f.properties)
    ctx.fill()
  }
  ctx.globalAlpha = 1
}

export function AddLayerDialog({ open, onOpenChange, scheme }: { open: boolean; onOpenChange: (open: boolean) => void; scheme: Theme }) {
  const { data: catalogue } = useQuery({ ...catalogueQuery(scheme), enabled: open })
  const [q, setQ] = useState('')
  const [itemId, setItemId] = useState<string | null>(null)
  const [presetId, setPresetId] = useState<string | null>(null)
  const list = useRef<HTMLDivElement>(null)
  const needle = q.trim().toLowerCase()
  const hits = useMemo(() => (catalogue ?? []).filter((c) => !needle || [c.title, c.summary, c.theme, c.publisher, c.shape, ...c.presets.map((p) => `${p.label} ${p.name} ${LAYER_TYPE_LABEL[p.type]}`)].join(' ').toLowerCase().includes(needle)), [catalogue, needle])
  const groups = useMemo(() => {
    const out: { theme: string; items: CatalogueItem[] }[] = []
    for (const c of hits) (out.find((g) => g.theme === c.theme) ?? out[out.push({ theme: c.theme, items: [] }) - 1]).items.push(c)
    return out
  }, [hits])
  const item = hits.find((c) => c.id === itemId) ?? hits[0]
  const preset = item?.presets.find((p) => p.id === presetId) ?? item?.presets[0]
  useEffect(() => {
    if (open) {
      setQ('')
      setItemId(null)
      setPresetId(null)
    }
  }, [open])
  const add = (p: Preset | undefined = preset) => {
    if (!p) return
    actions.add([p])
    onOpenChange(false)
  }
  const move = (d: number) => {
    const i = hits.findIndex((c) => c.id === item?.id)
    const next = hits[Math.max(0, Math.min(hits.length - 1, i + d))]
    if (!next) return
    setItemId(next.id)
    setPresetId(null)
    list.current?.querySelector(`[data-item="${CSS.escape(next.id)}"]`)?.scrollIntoView({ block: 'nearest' })
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby="add-layer-desc" className="top-[9vh] flex h-[min(620px,82vh)] max-w-[880px] flex-col overflow-hidden max-md:top-0 max-md:h-[100dvh] max-md:w-screen max-md:max-w-none max-md:rounded-none max-md:border-0">
        <div className="px-5 pt-4 pb-3 max-md:pt-[max(16px,env(safe-area-inset-top))]">
          <DialogTitle className="text-[15px] font-semibold tracking-[-0.015em] text-ink">Add layer</DialogTitle>
          <DialogDescription id="add-layer-desc" className="mt-0.5 text-[12.5px] text-ink3">
            Any dataset, indicator or grid in the catalogue. It goes above the selected layer.
          </DialogDescription>
          <div className="relative mt-3">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink3" />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                  e.preventDefault()
                  move(e.key === 'ArrowDown' ? 1 : -1)
                } else if (e.key === 'Enter') add()
              }}
              placeholder="Search flood, prices, hospitals, rail…"
              aria-label="Search the catalogue"
              className="h-10 w-full rounded-lg border border-line2 bg-bg3/50 pr-3 pl-9 text-[14px] text-ink outline-none placeholder:text-ink3 focus:border-ink3 max-md:h-11"
            />
          </div>
        </div>
        <div className="flex min-h-0 flex-1 border-t border-line max-md:flex-col">
          <div ref={list} role="listbox" aria-label="Datasets" className="scroll-thin min-h-0 w-[340px] shrink-0 overflow-y-auto p-2 max-md:w-full max-md:flex-1">
            {!catalogue && Array.from({ length: 8 }, (_, i) => <div key={i} className="skeleton mx-1 my-2 h-10" />)}
            {catalogue && hits.length === 0 && <p className="px-3 py-10 text-center text-[13px] text-ink3">Nothing in the catalogue matches “{q}”.</p>}
            {groups.map((g) => (
              <div key={g.theme} className="mb-1">
                <div className="t-label px-2.5 pt-2.5 pb-1.5 tracking-[0.08em] uppercase">{g.theme}</div>
                {g.items.map((c) => {
                  const Icon = SHAPE_ICON[c.shape]
                  const on = c.id === item?.id
                  return (
                    <button
                      key={c.id}
                      type="button"
                      role="option"
                      aria-selected={on}
                      data-item={c.id}
                      onClick={() => {
                        setItemId(c.id)
                        setPresetId(null)
                      }}
                      onDoubleClick={() => add(c.presets[0])}
                      className={cn('pressable flex min-h-11 w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left outline-none', on ? 'bg-bg4 text-ink shadow-[inset_0_0_0_1px_var(--line2)]' : 'text-ink2 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3')}
                    >
                      <Icon className={cn('size-4 shrink-0', on ? 'text-ink2' : 'text-ink3')} strokeWidth={1.7} />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13.5px] leading-tight font-medium">{c.title}</span>
                        <span className="mt-0.5 block truncate text-[11.5px] text-ink3">{c.sample ? `Sample data · ${c.publisher}` : c.publisher}</span>
                      </span>
                      <span className="shrink-0 font-mono text-[10.5px] text-ink3">{c.shape}</span>
                    </button>
                  )
                })}
              </div>
            ))}
          </div>
          {item && preset && (
            <div className="flex min-h-0 min-w-0 flex-1 flex-col border-l border-line max-md:max-h-[46%] max-md:flex-none max-md:border-t max-md:border-l-0">
              <div className="scroll-thin min-h-0 flex-1 overflow-y-auto p-5 max-md:p-4">
                <div className="flex gap-4">
                  <div className="w-[210px] shrink-0 max-md:hidden">
                    <Preview preset={preset} scheme={scheme} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <h3 className="min-w-0 truncate text-[16px] font-semibold tracking-[-0.02em] text-ink">{item.title}</h3>
                      <Badge variant={CLASS_BADGE[item.class].variant} className="shrink-0">
                        {CLASS_BADGE[item.class].label}
                      </Badge>
                      {item.sample && item.class !== 'synthetic' && (
                        <Badge variant="outline" className="shrink-0" title="Bundled sample data: the engine does not serve this dataset">
                          Sample
                        </Badge>
                      )}
                    </div>
                    <p className="mt-1.5 text-[13px] leading-[1.5] text-ink2">{item.summary}</p>
                    <dl className="mt-3.5 grid grid-cols-[76px_1fr] gap-x-3 gap-y-1.5 text-[12.5px] max-md:hidden">
                      <dt className="text-ink3">Publisher</dt>
                      <dd className="truncate text-ink2">{item.publisher}</dd>
                      <dt className="text-ink3">Licence</dt>
                      <dd>
                        <LicenceLight status={item.licence.status} label={item.licence.label} />
                      </dd>
                      <dt className="text-ink3">Coverage</dt>
                      <dd className="text-ink2">{item.coverage}</dd>
                      <dt className="text-ink3">Updated</dt>
                      <dd className="t-num font-mono text-[11.5px] text-ink2">{item.updated}</dd>
                    </dl>
                  </div>
                </div>
                <div className="t-label mt-5 mb-2 tracking-[0.08em] uppercase max-md:mt-3">Draw as</div>
                <div role="radiogroup" aria-label="How to draw it" className="grid grid-cols-2 gap-2 max-md:grid-cols-1">
                  {item.presets.map((p) => {
                    const on = p.id === preset.id
                    return (
                      <button key={p.id} type="button" role="radio" aria-checked={on} onClick={() => setPresetId(p.id)} className={cn('pressable flex min-h-[52px] items-center gap-3 rounded-xl border px-3 py-2 text-left outline-none', on ? 'border-ink/55 bg-ink/[0.06]' : 'border-line2 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/60')}>
                        <span className={cn('grid size-4 shrink-0 place-items-center rounded-full border', on ? 'border-ink bg-ink text-bg' : 'border-line2')}>{on && <Check className="size-2.5" strokeWidth={3.5} />}</span>
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center gap-2 text-[13px] font-medium text-ink">
                            <span className="truncate">{p.label}</span>
                            <span className="shrink-0 font-mono text-[10px] font-normal text-ink3 uppercase">{LAYER_TYPE_LABEL[p.type]}</span>
                          </span>
                          <span className="mt-0.5 block truncate text-[12px] text-ink3">{p.hint}</span>
                        </span>
                      </button>
                    )
                  })}
                </div>
              </div>
              <div className="flex items-center gap-3 border-t border-line px-5 py-3 max-md:px-4 max-md:pb-[max(12px,env(safe-area-inset-bottom))]">
                <span className="min-w-0 flex-1 truncate text-[12px] text-ink3">
                  Adds “{preset.name}”{preset.blend && preset.blend !== 'normal' ? `, blended ${preset.blend}` : ''}
                </span>
                <Button variant="primary" size="md" onClick={() => add()} className="max-md:h-11">
                  <Plus />
                  Add layer
                </Button>
              </div>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
