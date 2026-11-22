const nf0 = new Intl.NumberFormat('en-LK', { maximumFractionDigits: 0 })
const nf1 = new Intl.NumberFormat('en-LK', { maximumFractionDigits: 1, minimumFractionDigits: 1 })
const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

export const fmtInt = (n: number) => nf0.format(n)
export const fmt1 = (n: number) => nf1.format(n)
export const fmtCompact = (n: number) => compact.format(n)
const nf2 = new Intl.NumberFormat('en-LK', { maximumFractionDigits: 2 })
/** Area: whole km² from 10 up, otherwise up to two decimals so small GN divisions don't read as 0. */
export const fmtArea = (n: number) => (Math.abs(n) >= 10 ? nf0.format(n) : nf2.format(n))
export const fmtPct = (n: number, d = 1) => `${n.toFixed(d)}%`
export const fmtSigned = (n: number, d = 1) => `${n > 0 ? '+' : n < 0 ? '−' : ''}${Math.abs(n).toFixed(d)}`

export function fmtValue(v: number, unit: string): string {
  switch (unit) {
    case 'people':
    case 'households':
    case 'count':
      return v >= 100_000 ? fmtCompact(v) : fmtInt(v)
    case '%':
      return fmtPct(v)
    case 'per km²':
      return fmtInt(v)
    case 'min':
      return `${fmt1(v)} min`
    case 'mm':
      return `${fmtInt(v)} mm`
    case 'km²':
      return `${fmtArea(v)} km²`
    default:
      return Math.abs(v) >= 100 ? fmtInt(v) : fmt1(v)
  }
}

export const fmtCoord = (lat: number, lon: number) =>
  `${Math.abs(lat).toFixed(4)}°${lat >= 0 ? 'N' : 'S'} ${Math.abs(lon).toFixed(4)}°${lon >= 0 ? 'E' : 'W'}`

export const slugify = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '')
