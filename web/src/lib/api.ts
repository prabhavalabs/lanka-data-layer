import type { Envelope } from "@geopub/shared";

/**
 * Tiny typed client for the Geopub API. Base path and envelope shape follow
 * docs/architecture.md §4: every route returns
 * `{ success, message, payload, meta }`, and the dev server proxies `/v1`
 * to the API (see vite.config.ts) so this works unmodified in prod behind
 * the same-origin nginx vhost.
 */
const API_BASE = "/v1";

/** Thrown for network failures, non-2xx responses, or `success: false`. */
export class ApiError extends Error {
  /** HTTP status code, or 0 when the request never reached the server. */
  readonly status: number;

  /**
   * True when we never got a real API envelope back at all — a thrown
   * fetch (status 0), or a response that doesn't even parse as JSON. Per
   * docs/architecture.md §4 the API always answers with the
   * `{success,message,payload,meta}` envelope, even for its own errors, so
   * an unparseable response means something other than the API answered:
   * the Vite dev proxy or the prod nginx vhost returning its own error page
   * because the upstream is down. Almost always means "API isn't running".
   */
  readonly unreachable: boolean;

  constructor(message: string, status: number, unreachable = false) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.unreachable = unreachable;
  }
}

export interface ApiRequestOptions {
  params?: Record<string, string | number | boolean | undefined>;
  signal?: AbortSignal;
}

function buildQuery(params: ApiRequestOptions["params"]): string {
  if (!params) return "";
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) search.set(key, String(value));
  }
  const qs = search.toString();
  return qs ? `?${qs}` : "";
}

/**
 * Fetches `path` under the Geopub API base and returns the full envelope.
 * Throws {@link ApiError} on a network failure, non-2xx response, or a
 * `success: false` payload.
 */
export async function apiGet<T>(
  path: string,
  options: ApiRequestOptions = {}
): Promise<Envelope<T>> {
  const url = `${API_BASE}${path}${buildQuery(options.params)}`;

  let res: Response;
  try {
    res = await fetch(url, { signal: options.signal });
  } catch (err) {
    if (err instanceof DOMException && err.name === "AbortError") throw err;
    throw new ApiError(`Could not reach ${url}: ${(err as Error).message}`, 0, true);
  }

  let envelope: Envelope<T> | undefined;
  try {
    envelope = (await res.json()) as Envelope<T>;
  } catch {
    envelope = undefined;
  }

  if (!envelope) {
    throw new ApiError(`No response from the API at ${url} (${res.status})`, res.status, true);
  }
  if (!res.ok || !envelope.success) {
    throw new ApiError(envelope.message || `Request to ${url} failed (${res.status})`, res.status);
  }

  return envelope;
}

/** Like {@link apiGet}, but unwraps and returns just the payload. */
export async function apiGetPayload<T>(path: string, options?: ApiRequestOptions): Promise<T> {
  const envelope = await apiGet<T>(path, options);
  if (envelope.payload === null) {
    throw new ApiError(`${API_BASE}${path} returned a null payload`, 200);
  }
  return envelope.payload;
}
