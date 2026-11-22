// The map document, `ldl-map/1`, and the analysis request and result, `ldl-analysis/1`: the TypeScript
// mirror of docs/architecture-v2/ui-contract-explore.md (contract v0, 2026-11-22) and 10-map-composition.md §2.
//
// A document names data, it never holds it. `layers` is ordered bottom → top (draw order); the
// layer panel shows the reverse. Everything prefixed `x-` or marked (client) below is an extension
// of this mockup that a strict `/1` reader ignores.

export const MAP_SPEC = 'ldl-map/1' as const
export const ANALYSIS_SPEC = 'ldl-analysis/1' as const

/* ---------------- blend ---------------- */
/** `/1` lists the first six. `overlay` needs the backdrop inside the shader; this client has the isolation pass for it. */
export type BlendMode = 'normal' | 'multiply' | 'screen' | 'overlay' | 'darken' | 'lighten' | 'add'
export const BLEND_MODES: { id: BlendMode; label: string; tag: string; hint: string; inSpec: boolean }[] = [
  { id: 'normal', label: 'Normal', tag: '', hint: 'Drawn over what is below', inSpec: true },
  { id: 'multiply', label: 'Multiply', tag: 'MUL', hint: 'Darkens: a hazard over a choropleth', inSpec: true },
  { id: 'darken', label: 'Darken', tag: 'DRK', hint: 'Keeps the darker of the two', inSpec: true },
  { id: 'screen', label: 'Screen', tag: 'SCR', hint: 'Lightens: glow on a dark basemap', inSpec: true },
  { id: 'lighten', label: 'Lighten', tag: 'LGT', hint: 'Keeps the lighter of the two', inSpec: true },
  { id: 'add', label: 'Add', tag: 'ADD', hint: 'Sums light, clipping at white', inSpec: true },
  { id: 'overlay', label: 'Overlay', tag: 'OVR', hint: 'Contrast: multiply the darks, screen the lights', inSpec: false },
]

/* ---------------- time ---------------- */
/** A period code of the standard model (annual here: "2024"), the newest one, or the document's time cursor. */
export type PeriodRef = 'latest' | string | { bind: 'time'; match?: 'exact' | 'asof' }
export type TimeConfig = { cursor: string; range: [string, string]; freq: 'A'; playback: { fps: number; loop: boolean } }

/* ---------------- sources ---------------- */
export type Version = 'latest' | { dsv: string }
export type IndicatorSource = { kind: 'indicator'; indicator: string; /** two columns on one support, for a bivariate layer */ indicators?: string[]; level: number; scheme?: 'admin'; parent?: string; period: PeriodRef; dims?: Record<string, string>; version?: Version }
export type BoundariesSource = { kind: 'boundaries'; scheme: 'admin'; level: number; parent?: string; version?: Version }
export type FeaturesSource = { kind: 'features'; dataset: string; within?: string; fields?: string[]; version?: Version; /** (client) bind a date field to the time cursor */ 'x-time'?: { field: string; match: 'exact' | 'asof' } }
export type GridSource = { kind: 'grid'; grid: string; dims?: Record<string, string>; period: PeriodRef; version?: Version }
export type AnalysisSource = { kind: 'analysis'; ref: string; output: string }
export type Source = IndicatorSource | BoundariesSource | FeaturesSource | GridSource | AnalysisSource

