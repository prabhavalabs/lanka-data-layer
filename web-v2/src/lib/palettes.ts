// Data colour system. Rose is the brand accent and is never used for data.
// One sequential ramp per indicator family, light -> dark. On the dark
// surface the ramp is mirrored so that "more" is always the higher-contrast end.
import type { Theme } from './hooks'

export type RampId = 'people' | 'access' | 'water' | 'vegetation' | 'built'

const RAMPS: Record<RampId, string[]> = {
  people: ['#efeaff', '#d6ccfb', '#b9a8f5', '#9b84ec', '#7c61dd', '#6046c0', '#463296'],
  access: ['#e3f6f2', '#bce8df', '#8dd5c8', '#59bdae', '#2fa191', '#1c8073', '#125e55'],
  water: ['#e8f2ff', '#c7dcfb', '#9dc2f6', '#6fa4ed', '#4685de', '#2e67c0', '#214b92'],
  vegetation: ['#ecf6e6', '#cde8c0', '#a8d695', '#7ec069', '#58a445', '#3e8432', '#2c6225'],
  built: ['#fff0e0', '#fdd8b2', '#f9ba7e', '#f2994b', '#e17926', '#bf5c15', '#914410'],
}
const DARK_RAMPS: Record<RampId, string[]> = {
  people: ['#241f45', '#34297a', '#4a39a8', '#6650cf', '#8870e6', '#ad9bf2', '#d9d0fb'],
  access: ['#0d2f2b', '#12473f', '#19645a', '#218678', '#35aa98', '#6fcdbd', '#b9ebe1'],
  water: ['#11264b', '#19397a', '#2550a6', '#356bce', '#5a8fe6', '#8fb6f2', '#cbdffb'],
  vegetation: ['#14301a', '#1f4a22', '#2d682c', '#40893a', '#5fab51', '#93cc82', '#cdeac2'],
  built: ['#3a1d0b', '#5e2e0f', '#884212', '#b75a18', '#e07b2a', '#f3a763', '#fbd7b0'],
}

export const ramp = (id: RampId, theme: Theme) => (theme === 'dark' ? DARK_RAMPS : RAMPS)[id]

/** Diverging: cool <- neutral -> warm. Midpoint is a neutral grey, never a hue. */
export const diverging = (theme: Theme) =>
  theme === 'dark'
    ? ['#5a8fe6', '#3f66ac', '#2c3f63', '#333b45', '#6b4526', '#b0611f', '#e8873a']
    : ['#2e67c0', '#6f9be0', '#b8cdf0', '#e3e5e6', '#f3c9a0', '#e3904a', '#bf5c15']

/** Fixed-order categorical set (identity). Assigned in order, never cycled. */
export const categorical = (theme: Theme) =>
  theme === 'dark'
    ? ['#8870e6', '#35aa98', '#e8a33d', '#5a8fe6', '#e07b2a', '#93cc82', '#c58be0', '#8a97a5']
    : ['#6046c0', '#1c8073', '#b9770e', '#2e67c0', '#bf5c15', '#3e8432', '#9a4fc0', '#687480']

/** Quantile breaks -> class index. */
export function quantileBreaks(values: number[], k: number): number[] {
  const s = [...values].sort((a, b) => a - b)
  return Array.from({ length: k - 1 }, (_, i) => s[Math.min(s.length - 1, Math.floor(((i + 1) / k) * s.length))])
}
export const classOf = (v: number, breaks: number[]) => {
  let i = 0
  while (i < breaks.length && v >= breaks[i]) i++
  return i
}

/* ------------------------------------------------------------------
   Ramp registry for the map workspace (10-map-composition §2.6): named
   ramps, resolved per colour scheme of the basemap they are drawn on.
   ------------------------------------------------------------------ */
export type RampName = `ldl.seq.${'violet' | 'teal' | 'blue' | 'green' | 'orange'}` | 'ldl.div.blue-orange' | 'ldl.cat.default'
export const SEQ_RAMPS: { name: RampName; label: string; id: RampId }[] = [
  { name: 'ldl.seq.blue', label: 'Blue', id: 'water' },
  { name: 'ldl.seq.teal', label: 'Teal', id: 'access' },
  { name: 'ldl.seq.green', label: 'Green', id: 'vegetation' },
  { name: 'ldl.seq.orange', label: 'Orange', id: 'built' },
  { name: 'ldl.seq.violet', label: 'Violet', id: 'people' },
]
export const RAMP_OF: Record<RampId, RampName> = { people: 'ldl.seq.violet', access: 'ldl.seq.teal', water: 'ldl.seq.blue', vegetation: 'ldl.seq.green', built: 'ldl.seq.orange' }
export const rampLabel = (name: string) => (name === 'ldl.div.blue-orange' ? 'Blue–orange' : (SEQ_RAMPS.find((r) => r.name === name)?.label ?? 'Blue'))

