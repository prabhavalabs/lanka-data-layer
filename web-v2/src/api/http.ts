// Transport to the Lanka Data Layer API (the Rust engine): the public plane `/v2` and the app plane `/app/v1`.
//
// `VITE_API_BASE` is the origin the API lives on; empty (the default) means the page's own origin, which the
// Vite dev server proxies to the engine. The engine hands back relative paths (tile templates, analysis
// output `href`s) and absolute URLs built from its public base or the request's own host (TileJSON), so a
// relative path is prefixed with `API_BASE` and an absolute URL is used as it is.

export const API_BASE = ((import.meta.env.VITE_API_BASE as string | undefined) ?? '').replace(/\/+$/, '')

export type Attribution = { source_id: string; publisher: string; text: string; licence: string; licence_url?: string; licence_status: 'open' | 'attribution' | 'permission_unclear' | 'restricted'; redistribution: string }
export type Meta = {
  data_version: string | null
  contract_version: number
  dataset_versions: { dataset: string; dsv: string; vintage?: string | null }[]
  source?: { id: string; publisher: string; title: string; url: string; licence?: string | null; attribution?: string | null }[]
  licences?: { id: string; status: string; url?: string | null; attribution: string }[]
  attribution?: Attribution[]
}
/** The error payload of the envelope (ui-contract §2). */
export type Problem = {
  type?: string
  title?: string
  status?: number
  detail?: string
  problems?: { path: string; code: string; message: string }[]
  hints?: { retry_after_s?: number; valid_values?: string[]; did_you_mean?: string[]; available_levels?: number[]; available_periods?: string[]; [k: string]: unknown }
  /** a parameter the engine could not match (`UNKNOWN_DIM`, `DIMS_NO_MATCH`), with the values it would take */
  problem?: { code: string; param?: string; given?: string | string[]; valid?: Record<string, string[]>; valid_dims?: Record<string, string[]>; [k: string]: unknown }
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly problem: Problem | null,
  ) {
    super(message)
  }
  /** seconds to wait before retrying, for a 429 */
  get retryAfter(): number | null {
    return this.problem?.hints?.retry_after_s ?? null
  }
  /** periods that do have data, when the request asked for one that doesn't */
  get availablePeriods(): string[] | null {
    const v = this.problem?.hints?.available_periods
    return Array.isArray(v) && v.length ? v : null
  }
}

const ABSOLUTE = /^https?:\/\//
/** A path of the API on `API_BASE`; an absolute URL the engine built is already reachable and passes through. */
export const apiUrl = (pathOrUrl: string): string => (ABSOLUTE.test(pathOrUrl) ? pathOrUrl : `${API_BASE}${pathOrUrl.startsWith('/') ? '' : '/'}${pathOrUrl}`)
/** The same, absolute: MapLibre fetches tiles from a worker, which has no page URL to resolve a relative template against. */
export const absoluteApiUrl = (pathOrUrl: string) => {
  const url = apiUrl(pathOrUrl)
  return ABSOLUTE.test(url) ? url : `${window.location.origin}${url}`
}

type Params = Record<string, string | number | boolean | undefined | null>
const query = (params: Params) => {
  const qs = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== '') qs.set(k, String(v))
  const s = qs.toString()
  return s ? `?${s}` : ''
}

type Envelope<T> = { success: boolean; message: string; payload: T; meta?: Meta }

async function parse<T>(res: Response): Promise<{ payload: T; meta: Meta | undefined }> {
  let body: Envelope<T> | null = null
  try {
    body = (await res.json()) as Envelope<T>
  } catch {
    /* not JSON */
  }
  if (!res.ok || !body || body.success === false) {
    const problem = body && typeof body.payload === 'object' ? (body.payload as unknown as Problem) : null
    const retry = res.headers.get('retry-after')
    if (res.status === 429 && problem && !problem.hints?.retry_after_s && retry) problem.hints = { ...problem.hints, retry_after_s: Number(retry) }
    throw new ApiError(problem?.detail ?? problem?.title ?? body?.message ?? `${res.status} ${res.statusText}`, res.status, problem)
  }
  return { payload: body.payload, meta: body.meta }
}

/** GET an enveloped `/v2` (or `/app/v1`) resource; resolves to the payload and the envelope's meta. */
export async function getEnvelope<T>(path: string, params: Params = {}, init: RequestInit = {}): Promise<{ payload: T; meta: Meta | undefined }> {
  const app = path.startsWith('/app/')
  const res = await fetch(apiUrl(path) + query(params), { credentials: app ? 'include' : 'omit', ...init })
  return parse<T>(res)
}
export const getJson = async <T>(path: string, params: Params = {}, init: RequestInit = {}): Promise<T> => (await getEnvelope<T>(path, params, init)).payload

/** POST JSON; fetch follows a 303 to the GET it names, so `POST /v2/analyses` resolves to the result. */
export async function postJson<T>(path: string, body: unknown, init: RequestInit = {}): Promise<{ payload: T; meta: Meta | undefined; status: number }> {
  const app = path.startsWith('/app/')
  const res = await fetch(apiUrl(path), { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body), credentials: app ? 'include' : 'omit', ...init })
  const out = await parse<T>(res)
  return { ...out, status: res.status }
}

/** Raw bytes (value tiles, Arrow). */
export async function getBytes(pathOrUrl: string, signal?: AbortSignal): Promise<Response> {
  const res = await fetch(apiUrl(pathOrUrl), { credentials: 'omit', signal })
  if (!res.ok && res.status !== 204) throw new ApiError(`${res.status} ${pathOrUrl}`, res.status, null)
  return res
}

/* ---------------- is the API there at all ---------------- */
let probe: Promise<boolean> | null = null
let failedAt = 0
/**
 * One cheap request: when the engine is unreachable every reader falls back to the bundled sample data. A yes
 * holds for the page's life; a no only for a few seconds, so one slow answer (the engine busy rebuilding, say)
 * does not leave the whole page on sample data until a reload.
 */
export function apiAvailable(): Promise<boolean> {
  if (probe && failedAt && Date.now() - failedAt > 5000) probe = null
  probe ??= fetch(apiUrl('/v2/health'), { credentials: 'omit', signal: AbortSignal.timeout(8000) })
    .then((r) => r.ok)
    .catch(() => false)
    .then((ok) => {
      failedAt = ok ? 0 : Date.now()
      return ok
    })
  return probe
}
