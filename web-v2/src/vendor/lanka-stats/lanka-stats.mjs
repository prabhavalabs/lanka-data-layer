// lanka-stats in the browser (UI contract §7 StatsKernel), over the C-ABI
// module built by `build.sh` (wasm32-unknown-unknown, no wasm-bindgen).
// Usage: const k = await loadStats(fetch('/lanka_stats_wasm.wasm'))
//        k.classify(new Float64Array(values), 'jenks', 5) → { breaks, classOf }

const METHODS = { quantile: 0, equal: 1, jenks: 2, stddev: 3, headtail: 4, pretty: 5 }
const TRANSFORMS = { identity: 0, log: 1, log1p: 2, sqrt: 3, zscore: 4, rank: 5 }

export async function loadStats(source) {
  const res = await source
  const bytes = res instanceof Response ? await res.arrayBuffer() : res
  const { instance } = await WebAssembly.instantiate(bytes, {})
  return wrap(instance.exports)
}

export function wrap(x) {
  const f64 = () => new Float64Array(x.memory.buffer)
  const put = (arr) => {
    const bytes = Math.max(arr.length, 1) * 8
    const ptr = x.lsw_alloc(bytes)
    f64().set(arr, ptr / 8)
    return { ptr, bytes, n: arr.length }
  }
  const out = (cap) => ({ ptr: x.lsw_alloc(cap * 8), bytes: cap * 8, cap })
  const take = (o, len) => f64().slice(o.ptr / 8, o.ptr / 8 + Math.min(len, o.cap))
  const free = (...bs) => bs.forEach((b) => x.lsw_free(b.ptr, b.bytes))
  // call `fn(args…, out, cap)`; grows the output buffer when it was too small
  const call = (cap, fn) => {
    for (;;) {
      const o = out(cap)
      const len = fn(o.ptr, o.cap)
      if (len <= cap) {
        const v = take(o, len)
        free(o)
        return v
      }
      free(o)
      cap = len
    }
  }
  const classesOf = (values, breaks) => {
    const c = new Uint8Array(values.length)
    for (let i = 0; i < values.length; i++) {
      const v = values[i]
      if (!Number.isFinite(v)) { c[i] = 255; continue }
      let k = 0
      while (k < breaks.length && v >= breaks[k]) k++
      c[i] = k
    }
    return c
  }
  return {
    raw: x,
    classify(values, method = 'quantile', k = 5) {
      const v = put(values)
      const b = call(k, (o, cap) => x.lsw_classify(v.ptr, v.n, METHODS[method] ?? 0, k, o, cap))
      free(v)
      return { breaks: Array.from(b), classOf: classesOf(values, b) }
    },
    bivariate(xs, ys, method = 'quantile', k = 3) {
      const a = put(xs), b = put(ys)
      const r = call(2 * k + xs.length + 2, (o, cap) => x.lsw_bivariate(a.ptr, b.ptr, a.n, METHODS[method] ?? 0, k, o, cap))
      free(a, b)
      const nx = r[0], ny = r[1]
      return { xBreaks: Array.from(r.slice(2, 2 + nx)), yBreaks: Array.from(r.slice(2 + nx, 2 + nx + ny)), classOf: Uint8Array.from(r.slice(2 + nx + ny)) }
    },
    describe(values) {
      const v = put(values)
      const r = call(15, (o, cap) => x.lsw_describe(v.ptr, v.n, o, cap))
      free(v)
      const [n, missing, mean, sd, min, max, p5, p25, p50, p75, p95, mad, skewness, kurtosis, nonPositive] = r
      return { n, missing, mean, sd, min, max, q: [min, p5, p25, p50, p75, p95, max], mad, skewness, kurtosis, nonPositive }
    },
    correlate(xs, ys) {
      const a = put(xs), b = put(ys)
      const r = call(9, (o, cap) => x.lsw_correlate(a.ptr, b.ptr, a.n, o, cap))
      free(a, b)
      return { n: r[0], pearson: { r: r[1], ci: [r[2], r[3]], p: r[4] }, spearman: { rho: r[5], ci: [r[6], r[7]], p: r[8] } }
    },
    regress(y, columns) {
      const n = y.length, k = columns.length
      const flat = new Float64Array(n * k)
      columns.forEach((c, j) => flat.set(c, j * n))
      const a = put(y), b = put(flat)
      const r = call(3 * (k + 1) + 6, (o, cap) => x.lsw_regress(a.ptr, b.ptr, n, k, o, cap))
      free(a, b)
      if (r[0] !== 0) return { error: r[0] === 1 ? 'too_few_rows' : 'singular' }
      const p = r[2], at = (i) => Array.from(r.slice(3 + i * p, 3 + (i + 1) * p))
      return { n: r[1], coef: at(0), se_hc3: at(1), p_hc3: at(2), r2: r[3 + 3 * p], adj_r2: r[4 + 3 * p], sigma: r[5 + 3 * p] }
    },
    previewRelate(xs, ys, tx = 'identity', ty = 'identity') {
      const a = put(xs), b = put(ys)
      const r = call(5, (o, cap) => x.lsw_preview_relate(a.ptr, b.ptr, a.n, TRANSFORMS[tx] ?? 0, TRANSFORMS[ty] ?? 0, o, cap))
      free(a, b)
      return { n: r[0], r: r[1], rho: r[2], fit: { kind: 'ols', intercept: r[3], slope: r[4] } }
    },
    hexbin(xs, ys, radius) {
      const a = put(xs), b = put(ys)
      const r = call(2 + 3 * xs.length, (o, cap) => x.lsw_hexbin(a.ptr, b.ptr, a.n, radius, o, cap))
      free(a, b)
      const m = r[0]
      return { encoding: 'hexbin', cx: Array.from(r.slice(2, 2 + m)), cy: Array.from(r.slice(2 + m, 2 + 2 * m)), count: Array.from(r.slice(2 + 2 * m, 2 + 3 * m)), radius: r[1] }
    },
  }
}
