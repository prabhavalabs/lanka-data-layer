// Transport for every data read in the app.
//
// Today `get()` resolves against the in-browser mock router below with
// realistic latency. To point the app at the real Rust API, replace the body
// of `get()` with the fetch shown in the comment — nothing else changes,
// because query functions only ever call `get(path, params)` with REST v2 paths.
import * as db from '@/mocks/db'

type Params = Record<string, string | number | undefined>
type Handler = (m: RegExpMatchArray, q: Params) => unknown | Promise<unknown>

const routes: [RegExp, Handler][] = [
  [/^\/v2\/status$/, () => db.STATS],
  [/^\/v2\/themes$/, () => db.THEMES],
  [/^\/v2\/landmarks$/, () => db.LANDMARKS],
  [/^\/v2\/areas$/, async (_, q) => (await db.loadAreas()).list.filter((a) => q.level === undefined || a.level === Number(q.level))],
  [/^\/v2\/areas\/([A-Za-z0-9]+)$/, (m) => db.areaProfile(m[1])],
  [/^\/v2\/indicators$/, () => db.INDICATORS],
  [/^\/v2\/indicators\/([\w.]+)\/map$/, (m, q) => db.indicatorMap(m[1], (Number(q.level) || 2) as 1 | 2, Number(q.period) || 2024)],
  [/^\/v2\/indicators\/([\w.]+)\/series$/, (m, q) => {
    const ind = db.INDICATORS.find((i) => i.id === m[1]) ?? db.INDICATORS[0]
    return String(q.geo ?? 'LK').split(',').map((pcode) => ({ pcode, points: ind.years.map((year) => ({ year, value: db.valueFor(ind.id, pcode, year) })) }))
  }],
  [/^\/v2\/catalog$/, (_, q) => (q.q ? db.search(String(q.q)) : db.DATASETS)],
  [/^\/v2\/search$/, (_, q) => db.search(String(q.q ?? ''))],
  [/^\/v2\/datasets\/([\w-]+)$/, (m) => db.datasetDetail(m[1])],
  [/^\/v2\/observations\/([\w-]+)\/provenance$/, (m) => db.provenance(m[1])],
]

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms))

export async function get<T>(path: string, params: Params = {}): Promise<T> {
  // Real API:
  //   const qs = new URLSearchParams(Object.entries(params).filter(([, v]) => v !== undefined).map(([k, v]) => [k, String(v)]))
  //   const res = await fetch(`${import.meta.env.VITE_API_BASE}${path}?${qs}`)
  //   const body = await res.json()
  //   if (!body.success) throw new Error(body.message)
  //   return body.payload as T
  for (const [re, handler] of routes) {
    const m = path.match(re)
    if (!m) continue
    const [payload] = await Promise.all([handler(m, params), wait(140 + Math.random() * 260)])
    return payload as T
  }
  await wait(120)
  throw new Error(`404 ${path}`)
}
