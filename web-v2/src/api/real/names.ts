// Unit names by p-code, for the places a frame's codes are listed outside the map (the scatter, tables).
// There is no bulk listing of units at a level, so the tree is walked from the country down through
// `GET /v2/areas/{pcode}`: 1 + 9 + 25 requests reach every DS division, cached for the session.
import { getJson } from '@/api/http'

type Area = { pcode: string; level: number; name_en: string; children: { pcode: string; level: number; name_en: string }[] }
const byLevel = new Map<number, Promise<Map<string, string>>>()
const area = (pcode: string) => getJson<Area>(`/v2/areas/${pcode}`)

async function walk(level: number): Promise<Map<string, string>> {
  const out = new Map<string, string>()
  let frontier = ['LK']
  for (let l = 0; l < level; l++) {
    const areas = await Promise.all(frontier.map((p) => area(p).catch(() => null)))
    frontier = []
    for (const a of areas) for (const c of a?.children ?? []) if (c.level === l + 1) (frontier.push(c.pcode), l + 1 === level && out.set(c.pcode, c.name_en))
  }
  return out
}
/** Names of every unit at a level (ADM1–ADM3; GN divisions are named by the boundary tiles on the map). */
export function areaNames(level: number): Promise<Map<string, string>> {
  if (level > 3) return Promise.resolve(new Map())
  let p = byLevel.get(level)
  if (!p) byLevel.set(level, (p = walk(level)))
  return p
}
