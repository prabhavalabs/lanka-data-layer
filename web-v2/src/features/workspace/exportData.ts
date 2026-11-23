// Export clipped to an area: the admin units to clip to, an area's boundary, each layer's data inside it,
// and the zip that carries several files with their attribution. Clipping is by p-code (p-codes are the
// only admin join keys): a unit layer keeps the units inside the area, or the one unit that contains it;
// features keep what the engine places in the area (`within`).
import type { Feature, FeatureCollection, Geometry, MultiPolygon, Polygon } from 'geojson'
import { apiUrl, getBytes, getEnvelope, getJson } from '@/api/http'
import type { ResolvedSource } from '@/api/workspace'
import { LEVELS } from '@/api/workspace'
import type { Layer, Source } from '@/lib/map-spec'

export type Area = { pcode: string; level: number; name: string; parent: string | null }
export type AreaShape = Feature<Polygon | MultiPolygon> & { bbox: [number, number, number, number] }

type UnitRow = { pcode: string; level: number; parent: string | null; name_en: string }
type UnitPage = { units: UnitRow[]; next_cursor: string | null }

/** Every unit of a level (optionally under a parent), following the cursor. */
async function units(level: number, parent?: string): Promise<UnitRow[]> {
  const out: UnitRow[] = []
  let cursor: string | null = null
  do {
    const page: UnitPage = await getJson<UnitPage>('/v2/units', { level, parent, limit: 20000, cursor })
    out.push(...page.units)
    cursor = page.next_cursor
  } while (cursor)
  return out
}

/** Provinces, districts and DS divisions: what an export can be clipped to (GN divisions are too many to pick from a list). */
export async function listAreas(): Promise<Area[]> {
  const levels = await Promise.all([1, 2, 3].map((l) => units(l)))
  return levels.flat().map((u) => ({ pcode: u.pcode, level: u.level, name: u.name_en, parent: u.parent }))
}

/** The area's boundary, at display resolution (about 55 m): enough to mask an image with. */
export async function areaShape(pcode: string): Promise<AreaShape> {
  const res = await getBytes(`/v2/units/${encodeURIComponent(pcode)}/geometry?simplify=display&format=geojson`)
  return (await res.json()) as AreaShape
}

/* ---------------- what a layer exports as ---------------- */
export type DataFormat = 'csv' | 'geojson'
export type Exportable = { layer: Layer; formats: DataFormat[]; note?: string }

/** Which formats a layer's data leaves in; a grid or raster has none (it goes out in the image). */
export function exportable(layer: Layer, source: Source | undefined, r: ResolvedSource | null | undefined): Exportable {
  if (!source || !r) return { layer, formats: [], note: 'Sample data: not exported' }
  if (source.kind === 'indicator' && r.frame) return { layer, formats: ['csv', 'geojson'] }
  if (source.kind === 'boundaries') return { layer, formats: ['geojson', 'csv'] }
  if (source.kind === 'features' && (r.url || r.points || r.tiles)) return { layer, formats: ['geojson'] }
  if (source.kind === 'grid') return { layer, formats: [], note: 'A grid: exported in the image only' }
  if (source.kind === 'analysis') return { layer, formats: [], note: 'An analysis output: export it from Relate' }
  return { layer, formats: [], note: 'Not exportable as data' }
}

export type ExportFile = { name: string; type: string; body: string | Uint8Array }

/** A unit code is in the area when it is the area, below it, or the one unit above it that contains it. */
const inArea = (code: string, area: Area | null) => !area || code.startsWith(area.pcode) || area.pcode.startsWith(code)
/** `parent` narrows a request only when the area is coarser than what is asked for. */
const parentFor = (level: number, area: Area | null) => (area && area.level < level ? area.pcode : undefined)

type ServerFrame = { support: { level: number }; codes: string[]; columns: { indicator: string; period: string; unit: string; dims: Record<string, string>; values: (number | null)[] }[] }

const csvCell = (v: unknown) => {
  if (v === null || v === undefined) return ''
  const s = String(v)
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s
}
const csv = (header: string[], rows: unknown[][]) => [header, ...rows].map((r) => r.map(csvCell).join(',')).join('\n') + '\n'

