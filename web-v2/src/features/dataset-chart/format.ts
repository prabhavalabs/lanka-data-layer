// Number, unit and period formatting for the dataset chart, and the plain-language read-outs.
import type { RawIndicator } from '@/api/real/series'
import { fmtCompact, fmtInt } from '@/lib/format'

export const LEVEL_SINGULAR = ['country', 'province', 'district', 'DS division', 'GN division'] as const
export const LEVEL_PLURAL = ['the country', 'provinces', 'districts', 'DS divisions', 'GN divisions'] as const
export const LEVEL_SHORT = ['National', 'Province', 'District', 'DS div.', 'GN div.'] as const

const nf1 = new Intl.NumberFormat('en-LK', { maximumFractionDigits: 1 })
const nf2 = new Intl.NumberFormat('en-LK', { maximumFractionDigits: 2 })
const nf3 = new Intl.NumberFormat('en-LK', { maximumSignificantDigits: 3 })

/** A number for a label: compact from a million, grouped to the unit from a thousand, then as many decimals as read well. */
export function numShort(v: number): string {
  const a = Math.abs(v)
  if (a >= 1e6) return fmtCompact(v)
  if (a >= 1000) return fmtInt(v)
  if (a >= 10) return nf1.format(v)
  if (a >= 1) return nf2.format(v)
  return a === 0 ? '0' : nf3.format(v)
}
/** A number for a table: every digit that is meaningful. */
export function numFull(v: number): string {
  return Math.abs(v) >= 1000 ? fmtInt(v) : numShort(v)
}

export type ValueFormat = {
  /** a value for a bar label or axis tick */
  short: (v: number) => string
  /** a value for a table cell */
  full: (v: number) => string
  /** a value inside a sentence, with its unit word ("2.4M persons") */
  say: (v: number) => string
  /** the unit as a heading ("persons", "percent") */
  unit: string
}

