// The statistics kernels of the engine (`lanka-stats`), compiled to WebAssembly: class breaks, correlation
// and regression for instant previews, bit-identical to what the server computes (WP-29, 962 values checked).
// 51 KB gzip, fetched the first time the workspace needs it; until then the plain TS versions in ./stats answer.
import type { LankaStats } from '@/vendor/lanka-stats/lanka-stats.mjs'

let kernel: LankaStats | null = null
let loading: Promise<LankaStats | null> | null = null
const listeners = new Set<() => void>()

/** Loads the module once; resolves to null when WebAssembly is unavailable (the TS fallbacks stay in use). */
export function loadKernel(): Promise<LankaStats | null> {
  loading ??= (async () => {
    try {
      const [{ loadStats }, { default: url }] = await Promise.all([import('@/vendor/lanka-stats/lanka-stats.mjs'), import('@/vendor/lanka-stats/lanka_stats_wasm.wasm?url')])
      kernel = await loadStats(fetch(url))
      for (const l of listeners) l()
      return kernel
    } catch {
      return null
    }
  })()
  return loading
}
/** The kernel if it has loaded, else null: callers keep a synchronous fallback. */
export const statsKernel = () => kernel
/** Called once, when the kernel arrives: computed breaks may change by a rounding step. */
export function onKernel(fn: () => void): () => void {
  listeners.add(fn)
  return () => listeners.delete(fn)
}
