import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import type { StyleSpecification } from 'maplibre-gl'
import { createContext, useContext, useEffect, useMemo, useRef, type RefObject } from 'react'
import { layerDataQuery, reliefQuery, resolveQuery, type LayerData, type Resolved } from '@/api/workspace'
import type { Theme } from '@/lib/hooks'
import { allLayers, isGroup, type LayerNode, type MapDocument } from '@/lib/map-spec'
import type { Grid } from '@/mocks/synth'
import type { MapEngine } from './engine/MapEngine'

export type WorkspaceCtx = {
  engine: RefObject<MapEngine | null>
  /** what each layer's source resolved to; undefined while loading or while the layer is hidden */
  data: Map<string, LayerData | undefined>
  /** the colour scheme of the map (its basemap), which data colours follow; not necessarily the app theme */
  scheme: Theme
  /** bumps whenever the engine recompiled a layer: legends and problems may have changed */
  tick: number
  desktop: boolean
}
export const WorkspaceContext = createContext<WorkspaceCtx | null>(null)
export function useWorkspace(): WorkspaceCtx {
  const ctx = useContext(WorkspaceContext)
  if (!ctx) throw new Error('useWorkspace outside the workspace')
  return ctx
}

const BASEMAP: Record<Theme, string> = { dark: 'https://tiles.openfreemap.org/styles/dark', light: 'https://tiles.openfreemap.org/styles/positron' }
/** Basemap style as data: resolves to null when unreachable, so the map falls back to a blank canvas. */
const basemapQuery = (scheme: Theme) => ({
  queryKey: ['basemap', scheme],
  queryFn: async (): Promise<StyleSpecification | null> => {
    try {
      const res = await fetch(BASEMAP[scheme], { signal: AbortSignal.timeout(8000) })
      return res.ok ? ((await res.json()) as StyleSpecification) : null
    } catch {
      return null
    }
  },
  staleTime: Infinity,
  gcTime: Infinity,
  retry: false,
})
export const schemeOf = (doc: MapDocument, theme: Theme): Theme => (doc.basemap.style === 'ldl-dark' ? 'dark' : doc.basemap.style === 'ldl-light' ? 'light' : theme)
/** undefined while loading; null when there is to be none, or none could be fetched. */
export function useBasemap(doc: MapDocument, theme: Theme): { scheme: Theme; style: StyleSpecification | null | undefined } {
  const scheme = schemeOf(doc, theme)
  const none = doc.basemap.style === 'none'
  const client = useQueryClient()
  const { data } = useQuery({ ...basemapQuery(scheme), enabled: !none })
  useEffect(() => {
    void client.prefetchQuery(basemapQuery(scheme === 'dark' ? 'light' : 'dark'))
  }, [client, scheme])
  return { scheme, style: none ? null : data }
}

/** Ids of the layers that are on screen: visible themselves and through every group above them. */
export function shownLayers(nodes: LayerNode[], solo: string | null): Set<string> {
  const out = new Set<string>()
  const has = (n: LayerNode, id: string): boolean => n.id === id || (isGroup(n) && n.children.some((c) => has(c, id)))
  const visit = (list: LayerNode[], inSolo: boolean) => {
    for (const n of list) {
      if (!n.visible) continue
      const mine = inSolo || !solo || n.id === solo
      if (isGroup(n)) {
        if (mine || has(n, solo!)) visit(n.children, mine)
      } else if (mine) out.add(n.id)
    }
  }
  visit(nodes, false)
  return out
}

/**
 * Data for the layers that need it: those on screen, and the selected one (its inspector draws a
 * histogram). A hidden layer issues no request; its data is fetched on first reveal.
 */
/** The document's sources as the engine resolved them; `fresh` is false while a newer resolution is on its way. */
export function useResolved(doc: Pick<MapDocument, 'sources' | 'pin'>): { resolved: Resolved | null | undefined; fresh: boolean } {
  const q = useQuery(resolveQuery(doc.sources, doc.pin))
  return { resolved: q.isError ? null : q.data, fresh: !q.isPlaceholderData && !q.isFetching }
}
/** A source's entry: undefined = not known yet (wait), null = not served (sample data). */
export function resolvedFor(id: string, r: { resolved: Resolved | null | undefined; fresh: boolean }) {
  if (r.resolved === undefined) return undefined
  if (r.resolved === null) return null
  const hit = r.resolved.sources[id]
  if (hit) return hit
  return r.fresh ? null : undefined
}

