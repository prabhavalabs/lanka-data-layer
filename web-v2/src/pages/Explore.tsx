// /explore: opens a map document and hands it to the workspace.
//
// Which document: `#d=…` (a whole map in the link), `?map=<id>` (a starter or one of the visitor's own),
// the old `?indicator=&level=&area=` links from area pages (the single-indicator map), or the default
// showcase. What is not part of the document rides in the query string: the camera (`v`), the time
// cursor (`t`) and the selected area (`area`), so a link can point into a map without forking it.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { DEFAULT_MAP, glance, isStarter, mapQuery, saveMap } from '@/api/workspace'
import type { DocActions } from '@/features/workspace/DocBar'
import { exportJson, shareLink } from '@/features/workspace/exporting'
import { actions, getWs, useDoc, useWs } from '@/features/workspace/store'
import { Workspace } from '@/features/workspace/Workspace'
import { useTheme } from '@/lib/hooks'
import { blankDocument, decodeDocument, type MapDocument } from '@/lib/map-spec'

type Origin = DocActions['origin']
type Opened = { doc: MapDocument; origin: Origin }
// the draft the contract names: the document being edited, so a reload does not lose work in progress
const DRAFT = 'ldl:draft'
const readDraft = (key: string): MapDocument | null => {
  try {
    const d = JSON.parse(localStorage.getItem(DRAFT) ?? 'null') as { key: string; doc: MapDocument } | null
    return d?.key === key ? d.doc : null
  } catch {
    return null
  }
}

