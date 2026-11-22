// Classification and the statistics behind "Relate layers".
//
// In the real system this is `lanka-stats`, one Rust crate compiled natively for the server and to
// WebAssembly for the browser, so a legend drawn here and one produced by the API share their breaks
// by construction (10 §2.6, 11 §8). This file is the mockup's stand-in: same functions, plain TypeScript.

/* ---------------- classification ---------------- */
export type ClassMethod = 'quantile' | 'equal' | 'jenks'

const finite = (values: ArrayLike<number>) => {
  const out: number[] = []
  for (let i = 0; i < values.length; i++) if (Number.isFinite(values[i])) out.push(values[i])
  return out
}
export function extent(values: ArrayLike<number>): [number, number] {
  let lo = Infinity, hi = -Infinity
  for (let i = 0; i < values.length; i++) {
    const v = values[i]
    if (!Number.isFinite(v)) continue
    if (v < lo) lo = v
    if (v > hi) hi = v
  }
  return lo === Infinity ? [0, 1] : [lo, hi]
}
export function quantile(sorted: number[], p: number): number {
  if (!sorted.length) return NaN
  const x = (sorted.length - 1) * p
  const i = Math.floor(x)
  return sorted[i] + (sorted[Math.min(sorted.length - 1, i + 1)] - sorted[i]) * (x - i)
}
export const median = (values: number[]) => quantile([...values].sort((a, b) => a - b), 0.5)

/** Fisher–Jenks natural breaks. Above 400 values it runs on a deterministic, evenly spaced sample of the sorted data. */
function jenks(sortedAll: number[], k: number): number[] {
  const cap = 400
  const s = sortedAll.length > cap ? Array.from({ length: cap }, (_, i) => sortedAll[Math.round((i / (cap - 1)) * (sortedAll.length - 1))]) : sortedAll
  const n = s.length
  if (n <= k) return s.slice(1)
  const cost: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(k + 1).fill(Infinity))
  const cut: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(k + 1).fill(0))
  // prefix sums give the within-class variance of any run in O(1)
  const p1 = new Float64Array(n + 1), p2 = new Float64Array(n + 1)
  for (let i = 0; i < n; i++) {
    p1[i + 1] = p1[i] + s[i]
    p2[i + 1] = p2[i] + s[i] * s[i]
  }
  const ssd = (a: number, b: number) => {
    const m = b - a
    const sum = p1[b] - p1[a]
    return p2[b] - p2[a] - (sum * sum) / m
  }
  for (let i = 1; i <= n; i++) cost[i][1] = ssd(0, i)
  for (let c = 2; c <= k; c++)
    for (let i = c; i <= n; i++)
      for (let j = c - 1; j < i; j++) {
        const v = cost[j][c - 1] + ssd(j, i)
        if (v < cost[i][c]) {
          cost[i][c] = v
          cut[i][c] = j
        }
      }
  const breaks: number[] = []
  let i = n
  for (let c = k; c > 1; c--) {
    i = cut[i][c]
    breaks.unshift(s[i])
  }
  return breaks
}

/** k−1 ascending break values; a value belongs to the first class whose break it is below. */
export function classBreaks(values: ArrayLike<number>, method: ClassMethod, k: number): number[] {
  const s = finite(values).sort((a, b) => a - b)
  if (s.length === 0) return []
  const [lo, hi] = [s[0], s[s.length - 1]]
  let breaks: number[]
  if (method === 'equal') breaks = Array.from({ length: k - 1 }, (_, i) => lo + ((hi - lo) * (i + 1)) / k)
  else if (method === 'jenks') breaks = jenks(s, k)
  else breaks = Array.from({ length: k - 1 }, (_, i) => quantile(s, (i + 1) / k))
  // strictly ascending: ties collapse classes rather than producing empty ones in the middle
  return breaks.filter((b, i) => i === 0 || b > breaks[i - 1])
}
export function classIndex(v: number, breaks: number[]): number {
  let i = 0
  while (i < breaks.length && v >= breaks[i]) i++
  return i
}
/** A break rounded so that it reads as a number a person chose. */
export function tidy(v: number): number {
  if (v === 0 || !Number.isFinite(v)) return v
  const mag = 10 ** (Math.floor(Math.log10(Math.abs(v))) - 2)
  return Math.round(v / mag) * mag
}