/* ---------------- style: channels bound to fields through scales ---------------- */
export type ScaleType = 'quantile' | 'equal' | 'jenks' | 'manual' | 'continuous' | 'categorical' | 'diverging'
export type Scale = {
  type: ScaleType
  classes?: number
  /** a named ramp from the registry: `ldl.seq.*`, `ldl.div.*`, `ldl.cat.*` */
  ramp?: string
  reverse?: boolean
  /** present when `type` is manual, or once the document is frozen */
  breaks?: number[]
  domain?: 'extent' | 'data' | 'national'
  across?: 'all_periods' | 'period'
  transform?: 'linear' | 'sqrt' | 'log'
  /** absolute clamp of a continuous scale; null = the data's own end */
  clamp?: [number | null, number | null]
  /** categorical: code → colour (a hex, or a named tone) */
  mapping?: Record<string, string>
  /** output range in px, for `size` and `stroke.width` */
  range?: [number, number]
}
export type FieldBinding = { field: string; scale: Scale }
export type BivariateFill = { bivariate: { x: FieldBinding; y: FieldBinding; palette: string } }
/** A colour is a hex, or a named tone (`ldl.tone.*`, client) that follows the basemap's scheme. */
export type Fill = string | FieldBinding | BivariateFill
export type Stroke = { color?: string | FieldBinding; width?: number | FieldBinding; dash?: number[]; 'x-casing'?: boolean }
/** The dash patterns the editor offers; a document may carry any other. */
export const DASH: Record<'solid' | 'dash' | 'dot', number[] | undefined> = { solid: undefined, dash: [4.5, 2.4], dot: [0.4, 2] }
export const dashKind = (d: number[] | undefined): 'solid' | 'dash' | 'dot' => (!d?.length ? 'solid' : d[0] < 1 ? 'dot' : 'dash')
/** `halo`: a colour, or `none`; left out, the renderer picks the basemap's paper. */
export type LabelStyle = { text: string; size: number; halo?: string; zoom?: [number, number] }
export type Style = {
  fill?: Fill
  stroke?: Stroke
  size?: number | FieldBinding
  label?: LabelStyle
  uncertainty?: 'hatch' | 'fade' | 'off'
  /** raster: mask outside this range */
  range?: [number | null, number | null]
  resample?: 'nearest' | 'linear'
  /** heatmap */
  radius?: number
  intensity?: number
  weight?: string | null
  ramp?: string
  reverse?: boolean
  /** hillshade */
  exaggeration?: number
  /** cluster */
  aggregate?: 'count'
  /** (client) shade a raster with the terrain model */
  'x-relief'?: boolean
}

/* ---------------- filter: the CQL2-JSON subset of §2.4 ---------------- */
export type CqlProp = { property: string }
export type Cql =
  | { op: 'and' | 'or'; args: Cql[] }
  | { op: 'not'; args: [Cql] }
  | { op: '=' | '<>' | '<' | '<=' | '>' | '>='; args: [CqlProp, number | string] }
  | { op: 'between'; args: [CqlProp, number, number] }
  | { op: 'in'; args: [CqlProp, (string | number)[]] }
  | { op: 'isNull'; args: [CqlProp] }
/** What the filter UI edits: a flat conjunction of range and membership clauses. */
export type Clause = { field: string; kind: 'range'; range: [number, number] } | { field: string; kind: 'in'; values: string[] }

export function clausesOf(filter: Cql | null | undefined): Clause[] {
  if (!filter) return []
  const parts = filter.op === 'and' ? filter.args : [filter]
  const out: Clause[] = []
  for (const p of parts) {
    if (p.op === 'between') out.push({ field: p.args[0].property, kind: 'range', range: [p.args[1], p.args[2]] })
    else if (p.op === 'in') out.push({ field: p.args[0].property, kind: 'in', values: p.args[1].map(String) })
    else if (p.op === '>=' && typeof p.args[1] === 'number') out.push({ field: p.args[0].property, kind: 'range', range: [p.args[1], Infinity] })
    else if (p.op === '<=' && typeof p.args[1] === 'number') out.push({ field: p.args[0].property, kind: 'range', range: [-Infinity, p.args[1]] })
  }
  return out
}
export function filterOf(clauses: Clause[]): Cql | null {
  const args = clauses.map((c): Cql => {
    if (c.kind === 'in') return { op: 'in', args: [{ property: c.field }, c.values] }
    if (!Number.isFinite(c.range[1])) return { op: '>=', args: [{ property: c.field }, c.range[0]] }
    if (!Number.isFinite(c.range[0])) return { op: '<=', args: [{ property: c.field }, c.range[1]] }
    return { op: 'between', args: [{ property: c.field }, c.range[0], c.range[1]] }
  })
  return args.length === 0 ? null : args.length === 1 ? args[0] : { op: 'and', args }
}
/** Evaluate a filter against a plain record (frames, analysis tables). */
export function matches(filter: Cql | null | undefined, row: Record<string, unknown>): boolean {
  if (!filter) return true
  switch (filter.op) {
    case 'and':
      return filter.args.every((a) => matches(a, row))
    case 'or':
      return filter.args.some((a) => matches(a, row))
    case 'not':
      return !matches(filter.args[0], row)
    case 'isNull':
      return row[filter.args[0].property] == null
    case 'between': {
      const v = row[filter.args[0].property] as number
      return v != null && v >= filter.args[1] && v <= filter.args[2]
    }
    case 'in':
      return filter.args[1].map(String).includes(String(row[filter.args[0].property]))
    default: {
      const v = row[filter.args[0].property] as number | string
      const t = filter.args[1]
      if (v == null) return false
      return filter.op === '=' ? v === t : filter.op === '<>' ? v !== t : filter.op === '<' ? v < t : filter.op === '<=' ? v <= t : filter.op === '>' ? v > t : v >= t
    }
  }
}

