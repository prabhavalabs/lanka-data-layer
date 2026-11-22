// Transport for the page-level data reads (home stats, catalogue, dataset, area profile, provenance).
//
// `get()` tries the live v2 API first for the routes in `realRoutes` and falls back to the in-browser mock router
// below, with realistic latency, when the API is unreachable, errors, or lacks the data. Mock payloads carry
// `sample: ['*']` so the pages can mark them; live payloads list only the parts still filled from the mocks.
import * as db from '@/mocks/db'
import { apiAvailable, ApiError } from './http'
import * as real from './real/pages'

type Params = Record<string, string | number | undefined>
type Handler = (m: RegExpMatchArray, q: Params) => unknown | Promise<unknown>

/** Live adapters, tried before the mocks. Resolving to `undefined` means "not mine": fall through to the mock. */
const realRoutes: [RegExp, Handler][] = [
  [/^\/v2\/status$/, () => real.stats()],
  [/^\/v2\/catalog$/, (_, q) => (q.q ? undefined : real.datasets())],
  [/^\/v2\/datasets\/([\w.-]+)$/, (m) => real.dataset(m[1])],
  [/^\/v2\/areas\/([A-Za-z0-9]+)$/, (m) => real.areaProfile(m[1])],
  [/^\/v2\/observations\/(\d+)\/provenance$/, (m) => real.provenance(m[1])],
]
const SAMPLE = ['*']

const routes: [RegExp, Handler][] = [
  [/^\/v2\/status$/, () => ({ ...db.STATS, sample: SAMPLE })],
  [/^\/v2\/themes$/, () => db.THEMES],
  [/^\/v2\/landmarks$/, () => db.LANDMARKS],
  [/^\/v2\/areas$/, async (_, q) => (await db.loadAreas()).list.filter((a) => q.level === undefined || a.level === Number(q.level))],
  [/^\/v2\/areas\/([A-Za-z0-9]+)$/, async (m) => {
    if (!(await db.loadAreas()).byCode.has(m[1].toUpperCase())) throw new Error(`404 area ${m[1]}`)
    return { ...(await db.areaProfile(m[1])), sample: SAMPLE }
  }],
  [/^\/v2\/indicators$/, () => db.INDICATORS],
  [/^\/v2\/indicators\/([\w.]+)\/map$/, (m, q) => db.indicatorMap(m[1], (Number(q.level) || 2) as 1 | 2, Number(q.period) || 2024)],
  [/^\/v2\/indicators\/([\w.]+)\/series$/, (m, q) => {
    const ind = db.INDICATORS.find((i) => i.id === m[1]) ?? db.INDICATORS[0]
    return String(q.geo ?? 'LK').split(',').map((pcode) => ({ pcode, points: ind.years.map((year) => ({ year, value: db.valueFor(ind.id, pcode, year) })) }))
  }],
  [/^\/v2\/catalog$/, (_, q) => (q.q ? db.search(String(q.q)) : db.DATASETS.map((d) => ({ ...d, sample: SAMPLE })))],
  [/^\/v2\/search$/, (_, q) => db.search(String(q.q ?? ''))],
  [/^\/v2\/datasets\/([\w-]+)$/, (m) => {
    if (!db.DATASETS.some((d) => d.id === m[1])) throw new Error(`404 dataset ${m[1]}`)
    return { ...db.datasetDetail(m[1]), sample: SAMPLE }
  }],
  [/^\/v2\/observations\/([\w-]+)\/provenance$/, (m) => db.provenance(m[1])],
]

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms))

export async function get<T>(path: string, params: Params = {}): Promise<T> {
  for (const [re, handler] of realRoutes) {
    const m = path.match(re)
    if (!m || !(await apiAvailable())) continue
    try {
      const out = await handler(m, params)
      if (out !== undefined) return out as T
    } catch (e) {
      // an area the engine does not know is not an area the mocks should invent
      if (e instanceof ApiError && e.status === 404 && /^\/v2\/areas\//.test(path)) throw e
    }
  }
  for (const [re, handler] of routes) {
    const m = path.match(re)
    if (!m) continue
    const [payload] = await Promise.all([handler(m, params), wait(140 + Math.random() * 260)])
    return payload as T
  }
  await wait(120)
  throw new Error(`404 ${path}`)
}