/** Tercile breaks; when a third or more of the values are zero, "none" is its own class and the rest split above it. */
export function tercileBreaks(values: ArrayLike<number>): number[] {
  const s = finite(values).sort((a, b) => a - b)
  if (!s.length) return [0, 1]
  const zeros = s.filter((v) => v <= 0).length
  if (zeros >= s.length / 3) {
    const pos = s.filter((v) => v > 0)
    return pos.length ? [tidy(Math.max(pos[0], quantile(pos, 0.12))), tidy(quantile(pos, 0.6))] : [0.5, 1]
  }
  return [tidy(quantile(s, 1 / 3)), tidy(quantile(s, 2 / 3))]
}

export type Histogram = { bins: { x0: number; x1: number; n: number }[]; max: number; lo: number; hi: number }
export function histogram(values: ArrayLike<number>, count = 36, range?: [number, number]): Histogram {
  const [lo, hi] = range ?? extent(values)
  const span = hi - lo || 1
  const bins = Array.from({ length: count }, (_, i) => ({ x0: lo + (span * i) / count, x1: lo + (span * (i + 1)) / count, n: 0 }))
  for (let i = 0; i < values.length; i++) {
    const v = values[i]
    if (!Number.isFinite(v) || v < lo || v > hi) continue
    bins[Math.min(count - 1, Math.floor(((v - lo) / span) * count))].n++
  }
  return { bins, max: Math.max(1, ...bins.map((b) => b.n)), lo, hi }
}

/* ---------------- distributions ---------------- */
function lnGamma(x: number): number {
  const c = [76.18009172947146, -86.50532032941677, 24.01409824083091, -1.231739572450155, 0.1208650973866179e-2, -0.5395239384953e-5]
  let y = x
  let t = x + 5.5
  t -= (x + 0.5) * Math.log(t)
  let s = 1.000000000190015
  for (const cj of c) s += cj / ++y
  return -t + Math.log((2.5066282746310005 * s) / x)
}
/** Regularised incomplete beta, by continued fraction. */
function betaInc(x: number, a: number, b: number): number {
  if (x <= 0) return 0
  if (x >= 1) return 1
  const front = Math.exp(lnGamma(a + b) - lnGamma(a) - lnGamma(b) + a * Math.log(x) + b * Math.log(1 - x))
  const cf = (xx: number, aa: number, bb: number) => {
    let f = 1, c = 1, d = 0
    for (let i = 0; i <= 200; i++) {
      const m = i >> 1
      let num: number
      if (i === 0) num = 1
      else if (i % 2 === 0) num = (m * (bb - m) * xx) / ((aa + 2 * m - 1) * (aa + 2 * m))
      else num = -((aa + m) * (aa + bb + m) * xx) / ((aa + 2 * m) * (aa + 2 * m + 1))
      d = 1 + num * d
      if (Math.abs(d) < 1e-30) d = 1e-30
      d = 1 / d
      c = 1 + num / c
      if (Math.abs(c) < 1e-30) c = 1e-30
      const cd = c * d
      f *= cd
      if (Math.abs(1 - cd) < 1e-10) break
    }
    return f
  }
  return x < (a + 1) / (a + b + 2) ? (front * (cf(x, a, b) - 1)) / a : 1 - (front * (cf(1 - x, b, a) - 1)) / b
}
/** Two-sided p-value of a t statistic. */
export const pOfT = (t: number, df: number) => (df <= 0 || !Number.isFinite(t) ? NaN : betaInc(df / (df + t * t), df / 2, 0.5))
export const normCdf = (z: number) => {
  const t = 1 / (1 + 0.2316419 * Math.abs(z))
  const d = 0.3989423 * Math.exp((-z * z) / 2)
  const p = d * t * (0.3193815 + t * (-0.3565638 + t * (1.781478 + t * (-1.821256 + t * 1.330274))))
  return z > 0 ? 1 - p : p
}