/* ---------------- layer nodes ---------------- */
/** The types this client draws. `symbol`, `bins` and `annotation` are in `/1` and not built here. */
export type LayerType = 'choropleth' | 'bivariate' | 'polygon' | 'line' | 'point' | 'cluster' | 'heatmap' | 'raster' | 'hillshade' | 'label'
export const LAYER_TYPE_LABEL: Record<LayerType, string> = {
  choropleth: 'Choropleth',
  bivariate: 'Bivariate',
  polygon: 'Polygons',
  line: 'Lines',
  point: 'Points',
  cluster: 'Clusters',
  heatmap: 'Heatmap',
  raster: 'Raster',
  hillshade: 'Hillshade',
  label: 'Labels',
}
type NodeBase = { id: string; name: string; visible: boolean; opacity: number; locked?: boolean }
export type Layer = NodeBase & {
  type: LayerType
  /** key in `document.sources` */
  source: string
  blend: BlendMode
  zoom?: [number, number]
  filter?: Cql | null
  clip?: { geo: string }
  style: Style
  legend: { show: boolean; title: string | null }
  interaction?: { hover?: boolean; select?: boolean; tooltip?: string[] }
}
/** Groups nest to depth 3 and have no blend mode: visibility and opacity apply to the group as one image. */
export type Group = NodeBase & { type: 'group'; collapsed: boolean; children: LayerNode[] }
export type LayerNode = Layer | Group
export const isGroup = (n: LayerNode): n is Group => n.type === 'group'
export const MAX_GROUP_DEPTH = 3
export const LIMITS = { layers: 32, groups: 8, sources: 16, analyses: 6 }

/* ---------------- analyses: request (the document stores it, never the result) ---------------- */
export type Transform = 'identity' | 'log' | 'log1p' | 'sqrt' | 'zscore' | 'rank'
/** One variable of an analysis: a source of the document, or an indicator or a field named directly. */
export type Variable = { source?: string; indicator?: string; field?: string; period?: string; dims?: Record<string, string>; transform?: Transform; label?: string }
export type Support = { kind: 'admin'; level: number; scheme?: string; within?: string } | { kind: 'points'; source?: string; dataset?: string; within?: string }
export type RelateParams = {
  correlation?: ('pearson' | 'spearman')[]
  regression?: 'ols' | 'none'
  robust?: boolean
  spatial?: { weights: 'queen' | 'knn4' | 'knn6' | 'knn8'; local?: 'lee' | 'moran' | 'none'; permutations?: number } | null
  bivariate?: { classes: 3 | 4; method: 'quantile' | 'equal' | 'jenks' }
  scale_sensitivity?: number[]
}
export type GroupCompareParams = {
  zone: { source?: string; grid?: string; dataset?: string; where?: Cql }
  rings_m?: number[]
  compare: 'inside_vs_outside' | 'inside_vs_ring'
  within_groups?: { level: number } | null
  tests?: ('welch' | 'mann_whitney')[]
  bootstrap?: number
}
type RequestBase = { spec: typeof ANALYSIS_SPEC; pin?: 'latest' | { release: string }; override?: GuardCode[]; /** `an_…` once the document is frozen */ result?: string }
export type RelateRequest = RequestBase & { method: 'relate'; support: Extract<Support, { kind: 'admin' }>; variables: { x: Variable; y: Variable; controls?: Variable[] }; params: RelateParams }
export type GroupCompareRequest = RequestBase & { method: 'group_compare'; support: Extract<Support, { kind: 'points' }>; variables: { y: Variable }; params: GroupCompareParams }
export type AnalysisRequest = RelateRequest | GroupCompareRequest

