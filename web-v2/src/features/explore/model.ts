// Shared, framework-free helpers for the Explore map.
import type { Indicator, IndicatorMap, LicenceStatus } from '@/api/types'
import { fmtValue } from '@/lib/format'
import type { Theme } from '@/lib/hooks'
import { classOf, ramp } from '@/lib/palettes'

export type Level = 1 | 2
export type LayerKey = 'rail' | 'rivers' | 'protected' | 'places'
export type LayerState = Record<LayerKey, boolean>
export type LicenceFilter = Record<LicenceStatus, boolean>

export const DEFAULT_INDICATOR = 'pop.density'
export const LEVEL_NAME: Record<Level, string> = { 1: 'Province', 2: 'District' }
export const LEVEL_PLURAL: Record<Level, string> = { 1: 'provinces', 2: 'districts' }

/** Selection accent — the only place rose touches the map. */
export const SELECT_HEX: Record<Theme, string> = { dark: '#d8446b', light: '#c22f5a' }
export const NODATA_HEX: Record<Theme, string> = { dark: '#1b222b', light: '#e3e6e5' }

export const levelOfPcode = (pcode: string): Level | null => (pcode.length === 3 ? 1 : pcode.length === 4 ? 2 : null)

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

/** Percent thumbs -> value bounds. Thumbs resting at the ends mean "unbounded". */
export function rangeBounds(model: MapModel | null, pct: [number, number]): [number, number] {
  if (!model) return [-Infinity, Infinity]
  const span = model.max - model.min
  return [pct[0] <= 0 ? -Infinity : model.min + (span * pct[0]) / 100, pct[1] >= 100 ? Infinity : model.min + (span * pct[1]) / 100]
}