/* ---------------- association ---------------- */
export const mean = (a: ArrayLike<number>) => {
  let s = 0
  for (let i = 0; i < a.length; i++) s += a[i]
  return s / (a.length || 1)
}
export function pearson(x: ArrayLike<number>, y: ArrayLike<number>): number {
  const n = x.length
  const mx = mean(x), my = mean(y)
  let sxy = 0, sxx = 0, syy = 0
  for (let i = 0; i < n; i++) {
    const dx = x[i] - mx, dy = y[i] - my
    sxy += dx * dy
    sxx += dx * dx
    syy += dy * dy
  }
  return sxx && syy ? sxy / Math.sqrt(sxx * syy) : 0
}
/** Average ranks, ties shared. */
export function ranks(a: ArrayLike<number>): Float64Array {
  const idx = Array.from({ length: a.length }, (_, i) => i).sort((i, j) => a[i] - a[j])
  const r = new Float64Array(a.length)
  for (let i = 0; i < idx.length; ) {
    let j = i
    while (j + 1 < idx.length && a[idx[j + 1]] === a[idx[i]]) j++
    const avg = (i + j) / 2 + 1
    for (let k = i; k <= j; k++) r[idx[k]] = avg
    i = j + 1
  }
  return r
}
export const spearman = (x: ArrayLike<number>, y: ArrayLike<number>) => pearson(ranks(x), ranks(y))
export const pOfR = (r: number, n: number, k = 0) => {
  const df = n - 2 - k
  return df > 0 ? pOfT((r * Math.sqrt(df)) / Math.sqrt(Math.max(1e-12, 1 - r * r)), df) : NaN
}
/** 95 % interval of a correlation by Fisher's z. */
export function rInterval(r: number, n: number): [number, number] {
  if (n < 4) return [-1, 1]
  const z = Math.atanh(Math.max(-0.999999, Math.min(0.999999, r)))
  const se = 1 / Math.sqrt(n - 3)
  return [Math.tanh(z - 1.96 * se), Math.tanh(z + 1.96 * se)]
}

export type Ols = { slope: number; intercept: number; r2: number; se: number; n: number; xMean: number; sxx: number; sigma: number; slopeCi: [number, number] }
export function ols(x: ArrayLike<number>, y: ArrayLike<number>): Ols {
  const n = x.length
  const mx = mean(x), my = mean(y)
  let sxy = 0, sxx = 0, syy = 0
  for (let i = 0; i < n; i++) {
    sxy += (x[i] - mx) * (y[i] - my)
    sxx += (x[i] - mx) ** 2
    syy += (y[i] - my) ** 2
  }
  const slope = sxx ? sxy / sxx : 0
  const intercept = my - slope * mx
  let sse = 0
  for (let i = 0; i < n; i++) sse += (y[i] - intercept - slope * x[i]) ** 2
  const sigma = Math.sqrt(sse / Math.max(1, n - 2))
  const se = sxx ? sigma / Math.sqrt(sxx) : 0
  return { slope, intercept, r2: syy ? 1 - sse / syy : 0, se, n, xMean: mx, sxx, sigma, slopeCi: [slope - 1.96 * se, slope + 1.96 * se] }
}
/** Half-width of the 95 % band of the fitted mean at x. */
export const olsBand = (fit: Ols, x: number) => 1.96 * fit.sigma * Math.sqrt(1 / fit.n + (fit.sxx ? (x - fit.xMean) ** 2 / fit.sxx : 0))

/** Theil–Sen slope on a deterministic subsample: the "robust" line beside OLS. */
export function theilSen(x: ArrayLike<number>, y: ArrayLike<number>): { slope: number; intercept: number } {
  const n = x.length
  const step = Math.max(1, Math.floor(n / 220))
  const slopes: number[] = []
  for (let i = 0; i < n; i += step) for (let j = i + step; j < n; j += step) if (x[j] !== x[i]) slopes.push((y[j] - y[i]) / (x[j] - x[i]))
  const slope = slopes.length ? median(slopes) : 0
  const res: number[] = []
  for (let i = 0; i < n; i++) res.push(y[i] - slope * x[i])
  return { slope, intercept: median(res) }
}