/* ---------------- analyses: result, `ldl-analysis-result/1` ---------------- */
export type AnalysisState = 'queued' | 'running' | 'ready' | 'failed' | 'blocked'
export type Severity = 'note' | 'warn' | 'block'
export type GuardCode =
  | 'NOT_CAUSAL' | 'MIN_N' | 'SMALL_N' | 'MAUP' | 'MAUP_UNSTABLE' | 'ECOLOGICAL' | 'RESIDUAL_AUTOCORR' | 'SIZE_CONFOUND' | 'SHARED_LINEAGE' | 'ESTIMATED_INPUT'
  | 'MODEL_ON_MODEL' | 'INPUT_UNCERTAINTY_IGNORED' | 'PERIOD_MISMATCH' | 'LOW_COVERAGE' | 'PSEUDO_REPLICATION' | 'DISAGGREGATED_INPUT' | 'INTERP_SOCIO'
  | 'OUTLIER_LEVERAGE' | 'NO_CONTROLS' | 'MULTICOLLINEAR' | 'MULTIPLE_TESTING' | 'SUPPRESSED_CELLS' | 'SYNTHETIC_INPUT' | 'COMMUNITY_INPUT' | 'EXPLORATORY'
export type Guard = { code: GuardCode; severity: Severity; message: string; detail?: Record<string, unknown>; action?: { label?: string; request?: Partial<AnalysisRequest> }; overridden?: boolean }
export type AlignmentStep = { role: string; from: string; to: string; method: string; params?: Record<string, unknown>; coverage: number; quality?: 'A' | 'B' | 'C' | 'D'; dropped?: Record<string, number> }
export type Interval = [lo: number, hi: number]
export type FieldInfo = { id: string; label: string; type: 'number' | 'category' | 'string' | 'date'; unit?: string }
export type AnalysisOutput = { id: string; href: string; suggested_layer?: Pick<Layer, 'type' | 'style' | 'filter' | 'name'> & { opacity?: number } } & ({ kind: 'frame'; columns: string[] } | { kind: 'features'; fields: FieldInfo[] } | { kind: 'chart'; encoding: 'points' | 'hexbin' | 'histogram' | 'groups' })
export type AnalysisResult<E = unknown> = {
  spec: 'ldl-analysis-result/1'
  id: string
  state: AnalysisState
  method: { id: AnalysisRequest['method']; version: string; citations: string[] }
  request: AnalysisRequest
  inputs: { role: string; dataset: string; dsv: string; indicator?: string; field?: string; period?: string; class: 'official' | 'third_party' | 'derived' | 'estimated' | 'user'; licence: string; visibility: 'private' | 'unlisted' | 'public'; synthetic?: boolean }[]
  alignment: AlignmentStep[]
  n: { units?: number; points?: number; used: number; dropped: Record<string, number> }
  /** null when blocked */
  estimates: E | null
  /** sorted: block, warn, note; NOT_CAUSAL first among the notes */
  guards: Guard[]
  interpretation: { summary: string; caveats: string[] } | null
  outputs: AnalysisOutput[]
  provenance: { fingerprint: string; seed: number; engine: string; data_version: string; computed_at: string; elapsed_ms: number }
  citation: { text: string; url: string }
}
export type Coef = { term: string; b: number; se_hc3?: number; ci?: Interval; p_adjusted?: number }
export type RelateEstimates = {
  pearson?: { r: number; ci: Interval; ci_level: number; p?: number; p_adjusted?: number; n_effective?: number }
  spearman?: { rho: number; ci: Interval; p_adjusted?: number }
  ols?: { formula: string; coef: Coef[]; r2: number; adj_r2: number; reading: string }
  robust?: { coef: Coef[] }
  lee_l?: { L: number; p_perm: number }
  residual_moran?: { I: number; p_perm: number }
  scale_sensitivity?: { level: number; n: number; pearson_r: number; 'x-spearman'?: number }[]
  /** (client mock) correlation left once the controls are held constant, and the spatial pattern of each variable on its own */
  'x-partial'?: { r: number; p_adjusted: number; controls: string[] } | null
  'x-moran'?: { x: { I: number; p_perm: number }; y: { I: number; p_perm: number } }
}
export type GroupCompareEstimates = {
  groups: { id: string; label: string; n: number; median: number; mean: number; ci: Interval }[]
  contrasts: { a: string; b: string; diff: number; ci: Interval; hodges_lehmann?: number; cohens_d?: number; p_welch?: number; p_mann_whitney?: number; reading: string }[]
}

