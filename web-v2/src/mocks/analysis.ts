// Mock of the analysis engine (11-analysis-engine): `relate` two variables on a common support, and
// `group_compare` points inside a zone against the rings around it.
//
// The numbers are computed, not written down: the inputs are the synthetic surfaces and listings of
// ./synth, aligned and tested here with ../lib/stats. What a real backend returns has this shape.
import type { FeatureCollection, Point } from 'geojson'
import type { AnalysisOutput, AnalysisResult as AnalysisResultOf, Cql, Guard, GroupCompareEstimates, GroupCompareRequest, RelateEstimates, RelateRequest, Transform, Variable } from '@/lib/map-spec'
import { SITE } from '@/lib/site'
import * as st from '@/lib/stats'
import { DATA_VERSION } from './db'
import { indicatorMeta, LEVELS } from './layers'
import * as synth from './synth'

export type VarInfo = { indicator: string; label: string; short: string; unit: string; transform: Transform; alignment: string }
/** (mock) What the result's outputs would deliver over their own URLs, inlined so the mock needs no second request. */
export type RelateData = {
  support: { level: number; label: string; within: string }
  x: VarInfo
  y: VarInfo
  controls: VarInfo[]
  /** one row per unit, in raw units */
  table: { codes: string[]; names: string[]; x: number[]; y: number[]; n: number[]; estimated: boolean[]; cls: number[] }
  breaks: { x: number[]; y: number[] }
  /** fitted on the transformed outcome */
  fit: st.Ols & { robust: { slope: number; intercept: number }; per10: number }
  local: { codes: string[]; L: number[]; p: number[]; pFdr: number[]; cluster: string[] }
  significant: number
  weights: string
  permutations: number
}
/** `x-engine`: computed by the engine (src/api/real/analysis.ts); absent = this mock, on the synthetic sample. */
export type RelateResult = AnalysisResultOf<RelateEstimates> & { 'x-data': RelateData | null; 'x-engine'?: boolean }
export type CompareResult = AnalysisResultOf<GroupCompareEstimates> & { 'x-data': { unit: string; threshold: number; within: string | null; points: FeatureCollection<Point>; /** engine results: the scale estimates are on, and what was measured */ transform?: string; label?: string } | null; 'x-engine'?: boolean }
export type AnalysisResult = RelateResult | CompareResult
export type { Guard }

const pick = (S: synth.UnitStats, indicator: string): ArrayLike<number> | null =>
  indicator === 'exp.flood_pop_share' ? S.floodShare : indicator === '@demo/re.asking_price_per_perch_median' ? S.price : indicator === 'est.pop_density' ? S.popDensity : indicator === 'acc.travel_time_city' ? S.travelMin : null
const ALIGN: Record<string, string> = {
  'exp.flood_pop_share': 'population-weighted share of 220 m grid cells deeper than 0.15 m',
  '@demo/re.asking_price_per_perch_median': 'median of the listings inside the unit; modelled from the price surface where there are fewer than three',
  'est.pop_density': 'mean of the population grid over the unit',
  'acc.travel_time_city': 'drive time from the unit centroid',
}
/** How a variable is named inside a sentence. */
const PHRASE: Record<string, string> = { 'exp.flood_pop_share': 'flood exposure', '@demo/re.asking_price_per_perch_median': 'asking price', 'est.pop_density': 'population density', 'acc.travel_time_city': 'drive time to Colombo' }
const phrase = (v: VarInfo) => PHRASE[v.indicator] ?? v.label.toLowerCase()
const info = (v: Variable): VarInfo => {
  const m = indicatorMeta(v.indicator ?? '')
  return { indicator: v.indicator ?? '', label: v.label ?? m?.label ?? v.indicator ?? '', short: m?.short ?? '', unit: m?.unit ?? '', transform: v.transform ?? 'identity', alignment: ALIGN[v.indicator ?? ''] ?? 'value of the unit' }
}
const fnv = (s: string) => {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619)
  return (h >>> 0).toString(16).padStart(8, '0')
}
const tf = (v: number, t: VarInfo['transform']) => (t === 'log' ? (v > 0 ? Math.log(v) : NaN) : v)
const strength = (r: number) => (Math.abs(r) < 0.1 ? 'none' : Math.abs(r) < 0.3 ? 'weak' : Math.abs(r) < 0.55 ? 'moderate' : 'strong')
const tick = () => new Promise((r) => setTimeout(r, 0))
const neighbours = new Map<number, Promise<number[][]>>()
const nbOf = (level: number) => {
  let p = neighbours.get(level)
  if (!p) neighbours.set(level, (p = synth.units(level).then((u) => st.knn(u.centroids, level === 2 ? 3 : 6))))
  return p
}