/** Residuals of y on the columns of Z (with an intercept), by the normal equations. */
export function residualise(y: ArrayLike<number>, Z: ArrayLike<number>[]): Float64Array {
  const n = y.length, p = Z.length + 1
  const col = (j: number, i: number) => (j === 0 ? 1 : Z[j - 1][i])
  const A: number[][] = Array.from({ length: p }, () => new Array<number>(p + 1).fill(0))
  for (let i = 0; i < n; i++)
    for (let a = 0; a < p; a++) {
      const ca = col(a, i)
      A[a][p] += ca * y[i]
      for (let b = 0; b < p; b++) A[a][b] += ca * col(b, i)
    }
  for (let a = 0; a < p; a++) {
    let piv = a
    for (let r = a + 1; r < p; r++) if (Math.abs(A[r][a]) > Math.abs(A[piv][a])) piv = r
    ;[A[a], A[piv]] = [A[piv], A[a]]
    const d = A[a][a] || 1e-12
    for (let c = a; c <= p; c++) A[a][c] /= d
    for (let r = 0; r < p; r++) {
      if (r === a) continue
      const f = A[r][a]
      if (f) for (let c = a; c <= p; c++) A[r][c] -= f * A[a][c]
    }
  }
  const out = new Float64Array(n)
  for (let i = 0; i < n; i++) {
    let fit = 0
    for (let a = 0; a < p; a++) fit += A[a][p] * col(a, i)
    out[i] = y[i] - fit
  }
  return out
}

/* ---------------- spatial ---------------- */
export function seeded(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
const zscore = (a: ArrayLike<number>) => {
  const m = mean(a)
  let s = 0
  for (let i = 0; i < a.length; i++) s += (a[i] - m) ** 2
  const sd = Math.sqrt(s / (a.length || 1)) || 1
  return Float64Array.from(a as ArrayLike<number>, (v) => (v - m) / sd)
}
/** Row-standardised spatial lag: the mean of each unit's neighbours. */
function lag(z: ArrayLike<number>, nb: number[][]): Float64Array {
  const out = new Float64Array(z.length)
  for (let i = 0; i < nb.length; i++) {
    let s = 0
    for (const j of nb[i]) s += z[j]
    out[i] = nb[i].length ? s / nb[i].length : 0
  }
  return out
}
const shuffled = (n: number, rnd: () => number) => {
  const p = Array.from({ length: n }, (_, i) => i)
  for (let i = n - 1; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1))
    ;[p[i], p[j]] = [p[j], p[i]]
  }
  return p
}
const pseudoP = (obs: number, sims: number[]) => (sims.filter((s) => Math.abs(s) >= Math.abs(obs)).length + 1) / (sims.length + 1)