/** Many unit boundaries at once; units are fetched in parallel, a few at a time, so a district of DS divisions stays quick. */
async function unitShapes(codes: string[], onProgress?: (done: number, total: number) => void): Promise<Map<string, Geometry>> {
  const out = new Map<string, Geometry>()
  let next = 0
  let done = 0
  const worker = async () => {
    while (next < codes.length) {
      const code = codes[next++]
      const f = (await (await getBytes(`/v2/units/${encodeURIComponent(code)}/geometry?simplify=display&format=geojson`)).json()) as Feature
      out.set(code, f.geometry)
      onProgress?.(++done, codes.length)
    }
  }
  await Promise.all(Array.from({ length: Math.min(8, codes.length) }, worker))
  return out
}
/** Polygons for a unit layer are fetched one unit each: beyond this many, export the table instead. */
export const MAX_SHAPES = 600

/** One layer's data inside the area, in one format. */
export async function layerData(ex: Exportable, source: Source, r: ResolvedSource, area: Area | null, format: DataFormat, onProgress?: (done: number, total: number) => void): Promise<ExportFile> {
  const base = slug(ex.layer.name)
  if (source.kind === 'indicator' && r.frame) {
    const url = new URL(apiUrl(r.frame), window.location.origin)
    const parent = parentFor(source.level, area)
    if (parent) url.searchParams.set('parent', parent)
    else url.searchParams.delete('parent')
    const { payload: f } = await getEnvelope<ServerFrame>(url.pathname + url.search)
    const keep = f.codes.map((c, i) => [c, i] as const).filter(([c]) => inArea(c, area))
    const names = await unitNames(f.support.level, parent)
    const cols = f.columns.map((c) => ({ key: `${c.indicator}${Object.keys(c.dims ?? {}).length ? `[${Object.values(c.dims).join(',')}]` : ''}:${c.period}`, values: c.values }))
    if (format === 'csv') {
      const rows = keep.map(([c, i]) => [c, names.get(c) ?? '', ...cols.map((col) => col.values[i])])
      return { name: `${base}.csv`, type: 'text/csv', body: csv(['pcode', 'name', ...cols.map((c) => c.key)], rows) }
    }
    if (keep.length > MAX_SHAPES) throw new Error(`${keep.length.toLocaleString('en')} ${LEVELS[f.support.level].plural} in this area: too many to export as shapes. Export the CSV, or pick a smaller area.`)
    const shapes = await unitShapes(keep.map(([c]) => c), onProgress)
    const features: Feature[] = keep.map(([c, i]) => ({ type: 'Feature', id: c, geometry: shapes.get(c) ?? null!, properties: { pcode: c, name: names.get(c) ?? null, ...Object.fromEntries(cols.map((col) => [col.key, col.values[i]])) } }))
    return { name: `${base}.geojson`, type: 'application/geo+json', body: JSON.stringify({ type: 'FeatureCollection', features } satisfies FeatureCollection) }
  }
  if (source.kind === 'boundaries') {
    const parent = parentFor(source.level, area)
    const rows = (await units(source.level, parent)).filter((u) => inArea(u.pcode, area))
    if (format === 'csv') return { name: `${base}.csv`, type: 'text/csv', body: csv(['pcode', 'name', 'parent'], rows.map((u) => [u.pcode, u.name_en, u.parent])) }
    if (rows.length > MAX_SHAPES) throw new Error(`${rows.length.toLocaleString('en')} ${LEVELS[source.level].plural} in this area: too many to export as shapes. Export the CSV, or pick a smaller area.`)
    const shapes = await unitShapes(rows.map((u) => u.pcode), onProgress)
    const features: Feature[] = rows.map((u) => ({ type: 'Feature', id: u.pcode, geometry: shapes.get(u.pcode) ?? null!, properties: { pcode: u.pcode, name: u.name_en, parent: u.parent } }))
    return { name: `${base}.geojson`, type: 'application/geo+json', body: JSON.stringify({ type: 'FeatureCollection', features } satisfies FeatureCollection) }
  }
  if (source.kind === 'features') {
    // the features endpoint itself, whatever the layer is drawn from (tiles, points, a file): paged, inside the area
    const features: Feature[] = []
    let cursor: number | null = null
    do {
      const qs = new URLSearchParams({ format: 'geojson', limit: '2000' })
      if (area) qs.set('within', area.pcode)
      if (source.featureKind) qs.set('kind', source.featureKind)
      if (cursor !== null) qs.set('cursor', String(cursor))
      const page = (await (await getBytes(`/v2/features/${encodeURIComponent(source.dataset)}?${qs}`)).json()) as FeatureCollection & { next_cursor?: number | null }
      features.push(...page.features)
      cursor = page.next_cursor ?? null
      onProgress?.(features.length, r.count ?? features.length)
    } while (cursor !== null && features.length < 200_000)
    return { name: `${base}.geojson`, type: 'application/geo+json', body: JSON.stringify({ type: 'FeatureCollection', features } satisfies FeatureCollection) }
  }
  throw new Error(`${ex.layer.name} cannot be exported as data`)
}