const PREFIX: Record<string, string> = { USD: 'US$ ', LKR: 'Rs ' }
const unwanted = /\(|various|unitless|^count$/i

/** Formatters for an indicator's unit. `unit_mult` is the power of ten its stored values are in (6: millions). */
export function valueFormat(ind: Pick<RawIndicator, 'unit' | 'unit_label' | 'unit_mult'>): ValueFormat {
  const mult = 10 ** (ind.unit_mult || 0)
  const prefix = PREFIX[ind.unit] ?? ''
  const pct = ind.unit === '%'
  const word = pct || prefix || unwanted.test(ind.unit_label ?? '') ? '' : (ind.unit_label ?? '')
  const wrap = (n: (v: number) => string) => (v: number) => `${v < 0 ? '−' : ''}${prefix}${n(Math.abs(v * mult))}${pct ? '%' : ''}`
  const short = wrap(numShort)
  const full = wrap(numFull)
  return { short, full, say: (v) => (word ? `${short(v)} ${word}` : short(v)), unit: pct ? 'percent' : ind.unit_label || ind.unit }
}

/* ------------------------------ periods ------------------------------ */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
/** "2026-09" to "Sep 2026", "2026-09-24" to "24 Sep 2026", "2026-Q3" to "Q3 2026"; a year stays. */
export function periodLabel(p: string): string {
  let m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(p)
  if (m) return `${Number(m[3])} ${MONTHS[Number(m[2]) - 1]} ${m[1]}`
  m = /^(\d{4})-(\d{2})$/.exec(p)
  if (m) return `${MONTHS[Number(m[2]) - 1]} ${m[1]}`
  m = /^(\d{4})-Q(\d)$/.exec(p)
  if (m) return `Q${m[2]} ${m[1]}`
  return p
}
/** A period as the date it starts on (UTC), for a time axis; null when it is not one of the engine's period forms. */
export function periodDate(p: string): Date | null {
  let m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(p)
  if (m) return new Date(Date.UTC(+m[1], +m[2] - 1, +m[3]))
  m = /^(\d{4})-(\d{2})$/.exec(p)
  if (m) return new Date(Date.UTC(+m[1], +m[2] - 1, 1))
  m = /^(\d{4})-Q(\d)$/.exec(p)
  if (m) return new Date(Date.UTC(+m[1], (+m[2] - 1) * 3, 1))
  m = /^(\d{4})$/.exec(p)
  return m ? new Date(Date.UTC(+m[1], 0, 1)) : null
}
/** The periods from the first to the last at the indicator's own step, so a gap in the data is a gap in the line. */
export function fillPeriods(periods: string[], type: string): string[] {
  if (periods.length < 2) return periods
  const a = periods[0]
  const b = periods[periods.length - 1]
  const out: string[] = []
  if (type === 'year' && /^\d{4}$/.test(a) && /^\d{4}$/.test(b)) {
    if (+b - +a > 500) return periods
    for (let y = +a; y <= +b; y++) out.push(String(y))
    return out
  }
  if (type === 'month' && /^\d{4}-\d{2}$/.test(a) && /^\d{4}-\d{2}$/.test(b)) {
    let y = +a.slice(0, 4)
    let m = +a.slice(5)
    while (`${y}-${String(m).padStart(2, '0')}` <= b && out.length < 3000) {
      out.push(`${y}-${String(m).padStart(2, '0')}`)
      if (++m > 12) (m = 1), y++
    }
    return out
  }
  if (type === 'day' && /^\d{4}-\d{2}-\d{2}$/.test(a) && /^\d{4}-\d{2}-\d{2}$/.test(b)) {
    const d = periodDate(a)!
    const end = periodDate(b)!
    while (d <= end && out.length < 4000) {
      out.push(d.toISOString().slice(0, 10))
      d.setUTCDate(d.getUTCDate() + 1)
    }
    return out
  }
  return periods
}

/* ------------------------------ dims ------------------------------ */
export const TOTALS = new Set(['total', 'all', 'overall'])
export const humanise = (s: string) => {
  const t = s.replace(/_/g, ' ')
  return t.charAt(0).toUpperCase() + t.slice(1)
}

/* ------------------------------ statistics ------------------------------ */
export function medianOf(sorted: number[]): number {
  const n = sorted.length
  return n % 2 ? sorted[(n - 1) / 2] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2
}

export type Ranked = { pcode: string; name: string; value: number | null }

/** "Colombo has the highest value (X), 3.2× the lowest (Y). The median district is Z." */
export function rankingReadout(rows: Ranked[], f: ValueFormat, label: string, level: number, national: number | null, period: string): string {
  const have = rows.filter((r): r is Ranked & { value: number } => r.value !== null).sort((a, b) => b.value - a.value)
  if (have.length === 0) return `No unit has a value for ${label} in ${periodLabel(period)}.`
  const hi = have[0]
  const lo = have[have.length - 1]
  const missing = rows.length - have.length
  if (have.length === 1) return `${hi.name} has ${f.say(hi.value)} for ${label}, ${periodLabel(period)}.`
  const med = medianOf(have.map((r) => r.value).reverse())
  const ratio = lo.value > 0 && hi.value > 0 ? hi.value / lo.value : null
  const parts = [
    `${hi.name} has the highest value (${f.say(hi.value)})${ratio && ratio >= 1.05 ? `, ${ratio >= 10 ? Math.round(ratio) : ratio.toFixed(1)}× the lowest (${lo.name}, ${f.say(lo.value)})` : `; the lowest is ${lo.name} (${f.say(lo.value)})`}.`,
    `The median ${LEVEL_SINGULAR[level]} is ${f.say(med)}${national !== null ? `; the national figure is ${f.say(national)}` : ''}.`,
  ]
  if (missing) parts.push(`${missing} of ${rows.length} ${LEVEL_PLURAL[level]} have no value and are left out, not counted as zero.`)
  return parts.join(' ')
}

export function trendReadout(name: string, pts: { period: string; value: number | null }[], f: ValueFormat, label: string): string {
  const have = pts.filter((p): p is { period: string; value: number } => p.value !== null)
  if (have.length < 2) return ''
  const first = have[0]
  const last = have[have.length - 1]
  const hi = have.reduce((a, b) => (b.value > a.value ? b : a))
  const lo = have.reduce((a, b) => (b.value < a.value ? b : a))
  const change = first.value !== 0 ? ((last.value - first.value) / Math.abs(first.value)) * 100 : null
  const dir = last.value > first.value ? 'rose' : last.value < first.value ? 'fell' : 'did not change'
  const parts = [`${name}: ${label} ${dir} from ${f.say(first.value)} in ${periodLabel(first.period)} to ${f.say(last.value)} in ${periodLabel(last.period)}${change !== null && dir !== 'did not change' && Math.abs(change) < 1e5 ? ` (${change > 0 ? '+' : '−'}${Math.abs(change) >= 10 ? Math.round(Math.abs(change)) : Math.abs(change).toFixed(1)}%)` : ''}.`]
  if (hi.period !== last.period || lo.period !== first.period) parts.push(`The highest was ${f.say(hi.value)} (${periodLabel(hi.period)}) and the lowest ${f.say(lo.value)} (${periodLabel(lo.period)}).`)
  const missing = pts.length - have.length
  if (missing) parts.push(`${missing} ${missing === 1 ? 'period has' : 'periods have'} no value and show as gaps.`)
  return parts.join(' ')
}

/** A dimension value ("sri_lankan_tamil") as a name ("Sri Lankan Tamil"). */
export const valueLabel = (s: string) => s.replace(/_/g, ' ').replace(/(^|\s)([a-z])/g, (_, a: string, b: string) => a + b.toUpperCase())