/** x, y and controls on one support, complete cases only. */
async function align(level: number, vars: VarInfo[]) {
  const [u, S] = await Promise.all([synth.units(level), synth.unitStats(String(level))])
  const wp = u.codes.map((c, i) => (c.startsWith('LK1') ? i : -1)).filter((i) => i >= 0)
  const cols = vars.map((v) => pick(S, v.indicator))
  const keep = wp.filter((i) => cols.every((c, k) => c && Number.isFinite(tf(c[i], vars[k].transform))))
  return { u, S, keep, dropped: wp.length - keep.length, raw: cols.map((c) => keep.map((i) => (c ? c[i] : NaN))), t: cols.map((c, k) => keep.map((i) => tf(c ? c[i] : NaN, vars[k].transform))) }
}
const cache = new Map<string, Promise<AnalysisResult>>()
export function runAnalysis(req: RelateRequest | GroupCompareRequest): Promise<AnalysisResult> {
  const { result: _r, ...body } = req
  void _r
  const key = JSON.stringify(body)
  let p = cache.get(key)
  if (!p) cache.set(key, (p = req.method === 'relate' ? relate(req, `an_${fnv(key)}${fnv(`${key}#`)}`) : compare(req, `an_${fnv(key)}${fnv(`${key}#`)}`)))
  return p
}

const SEVERITY = { block: 0, warn: 1, note: 2 }
/** block, then warn, then note; NOT_CAUSAL first among the notes. */
const sortGuards = (g: Guard[]) => g.sort((a, b) => SEVERITY[a.severity] - SEVERITY[b.severity] || Number(b.code === 'NOT_CAUSAL') - Number(a.code === 'NOT_CAUSAL'))
const DSV: Record<string, [dataset: string, dsv: string, cls: 'estimated' | 'user', licence: string]> = {
  'exp.flood_pop_share': ['derived-flood-exposure', 'dsv_41c0a9e77b1d2f08', 'estimated', 'CC-BY-4.0'],
  '@demo/re.asking_price_per_perch_median': ['@demo/property-asking-prices-sample', 'dsv_0d7e55aa91c3b672', 'user', 'workspace data'],
  'est.pop_density': ['pop-grid-100m', 'dsv_7ab21f0c33d9e514', 'estimated', 'CC-BY-4.0'],
  'acc.travel_time_city': ['access-health', 'dsv_c1e88a7b02f64d39', 'estimated', 'CC-BY-4.0'],
}
const inputOf = (role: string, v: VarInfo) => {
  const [dataset, dsv, cls, licence] = DSV[v.indicator] ?? [v.indicator, 'dsv_0000000000000000', 'estimated', 'CC-BY-4.0']
  return { role, dataset, dsv, indicator: v.indicator, period: '2026', class: cls, licence, visibility: cls === 'user' ? ('private' as const) : ('public' as const), ...(cls === 'user' ? { synthetic: true } : {}) }
}
const provenance = (id: string, t0: number) => ({ fingerprint: id, seed: 11, engine: 'lanka-stats (mock, TypeScript)', data_version: DATA_VERSION, computed_at: '2026-11-22T04:12:00Z', elapsed_ms: Math.round(performance.now() - t0) })
const outline = { color: 'ldl.tone.paper', width: 0.3 }

async function relate(req: RelateRequest, id: string): Promise<RelateResult> {
  const t0 = performance.now()
  const level = req.support.level
  const within = req.support.within ?? 'LK'
  const x = info(req.variables.x), y = info(req.variables.y)
  const controls = (req.variables.controls ?? []).map(info)
  const A = await align(level, [x, y, ...controls])
  const [tx, ty, ...tc] = A.t
  const n = A.keep.length
  const unitWord = LEVELS[level].plural
  const xs = phrase(x), ys = phrase(y)
  const estimated = A.keep.map((i) => y.indicator.includes('asking_price') && A.S.nListings[i] < 3)
  const estShare = estimated.filter(Boolean).length / (n || 1)
  const base = {
    spec: 'ldl-analysis-result/1' as const,
    id,
    method: { id: 'relate' as const, version: 'mock-0.3', citations: ['Lee, S.-I. (2001). Developing a bivariate spatial association measure. J. Geographical Systems 3.', 'Clifford, Richardson & Hémon (1989). Assessing the significance of the correlation between two spatial processes. Biometrics 45.'] },
    request: req,
    inputs: [inputOf('x', x), inputOf('y', y), ...controls.map((c) => inputOf('control', c))],
    alignment: [
      { role: 'x', from: x.indicator === 'exp.flood_pop_share' ? 'grid, 220 m' : 'indicator', to: LEVELS[level].code, method: x.alignment, coverage: 1, quality: 'B' as const },
      { role: 'y', from: y.indicator.includes('asking_price') ? 'points' : 'indicator', to: LEVELS[level].code, method: y.alignment, coverage: +(1 - estShare).toFixed(3), quality: (estShare > 0.3 ? 'D' : estShare > 0.1 ? 'C' : 'B') as 'B' | 'C' | 'D', dropped: { no_value: A.dropped } },
    ],
    n: { units: n + A.dropped, used: n, dropped: { no_value: A.dropped } },
    citation: { text: `Lanka Data Layer (2026). ${x.label} and ${y.label.toLowerCase()} across ${unitWord} of Western Province. Analysis ${id}, data version ${DATA_VERSION}.`, url: `${SITE}/a/${id}` },
  }
  // too few units to estimate anything: say so, and offer the next finer support
  if (n < 10)
    return {
      ...base,
      state: 'blocked',
      estimates: null,
      interpretation: null,
      outputs: [],
      'x-data': null,
      provenance: provenance(id, t0),
      guards: [{ code: 'MIN_N', severity: 'block', message: `${n} ${unitWord} are too few to estimate an association: a correlation needs at least ten units.`, detail: { n, min: 10 }, action: { label: `Use ${LEVELS[Math.min(4, level + 1)].plural}`, request: { support: { ...req.support, level: Math.min(4, level + 1) } } } }],
    }

  const r = st.pearson(tx, ty)
  const rho = st.spearman(tx, ty)
  const fit = st.ols(tx, ty)
  const robust = st.theilSen(tx, ty)
  await tick()

  // spatial structure: on the neighbour graph of the level, restricted to the units kept
  const nbAll = await nbOf(level)
  const pos = new Map(A.keep.map((i, k) => [i, k]))
  const nb = A.keep.map((i) => nbAll[i].map((j) => pos.get(j)).filter((j): j is number => j !== undefined))
  const P = req.params.spatial?.permutations ?? 199
  const moranX = st.moran(tx, nb, P, 5)
  await tick()
  const moranY = st.moran(ty, nb, P, 6)
  await tick()
  const residuals = Float64Array.from(tx, (v, i) => ty[i] - fit.intercept - fit.slope * v)
  const moranRes = st.moran(residuals, nb, P, 8)
  await tick()
  const L = st.lee(tx, ty, nb, P)
  const pFdr = st.fdr(L.localP)
  const cluster = L.cluster.map((c, i) => (pFdr[i] <= 0.05 ? c : 'ns'))
  await tick()
  // neighbours are not independent observations: fewer effective units, a wider interval, a larger p
  const nEff = Math.max(4, Math.min(n, Math.round((n * (1 - moranX.I * moranY.I)) / (1 + moranX.I * moranY.I))))
  let partial: RelateEstimates['x-partial'] = null
  if (tc.length && n > tc.length + 4) {
    const pr = st.pearson(st.residualise(tx, tc), st.residualise(ty, tc))
    partial = { r: pr, p_adjusted: st.pOfR(pr, nEff, tc.length), controls: controls.map(phrase) }
  }

  const scale: NonNullable<RelateEstimates['scale_sensitivity']> = [{ level, n, pearson_r: r, 'x-spearman': rho }]
  for (const l of req.params.scale_sensitivity ?? []) {
    if (l === level) continue
    const B = await align(l, [x, y])
    if (B.keep.length >= 10) scale.push({ level: l, n: B.keep.length, pearson_r: st.pearson(B.t[0], B.t[1]), 'x-spearman': st.spearman(B.t[0], B.t[1]) })
  }
  scale.sort((a, b) => a.level - b.level)

  const [rx, ry] = A.raw
  const bx = st.tercileBreaks(rx), by = st.tercileBreaks(ry)
  const cls = rx.map((v, i) => st.classIndex(ry[i], by) * 3 + st.classIndex(v, bx))
  const str = strength(rho)
  const dir = str === 'none' ? 'none' : rho < 0 ? 'negative' : 'positive'
  // a 10-point step in x, as a percentage change in y when y is logged
  const per10 = y.transform === 'log' ? (Math.exp(fit.slope * 10) - 1) * 100 : fit.slope * 10
  const spread = scale.length > 1 ? Math.max(...scale.map((s) => s['x-spearman'] ?? 0)) - Math.min(...scale.map((s) => s['x-spearman'] ?? 0)) : 0
  const pAdj = st.pOfR(r, nEff)

  const guards: Guard[] = [{ code: 'NOT_CAUSAL', severity: 'note', message: `This is an association between places, not evidence that ${xs} changes ${ys}. Low-lying land near rivers is also further from the centre and less served; any of that could account for it.` }]
  if (Math.max(moranX.I, moranY.I) > 0.25) guards.push({ code: 'RESIDUAL_AUTOCORR', severity: 'warn', message: `Neighbouring ${unitWord} resemble each other (Moran's I ${moranX.I.toFixed(2)} and ${moranY.I.toFixed(2)}), so the ${n.toLocaleString('en')} units carry the information of about ${nEff.toLocaleString('en')} independent ones. Intervals and p-values are adjusted for it.`, detail: { n_effective: nEff } })
  if (estShare > 0.05) guards.push({ code: 'ESTIMATED_INPUT', severity: 'warn', message: `${Math.round(estShare * 100)}% of the ${unitWord} have fewer than three listings; their price is modelled from the surface around them. They are hatched on the map and hollow in the scatter.`, detail: { share: estShare } })
  if (y.indicator.startsWith('@demo') || x.indicator.startsWith('@demo')) guards.push({ code: 'SYNTHETIC_INPUT', severity: 'warn', message: 'The price data is a synthetic sample made for demonstration: no open sub-district price data exists for Sri Lanka. The method is real; the finding is not.' })
  if (n < 30) guards.push({ code: 'SMALL_N', severity: 'warn', message: `With ${n} units, one unusual division can set the sign of the coefficient. Read the scatter, not the number.` })
  if (scale.length > 1) guards.push({ code: spread > 0.12 ? 'MAUP_UNSTABLE' : 'MAUP', severity: spread > 0.12 ? 'warn' : 'note', message: `The coefficient depends on the units: ${scale.map((s) => `ρ ${(s['x-spearman'] ?? 0).toFixed(2)} across ${s.n.toLocaleString('en')} ${LEVELS[s.level].plural}`).join(', ')}. The boundaries were not drawn for this question.` })
  guards.push({ code: 'ECOLOGICAL', severity: 'note', message: `The result describes ${unitWord}. It says nothing about any one property inside them.` })
  if (!controls.length) guards.push({ code: 'NO_CONTROLS', severity: 'note', message: 'Nothing is held constant. Distance to the city and density move with both variables; add them as controls to see what is left.' })

  const summary = dir === 'none' ? `No clear association between ${xs} and ${ys} across ${unitWord}.` : `${unitWord.replace(/^./, (c) => c.toUpperCase())} with more ${xs} tend to have ${dir === 'negative' ? 'lower' : 'higher'} ${ys}.`
  const reading =
    dir === 'none'
      ? `ρ = ${rho.toFixed(2)} across ${n.toLocaleString('en')} ${unitWord}.`
      : `A ${str} ${dir} association (ρ = ${rho.toFixed(2)}, r = ${r.toFixed(2)}, n = ${n.toLocaleString('en')}). Ten points more of ${xs} is associated with ${Math.abs(per10).toFixed(1)}${y.transform === 'log' ? '%' : ` ${y.unit}`} ${per10 < 0 ? 'lower' : 'higher'} ${ys}${partial ? `; with ${partial.controls.join(' and ')} held constant the correlation is ${partial.r.toFixed(2)}` : ''}.`
  const outputs: AnalysisOutput[] = [
    { id: 'bivariate', href: `/v2/analyses/${id}/outputs/bivariate`, kind: 'frame', columns: ['x', 'y', 'class', 'n_listings', 'basis'], suggested_layer: { type: 'bivariate', name: `${x.short || x.label} × ${(y.short || y.label).toLowerCase()}`, opacity: 0.9, style: { fill: { bivariate: { x: { field: 'x', scale: { type: 'quantile', classes: 3 } }, y: { field: 'y', scale: { type: 'quantile', classes: 3 } }, palette: 'ldl.biv.blue-orange' } }, stroke: outline, uncertainty: 'hatch' } } },
    { id: 'local_lee', href: `/v2/analyses/${id}/outputs/local_lee`, kind: 'frame', columns: ['L', 'p', 'p_fdr', 'cluster'], suggested_layer: { type: 'polygon', name: 'Significant local association', filter: { op: '<=', args: [{ property: 'p_fdr' }, 0.05] }, style: { fill: 'none', stroke: { width: 1.5, color: { field: 'cluster', scale: { type: 'categorical', mapping: { HH: 'ldl.tone.orange', HL: 'ldl.tone.ink', LH: 'ldl.tone.green', LL: 'ldl.tone.blue' } } } } } } },
    { id: 'scatter', href: `/v2/analyses/${id}/outputs/scatter`, kind: 'chart', encoding: n > 400 ? 'hexbin' : 'points' },
  ]
  return {
    ...base,
    state: 'ready',
    estimates: {
      pearson: { r, ci: st.rInterval(r, nEff), ci_level: 0.95, p: st.pOfR(r, n), p_adjusted: pAdj, n_effective: nEff },
      spearman: { rho, ci: st.rInterval(rho, nEff), p_adjusted: st.pOfR(rho, nEff) },
      ols: { formula: `${y.transform === 'log' ? 'log(y)' : 'y'} ~ ${x.transform === 'log' ? 'log(x)' : 'x'}`, coef: [{ term: '(intercept)', b: fit.intercept }, { term: x.label, b: fit.slope, ci: fit.slopeCi }], r2: fit.r2, adj_r2: 1 - ((1 - fit.r2) * (n - 1)) / Math.max(1, n - 2), reading },
      robust: { coef: [{ term: '(intercept)', b: robust.intercept }, { term: x.label, b: robust.slope }] },
      lee_l: { L: L.L, p_perm: L.p },
      residual_moran: { I: moranRes.I, p_perm: moranRes.p },
      scale_sensitivity: scale,
      'x-partial': partial,
      'x-moran': { x: { I: moranX.I, p_perm: moranX.p }, y: { I: moranY.I, p_perm: moranY.p } },
    },
    guards: sortGuards(guards),
    interpretation: { summary, caveats: guards.filter((g) => g.severity !== 'note').map((g) => g.message) },
    outputs,
    provenance: provenance(id, t0),
    'x-data': {
      support: { level, label: unitWord, within },
      x,
      y,
      controls,
      table: { codes: A.keep.map((i) => A.u.codes[i]), names: A.keep.map((i) => A.u.names[i]), x: rx, y: ry, n: A.keep.map((i) => A.S.nListings[i]), estimated, cls },
      breaks: { x: bx, y: by },
      fit: { ...fit, robust, per10 },
      local: { codes: A.keep.map((i) => A.u.codes[i]), L: L.local, p: L.localP, pFdr, cluster },
      significant: cluster.filter((c) => c !== 'ns').length,
      weights: req.params.spatial?.weights === 'queen' ? 'queen contiguity' : `${level === 2 ? 3 : 6} nearest neighbours`,
      permutations: P,
    },
  }
}

const zoneThreshold = (where: Cql | undefined): number => (where && (where.op === '>=' || where.op === '>') ? Number(where.args[1]) : synth.FLOOD_ZONE_M)

async function compare(req: GroupCompareRequest, id: string): Promise<CompareResult> {
  const t0 = performance.now()
  const [L, W, D, gn] = await Promise.all([synth.listings(), synth.fields('wp'), synth.zoneDistance(), synth.units(4)])
  const threshold = zoneThreshold(req.params.zone.where)
  const edges = req.params.rings_m ?? [0, 250, 500]
  const rings = edges.slice(1)
  const ids = ['inside', ...rings.map((r, i) => `${edges[i]}-${r}`), 'beyond']
  const labels = ['Inside the zone', ...rings.map((r, i) => `${edges[i]}–${r} m outside`), `Beyond ${rings[rings.length - 1]} m`]
  const groupOf = new Int8Array(L.n).fill(-1)
  const logp = new Float64Array(L.n)
  const depth = new Float32Array(L.n)
  const flood = { ...synth.WP, data: W.flood }
  for (let s = 0; s < L.n; s++) {
    if (L.gn[s] < 0 || L.use[s] !== 0) continue
    const d = synth.sample(flood, L.lon[s], L.lat[s])
    if (!Number.isFinite(d)) continue
    depth[s] = d
    logp[s] = Math.log(L.price[s] * Math.pow(1.07, 2026 - L.year[s]))
    if (d >= threshold) groupOf[s] = 0
    else {
      const m = synth.sample(D, L.lon[s], L.lat[s]) * 1000
      groupOf[s] = rings.findIndex((r) => m <= r) + 1 || rings.length + 1
    }
  }
  // compare like with like: remove each DS division's own price level first
  const fe = req.params.within_groups
  const centred = Float64Array.from(logp)
  if (fe) {
    const sum = new Map<string, [number, number]>()
    for (let s = 0; s < L.n; s++) {
      if (groupOf[s] < 0) continue
      const k = gn.parents[L.gn[s]]
      const e = sum.get(k) ?? [0, 0]
      e[0] += logp[s]
      e[1]++
      sum.set(k, e)
    }
    for (let s = 0; s < L.n; s++) {
      if (groupOf[s] < 0) continue
      const e = sum.get(gn.parents[L.gn[s]])!
      centred[s] -= e[0] / e[1]
    }
  }
  await tick()
  const members = ids.map((_, g) => Array.from({ length: L.n }, (_x, s) => s).filter((s) => groupOf[s] === g))
  const rnd = st.seeded(17)
  const groups = members.map((m, g) => {
    const prices = m.map((s) => Math.exp(logp[s]))
    const boot = Array.from({ length: 300 }, () => st.median(Array.from({ length: prices.length }, () => prices[Math.floor(rnd() * prices.length)]))).sort((a, b) => a - b)
    return { id: ids[g], label: labels[g], n: m.length, median: st.median(prices), mean: st.mean(prices), ci: (prices.length ? [st.quantile(boot, 0.025), st.quantile(boot, 0.975)] : [NaN, NaN]) as [number, number] }
  })
  await tick()
  const a = members[0].map((s) => centred[s]), b = members[1].map((s) => centred[s])
  const diff = st.mean(a) - st.mean(b)
  const ci = st.bootstrapDiff(a, b, Math.min(req.params.bootstrap ?? 1000, 1200))
  const pct = (Math.exp(diff) - 1) * 100
  const w = st.welch(a, b), mw = st.mannWhitney(a, b)
  const lo = (Math.exp(ci[0]) - 1) * 100, hi = (Math.exp(ci[1]) - 1) * 100
  const sure = hi < 0 || lo > 0
  const within = fe ? LEVELS[fe.level].name : null
  const guards: Guard[] = [
    { code: 'NOT_CAUSAL', severity: 'note', message: 'Listings just outside the zone share a neighbourhood with those inside it, which removes most of the location effect; sellers and plots still differ in ways this sample does not record.' },
    { code: 'SYNTHETIC_INPUT', severity: 'warn', message: 'The listings are a synthetic sample. Asking prices are also not sale prices: a flood-prone plot may simply sit unsold at an optimistic figure.' },
  ]
  if (Math.min(a.length, b.length) < 40) guards.push({ code: 'SMALL_N', severity: 'warn', message: `Only ${Math.min(a.length, b.length)} listings in the smaller group: the interval is wide. Widen the rings to bring more in.` })
  const points: FeatureCollection<Point> = {
    type: 'FeatureCollection',
    features: members.slice(0, rings.length + 1).flatMap((m, g) =>
      m.map((s) => ({ type: 'Feature' as const, properties: { id: s, ring: g === 0 ? 'inside' : ids[g], price_per_perch: L.price[s], depth_m: +depth[s].toFixed(2) }, geometry: { type: 'Point' as const, coordinates: [+L.lon[s].toFixed(5), +L.lat[s].toFixed(5)] } })),
    ),
  }
  const sgn = (v: number) => `${v > 0 ? '+' : '−'}${Math.abs(v).toFixed(1)}%`
  return {
    spec: 'ldl-analysis-result/1',
    id,
    state: 'ready',
    method: { id: 'group_compare', version: 'mock-0.3', citations: ['Welch, B. L. (1947). The generalization of Student\'s problem. Biometrika 34.', 'Efron & Tibshirani (1993). An Introduction to the Bootstrap.'] },
    request: req,
    inputs: [
      { role: 'points', dataset: '@demo/property-asking-prices-sample', dsv: 'dsv_3a91f0e2c7d45b10', field: 'price_per_perch', period: '2022–2026', class: 'user', licence: 'workspace data', visibility: 'private', synthetic: true },
      { role: 'zone', dataset: 'flood-hazard', dsv: 'dsv_7ab2c94e10f35d66', indicator: 'haz.flood_depth', period: '2026', class: 'estimated', licence: 'CC-BY-4.0', visibility: 'public' },
    ],
    alignment: [{ role: 'zone', from: 'grid, 220 m', to: 'points', method: `sample · depth ≥ ${threshold} m, then distance to the zone edge`, coverage: 1, quality: 'C' }],
    n: { points: groups.reduce((s, g) => s + g.n, 0), used: a.length + b.length, dropped: { beyond_rings: groups[groups.length - 1].n } },
    estimates: {
      groups,
      contrasts: [{ a: ids[0], b: ids[1], diff: pct, ci: [lo, hi], p_welch: w.p, p_mann_whitney: mw.p, reading: `${sgn(pct)} (95% interval ${sgn(lo)} to ${sgn(hi)}), comparing ${a.length.toLocaleString('en')} listings inside the zone with ${b.length.toLocaleString('en')} within ${rings[0]} m of its edge${within ? `, inside the same ${within}` : ''}.` }],
    },
    guards: sortGuards(guards),
    interpretation: { summary: sure ? `Listings inside the flood zone ask ${Math.abs(pct).toFixed(0)}% ${pct < 0 ? 'less' : 'more'} than those just outside it.` : 'No clear price difference between listings inside the flood zone and those just outside it.', caveats: guards.filter((g) => g.severity !== 'note').map((g) => g.message) },
    outputs: [
      {
        id: 'points', href: `/v2/analyses/${id}/outputs/points`, kind: 'features',
        fields: [{ id: 'ring', label: 'Position', type: 'category' }, { id: 'price_per_perch', label: 'Asking price', type: 'number', unit: 'LKR/perch' }, { id: 'depth_m', label: 'Flood depth', type: 'number', unit: 'm' }],
        suggested_layer: { type: 'point', name: 'Listings inside and near the flood zone', opacity: 0.95, style: { size: 3.4, fill: { field: 'ring', scale: { type: 'categorical', mapping: { inside: 'ldl.tone.blue', '0-250': 'ldl.tone.orange', '250-500': 'ldl.tone.grey' } } }, stroke: { color: 'ldl.tone.paper', width: 0.7 } } },
      },
    ],
    provenance: provenance(id, t0),
    citation: { text: `Lanka Data Layer (2026). Asking prices inside and near the modelled flood zone, Western Province. Analysis ${id}, data version ${DATA_VERSION}.`, url: `${SITE}/a/${id}` },
    'x-data': { unit: 'LKR/perch', threshold, within, points },
  }
}
