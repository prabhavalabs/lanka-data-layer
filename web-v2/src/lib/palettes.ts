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
