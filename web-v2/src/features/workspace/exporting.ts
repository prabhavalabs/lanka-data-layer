// Export and share: all client-side (10 §6.4). The image is the map as drawn, with its title, its
// legend and its attribution composited into the bitmap, so a shared picture cannot lose them.
import { encodeDocument, type MapDocument } from '@/lib/map-spec'
import { hexToRgb } from '@/lib/palettes'
import type { LegendModel } from './engine/compile'
import type { MapEngine } from './engine/MapEngine'
import type { AreaShape } from './exportData'
import { fmt, unitSuffix } from './format'

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '') || 'map'
function download(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 2000)
}

export function exportJson(doc: MapDocument) {
  download(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }), `${slug(doc.title)}.ldlmap.json`)
}
/** `/explore#d=…`: the whole document in the fragment, no server state. */
export async function shareLink(doc: MapDocument): Promise<string> {
  return `${location.origin}/explore#d=${await encodeDocument(doc)}`
}

type Entry = { title: string; model: LegendModel }
const FONT = '"Geist Variable", system-ui, sans-serif'
const MONO = '"Geist Mono Variable", ui-monospace, monospace'

function drawLegend(ctx: CanvasRenderingContext2D, e: Entry, x: number, y: number, w: number, ink: string, ink3: string): number {
  ctx.fillStyle = ink
  ctx.font = `500 12px ${FONT}`
  ctx.textBaseline = 'top'
  ctx.textAlign = 'left'
  let title = e.title
  while (ctx.measureText(title).width > w && title.length > 8) title = `${title.slice(0, -2).trimEnd()}…`
  ctx.fillText(title, x, y)
  y += 20
  const m = e.model
  ctx.font = `10px ${MONO}`
  ctx.fillStyle = ink3
  if (m.kind === 'classes') {
    const n = m.colors.length
    m.colors.forEach((c, i) => {
      ctx.fillStyle = c
      ctx.fillRect(x + (i * w) / n, y, w / n - 1, 9)
    })
    ctx.fillStyle = ink3
    ctx.textAlign = 'center'
    m.breaks.forEach((b, i) => (n > 6 && i % 2 ? null : ctx.fillText(fmt(b, m.unit), x + ((i + 1) * w) / n, y + 14)))
    ctx.textAlign = 'left'
    return y + 34
  }
  if (m.kind === 'ramp' || m.kind === 'heat') {
    const g = ctx.createLinearGradient(x, 0, x + w, 0)
    m.stops.forEach((c, i) => g.addColorStop(i / (m.stops.length - 1), c))
    ctx.fillStyle = g
    ctx.fillRect(x, y, w, 9)
    ctx.fillStyle = ink3
    if (m.kind === 'ramp') {
      ctx.fillText(fmt(m.domain[0], m.unit), x, y + 14)
      ctx.textAlign = 'right'
      ctx.fillText(`${fmt(m.domain[1], m.unit)} ${unitSuffix(m.unit)}`, x + w, y + 14)
      ctx.textAlign = 'left'
    } else ctx.fillText('sparse → dense', x, y + 14)
    return y + 34
  }
  if (m.kind === 'categories') {
    m.items.slice(0, 6).forEach((it, i) => {
      const cx = x + (i % 2) * (w / 2), cy = y + Math.floor(i / 2) * 17
      ctx.fillStyle = it.color
      if (it.shape === 'line' || it.shape === 'dash' || it.shape === 'ring') ctx.fillRect(cx, cy + 4, 12, 2)
      else {
        ctx.beginPath()
        ctx.arc(cx + 5, cy + 5, 4.5, 0, Math.PI * 2)
        ctx.fill()
      }
      ctx.fillStyle = ink3
      ctx.font = `11px ${FONT}`
      ctx.fillText(it.label.replace(/^./, (c) => c.toUpperCase()), cx + 17, cy - 1)
    })
    return y + Math.ceil(Math.min(6, m.items.length) / 2) * 17 + 10
  }
  if (m.kind === 'bivariate') {
    const s = 15
    ;[6, 7, 8, 3, 4, 5, 0, 1, 2].forEach((k, i) => {
      ctx.fillStyle = m.colors[k]
      ctx.fillRect(x + (i % 3) * s, y + Math.floor(i / 3) * s, s - 1, s - 1)
    })
    ctx.fillStyle = ink3
    ctx.font = `11px ${FONT}`
    ctx.fillText(`↑ ${m.y}`, x + s * 3 + 12, y + 4)
    ctx.fillText(`→ ${m.x}`, x + s * 3 + 12, y + 24)
    return y + s * 3 + 12
  }
  return y
}

