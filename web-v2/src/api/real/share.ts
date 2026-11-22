// Sharing through the app plane (WP-28): an immutable snapshot of the document, addressed by its content.
//
// `POST /app/v1/maps/snapshots` → `ms_<16 hex>` (the same content always gets the same id; 5 new ones per
// hour per visitor, then 429 with `retry_after_s`); `/m/{id}` and `/embed/m/{id}` read it back through
// `GET /app/v1/maps/{id}/resolved`, whose attribution and notices are always drawn.
import { ApiError, getJson, postJson } from '@/api/http'
import { blankDocument, type MapDocument } from '@/lib/map-spec'
import type { Resolved } from './layers'

export type Snapshot = { id: string; url: string; embed: string; created: boolean }

/** Stores the document as a snapshot; the links point at this deployment's own `/m/` and `/embed/m/` pages. */
export async function createSnapshot(doc: MapDocument): Promise<Snapshot> {
  const { id: _own, ...body } = doc
  void _own
  const { payload } = await postJson<{ id: string; created: boolean }>('/app/v1/maps/snapshots', body)
  const origin = window.location.origin
  return { id: payload.id, url: `${origin}/m/${payload.id}`, embed: `${origin}/embed/m/${payload.id}`, created: payload.created }
}
/** A sentence for a refused snapshot: the JSON pointer of each problem, or when to try again. */
export function snapshotError(e: unknown): string {
  if (!(e instanceof ApiError)) return 'Could not reach the server'
  if (e.status === 429) return `Too many new links: try again in ${Math.ceil((e.retryAfter ?? 60) / 60)} min`
  if (e.status === 413) return 'This map is too large to share as a link'
  const first = e.problem?.problems?.[0]
  return first ? `Not shareable: ${first.path || '/'} ${first.message}` : e.message
}

/** The shared document, completed with this client's defaults, and its resolution. */
export async function fetchShared(id: string): Promise<{ doc: MapDocument; resolved: Resolved }> {
  const payload = await getJson<MapDocument & { resolved: Resolved }>(`/app/v1/maps/${encodeURIComponent(id)}/resolved`)
  const { resolved, ...rest } = payload
  const base = blankDocument(typeof rest.title === 'string' ? rest.title : 'Shared map')
  const doc: MapDocument = { ...base, ...rest, basemap: { ...base.basemap, ...rest.basemap }, meta: { ...base.meta, ...(rest.meta ?? {}) }, analyses: rest.analyses ?? {}, panels: rest.panels ?? base.panels, time: rest.time ?? null, extent: rest.extent ?? null, id }
  return { doc, resolved }
}