/* ---------------- document ---------------- */
export type BasemapStyle = 'ldl-auto' | 'ldl-light' | 'ldl-dark' | 'none'
export type Panel =
  | { id: string; type: 'legend' }
  | { id: string; type: 'chart'; chart: 'scatter'; analysis: string; linked?: string; options?: { fit?: ('ols' | 'robust')[]; density?: 'points' | 'hexbin'; brush?: boolean } }
  | { id: string; type: 'stats'; analysis: string }
export type MapDocument = {
  spec: typeof MAP_SPEC
  /** `m_…` saved, `ms_…` snapshot, absent while unsaved */
  id?: string
  title: string
  description?: string
  pin: 'latest' | { release: string }
  view: { center: [number, number]; zoom: number; bearing?: number; pitch?: number; bounds?: [number, number, number, number] }
  extent: { geo: string } | null
  /** `ldl-auto` (client): follow the app theme */
  basemap: { style: BasemapStyle; labels: 'top' | 'bottom' | 'off'; hillshade: { on: boolean; exaggeration: number }; 'x-coast'?: boolean }
  time: TimeConfig | null
  sources: Record<string, Source>
  analyses: Record<string, AnalysisRequest>
  layers: LayerNode[]
  panels: Panel[]
  meta: { created_at: string; updated_at: string; author?: string; forked_from?: string | null; licence: string; generator: string }
}
export const GENERATOR = 'lanka-web-v2 mockup'

/* ---------------- ids ---------------- */
const ALPHABET = '0123456789abcdefghjkmnpqrstvwxyz'
export function uid(prefix: string, n = 8): string {
  const bytes = crypto.getRandomValues(new Uint8Array(n))
  return `${prefix}_${Array.from(bytes, (b) => ALPHABET[b & 31]).join('')}`
}

