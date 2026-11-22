// Export and share: all client-side (10 §6.4). The image is the map as drawn, with its title, its
// legend and its attribution composited into the bitmap, so a shared picture cannot lose them.
import { encodeDocument, type MapDocument } from '@/lib/map-spec'
import { hexToRgb } from '@/lib/palettes'
import type { LegendModel } from './engine/compile'
import type { MapEngine } from './engine/MapEngine'
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

/** The visible map area with a caption block drawn in: title, legend, attribution. */
export async function exportPng(engine: MapEngine, doc: MapDocument, entries: Entry[], crop: { left: number; top: number; right: number; bottom: number }, dark: boolean) {
  await document.fonts?.ready
  const shot = await engine.snapshot()
  const box = engine.map.getContainer().getBoundingClientRect()
  const k = shot.width / box.width
  const sx = Math.max(0, crop.left) * k, sy = Math.max(0, crop.top) * k
  const sw = shot.width - sx - Math.max(0, crop.right) * k, sh = shot.height - sy - Math.max(0, crop.bottom) * k
  const out = document.createElement('canvas')
  out.width = Math.round(sw)
  out.height = Math.round(sh)
  const ctx = out.getContext('2d')!
  ctx.drawImage(shot, sx, sy, sw, sh, 0, 0, out.width, out.height)
  ctx.scale(k, k)
  const W = out.width / k, H = out.height / k
  const ink = dark ? '#e8edf2' : '#14181c', ink3 = dark ? '#9aa7b4' : '#515c66'
  const [r, g, b] = hexToRgb(dark ? '#0e1217' : '#ffffff')

  // caption: title, then as many legend entries as fit
  const shown = entries.filter((e) => e.model.kind !== 'relief').slice(0, 4)
  const cw = 292, pad = 14
  ctx.font = `600 15px ${FONT}`
  const measure = document.createElement('canvas').getContext('2d')!
  let h = pad + 22 + 8
  for (const e of shown) h = drawLegend(measure, e, 0, h, cw - pad * 2, ink, ink3) + 4
  h += pad - 8
  const x = 14, y = H - h - 14
  ctx.fillStyle = `rgba(${r},${g},${b},0.9)`
  ctx.beginPath()
  ctx.roundRect(x, y, cw, h, 12)
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

  // attribution is part of the bitmap
  const credit = 'Lanka Data Layer · mock data · © OpenStreetMap contributors · OpenFreeMap'
  ctx.font = `10.5px ${FONT}`
  const tw = ctx.measureText(credit).width
  ctx.fillStyle = `rgba(${r},${g},${b},0.82)`
  ctx.beginPath()
  ctx.roundRect(W - tw - 14 - 16, H - 14 - 22, tw + 16, 22, 11)
  ctx.fill()
  ctx.fillStyle = ink3
  ctx.textBaseline = 'middle'
  ctx.fillText(credit, W - tw - 14 - 8, H - 14 - 11)

  const blob = await new Promise<Blob | null>((res) => out.toBlob(res, 'image/png'))
  if (blob) download(blob, `${slug(doc.title)}.png`)
}
