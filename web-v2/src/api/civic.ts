// The civic record: ministries, the institutions in each minister's charge, and how both change with each presidential
// gazette (docs/architecture-v2/13-civic-record.md §2.1). Reads the live `/v2/civic` and `/v2/documents` endpoints;
// when the engine is unreachable or does not have them yet, each read falls back to the bundled sample in
// `src/mocks/civic.ts`, and the result says so through `sample` so the page can mark it.
import { queryOptions } from '@tanstack/react-query'
import { mockCivic } from '@/mocks/civic'
import { apiAvailable, apiUrl, ApiError, getJson } from './http'

export type EntityKind = 'department' | 'statutory_body' | 'corporation' | 'company' | 'fund' | 'university' | 'force' | 'other'
export type InstrumentStatus = 'validated' | 'needs_review' | 'approved' | 'rejected'

export type Instrument = {
  doc_id: string
  kind: 'baseline' | 'supplement' | 'amendment'
  effective_from: string
  amends: string | null
  head_of_state: string | null
  status: InstrumentStatus
  portfolios: number
  institutions: number
  title: string
  number: string
}

export type Item = { item_no: number; text: string }
export type StructureInstitution = Item & { entity_key: string | null; kind: EntityKind; page: number | null }
export type StructurePortfolio = {
  /** the gazette this ministry is printed in; in a composed structure the pages belong to different gazettes */
  doc_id: string
  seq: number
  title: string
  short_title?: string
  portfolio_key: string
  page: number | null
  functions: Item[]
  institutions: StructureInstitution[]
  laws: Item[]
}
export type Structure = {
  at: string
  instrument: { doc_id: string; effective_from: string; number: string }
  composed_of?: { doc_id: string; effective_from: string }[]
  amendments_since: { doc_id: string; effective_from: string; title: string }[]
  portfolios: StructurePortfolio[]
}

export type PortfolioRef = { portfolio_key: string; title: string; short_title?: string; page?: number | null }
export type DiffInstrument = { doc_id: string; effective_from: string; number?: string }
export type Moved = { entity_key: string; name: string; kind: EntityKind; from: PortfolioRef; to: PortfolioRef }
export type Added = { entity_key: string; name: string; kind: EntityKind; to: PortfolioRef }
export type Removed = { entity_key: string; name: string; kind: EntityKind; from: PortfolioRef }
export type Diff = {
  from: DiffInstrument
  to: DiffInstrument
  moved: Moved[]
  added: Added[]
  removed: Removed[]
  portfolios: { added: string[]; removed: string[]; kept: { from_title: string; to_title: string; from_key?: string; to_key?: string; overlap?: number; share?: number; relation?: 'same' | 'renamed' | 'successor'; renamed?: boolean }[] }
  counts?: { moved: number; added: number; removed: number; portfolios_added: number; portfolios_removed: number; portfolios_renamed: number }
}

export type EntityHit = { entity_key: string; name: string; kind: EntityKind; current: { portfolio_title: string } | null }
export type Assignment = { portfolio_title: string; portfolio_key: string; valid_from: string; valid_to: string | null; doc_id: string; page: number | null }
export type EntityDetail = { entity_key: string; name: string; kind: EntityKind; aliases: string[]; history: Assignment[] }

export type GazetteDocument = {
  doc_id: string
  number: string
  issued_on: string
  title: string
  files: { lang: string; status: string; pages: number; bytes: number; sha256: string }[]
  source_url: string | null
}

/** A read and whether it came from the bundled sample rather than the engine. */
export type Civic<T> = { data: T; sample: boolean }

/** Engine first; sample when the engine is unreachable, answers 404 (the route is not there yet) or fails on its side. */
async function read<T>(path: string, params: Record<string, string | number | undefined>, fallback: () => T | Promise<T>): Promise<Civic<T>> {
  if (await apiAvailable()) {
    try {
      return { data: await getJson<T>(path, params), sample: false }
    } catch (e) {
      // a missing route (404 without the engine's problem body) or a broken engine: sample. A real answer, 'not in coverage' included, is the reader's to see.
      const down = !(e instanceof ApiError) || (e.status === 404 && !e.problem) || e.status >= 500
      if (!down) throw e
    }
  }
  return { data: await fallback(), sample: true }
}