export default function Explore() {
  const [theme] = useTheme()
  const [params] = useSearchParams()
  const { hash } = useLocation()
  const navigate = useNavigate()
  const client = useQueryClient()
  const [imported, setImported] = useState<{ doc: MapDocument; n: number } | null>(null)

  /* ---- which document ---- */
  const inline = hash.startsWith('#d=') ? hash.slice(3) : null
  const mapParam = params.get('map')
  const legacy = params.get('indicator')
  const area = params.get('area')?.toUpperCase() ?? null
  const level = params.get('level') === '1' || area?.length === 3 ? 1 : 2
  const id = mapParam ?? (legacy || area ? 'm_glance' : DEFAULT_MAP)
  // what identifies the document, whatever theme it is shown in
  const key = imported ? `import:${imported.n}` : inline ? `link:${inline.slice(0, 48)}` : mapParam === 'new' ? 'new' : `${id}:${legacy ?? ''}:${id === 'm_glance' ? level : ''}`
  const stored = useQuery({ ...mapQuery(id, theme), enabled: !imported && !inline && mapParam !== 'new' && !(id === 'm_glance' && !!legacy) })
  const linked = useQuery({ queryKey: ['map-link', inline], queryFn: () => decodeDocument(inline!), enabled: !!inline && !imported, staleTime: Infinity, retry: false })
  const opened = useMemo<Opened | null>(() => {
    if (imported) return { doc: imported.doc, origin: 'new' }
    if (inline) return linked.data ? { doc: linked.data, origin: 'link' } : null
    if (mapParam === 'new') return { doc: blankDocument(), origin: 'new' }
    if (id === 'm_glance' && legacy) return { doc: glance(legacy, level, theme), origin: 'starter' }
    return stored.data ? { doc: stored.data, origin: isStarter(id) ? 'starter' : 'own' } : null
  }, [imported, inline, linked.data, mapParam, id, legacy, level, stored.data, theme])

  /* ---- into the store ---- */
  const loaded = useRef('')
  const skip = useRef('')
  const [docKey, setDocKey] = useState('')
  const [origin, setOrigin] = useState<Origin>('starter')
  const open = useCallback(
    (o: Opened, k: string, fresh: boolean) => {
      const t = params.get('t')
      const base = o.doc.time && t && t >= o.doc.time.range[0] && t <= o.doc.time.range[1] ? { ...o.doc, time: { ...o.doc.time, cursor: t } } : o.doc
      const draft = fresh ? readDraft(k) : null
      actions.load(draft ?? base, o.origin === 'link' || o.origin === 'new' ? null : base)
      loaded.current = k
      setOrigin(o.origin)
      if (fresh) setDocKey(k)
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  )
  useEffect(() => {
    if (!opened) return
    if (skip.current === key) {
      // the document just saved under this id is already the one in the store
      skip.current = ''
      loaded.current = key
      return setOrigin(opened.origin)
    }
    const same = loaded.current === key
    // a theme change re-issues a starter in the other scheme; edits in progress are kept instead
    if (same && getWs().doc !== getWs().saved) return
    open(opened, key, !same)
  }, [opened, key, open])
  const doc = useDoc()
  const dirty = useWs((s) => s.doc !== s.saved)
  useEffect(() => {
    const t = window.setTimeout(() => {
      try {
        if (dirty && doc && loaded.current) localStorage.setItem(DRAFT, JSON.stringify({ key: loaded.current, doc }))
        else localStorage.removeItem(DRAFT)
      } catch {
        /* private mode */
      }
    }, 500)
    return () => window.clearTimeout(t)
  }, [doc, dirty])

  /* ---- what rides in the URL beside the document ---- */
  const view = useRef<{ center: [number, number]; zoom: number } | null>(null)
  // through `navigate`, not `setSearchParams`: a map that lives in the fragment (`#d=`) must keep it
  const here = useRef({ search: '', hash: '' })
  here.current = { search: params.toString(), hash }
  const patch = useCallback(
    (p: Record<string, string | null>) => {
      const next = new URLSearchParams(here.current.search)
      for (const [k, v] of Object.entries(p)) {
        if (v === null) next.delete(k)
        else next.set(k, v)
      }
      if (next.toString() === here.current.search) return
      here.current.search = next.toString()
      navigate({ search: next.toString(), hash: here.current.hash }, { replace: true, preventScrollReset: true })
    },
    [navigate],
  )
  const onView = useCallback(
    (v: { center: [number, number]; zoom: number }) => {
      view.current = v
      patch({ v: `${v.center[0]},${v.center[1]},${v.zoom}` })
    },
    [patch],
  )
  const cursor = doc?.time?.cursor ?? null
  useEffect(() => {
    if (loaded.current) patch({ t: cursor })
  }, [cursor, patch])
  const onArea = useCallback((pcode: string | null) => patch({ area: pcode }), [patch])
  const [initialView] = useState(() => {
    const v = params.get('v')?.split(',').map(Number)
    return v && v.length === 3 && v.every(Number.isFinite) ? { center: [v[0], v[1]] as [number, number], zoom: v[2] } : null
  })

  /* ---- document actions ---- */
  const withView = (d: MapDocument): MapDocument => (view.current ? { ...d, view: { center: view.current.center, zoom: view.current.zoom } } : d)
  const save = useMutation({
    mutationFn: (d: MapDocument) => saveMap(d),
    onSuccess: (saved) => {
      actions.markSaved(saved)
      void client.invalidateQueries({ queryKey: ['maps'] })
      setImported(null)
      if (saved.id !== mapParam) {
        // the map now lives under its own id: point the address at it without reloading it
        skip.current = `${saved.id}::`
        navigate(`/explore?map=${saved.id}`, { replace: true })
      }
    },
  })
  const docActions = useMemo<Omit<DocActions, 'exportPng'>>(
    () => ({
      origin,
      saving: save.isPending,
      save: () => {
        const d = getWs().doc
        if (d && !save.isPending) save.mutate(withView(d))
      },
      revert: () => {
        try {
          localStorage.removeItem(DRAFT)
        } catch {
          /* ignore */
        }
        if (opened) open(opened, loaded.current, false)
      },
      open: (next) => {
        setImported(null)
        loaded.current = ''
        navigate(next ? `/explore?map=${next}` : '/explore')
      },
      openDocument: (d) => {
        loaded.current = ''
        setImported((cur) => ({ doc: { ...d, id: undefined }, n: (cur?.n ?? 0) + 1 }))
      },
      blank: () => {
        setImported(null)
        loaded.current = ''
        navigate('/explore?map=new')
      },
      share: async () => {
        const d = getWs().doc
        if (d) await navigator.clipboard?.writeText(await shareLink(withView(d))).catch(() => {})
      },
      exportJson: () => {
        const d = getWs().doc
        if (d) exportJson(withView(d))
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [origin, save.isPending, opened, navigate, open],
  )

  if (!doc || !docKey) {
    const failed = (inline && linked.isError) || (!inline && !imported && stored.isFetched && !stored.data && mapParam !== 'new')
    return (
      <div className="grid h-[100dvh] place-items-center bg-bg px-6">
        {failed ? (
          <div className="max-w-sm text-center">
            <h1 className="t-h3">This map could not be opened</h1>
            <p className="mt-2 text-[14px] leading-[1.55] text-ink2">{inline ? 'The link is incomplete, or it was made by a newer version of the map format.' : 'It is not among the starters or the maps saved in this browser.'}</p>
            <button type="button" onClick={() => navigate('/explore')} className="pressable mt-5 h-9 rounded-lg bg-ink px-4 text-[13.5px] font-medium text-bg">
              Open the default map
            </button>
          </div>
        ) : (
          <div className="skeleton h-1 w-40" role="status" aria-label="Opening the map" />
        )}
      </div>
    )
  }
  return <Workspace doc={doc} docActions={docActions} initialView={initialView ?? doc.view} docKey={docKey} onView={onView} area={area} onArea={onArea} />
}
