// Number formatting for the workspace: one function, driven by the unit a column declares.
const int = new Intl.NumberFormat('en-LK', { maximumFractionDigits: 0 })
const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

/** A value without its unit, for axes, legends and tables. */
export function fmt(v: number | null | undefined, unit = ''): string {
  if (v == null || !Number.isFinite(v)) return '—'
  switch (unit) {
    case 'LKR/perch':
      return Math.abs(v) >= 1e6 ? `${(v / 1e6).toFixed(Math.abs(v) >= 1e7 ? 1 : 2).replace(/\.?0+$/, '')}M` : `${Math.round(v / 1e3)}k`
    case 'm':
      return v.toFixed(2)
    case '%':
      return v.toFixed(Math.abs(v) >= 100 ? 0 : 1)
    case 'min':
      return v.toFixed(Math.abs(v) >= 100 ? 0 : 1).replace(/\.0$/, '')
    case 'people':
    case 'households':
    case 'count':
      return Math.abs(v) >= 100_000 ? compact.format(v) : int.format(v)
    case 'per km²':
    case 'mm':
    case 'beds':
    case 'trains':
    case 'perches':
      return int.format(v)
    default: {
      const a = Math.abs(v)
      return a >= 100_000 ? compact.format(v) : a >= 100 ? int.format(v) : a >= 10 ? v.toFixed(1).replace(/\.0$/, '') : v.toFixed(2).replace(/\.?0+$/, '')
    }
  }
}
/** The unit as it reads after a number. */
export function unitSuffix(unit: string): string {
  switch (unit) {
    case 'per km²':
      return '/km²'
    case 'LKR/perch':
      return 'LKR / perch'
    default:
      return unit
  }
}
/** A value with its unit, for tooltips. */
export const fmtWith = (v: number | null | undefined, unit = '') => {
  const s = fmt(v, unit)
  if (s === '—' || !unit) return s
  if (unit === '%') return `${s}%`
  if (unit === 'LKR/perch') return `LKR ${s}`
  return `${s} ${unitSuffix(unit)}`
}
export const pct = (v: number, d = 0) => `${Math.round(v * 100 * 10 ** d) / 10 ** d}%`
export function ago(iso: string): string {
  const s = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000)
  if (s < 50) return 'just now'
  if (s < 3600) return `${Math.round(s / 60)} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  if (s < 86400 * 30) return `${Math.round(s / 86400)} d ago`
  return new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
}
