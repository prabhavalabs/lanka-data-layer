// Value grids are coloured in the browser: the server ships numbers once, and the ramp, the classes,
// the range mask and the relief shading are a repaint of a canvas that MapLibre draws as one quad.
// (The production path does the same in a fragment shader over `ldl-v16` value tiles, 10 §3.2.)
import { hexToRgb, rampAt } from '@/lib/palettes'
import type { Grid } from '@/mocks/synth'

export type RasterPaint = {
  /** class colours when `breaks` is set, ramp stops otherwise */
  colors: string[]
  breaks: number[] | null
  domain: [number, number]
  transform: 'linear' | 'sqrt' | 'log'
  /** values outside are not drawn */
  range: [number | null, number | null]
  /** shaded relief to multiply in, 0 … 1 with 0.5 as flat ground */
  relief: Grid | null
  /** interpolate to twice the grid's resolution before colouring, so class edges are curves and not stairs */
  smooth: boolean
}

const warp = (v: number, t: RasterPaint['transform']) => (t === 'log' ? Math.log(Math.max(1e-9, v)) : t === 'sqrt' ? Math.sqrt(Math.max(0, v)) : v)

export function paintRaster(canvas: HTMLCanvasElement, grid: Grid, p: RasterPaint) {
  const s = p.smooth ? 2 : 1
  const W = grid.w * s, H = grid.h * s
  if (canvas.width !== W || canvas.height !== H) {
    canvas.width = W
    canvas.height = H
  }
  const ctx = canvas.getContext('2d')!
  const img = ctx.createImageData(W, H)
  const out = new Uint32Array(img.data.buffer)

  // colour lookup: one entry per class, or 256 steps along the ramp
  const stepped = p.breaks !== null
  const n = stepped ? p.colors.length : 256
  const lut = new Uint32Array(n)
  const rgb = new Uint8Array(n * 3)
  for (let i = 0; i < n; i++) {
    const c = stepped ? hexToRgb(p.colors[i]) : rampAt(p.colors, i / 255)
    rgb[i * 3] = c[0]
    rgb[i * 3 + 1] = c[1]
    rgb[i * 3 + 2] = c[2]
    lut[i] = (255 << 24) | (Math.round(c[2]) << 16) | (Math.round(c[1]) << 8) | Math.round(c[0])
  }
  const breaks = p.breaks ?? []
  const d0 = warp(p.domain[0], p.transform), d1 = warp(p.domain[1], p.transform)
  const span = d1 - d0 || 1
  const lo = p.range[0] ?? -Infinity, hi = p.range[1] ?? Infinity
  const src = grid.data, shade = p.relief?.data
  const gw = grid.w, gh = grid.h

  for (let Y = 0; Y < H; Y++) {
    const y = s === 1 ? Y : (Y + 0.5) / 2 - 0.5
    const y0 = Math.max(0, Math.floor(y)), y1 = Math.min(gh - 1, y0 + 1), fy = s === 1 ? 0 : Math.max(0, y - y0)
    for (let X = 0; X < W; X++) {
      let v: number
      let k: number
      if (s === 1) {
        k = Y * gw + X
        v = src[k]
      } else {
        const x = (X + 0.5) / 2 - 0.5
        const x0 = Math.max(0, Math.floor(x)), x1 = Math.min(gw - 1, x0 + 1), fx = Math.max(0, x - x0)
        const a = src[y0 * gw + x0], b = src[y0 * gw + x1], c = src[y1 * gw + x0], d = src[y1 * gw + x1]
        k = (fy < 0.5 ? y0 : y1) * gw + (fx < 0.5 ? x0 : x1)
        // at the coast one of the four is empty: take the nearest cell rather than bleed nothing into something
        v = a !== a || b !== b || c !== c || d !== d ? src[k] : (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy
      }
      if (v !== v || v < lo || v > hi) continue
      let i: number
      if (stepped) {
        i = 0
        while (i < breaks.length && v >= breaks[i]) i++
      } else {
        const t = (warp(v, p.transform) - d0) / span
        i = t <= 0 ? 0 : t >= 1 ? 255 : (t * 255) | 0
      }
      if (shade) {
        const m = 0.5 + shade[k]
        const r = Math.min(255, rgb[i * 3] * m), g = Math.min(255, rgb[i * 3 + 1] * m), bl = Math.min(255, rgb[i * 3 + 2] * m)
        out[Y * W + X] = (255 << 24) | (bl << 16) | (g << 8) | r
      } else out[Y * W + X] = lut[i]
    }
  }
  ctx.putImageData(img, 0, 0)
}

/** Shaded relief as light and shadow over whatever is below: shadows carry the layer, highlights only a little. */
export function paintHillshade(canvas: HTMLCanvasElement, relief: Grid, dark: boolean) {
  if (canvas.width !== relief.w || canvas.height !== relief.h) {
    canvas.width = relief.w
    canvas.height = relief.h
  }
  const ctx = canvas.getContext('2d')!
  const img = ctx.createImageData(relief.w, relief.h)
  const d = img.data
  const [sr, sg, sb] = dark ? [0, 0, 0] : [38, 52, 66]
  for (let i = 0; i < relief.data.length; i++) {
    const s = relief.data[i]
    if (s !== s) continue
    const o = i * 4
    if (s < 0.5) {
      d[o] = sr
      d[o + 1] = sg
      d[o + 2] = sb
      d[o + 3] = Math.min(215, (0.5 - s) * 2 * 255 * (dark ? 1.25 : 0.95))
    } else {
      d[o] = d[o + 1] = d[o + 2] = 255
      d[o + 3] = Math.min(150, (s - 0.5) * 2 * 255 * (dark ? 0.5 : 0.75))
    }
  }
  ctx.putImageData(img, 0, 0)
}

/** A sample of the grid's values (land only), for histograms and class breaks. */
export function gridSample(grid: Grid, range: [number | null, number | null] = [null, null], max = 24000): number[] {
  // an odd stride, so the sample does not march down one column of the grid
  const step = Math.max(1, Math.floor(grid.data.length / (max * 2.2))) | 1
  const out: number[] = []
  const lo = range[0] ?? -Infinity, hi = range[1] ?? Infinity
  for (let i = 0; i < grid.data.length; i += step) {
    const v = grid.data[i]
    if (v === v && v >= lo && v <= hi) out.push(v)
  }
  return out
}
