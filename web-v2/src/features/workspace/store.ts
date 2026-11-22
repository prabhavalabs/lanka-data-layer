// Workspace state: the document being edited, its undo history, and what is selected.
//
// A tiny external store (no dependency): components subscribe to exactly the slice they render, so
// dragging one layer's slider does not re-render the other thirty rows. Every edit is an immutable
// transform of the document; unchanged subtrees keep their identity, which is what the map engine
// and the selectors compare.
import { useSyncExternalStore } from 'react'
import {
  ancestorsOf,
  cloneNode,
  containsNode,
  countGroups,
  findNode,
  groupHeight,
  insertNode,
  instantiate,
  isGroup,
  LIMITS,
  MAX_GROUP_DEPTH,
  parentOf,
  pruneSources,
  removeNode,
  uid,
  updateNode,
  allLayers,
  type AnalysisRequest,
  type BlendMode,
  type Cql,
  type Group,
  type Layer,
  type LayerNode,
  type LayerTemplate,
  type MapDocument,
  type Source,
  type Style,
} from '@/lib/map-spec'

export type WorkspaceState = {
  doc: MapDocument | null
  /** the document as loaded or last saved; the map is dirty when `doc !== saved` */
  saved: MapDocument | null
  past: { doc: MapDocument; key?: string; at: number }[]
  future: MapDocument[]
  selected: string | null
  solo: string | null
  /** what the last edit was, for the status line and for screen readers */
  said: string
}
let state: WorkspaceState = { doc: null, saved: null, past: [], future: [], selected: null, solo: null, said: '' }
const listeners = new Set<() => void>()
const set = (patch: Partial<WorkspaceState>) => {
  state = { ...state, ...patch }
  listeners.forEach((l) => l())
}
export const getWs = () => state
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => void listeners.delete(l)
}
export function useWs<T>(selector: (s: WorkspaceState) => T): T {
  return useSyncExternalStore(subscribe, () => selector(state), () => selector(state))
}
export const useDoc = () => useWs((s) => s.doc)
export const useNode = (id: string | null) => useWs((s) => (s.doc && id ? findNode(s.doc.layers, id) : null))

const HISTORY = 100
/** Apply an edit. Edits sharing a `key` within a second and a half fold into one undo step (a slider drag, typing a name). */
function edit(fn: (doc: MapDocument) => MapDocument, said: string, key?: string) {
  const cur = state.doc
  if (!cur) return
  const next = fn(cur)
  if (next === cur) return
  const now = Date.now()
  const last = state.past[state.past.length - 1]
  const fold = key && last?.key === key && now - last.at < 1500
  set({ doc: next, past: fold ? [...state.past.slice(0, -1), { ...last, at: now }] : [...state.past.slice(-HISTORY + 1), { doc: cur, key, at: now }], future: [], said })
}
const layersOf = (doc: MapDocument, layers: LayerNode[]): MapDocument => (layers === doc.layers ? doc : { ...doc, layers })
const patchNode = (id: string, patch: Partial<Layer> & Partial<Group>, said: string, key?: string) => edit((d) => layersOf(d, updateNode(d.layers, id, (n) => ({ ...n, ...patch }) as LayerNode)), said, key)
const layerAt = (doc: MapDocument, id: string): Layer | null => {
  const n = findNode(doc.layers, id)
  return n && !isGroup(n) ? n : null
}

