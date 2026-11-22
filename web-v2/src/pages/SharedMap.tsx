// `/m/:id` and `/embed/m/:id`: a shared snapshot, read back from the app plane with its resolution.
//
// The resolution that comes with it (attribution, notices) seeds the workspace's own resolve query, so
// the map draws from exactly what the server resolved and asks for nothing twice.
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { resolveKey } from '@/api/real/layers'
import { fetchShared } from '@/api/real/share'
import { MapCanvas } from '@/features/workspace/MapCanvas'
import { LegendCard } from '@/features/workspace/LegendCard'
import { useBasemap, useLayerData, WorkspaceContext, type WorkspaceCtx } from '@/features/workspace/context'
import type { MapEngine } from '@/features/workspace/engine/MapEngine'
import { actions, useWs } from '@/features/workspace/store'
import '@/features/workspace/workspace.css'
import { useMediaQuery, useReducedMotion, useTheme } from '@/lib/hooks'
import type { MapDocument } from '@/lib/map-spec'
import Explore from './Explore'

function useShared(id: string | undefined) {
  const client = useQueryClient()
  return useQuery({
    queryKey: ['shared', id],
    queryFn: async () => {
      const out = await fetchShared(id!)
      client.setQueryData(['resolve', resolveKey(out.doc.sources, out.doc.pin)], out.resolved)
      return out
    },
    enabled: !!id,
    staleTime: Infinity,
    retry: false,
  })
}

function Missing({ embed }: { embed?: boolean }) {
  return (
    <div className="grid h-[100dvh] place-items-center bg-bg px-6">
      <div className="max-w-sm text-center">
        <h1 className="t-h3">This shared map could not be opened</h1>
        <p className="mt-2 text-[14px] leading-[1.55] text-ink2">The link may be mistyped, or the map was withdrawn.</p>
        {!embed && (
          <Link to="/explore" className="pressable mt-5 inline-block h-9 rounded-lg bg-ink px-4 py-2 text-[13.5px] font-medium text-bg">
            Open the default map
          </Link>
        )}
      </div>
    </div>
  )
}

/** `/m/:id`: the snapshot in the full workspace. Edits stay in this browser; saving makes a copy. */
export default function SharedMap() {
  const { id } = useParams()
  const q = useShared(id)
  if (q.isError) return <Missing />
  if (!q.data) return <div className="grid h-[100dvh] place-items-center bg-bg"><div className="skeleton h-1 w-40" role="status" aria-label="Opening the shared map" /></div>
  return <Explore shared={q.data.doc} />
}

/** `/embed/m/:id`: the map alone, read-only, with its legend, attribution and notices. */
export function EmbedMap() {
  const { id } = useParams()
  const q = useShared(id)
  if (q.isError) return <Missing embed />
  if (!q.data) return <div className="h-[100dvh] bg-bg" role="status" aria-label="Opening the map" />
  return <Embedded doc={q.data.doc} />
}

function Embedded({ doc }: { doc: MapDocument }) {
  const [theme] = useTheme()
  const desktop = useMediaQuery('(min-width: 768px)')
  const reducedMotion = useReducedMotion()
  const engine = useRef<MapEngine | null>(null)
  const tooltip = useRef<HTMLDivElement | null>(null)
  const [tick, setTick] = useState(0)
  // the legend reads the document through the store
  useEffect(() => {
    actions.load(doc, doc)
  }, [doc])
  const loaded = useWs((s) => s.doc === doc)
  const { scheme, style } = useBasemap(doc, theme)
  const { data, relief, resolved } = useLayerData(doc, null, null)
  const ctx = useMemo<WorkspaceCtx>(() => ({ engine, data, scheme, tick, desktop }), [data, scheme, tick, desktop])
  useEffect(() => {
    const esc = (t: string) => t.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!)
    engine.current?.setAttribution([...(resolved?.notices ?? []).map((n) => `<strong>${esc(n.code)}:</strong> ${esc(n.text)}`), ...[...new Set((resolved?.attribution ?? []).map((a) => a.text))].map(esc), `<a href="/m/${esc(doc.id ?? '')}" target="_blank" rel="noopener">Lanka Data Layer</a>`])
  }, [resolved, tick, doc.id])
  const padding = useMemo(() => ({ top: 16, bottom: 16, left: 16, right: 16 }), [])
  return (
    <WorkspaceContext.Provider value={ctx}>
      <div className="relative h-[100dvh] w-full overflow-hidden bg-bg">
        <h1 className="sr-only">{doc.title}</h1>
        <MapCanvas doc={doc} data={data} relief={relief} scheme={scheme} basemap={style} solo={null} padding={padding} reducedMotion={reducedMotion} engine={engine} tooltipRef={tooltip} onHover={() => {}} onClick={() => {}} onView={() => {}} onCompiled={() => setTick((t) => t + 1)} initialView={doc.view} />
        <div className="glass absolute top-3 left-3 z-20 max-w-[min(360px,calc(100%-24px))] truncate rounded-xl px-3 py-2 text-[13px] font-semibold tracking-[-0.01em] text-ink">{doc.title}</div>
        {loaded && <LegendCard className="ws-float absolute bottom-3 left-3 z-20" />}
      </div>
    </WorkspaceContext.Provider>
  )
}