async function unitNames(level: number, parent?: string): Promise<Map<string, string>> {
  try {
    return new Map((await units(level, parent)).map((u) => [u.pcode, u.name_en]))
  } catch {
    return new Map()
  }
}

export const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '') || 'map'

/* ---------------- delivery ---------------- */
export function download(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 2000)
}

/* A stored (uncompressed) zip: several files in one download, no dependency. PNG is already compressed;
   GeoJSON and CSV would shrink, but a stored archive opens everywhere and is a few dozen lines. */
const CRC = (() => {
  const t = new Uint32Array(256)
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    t[n] = c >>> 0
  }
  return t
})()
function crc32(b: Uint8Array) {
  let c = 0xffffffff
  for (let i = 0; i < b.length; i++) c = CRC[(c ^ b[i]) & 0xff] ^ (c >>> 8)
  return (c ^ 0xffffffff) >>> 0
}
export function zip(files: ExportFile[]): Blob {
  const enc = new TextEncoder()
  const parts: Uint8Array[] = []
  const central: Uint8Array[] = []
  let offset = 0
  const now = new Date()
  const time = (now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1)
  const date = ((now.getFullYear() - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate()
  for (const f of files) {
    const name = enc.encode(f.name)
    const data = typeof f.body === 'string' ? enc.encode(f.body) : f.body
    const crc = crc32(data)
    const local = new DataView(new ArrayBuffer(30))
    local.setUint32(0, 0x04034b50, true)
    local.setUint16(4, 20, true)
    local.setUint16(6, 0x0800, true) // UTF-8 names
    local.setUint16(10, time, true)
    local.setUint16(12, date, true)
    local.setUint32(14, crc, true)
    local.setUint32(18, data.length, true)
    local.setUint32(22, data.length, true)
    local.setUint16(26, name.length, true)
    parts.push(new Uint8Array(local.buffer), name, data)
    const cen = new DataView(new ArrayBuffer(46))
    cen.setUint32(0, 0x02014b50, true)
    cen.setUint16(4, 20, true)
    cen.setUint16(6, 20, true)
    cen.setUint16(8, 0x0800, true)
    cen.setUint16(12, time, true)
    cen.setUint16(14, date, true)
    cen.setUint32(16, crc, true)
    cen.setUint32(20, data.length, true)
    cen.setUint32(24, data.length, true)
    cen.setUint16(28, name.length, true)
    cen.setUint32(42, offset, true)
    central.push(new Uint8Array(cen.buffer), name)
    offset += 30 + name.length + data.length
  }
  const size = central.reduce((s, p) => s + p.length, 0)
  const end = new DataView(new ArrayBuffer(22))
  end.setUint32(0, 0x06054b50, true)
  end.setUint16(8, files.length, true)
  end.setUint16(10, files.length, true)
  end.setUint32(12, size, true)
  end.setUint32(16, offset, true)
  return new Blob([...parts, ...central, new Uint8Array(end.buffer)] as BlobPart[], { type: 'application/zip' })
}