type Crop = { left: number; top: number; right: number; bottom: number }

/**
 * The map with its caption (title, legend, attribution) as a PNG blob, drawn at least at 2x.
 *
 * Without an `area` it is the visible map with the caption laid over its corners. With one, the map is first
 * framed on the area and cut to its outline (transparent outside it, unless `mask` is off), the canvas is
 * cropped to the area's box, and the caption goes in a band below the map so it covers none of the area.
 */
export async function renderPng(engine: MapEngine, doc: MapDocument, entries: Entry[], crop: Crop, dark: boolean, credits: string[], area?: { shape: AreaShape; mask: boolean; name: string }): Promise<Blob | null> {
  await document.fonts?.ready
  const ratio = engine.map.getPixelRatio()
  const sharp = Math.max(2, ratio)
  if (sharp !== ratio) engine.map.setPixelRatio(sharp)
  let shot: HTMLCanvasElement
  let rings: [number, number][][] | null = null
  try {
    if (area) await engine.frameNow(area.shape.bbox)
    else await engine.settle()
    shot = await engine.snapshot()
    // the outline in CSS pixels of the map container, while the camera is where the snapshot was taken
    if (area) rings = outlineRings(engine, area.shape)
  } finally {
    if (sharp !== ratio) engine.map.setPixelRatio(ratio)
  }
  const box = engine.map.getContainer().getBoundingClientRect()
  const k = shot.width / box.width
  let left = Math.max(0, crop.left), top = Math.max(0, crop.top), right = box.width - Math.max(0, crop.right), bottom = box.height - Math.max(0, crop.bottom)
  if (rings) {
    const xs = rings.flat().map((q) => q[0]), ys = rings.flat().map((q) => q[1])
    const m = 12
    left = Math.max(left, Math.min(...xs) - m)
    top = Math.max(top, Math.min(...ys) - m)
    right = Math.min(right, Math.max(...xs) + m)
    bottom = Math.min(bottom, Math.max(...ys) + m)
  }
  const mw = right - left, mh = bottom - top

  const ink = dark ? '#e8edf2' : '#14181c', ink3 = dark ? '#9aa7b4' : '#515c66'
  const [r, g, b] = hexToRgb(dark ? '#0e1217' : '#ffffff')
  if (area) credits = [`Clipped to ${area.name}`, ...credits]

  // the caption's size, measured before the canvas is made: a clipped map gets a band of that height below it
  const shown = entries.filter((e) => e.model.kind !== 'relief').slice(0, 4)
  const cw = 292, pad = 14, gap = 14
  const measure = document.createElement('canvas').getContext('2d')!
  let ch = pad + 22 + 8
  for (const e of shown) ch = drawLegend(measure, e, 0, ch, cw - pad * 2, ink, ink3) + 4
  ch += pad - 8
  const W = area ? Math.max(mw, cw + 260 + gap * 3) : mw
  measure.font = `10.5px ${FONT}`
  const maxW = Math.max(180, W - cw - gap * 3 - 16)
  const lines: string[] = []
  for (const part of [...credits, '© OpenStreetMap contributors · OpenFreeMap']) {
    const last = lines[lines.length - 1]
    if (last !== undefined && measure.measureText(`${last} · ${part}`).width <= maxW) lines[lines.length - 1] = `${last} · ${part}`
    else lines.push(part)
  }
  const lh = 15, bh = lines.length * lh + 8
  const band = area ? Math.max(ch, bh) + gap * 2 : 0
  const H = mh + band

  const out = document.createElement('canvas')
  out.width = Math.max(1, Math.round(W * k))
  out.height = Math.max(1, Math.round(H * k))
  const ctx = out.getContext('2d')!
  ctx.scale(k, k)
  // the map, centred when the band made the canvas wider than the area
  const mx = (W - mw) / 2
  ctx.drawImage(shot, left * k, top * k, mw * k, mh * k, mx, 0, mw, mh)
  if (rings && area?.mask) {
    const path = () => {
      ctx.beginPath()
      for (const ring of rings!) ring.forEach(([x, y], i) => (i ? ctx.lineTo(x - left + mx, y - top) : ctx.moveTo(x - left + mx, y - top)))
    }
    // keep only what lies inside the boundary (holes stay out: even-odd), but never the band below
    ctx.save()
    ctx.beginPath()
    ctx.rect(0, 0, W, mh)
    ctx.clip()
    ctx.globalCompositeOperation = 'destination-in'
    path()
    ctx.fill('evenodd')
    ctx.restore()
    // and draw the boundary itself, so the cut reads as a border rather than a ragged edge
    ctx.save()
    ctx.strokeStyle = dark ? 'rgba(232,237,242,0.55)' : 'rgba(20,24,28,0.5)'
    ctx.lineWidth = 1
    path()
    ctx.stroke()
    ctx.restore()
  }

  // caption: title, then as many legend entries as fit
  const x = gap, y = H - ch - gap
  ctx.fillStyle = `rgba(${r},${g},${b},0.9)`
  ctx.beginPath()
  ctx.roundRect(x, y, cw, ch, 12)
  ctx.fill()
  ctx.strokeStyle = dark ? 'rgba(255,255,255,0.1)' : 'rgba(20,24,28,0.12)'
  ctx.lineWidth = 1
  ctx.stroke()
  ctx.fillStyle = ink
  ctx.font = `600 15px ${FONT}`
  ctx.textBaseline = 'top'
  let title = doc.title
  while (ctx.measureText(title).width > cw - pad * 2 && title.length > 8) title = `${title.slice(0, -2).trimEnd()}…`
  ctx.fillText(title, x + pad, y + pad)
  let cy = y + pad + 22 + 8
  for (const e of shown) cy = drawLegend(ctx, e, x + pad, cy, cw - pad * 2, ink, ink3) + 4

  // attribution (and every notice of the resolved document) is part of the bitmap, wrapped to the free width
  ctx.font = `10.5px ${FONT}`
  const tw = Math.min(maxW, Math.max(...lines.map((l) => ctx.measureText(l).width)))
  ctx.fillStyle = `rgba(${r},${g},${b},0.82)`
  ctx.beginPath()
  ctx.roundRect(W - tw - gap - 16, H - gap - bh, tw + 16, bh, 10)
  ctx.fill()
  ctx.fillStyle = ink3
  ctx.textBaseline = 'middle'
  lines.forEach((l, i) => ctx.fillText(l, W - tw - gap - 8, H - gap - bh + 4 + lh * i + lh / 2, maxW))

  return new Promise<Blob | null>((res) => out.toBlob(res, 'image/png'))
}

/** Every ring of the area's outline, projected to the map container's CSS pixels. */
function outlineRings(engine: MapEngine, shape: AreaShape): [number, number][][] {
  const polys = shape.geometry.type === 'Polygon' ? [shape.geometry.coordinates] : shape.geometry.coordinates
  return polys.flatMap((poly) => poly.map((ring) => ring.map(([lng, lat]) => {
    const p = engine.map.project([lng, lat])
    return [p.x, p.y] as [number, number]
  })))
}