export const actions = {
  /** Open a document. `saved` is what it is compared against for the dirty flag (omit for an unsaved one). */
  load(doc: MapDocument, saved: MapDocument | null = doc) {
    const top = [...doc.layers].reverse().find((n) => !isGroup(n) && n.visible)
    set({ doc, saved, past: [], future: [], selected: state.selected && findNode(doc.layers, state.selected) ? state.selected : (top?.id ?? null), solo: null, said: '' })
  },
  markSaved(doc: MapDocument) {
    set({ doc, saved: doc })
  },
  undo() {
    const last = state.past[state.past.length - 1]
    if (!last || !state.doc) return
    set({ doc: last.doc, past: state.past.slice(0, -1), future: [state.doc, ...state.future], selected: state.selected && findNode(last.doc.layers, state.selected) ? state.selected : null, said: 'Undone' })
  },
  redo() {
    const [next, ...rest] = state.future
    if (!next || !state.doc) return
    set({ doc: next, past: [...state.past, { doc: state.doc, at: Date.now() }], future: rest, said: 'Redone' })
  },
  select(id: string | null) {
    if (id !== state.selected) set({ selected: id })
  },
  solo(id: string | null) {
    set({ solo: id === state.solo ? null : id })
  },
  say(said: string) {
    set({ said })
  },

  /* ---- nodes ---- */
  setVisible: (id: string, visible: boolean) => patchNode(id, { visible }, visible ? 'Layer shown' : 'Layer hidden'),
  setOpacity: (id: string, opacity: number) => patchNode(id, { opacity: Math.round(opacity * 100) / 100 }, `Opacity ${Math.round(opacity * 100)}%`, `opacity:${id}`),
  setBlend: (id: string, blend: BlendMode) => patchNode(id, { blend }, `Blend mode ${blend}`),
  rename: (id: string, name: string) => name.trim() && patchNode(id, { name: name.trim() }, 'Renamed', `name:${id}`),
  toggleLock: (id: string) => {
    const n = state.doc && findNode(state.doc.layers, id)
    if (n) patchNode(id, { locked: !n.locked }, n.locked ? 'Unlocked' : 'Locked')
  },
  toggleCollapsed: (id: string) =>
    edit((d) => layersOf(d, updateNode(d.layers, id, (n) => (isGroup(n) ? { ...n, collapsed: !n.collapsed } : n))), ''),
  setZoom: (id: string, zoom: [number, number] | undefined) => patchNode(id, { zoom }, 'Zoom range changed', `zoom:${id}`),
  setLegend: (id: string, legend: Layer['legend']) => patchNode(id, { legend }, 'Legend changed', `legend:${id}`),
  setFilter: (id: string, filter: Cql | null) => patchNode(id, { filter }, 'Filter changed', `filter:${id}`),
  /** Replace or patch a layer's style. `key` folds a drag into one undo step. */
  setStyle: (id: string, style: Style | ((s: Style) => Style), key?: string) =>
    edit((d) => layersOf(d, updateNode(d.layers, id, (n) => (isGroup(n) ? n : { ...n, style: typeof style === 'function' ? style(n.style) : style }))), 'Style changed', key ? `style:${id}:${key}` : undefined),
  setType: (id: string, type: Layer['type'], style?: Style) => edit((d) => layersOf(d, updateNode(d.layers, id, (n) => (isGroup(n) ? n : { ...n, type, style: style ?? n.style }))), 'Layer type changed'),
  /** Change a layer's source. A source shared with another layer is forked first, so the other layer is untouched. */
  setSource(id: string, fn: (s: Source) => Source, said = 'Source changed') {
    edit((d) => {
      const layer = layerAt(d, id)
      if (!layer) return d
      const cur = d.sources[layer.source]
      if (!cur) return d
      const next = fn(cur)
      if (next === cur) return d
      const shared = allLayers(d.layers).some((l) => l.id !== id && l.source === layer.source)
      if (!shared) return { ...d, sources: { ...d.sources, [layer.source]: next } }
      const sid = uid('s')
      return { ...d, sources: { ...d.sources, [sid]: next }, layers: updateNode(d.layers, id, (n) => ({ ...n, source: sid }) as LayerNode) }
    }, said)
  },
  remove(id: string) {
    edit((d) => {
      const [layers, removed] = removeNode(d.layers, id)
      return removed ? pruneSources({ ...d, layers }) : d
    }, 'Layer removed')
    if (state.selected && state.doc && !findNode(state.doc.layers, state.selected)) set({ selected: null })
    if (state.solo && state.doc && !findNode(state.doc.layers, state.solo)) set({ solo: null })
  },
  duplicate(id: string) {
    let made: string | null = null
    edit((d) => {
      const node = findNode(d.layers, id)
      if (!node || allLayers(d.layers).length + allLayers([node]).length > LIMITS.layers) return d
      const sources = { ...d.sources }
      const copy = cloneNode(node, (sid) => {
        const next = uid('s')
        if (sources[sid]) sources[next] = structuredClone(sources[sid])
        return next
      })
      made = copy.id
      const parent = parentOf(d.layers, id) ?? null
      const siblings = parent ? parent.children : d.layers
      return { ...d, sources, layers: insertNode(d.layers, parent?.id ?? null, siblings.findIndex((n) => n.id === id) + 1, copy) }
    }, 'Layer duplicated')
    if (made) set({ selected: made })
  },
  /** Wrap a node in a new group, where it stands. */
  group(id: string) {
    let made: string | null = null
    edit((d) => {
      const node = findNode(d.layers, id)
      if (!node || countGroups(d.layers) >= LIMITS.groups) return d
      if (ancestorsOf(d.layers, id).length + 1 + groupHeight(node) > MAX_GROUP_DEPTH) return d
      const parent = parentOf(d.layers, id) ?? null
      const siblings = parent ? parent.children : d.layers
      const at = siblings.findIndex((n) => n.id === id)
      const [layers] = removeNode(d.layers, id)
      const g: Group = { id: uid('g'), type: 'group', name: 'Group', visible: true, opacity: 1, collapsed: false, children: [node] }
      made = g.id
      return { ...d, layers: insertNode(layers, parent?.id ?? null, at, g) }
    }, 'Grouped')
    if (made) set({ selected: made })
  },
  addGroup() {
    let made: string | null = null
    edit((d) => {
      if (countGroups(d.layers) >= LIMITS.groups) return d
      const g: Group = { id: uid('g'), type: 'group', name: 'Group', visible: true, opacity: 1, collapsed: false, children: [] }
      made = g.id
      return { ...d, layers: [...d.layers, g] }
    }, 'Group added')
    if (made) set({ selected: made })
  },
  /** Dissolve a group: its children take its place. */
  ungroup(id: string) {
    edit((d) => {
      const g = findNode(d.layers, id)
      if (!g || !isGroup(g)) return d
      const parent = parentOf(d.layers, id) ?? null
      const siblings = parent ? parent.children : d.layers
      const at = siblings.findIndex((n) => n.id === id)
      let [layers] = removeNode(d.layers, id)
      g.children.forEach((c, i) => (layers = insertNode(layers, parent?.id ?? null, at + i, c)))
      return { ...d, layers }
    }, 'Ungrouped')
    set({ selected: null })
  },
  /** Move a node to `index` (document order) inside `parentId` (`null` = top level). Refuses a move into itself or past the nesting limit. */
  move(id: string, parentId: string | null, index: number): boolean {
    const d = state.doc
    if (!d) return false
    const node = findNode(d.layers, id)
    if (!node) return false
    if (parentId) {
      const target = findNode(d.layers, parentId)
      if (!target || !isGroup(target) || containsNode(node, parentId)) return false
      if (ancestorsOf(d.layers, parentId).length + 1 + groupHeight(node) > MAX_GROUP_DEPTH) return false
    }
    const from = parentOf(d.layers, id) ?? null
    const siblings = from ? from.children : d.layers
    const cur = siblings.findIndex((n) => n.id === id)
    // removing the node first shifts everything above it down by one
    const at = (from?.id ?? null) === parentId && cur < index ? index - 1 : index
    if ((from?.id ?? null) === parentId && cur === at) return false
    edit((doc) => {
      const [without, removed] = removeNode(doc.layers, id)
      return removed ? { ...doc, layers: insertNode(without, parentId, at, removed) } : doc
    }, 'Layer moved')
    return true
  },
  /** Add layers from templates (a catalogue preset, an analysis output) above the selection, or on top. */
  add(templates: (LayerTemplate & { sourceId?: string })[], extraSources: Record<string, Source> = {}): string | null {
    let first: string | null = null
    edit((d) => {
      if (allLayers(d.layers).length + templates.length > LIMITS.layers) return d
      const sources = { ...d.sources, ...extraSources }
      let layers = d.layers
      const sel = state.selected ? findNode(d.layers, state.selected) : null
      const parent = sel ? (parentOf(d.layers, sel.id) ?? null) : null
      const siblings = parent ? parent.children : d.layers
      let at = sel ? siblings.findIndex((n) => n.id === sel.id) + 1 : d.layers.length
      // templates arrive top-first, as a panel lists them
      for (const t of [...templates].reverse()) {
        const made = instantiate(t, t.sourceId)
        sources[made.sourceId] = sources[made.sourceId] ?? made.source
        layers = insertNode(layers, parent?.id ?? null, at++, made.layer)
        first = made.layer.id
      }
      return { ...d, sources, layers }
    }, templates.length === 1 ? 'Layer added' : `${templates.length} layers added`)
    if (first) set({ selected: first })
    return first
  },

  /* ---- document ---- */
  setTitle: (title: string) => edit((d) => (title.trim() && title !== d.title ? { ...d, title: title.trim() } : d), 'Map renamed', 'title'),
  setBasemap: (patch: Partial<MapDocument['basemap']>) => edit((d) => ({ ...d, basemap: { ...d.basemap, ...patch } }), 'Basemap changed'),
  /** The time cursor moves without an undo step: it is a way of looking, not an edit. */
  setCursor(cursor: string) {
    const d = state.doc
    if (!d?.time || d.time.cursor === cursor) return
    const next = { ...d, time: { ...d.time, cursor } }
    set({ doc: next, saved: state.saved === d ? next : state.saved })
  },
  setTime: (time: MapDocument['time']) => edit((d) => ({ ...d, time }), 'Timeline changed'),
  setAnalysis: (id: string, req: AnalysisRequest, said = 'Analysis changed') => edit((d) => ({ ...d, analyses: { ...d.analyses, [id]: req } }), said, `analysis:${id}`),
}
export type Actions = typeof actions

/** Rows of the layer panel: the tree flattened top → bottom, children of collapsed groups left out. */
export type Row = { node: LayerNode; depth: number; parent: Group | null; hidden: boolean; lockedBy: boolean; last: boolean }
export function rowsOf(layers: LayerNode[]): Row[] {
  const out: Row[] = []
  const visit = (nodes: LayerNode[], depth: number, parent: Group | null, hidden: boolean, locked: boolean) => {
    for (let i = nodes.length - 1; i >= 0; i--) {
      const node = nodes[i]
      out.push({ node, depth, parent, hidden: hidden || !node.visible, lockedBy: locked, last: i === 0 })
      if (isGroup(node) && !node.collapsed) visit(node.children, depth + 1, node, hidden || !node.visible, locked || !!node.locked)
    }
  }
  visit(layers, 0, null, false, false)
  return out
}
