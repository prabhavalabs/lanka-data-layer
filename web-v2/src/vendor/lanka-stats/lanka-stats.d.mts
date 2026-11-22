// Types of the hand-written wrapper around the lanka-stats C-ABI WebAssembly module (engine crate
// `lanka-stats-wasm`, copied verbatim with its .wasm; the native build gives the same bits).
export type Method = 'quantile' | 'equal' | 'jenks' | 'stddev' | 'headtail' | 'pretty'
export type Transform = 'identity' | 'log' | 'log1p' | 'sqrt' | 'zscore' | 'rank'
export type LankaStats = {
  classify(values: Float64Array, method?: Method, k?: number): { breaks: number[]; classOf: Uint8Array }
  bivariate(xs: Float64Array, ys: Float64Array, method?: Method, k?: number): { xBreaks: number[]; yBreaks: number[]; classOf: Uint8Array }
  describe(values: Float64Array): { n: number; missing: number; mean: number; sd: number; min: number; max: number; q: number[]; mad: number; skewness: number; kurtosis: number; nonPositive: number }
  correlate(xs: Float64Array, ys: Float64Array): { n: number; pearson: { r: number; ci: [number, number]; p: number }; spearman: { rho: number; ci: [number, number]; p: number } }
  regress(y: Float64Array, columns: Float64Array[]): { n: number; coef: number[]; se_hc3: number[]; p_hc3: number[]; r2: number; adj_r2: number; sigma: number } | { error: 'too_few_rows' | 'singular' }
  previewRelate(xs: Float64Array, ys: Float64Array, tx?: Transform, ty?: Transform): { n: number; r: number; rho: number; fit: { kind: 'ols'; intercept: number; slope: number } }
  hexbin(xs: Float64Array, ys: Float64Array, radius: number): { encoding: 'hexbin'; cx: number[]; cy: number[]; count: number[]; radius: number }
}
export function loadStats(source: Response | Promise<Response> | ArrayBuffer | Uint8Array): Promise<LankaStats>
export function wrap(exports: WebAssembly.Exports): LankaStats