/* ---------------- tree: pure helpers over `layers` (document order, bottom → top) ---------------- */
export function walk(nodes: LayerNode[], fn: (n: LayerNode, parent: Group | null, depth: number) => void, parent: Group | null = null, depth = 0) {
  for (const n of nodes) {
    fn(n, parent, depth)
    if (isGroup(n)) walk(n.children, fn, n, depth + 1)
  }
}
export function findNode(nodes: LayerNode[], id: string | null | undefined): LayerNode | null {
  if (!id) return null
  for (const n of nodes) {
    if (n.id === id) return n
    if (isGroup(n)) {
      const hit = findNode(n.children, id)
      if (hit) return hit
    }
  }
  return null
}
export function parentOf(nodes: LayerNode[], id: string, parent: Group | null = null): Group | null | undefined {
  for (const n of nodes) {
    if (n.id === id) return parent
    if (isGroup(n)) {
      const hit = parentOf(n.children, id, n)
      if (hit !== undefined) return hit
    }
  }
  return undefined
}
export function ancestorsOf(nodes: LayerNode[], id: string): Group[] {
  const out: Group[] = []
  let p = parentOf(nodes, id)
  while (p) {
    out.unshift(p)
    p = parentOf(nodes, p.id)
  }
  return out
}
export const allLayers = (nodes: LayerNode[]): Layer[] => {
  const out: Layer[] = []
  walk(nodes, (n) => void (!isGroup(n) && out.push(n)))
  return out
}
export const countGroups = (nodes: LayerNode[]) => {
  let c = 0
  walk(nodes, (n) => void (isGroup(n) && c++))
  return c
}
/** Depth of the deepest group at or below this node (a layer is 0). */
export const groupHeight = (n: LayerNode): number => (isGroup(n) ? 1 + Math.max(0, ...n.children.map(groupHeight)) : 0)
export function updateNode(nodes: LayerNode[], id: string, fn: (n: LayerNode) => LayerNode): LayerNode[] {
  let changed = false
  const next = nodes.map((n) => {
    if (n.id === id) {
      changed = true
      return fn(n)
    }
    if (isGroup(n)) {
      const children = updateNode(n.children, id, fn)
      if (children !== n.children) {
        changed = true
        return { ...n, children }
      }
    }
    return n
  })
  return changed ? next : nodes
}
export function removeNode(nodes: LayerNode[], id: string): [LayerNode[], LayerNode | null] {
  let removed: LayerNode | null = null
  const next: LayerNode[] = []
  for (const n of nodes) {
    if (n.id === id) {
      removed = n
      continue
    }
    if (isGroup(n) && !removed) {
      const [children, r] = removeNode(n.children, id)
      if (r) {
        removed = r
        next.push({ ...n, children })
        continue
      }
    }
    next.push(n)
  }
  return removed ? [next, removed] : [nodes, null]
}
/** Insert at `index` (document order) of the given parent; `null` is the top level. */
export function insertNode(nodes: LayerNode[], parentId: string | null, index: number, node: LayerNode): LayerNode[] {
  if (parentId === null) {
    const next = [...nodes]
    next.splice(Math.max(0, Math.min(index, next.length)), 0, node)
    return next
  }
  return updateNode(nodes, parentId, (p) => {
    if (!isGroup(p)) return p
    const children = [...p.children]
    children.splice(Math.max(0, Math.min(index, children.length)), 0, node)
    return { ...p, children, collapsed: false }
  })
}
export const containsNode = (root: LayerNode, id: string): boolean => root.id === id || (isGroup(root) && root.children.some((c) => containsNode(c, id)))

/** A deep copy with fresh ids (and, through `remap`, fresh source keys). */
export function cloneNode(node: LayerNode, remap: (sourceId: string) => string, rename = true): LayerNode {
  if (isGroup(node)) return { ...node, id: uid('g'), name: rename ? `${node.name} copy` : node.name, children: node.children.map((c) => cloneNode(c, remap, false)) }
  return { ...structuredClone(node), id: uid('l'), name: rename ? `${node.name} copy` : node.name, source: remap(node.source) }
}

/* ---------------- permalink: the whole document in the URL fragment (§6.2) ---------------- */
const b64url = (bytes: Uint8Array) => {
  let s = ''
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}
const unb64url = (s: string) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0))
async function pipe(bytes: Uint8Array, stream: CompressionStream | DecompressionStream): Promise<Uint8Array> {
  const out = new Response(new Blob([bytes as BlobPart]).stream().pipeThrough(stream))
  return new Uint8Array(await out.arrayBuffer())
}
/** deflate + base64url of the canonical JSON. */
export async function encodeDocument(doc: MapDocument): Promise<string> {
  const { id: _id, ...rest } = doc
  void _id
  return b64url(await pipe(new TextEncoder().encode(JSON.stringify(rest)), new CompressionStream('deflate-raw')))
}
export async function decodeDocument(encoded: string): Promise<MapDocument> {
  const json: unknown = JSON.parse(new TextDecoder().decode(await pipe(unb64url(encoded), new DecompressionStream('deflate-raw'))))
  return assertDocument(json)
}
/** Readers MUST reject an unknown major. */
export function assertDocument(json: unknown): MapDocument {
  const d = json as Partial<MapDocument> | null
  if (!d || d.spec !== MAP_SPEC || !Array.isArray(d.layers) || typeof d.sources !== 'object') throw new Error('Not an ldl-map/1 document')
  return d as MapDocument
}