type RGB = [number, number, number]
export const hexToRgb = (hex: string): RGB => {
  const h = hex.replace('#', '')
  const n = parseInt(h.length === 3 ? h.replace(/./g, '$&$&') : h.slice(0, 6), 16)
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}
export const rgbToHex = ([r, g, b]: RGB) => `#${[r, g, b].map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, '0')).join('')}`

/** The stops of a named ramp, light → dark on a light map, mirrored on a dark one so "more" is always the stronger end. */
export function rampStops(name: string, theme: Theme, reverse = false): string[] {
  const stops = name === 'ldl.div.blue-orange' ? diverging(theme) : name === 'ldl.cat.default' ? categorical(theme) : ramp(SEQ_RAMPS.find((r) => r.name === name)?.id ?? 'water', theme)
  return reverse ? [...stops].reverse() : stops
}
/** Colour at t ∈ [0,1] along a ramp, interpolated between its stops. */
export function rampAt(stops: string[], t: number): RGB {
  const x = Math.min(1, Math.max(0, t)) * (stops.length - 1)
  const i = Math.min(stops.length - 2, Math.floor(x))
  const a = hexToRgb(stops[i]), b = hexToRgb(stops[i + 1])
  const f = x - i
  return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}
/** `n` class colours spread over a ramp. */
export function rampClasses(name: string, theme: Theme, n: number, reverse = false): string[] {
  const stops = rampStops(name, theme, reverse)
  if (n === stops.length) return stops
  return Array.from({ length: n }, (_, i) => rgbToHex(rampAt(stops, n === 1 ? 0.6 : i / (n - 1))))
}

/* Bivariate 3×3 palettes. Index = y * 3 + x, both low → high. Built by mixing two inks into a neutral ground:
   subtractive on a light map (both high = darkest), additive on a dark one (both high = lightest). */
export const BIVARIATE: { name: string; label: string; x: [string, string]; y: [string, string]; ground: [string, string] }[] = [
  { name: 'ldl.biv.blue-orange', label: 'Blue × orange', x: ['#2f6fc4', '#4a94f5'], y: ['#d9741a', '#f29a3c'], ground: ['#ecece9', '#2b333e'] },
  { name: 'ldl.biv.teal-violet', label: 'Teal × violet', x: ['#14897b', '#2fb8a4'], y: ['#6a4bc9', '#9478f0'], ground: ['#ecece9', '#2b333e'] },
  { name: 'ldl.biv.green-blue', label: 'Green × blue', x: ['#3f8a2f', '#62b84e'], y: ['#2e62bd', '#4f8cf0'], ground: ['#ecece9', '#2b333e'] },
]
export function bivariateColors(name: string, theme: Theme): string[] {
  const p = BIVARIATE.find((b) => b.name === name) ?? BIVARIATE[0]
  const dark = theme === 'dark'
  const g = hexToRgb(p.ground[dark ? 1 : 0])
  const x = hexToRgb(p.x[dark ? 1 : 0]), y = hexToRgb(p.y[dark ? 1 : 0])
  const out: string[] = []
  // how far each step travels toward its ink; the second step is the longer one, so the corners separate
  const k = dark ? [0, 0.52, 1] : [0, 0.42, 0.86]
  for (let j = 0; j < 3; j++)
    for (let i = 0; i < 3; i++) {
      const c = g.map((gv, ch) => {
        if (dark) {
          // additive: each ink adds its light to the ground; where both are present the sum is held under white
          const sum = gv + (x[ch] - gv) * k[i] + (y[ch] - gv) * k[j] * (i ? 0.78 : 1) + (i && j ? 34 * k[i] * k[j] : 0)
          return Math.min(246, sum)
        }
        // multiply the two tints
        const tx = 1 - (1 - x[ch] / 255) * k[i], ty = 1 - (1 - y[ch] / 255) * k[j]
        return gv * tx * ty
      }) as RGB
      out.push(rgbToHex(c))
    }
  return out
}

/** Named tones for constant colours, so a line or a dot follows the basemap's scheme instead of freezing one hex. */
export const TONES = ['ink', 'grey', 'blue', 'teal', 'green', 'orange', 'violet'] as const
export type Tone = (typeof TONES)[number]
export function toneColor(value: string | undefined, theme: Theme, fallback: Tone = 'ink'): string {
  if (value && value.startsWith('#')) return value
  const t = (value?.replace('ldl.tone.', '') ?? fallback) as Tone
  const dark = theme === 'dark'
  switch (t as Tone | 'paper') {
    case 'paper':
      return paperColor(theme)
    case 'ink':
      return dark ? '#e8edf2' : '#14181c'
    case 'grey':
      return dark ? '#8a97a5' : '#687480'
    case 'blue':
      return ramp('water', theme)[dark ? 4 : 4]
    case 'teal':
      return ramp('access', theme)[4]
    case 'green':
      return ramp('vegetation', theme)[4]
    case 'orange':
      return ramp('built', theme)[4]
    case 'violet':
      return ramp('people', theme)[4]
    default:
      return dark ? '#e8edf2' : '#14181c'
  }
}
export const paperColor = (theme: Theme) => (theme === 'dark' ? '#0b0e12' : '#ffffff')
