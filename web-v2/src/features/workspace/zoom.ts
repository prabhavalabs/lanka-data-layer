// The map's zoom, for the parts of the workspace that say when a layer is out of reach. It lives outside
// the document store: moving the camera is not an edit, and must not touch the undo history.
import { useSyncExternalStore } from 'react'
import type { LayerData } from '@/api/workspace'
import type { LayerNode } from '@/lib/map-spec'

let zoom = 7
const listeners = new Set<() => void>()
export function setMapZoom(z: number) {
  if (z === zoom) return
  zoom = z
  listeners.forEach((l) => l())
}
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => void listeners.delete(l)
}
export const useMapZoom = () => useSyncExternalStore(subscribe, () => zoom)

/**
 * The zoom a layer starts drawing at: its own zoom range, and the zoom its tiles start at (the admin
 * tileset carries DS divisions from z5 and GN divisions from z9; some feature tilesets from z9 too).
 */
export function drawnFrom(node: LayerNode, data: LayerData | undefined): number {
  const own = node.type !== 'group' ? (node.zoom?.[0] ?? 0) : 0
  const t = data && 'tiles' in data ? data.tiles : undefined
  const tiles = t && !Array.isArray(t) && 'minzoom' in t ? (t.minzoom ?? 0) : 0
  return Math.max(own, tiles)
}
/** How the zoom is said to a person: one decimal, no trailing zero. */
export const zoomLabel = (z: number) => String(Math.round(z * 10) / 10)