/** Global Moran's I with a permutation p-value. */
export function moran(values: ArrayLike<number>, nb: number[][], permutations: number, seed = 7): { I: number; p: number } {
  const z = zscore(values)
  const stat = (zz: ArrayLike<number>) => mean(Float64Array.from(lag(zz, nb), (l, i) => l * zz[i]))
  const I = stat(z)
  const rnd = seeded(seed)
  const sims = Array.from({ length: permutations }, () => {
    const p = shuffled(z.length, rnd)
    return stat(Float64Array.from(p, (i) => z[i]))
  })
  return { I, p: pseudoP(I, sims) }
}
/** Lee's L: bivariate spatial association (Pearson's r smoothed through the neighbourhood), global and per unit. */
export function lee(x: ArrayLike<number>, y: ArrayLike<number>, nb: number[][], permutations: number, seed = 11) {
  const zx = zscore(x), zy = zscore(y)
  // Lee's statistic lags a unit together with its neighbours
  const self = nb.map((list, i) => [i, ...list])
  const local = (a: ArrayLike<number>, b: ArrayLike<number>) => {
    const la = lag(a, self), lb = lag(b, self)
    return Float64Array.from(la, (v, i) => v * lb[i])
  }
  const Li = local(zx, zy)
  const L = mean(Li)
  const rnd = seeded(seed)
  const n = zx.length
  const exceed = new Uint16Array(n)
  const sims: number[] = []
  for (let k = 0; k < permutations; k++) {
    const p = shuffled(n, rnd)
    const sim = local(Float64Array.from(p, (i) => zx[i]), Float64Array.from(p, (i) => zy[i]))
    sims.push(mean(sim))
    for (let i = 0; i < n; i++) if (Math.abs(sim[i]) >= Math.abs(Li[i])) exceed[i]++
  }
  const p = Array.from(exceed, (e) => (e + 1) / (permutations + 1))
  const lx = lag(zx, self), ly = lag(zy, self)
  const cluster = Array.from(Li, (_, i) => `${lx[i] >= 0 ? 'H' : 'L'}${ly[i] >= 0 ? 'H' : 'L'}` as 'HH' | 'HL' | 'LH' | 'LL')
  return { L, p: pseudoP(L, sims), local: Array.from(Li), localP: p, cluster }
}
/** Benjamini–Hochberg adjusted p-values. */
export function fdr(p: number[]): number[] {
  const idx = p.map((_, i) => i).sort((a, b) => p[a] - p[b])
  const out = new Array<number>(p.length)
  let prev = 1
  for (let k = idx.length - 1; k >= 0; k--) {
    prev = Math.min(prev, (p[idx[k]] * idx.length) / (k + 1))
    out[idx[k]] = prev
  }
  return out
}
/** k nearest neighbours by centroid, made symmetric. */
export function knn(points: [number, number][], k: number): number[][] {
  const n = points.length
  const nb: Set<number>[] = Array.from({ length: n }, () => new Set<number>())
  for (let i = 0; i < n; i++) {
    const d = points.map((p, j) => [(p[0] - points[i][0]) ** 2 + (p[1] - points[i][1]) ** 2, j] as [number, number])
    d.sort((a, b) => a[0] - b[0])
    for (let m = 1; m <= Math.min(k, n - 1); m++) {
      nb[i].add(d[m][1])
      nb[d[m][1]].add(i)
    }
  }
  return nb.map((s) => [...s])
}

/* ---------------- two groups ---------------- */
export function welch(a: number[], b: number[]): { t: number; df: number; p: number } {
  const ma = mean(a), mb = mean(b)
  const va = a.reduce((s, v) => s + (v - ma) ** 2, 0) / Math.max(1, a.length - 1)
  const vb = b.reduce((s, v) => s + (v - mb) ** 2, 0) / Math.max(1, b.length - 1)
  const se = Math.sqrt(va / a.length + vb / b.length) || 1e-12
  const df = (va / a.length + vb / b.length) ** 2 / ((va / a.length) ** 2 / Math.max(1, a.length - 1) + (vb / b.length) ** 2 / Math.max(1, b.length - 1))
  const t = (ma - mb) / se
  return { t, df, p: pOfT(t, df) }
}
export function mannWhitney(a: number[], b: number[]): { u: number; p: number } {
  const r = ranks([...a, ...b])
  let ra = 0
  for (let i = 0; i < a.length; i++) ra += r[i]
  const u = ra - (a.length * (a.length + 1)) / 2
  const mu = (a.length * b.length) / 2
  const sd = Math.sqrt((a.length * b.length * (a.length + b.length + 1)) / 12) || 1
  return { u, p: 2 * (1 - normCdf(Math.abs((u - mu) / sd))) }
}
/** Percentile bootstrap interval of mean(a) − mean(b). */
export function bootstrapDiff(a: number[], b: number[], B: number, seed = 3): [number, number] {
  const rnd = seeded(seed)
  const draw = (v: number[]) => {
    let s = 0
    for (let i = 0; i < v.length; i++) s += v[Math.floor(rnd() * v.length)]
    return s / v.length
  }
  const d = Array.from({ length: B }, () => draw(a) - draw(b)).sort((p, q) => p - q)
  return [quantile(d, 0.025), quantile(d, 0.975)]
}
