// Shared, framework-free helpers for the area drawer: one indicator on admin units.
import type { Indicator, IndicatorMap } from '@/api/types'
import { fmtValue } from '@/lib/format'
import type { Theme } from '@/lib/hooks'
import { classOf, ramp } from '@/lib/palettes'

export type Level = 1 | 2
export const LEVEL_NAME: Record<Level, string> = { 1: 'Province', 2: 'District' }

/** Selection accent — the only place rose touches the map. */
export const SELECT_HEX: Record<Theme, string> = { dark: '#d8446b', light: '#c22f5a' }

export function ordinal(n: number): string {
  const s = ['th', 'st', 'nd', 'rd']
  const v = n % 100
  return `${n}${s[(v - 20) % 10] ?? s[v] ?? s[0]}`
}

/** Short unit suffix set beside a number. */
export function unitSuffix(unit: string): string {
  switch (unit) {
    case 'per km²':
      return '/km²'
    case '%':
      return '%'
    default:
      return unit
  }
}

/** A value without its unit, for axes, legends and big numerals. */
export function fmtBare(v: number, unit: string): string {
  return fmtValue(v, unit)
    .replace(/\s?(min|mm|km²)$/, '')
    .replace('%', '')
}

export const isSum = (unit: string) => unit === 'people' || unit === 'households' || unit === 'count'

/** Everything the UI derives from one choropleth payload. */
export type MapModel = {
  indicator: Indicator
  data: IndicatorMap
  colors: string[]
  byPcode: Map<string, { value: number; rank: number; cls: number; color: string }>
  min: number
  max: number
}

export function buildModel(indicator: Indicator, data: IndicatorMap, theme: Theme): MapModel {
  const colors = ramp(indicator.ramp, theme)
  const byPcode: MapModel['byPcode'] = new Map()
  data.pcodes.forEach((p, i) => {
    const value = data.values[i]
    const cls = classOf(value, data.breaks)
    byPcode.set(p, { value, cls, color: colors[cls], rank: data.values.filter((v) => v > value).length + 1 })
  })
  return { indicator, data, colors, byPcode, min: Math.min(...data.values), max: Math.max(...data.values) }
}