/* ---------------- building documents ---------------- */
/** A layer and its source before they have ids: what the catalogue's presets and the starter maps are written in. */
export type LayerTemplate = { type: LayerType; name: string; source: Source; style: Style; opacity?: number; blend?: BlendMode; filter?: Cql; zoom?: [number, number]; visible?: boolean; legend?: boolean }
const bound = (v: unknown) => typeof v === 'object' && v !== null
const encodes = (t: LayerTemplate) => (t.type === 'label' || t.type === 'hillshade' ? false : t.type === 'line' ? bound(t.style.stroke?.color) : t.type === 'point' ? bound(t.style.fill) : t.type === 'polygon' ? bound(t.style.fill) || bound(t.style.stroke?.color) : true)
export function instantiate(t: LayerTemplate, sourceId: string = uid('s')): { sourceId: string; source: Source; layer: Layer } {
  return {
    sourceId,
    source: structuredClone(t.source),
    layer: {
      id: uid('l'),
      type: t.type,
      name: t.name,
      source: sourceId,
      visible: t.visible ?? true,
      opacity: t.opacity ?? 1,
      blend: t.blend ?? 'normal',
      ...(t.zoom ? { zoom: t.zoom } : {}),
      filter: t.filter ?? null,
      style: structuredClone(t.style),
      // a plain reference layer explains itself; a key is for layers that encode something
      legend: { show: t.legend ?? encodes(t), title: null },
      interaction: { hover: true, select: true },
    },
  }
}
export const LK_VIEW: MapDocument['view'] = { center: [80.72, 7.87], zoom: 6.7, bounds: [79.35, 5.75, 82.15, 10.0] }
export function blankDocument(title = 'Untitled map'): MapDocument {
  const now = new Date().toISOString()
  return {
    spec: MAP_SPEC,
    title,
    pin: 'latest',
    view: LK_VIEW,
    extent: null,
    basemap: { style: 'ldl-auto', labels: 'off', hillshade: { on: false, exaggeration: 1 }, 'x-coast': true },
    time: null,
    sources: {},
    analyses: {},
    layers: [],
    panels: [{ id: 'p_legend', type: 'legend' }],
    meta: { created_at: now, updated_at: now, forked_from: null, licence: 'CC-BY-4.0', generator: GENERATOR },
  }
}
/** Sources no layer or analysis refers to any more. */
export function pruneSources(doc: MapDocument): MapDocument {
  const used = new Set(allLayers(doc.layers).map((l) => l.source))
  for (const a of Object.values(doc.analyses)) {
    if (a.method === 'relate') for (const v of [a.variables.x, a.variables.y, ...(a.variables.controls ?? [])]) v.source && used.add(v.source)
    else for (const id of [a.support.source, a.params.zone.source]) id && used.add(id)
  }
  const sources = Object.fromEntries(Object.entries(doc.sources).filter(([id]) => used.has(id)))
  return Object.keys(sources).length === Object.keys(doc.sources).length ? doc : { ...doc, sources }
}
export function resolvePeriod(ref: PeriodRef | undefined, time: TimeConfig | null, available: string[]): string {
  const last = available[available.length - 1] ?? '2026'
  if (!ref || ref === 'latest') return last
  const want = typeof ref === 'string' ? ref : (time?.cursor ?? last)
  if (available.includes(want)) return want
  // as-of: the newest period not after the cursor
  return available.filter((p) => p <= want).pop() ?? available[0] ?? last
}
export const isTimeBound = (s: Source | undefined) => !!s && (((s.kind === 'indicator' || s.kind === 'grid') && typeof s.period === 'object') || (s.kind === 'features' && !!s['x-time']))