const HOUR = 60 * 60 * 1000

export const instrumentsQuery = () =>
  queryOptions({ queryKey: ['civic', 'instruments'], queryFn: () => read<Instrument[]>('/v2/civic/instruments', {}, () => mockCivic.instruments()), staleTime: HOUR })

export const structureQuery = (at: string) =>
  queryOptions({ queryKey: ['civic', 'structure', at], queryFn: () => read<Structure>('/v2/civic/structure', { at }, () => mockCivic.structure(at)), staleTime: HOUR, retry: (n, e) => !(e instanceof ApiError && e.status < 500) && n < 1 })

/** The name to show for a ministry: the engine's `short_title`, else the printed title without its prefix. */
export const plain = (t: string) => t.replace(/^Minister (of|for) /, '')
export const shortOf = (r: { title: string; short_title?: string }) => r.short_title ?? plain(r.title)
const withShort = <T extends { title: string; short_title?: string }>(r: T): T => ({ ...r, short_title: shortOf(r) })

/** Every ministry reference gets a `short_title`, so the view reads one field. */
function cleanDiff(c: Civic<Diff>): Civic<Diff> {
  const d = c.data
  return {
    ...c,
    data: {
      ...d,
      moved: d.moved.map((m) => ({ ...m, from: withShort(m.from), to: withShort(m.to) })),
      added: d.added.map((m) => ({ ...m, to: withShort(m.to) })),
      removed: d.removed.map((m) => ({ ...m, from: withShort(m.from) })),
    },
  }
}

/** The earliest date with a schedule, from the 404 a date before the first one gets. */
export const earliestOf = (e: unknown): string | null => (e instanceof ApiError && e.status === 404 ? ((e.problem?.problem as { earliest?: string } | undefined)?.earliest ?? null) : null)

export const diffQuery = (from: string, to: string) =>
  queryOptions({ queryKey: ['civic', 'diff', from, to], queryFn: async () => cleanDiff(await read<Diff>('/v2/civic/diff', { from, to }, () => mockCivic.diff(from, to))), staleTime: HOUR, enabled: !!from && !!to, retry: false })

export const entitySearchQuery = (q: string) =>
  queryOptions({ queryKey: ['civic', 'entities', q], queryFn: () => read<EntityHit[]>('/v2/civic/entities', { q, limit: 8 }, () => mockCivic.entities(q)), staleTime: HOUR, enabled: q.trim().length >= 2 })

export const entityQuery = (key: string | null) =>
  queryOptions({ queryKey: ['civic', 'entity', key], queryFn: () => read<EntityDetail>(`/v2/civic/entities/${encodeURIComponent(key ?? '')}`, {}, () => mockCivic.entity(key ?? '')), staleTime: HOUR, enabled: !!key, retry: false })

/** The gazette's archived PDF; `page` opens it there. */
export const gazettePdf = (docId: string, page?: number | null, lang = 'en') => `${apiUrl(`/v2/documents/${encodeURIComponent(docId)}/${lang}.pdf`)}${page ? `#page=${page}` : ''}`

export const documentQuery = (docId: string) =>
  queryOptions({ queryKey: ['civic', 'document', docId], queryFn: () => read<GazetteDocument>(`/v2/documents/${encodeURIComponent(docId)}`, {}, () => mockCivic.document(docId)), staleTime: HOUR })

/** "2412/08" from "gze-2412-08". */
export const gazetteNumber = (docId: string) => docId.replace(/^gze-/, '').replace('-', '/')

export const KIND_LABEL: Record<EntityKind, string> = {
  department: 'Department',
  statutory_body: 'Statutory body',
  corporation: 'Corporation',
  company: 'Company',
  fund: 'Fund',
  university: 'University',
  force: 'Armed force or police',
  other: 'Other',
}
export const KINDS = Object.keys(KIND_LABEL) as EntityKind[]