export function useLayerData(doc: MapDocument, selected: string | null, solo: string | null) {
  const layers = useMemo(() => allLayers(doc.layers), [doc.layers])
  const shown = useMemo(() => shownLayers(doc.layers, solo), [doc.layers, solo])
  const resolution = useResolved(doc)
  // layers that share a source share one request; the cursor only changes the request of a grid (frames carry every period)
  const plan = useMemo(() => {
    const unique: { key: string; options: ReturnType<typeof layerDataQuery>; enabled: boolean }[] = []
    const at = layers.map((l) => {
      const options = layerDataQuery(doc.sources[l.source], doc, doc.time, resolvedFor(l.source, resolution))
      const key = JSON.stringify(options.queryKey)
      const need = !!doc.sources[l.source] && (shown.has(l.id) || l.id === selected)
      let i = unique.findIndex((u) => u.key === key)
      const can = options.enabled !== false
      if (i < 0) i = unique.push({ key, options, enabled: need && can }) - 1
      else if (need && can) unique[i].enabled = true
      return i
    })
    return { unique, at }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layers, doc.sources, doc.analyses, doc.time, shown, selected, resolution.resolved, resolution.fresh])
  const results = useQueries({ queries: plan.unique.map((u) => ({ ...u.options, enabled: u.enabled })) })
  const last = useRef<{ ids: string[]; values: (LayerData | undefined)[]; map: Map<string, LayerData | undefined> }>({ ids: [], values: [], map: new Map() })
  const ids = layers.map((l) => l.id)
  const values = plan.at.map((i) => results[i]?.data)
  if (ids.length !== last.current.ids.length || ids.some((id, i) => id !== last.current.ids[i]) || values.some((v, i) => v !== last.current.values[i]))
    last.current = { ids, values, map: new Map(ids.map((id, i) => [id, values[i]])) }
  const loading = resolution.resolved === undefined || layers.some((l, i) => shown.has(l.id) && results[plan.at[i]]?.isFetching)

  // the terrain model's shading, once per exaggeration in use
  const reliefs = useMemo(() => {
    const z = new Set<number>()
    for (const [i, l] of layers.entries()) {
      // only the sample terrain needs this; the engine's relief is a raster-dem drawn by MapLibre itself
      if (!shown.has(l.id) || results[plan.at[i]]?.data?.origin === 'api') continue
      if (l.type === 'hillshade' && results[plan.at[i]]?.data) z.add(l.style.exaggeration ?? 1)
      if (l.type === 'raster' && l.style['x-relief'] && results[plan.at[i]]?.data) z.add(1)
    }
    return [...z].sort()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layers, shown, values])
  const reliefResults = useQueries({ queries: reliefs.map((z) => reliefQuery(z)) })
  const reliefRef = useRef<{ key: string; map: Map<number, Grid> }>({ key: '', map: new Map() })
  const key = reliefs.map((z, i) => `${z}:${reliefResults[i].data ? 1 : 0}`).join()
  if (key !== reliefRef.current.key) reliefRef.current = { key, map: new Map(reliefs.flatMap((z, i) => (reliefResults[i].data ? [[z, reliefResults[i].data!] as [number, Grid]] : []))) }
  return { data: last.current.map, relief: reliefRef.current.map, loading, shown, resolved: resolution.resolved }
}

/** A function that runs at most once per frame with the latest arguments: for slider drags that drive the map. */
export function useRafCallback<A extends unknown[]>(fn: (...args: A) => void): ((...args: A) => void) & { cancel: () => void } {
  const latest = useRef(fn)
  latest.current = fn
  const pending = useRef<{ raf: number; args: A | null }>({ raf: 0, args: null })
  useEffect(() => () => cancelAnimationFrame(pending.current.raf), [])
  return useMemo(() => {
    const run = (...args: A) => {
      pending.current.args = args
      if (pending.current.raf) return
      pending.current.raf = requestAnimationFrame(() => {
        pending.current.raf = 0
        if (pending.current.args) latest.current(...pending.current.args)
      })
    }
    run.cancel = () => {
      cancelAnimationFrame(pending.current.raf)
      pending.current.raf = 0
      pending.current.args = null
    }
    return run
  }, [])
}
