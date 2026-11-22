// Unit names by p-code, for the places a frame's codes are listed outside the map (the scatter, tables):
// `GET /v2/units?level=` pages through every unit of a level (up to 20,000 a page, so one request even
// for the 14,043 GN divisions), cached for the session and revalidated by its ETag.
import { getJson } from '@/api/http'

export type Unit = { pcode: string; level: number; parent: string | null; name_en: string; name_si: string | null; name_ta: string | null; centroid: [number, number]; area_km2: number | null }
type Page = { total: number; units: Unit[]; next_cursor: string | null }
const PAGE = 20_000
const byLevel = new Map<string, Promise<Unit[]>>()

/** Every unit of a level, optionally within a parent; `centroid` is `[lon, lat]`. */
export function units(level: number, parent?: string): Promise<Unit[]> {
  const key = `${level}|${parent ?? ''}`
  let p = byLevel.get(key)
  if (!p) {
    p = (async () => {
      const out: Unit[] = []
      let cursor: string | null = null
      do {
        const page: Page = await getJson<Page>('/v2/units', { level, parent, cursor, limit: PAGE })
        out.push(...page.units)
        cursor = page.next_cursor && page.units.length ? page.next_cursor : null
      } while (cursor)
      return out
    })()
    p.catch(() => byLevel.delete(key))
    byLevel.set(key, p)
  }
  return p
}
/** Names of every unit at a level. */
export const areaNames = async (level: number): Promise<Map<string, string>> => new Map((await units(level)).map((u) => [u.pcode, u.name_en]))
