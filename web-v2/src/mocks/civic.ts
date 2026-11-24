// SAMPLE DATA for the Government view, used while the engine's `/v2/civic` endpoints are not reachable.
//
// Real instruments and real institution names, but a subset: each baseline carries 8 to 9 of the ~24 ministries, and
// the 2403/53 schedule is illustrative (it is not parsed from the gazette). The timeline's earlier baselines are
// real gazette numbers with approximate dates and no parsed schedule. Everything this file returns is flagged
// `sample` by `src/api/civic.ts`, and the page marks it.
import { ApiError } from '@/api/http'
import type { AmendmentChange, AmendmentChanges, Diff, EntityDetail, EntityHit, EntityKind, GazetteDocument, Instrument, Structure, StructureInstitution, StructurePortfolio } from '@/api/civic'

/* ---------------- keys, as docs/architecture-v2/13-civic-record.md §2.1 defines them ---------------- */
const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '')
export function entityKey(name: string): string {
  return slug(
    name
      .toLowerCase()
      .replace(/&/g, 'and')
      .replace(/\(pvt\)\s*(?:ltd\.?)?|\bltd\.?(?=\W|$)/g, 'limited')
      .replace(/^the\s+/, ''),
  )
}
function guessKind(name: string): EntityKind {
  if (/^(sri lanka (army|navy|air force|police)|office of the chief of defence staff)$/i.test(name)) return 'force'
  if (/^university|university$/i.test(name)) return 'university'
  if (/^department|^office|^state intelligence|^civil security|^national cadet|^sri lanka customs/i.test(name)) return 'department'
  if (/ fund$/i.test(name)) return 'fund'
  if (/\b(limited|plc)\b|\(private\)|\bltd\b/i.test(name)) return 'company'
  if (/corporation$/i.test(name)) return 'corporation'
  if (/^(bank of ceylon|people's bank|sri lanka ports authority)$/i.test(name)) return /bank/i.test(name) ? 'corporation' : 'statutory_body'
  if (/(authority|board|commission|council|agency|bureau|institute|college|centre|team|foundation)$/i.test(name)) return 'statutory_body'
  return 'other'
}

/* ---------------- the baselines ---------------- */
type Min = { title: string; fn: string[]; inst: string[] }
const FN = (subject: string) => [
  `Formulation, implementation, monitoring and evaluation of policies, strategies, programmes and projects in relation to ${subject} and those subjects that come under the purview of the Departments, Statutory Institutions and Public Corporations listed in Column II.`,
  'Provision of relevant public services under the purview of the Ministry in an efficient and people friendly manner.',
  `Reforming all systems and procedures using modern management techniques and technology, ensuring that the functions of the Ministry relating to ${subject} are fulfilled while eliminating corruption and waste.`,
]

const MEDIA = ['Department of Government Information', 'Sri Lanka Rupavahini Corporation', 'Sri Lanka Broadcasting Corporation', 'Independent Television Network Limited']
const TOURISM = ['Sri Lanka Tourism Development Authority', 'Sri Lanka Tourism Promotion Bureau', 'Sri Lanka Institute of Tourism and Hotel Management']
const POLICING = ['Sri Lanka Police', 'Department of Immigration and Emigration', 'Department of Registration of Persons', 'National Dangerous Drugs Control Board']
const DEFENCE = [
  'Office of the Chief of Defence Staff',
  'Sri Lanka Army',
  'Sri Lanka Navy',
  'Sri Lanka Air Force',
  'State Intelligence Service',
  'Civil Security Department',
  'National Cadet Corps',
  'National Authority for the Implementation of Chemical Weapons Convention',
  'Sir John Kotelawala Defence University',
  'Defence Services Command and Staff College',
  'Department of Meteorology',
  'Disaster Management Centre',
  'Defence Services College',
]
const FINANCE = [
  'Department of Fiscal Policy',
  'Department of National Planning',
  'Department of External Resources',
  'Department of Management Services',
  'Department of Inland Revenue',
  'Sri Lanka Customs',
  'Securities and Exchange Commission of Sri Lanka',
  'Insurance Regulatory Commission of Sri Lanka',
  'Bank of Ceylon',
  "People's Bank",
]
const ENERGY = [
  'Ceylon Electricity Board',
  'Ceylon Petroleum Corporation',
  'Ceylon Petroleum Storage Terminals Limited',
  'Lanka Coal Company (Pvt) Ltd',
  'Sri Lanka Sustainable Energy Authority',
  'Public Utilities Commission of Sri Lanka',
  'Atomic Energy Board',
  'Lanka Electricity Company (Private) Limited',
]
const EDUCATION = [
  'Department of Examinations',
  'Department of Educational Publications',
  'National Institute of Education',
  'National Education Commission',
  'University Grants Commission',
  'Tertiary and Vocational Education Commission',
  'Vocational Training Authority of Sri Lanka',
  'National Apprentice and Industrial Training Authority',
  'University of Colombo',
  'University of Peradeniya',
  'Mahapola Higher Education Scholarship Trust Fund',
]
const HEALTH = [
  'Medical Research Institute',
  'National Medicines Regulatory Authority',
  'State Pharmaceuticals Corporation of Sri Lanka',
  'State Pharmaceuticals Manufacturing Corporation of Sri Lanka',
  'Sri Lanka Medical Council',
  'National Institute of Health Sciences',
]
const TRANSPORT = [
  'Department of Motor Traffic',
  'Road Development Authority',
  'Sri Lanka Transport Board',
  'Department of Sri Lanka Railways',
  'National Transport Commission',
  'National Council for Road Safety',
  'Sri Lanka Ports Authority',
  'Civil Aviation Authority of Sri Lanka',
]

const B2403: Min[] = [
  { title: 'Defence', fn: FN('defence and internal security'), inst: [...DEFENCE, ...POLICING] },
  { title: 'Finance, Planning and Economic Development', fn: FN('finance, planning and economic development'), inst: [...FINANCE, 'Airport and Aviation Services (Sri Lanka) Limited', 'SriLankan Airlines Limited'] },
  { title: 'Energy', fn: FN('energy'), inst: ENERGY },
  { title: 'Education, Higher Education and Vocational Education', fn: FN('education'), inst: EDUCATION },
  { title: 'Health', fn: FN('health'), inst: [...HEALTH, 'Lanka Hospitals Corporation PLC'] },
  { title: 'Transport, Highways and Ports', fn: FN('transport and highways'), inst: [...TRANSPORT, ...TOURISM] },
  {
    title: 'Technology',
    fn: FN('technology and telecommunications'),
    inst: ['Information and Communication Technology Agency of Sri Lanka', 'Telecommunications Regulatory Commission of Sri Lanka', 'Sri Lanka Computer Emergency Readiness Team', 'Sri Lanka Telecom PLC', ...MEDIA],
  },
  { title: 'Foreign Affairs', fn: FN('foreign affairs'), inst: ['Bandaranaike International Diplomatic Training Institute', 'Lakshman Kadirgamar Institute of International Relations and Strategic Studies', 'Sri Lanka Foundation'] },
]

const B2412: Min[] = [
  { title: 'Defence', fn: FN('defence and disaster management'), inst: DEFENCE },
  { title: 'Finance, Planning and Economic Development', fn: FN('finance, planning and economic development'), inst: FINANCE },
  { title: 'Energy', fn: FN('energy'), inst: ENERGY },
  { title: 'Education, Higher Education and Vocational Education', fn: FN('education'), inst: EDUCATION },
  { title: 'Health and Mass Media', fn: FN('health and mass media'), inst: [...HEALTH, ...MEDIA] },
  { title: 'Transport, Highways, Ports and Civil Aviation', fn: FN('transport, highways, ports and civil aviation'), inst: [...TRANSPORT, 'Airport and Aviation Services (Sri Lanka) Limited', 'SriLankan Airlines Limited'] },
  {
    title: 'Digital Economy',
    fn: FN('the digital economy'),
    inst: ['Information and Communication Technology Agency of Sri Lanka', 'Telecommunications Regulatory Commission of Sri Lanka', 'Sri Lanka Computer Emergency Readiness Team', 'Lanka Government Network (Pvt) Ltd'],
  },
  { title: 'Public Security and Parliamentary Affairs', fn: FN('public security'), inst: POLICING },
  {
    title: 'Foreign Affairs, Foreign Employment and Tourism',
    fn: FN('foreign affairs, foreign employment and tourism'),
    inst: ['Bandaranaike International Diplomatic Training Institute', 'Lakshman Kadirgamar Institute of International Relations and Strategic Studies', 'Sri Lanka Foundation', 'Sri Lanka Bureau of Foreign Employment', ...TOURISM],
  },
]
/** The ministries that were renamed rather than replaced, as the engine's `portfolios.kept` reports them. */
const RENAMED: Record<string, string> = { Health: 'Health and Mass Media', 'Transport, Highways and Ports': 'Transport, Highways, Ports and Civil Aviation', 'Foreign Affairs': 'Foreign Affairs, Foreign Employment and Tourism' }

type Doc = { id: string; number: string; date: string; kind: 'baseline' | 'amendment'; amends: string | null; head: string; schedule: Min[] | null; issued?: string }
const DOCS: Doc[] = [
  { id: 'gze-2458-65', number: '2458/65', date: '2025-10-17', kind: 'amendment', amends: 'gze-2412-08', head: 'Dissanayake', schedule: null, issued: '2025-10-16' },
  { id: 'gze-2412-08', number: '2412/08', date: '2024-11-25', kind: 'baseline', amends: null, head: 'Dissanayake', schedule: B2412 },
  { id: 'gze-2403-53', number: '2403/53', date: '2024-09-27', kind: 'baseline', amends: null, head: 'Dissanayake', schedule: B2403 },
  { id: 'gze-2289-43', number: '2289/43', date: '2022-07-22', kind: 'baseline', amends: null, head: 'Wickremesinghe', schedule: null },
  { id: 'gze-2187-27', number: '2187/27', date: '2020-08-09', kind: 'baseline', amends: null, head: 'G. Rajapaksa', schedule: null },
  { id: 'gze-2153-12', number: '2153/12', date: '2019-12-10', kind: 'baseline', amends: null, head: 'G. Rajapaksa', schedule: null },
  { id: 'gze-2103-33', number: '2103/33', date: '2018-12-20', kind: 'baseline', amends: null, head: 'Sirisena', schedule: null },
  { id: 'gze-2096-68', number: '2096/68', date: '2018-10-29', kind: 'baseline', amends: null, head: 'Sirisena', schedule: null },
  { id: 'gze-2022-34', number: '2022/34', date: '2017-09-20', kind: 'baseline', amends: null, head: 'Sirisena', schedule: null },
  { id: 'gze-1933-13', number: '1933/13', date: '2015-09-21', kind: 'baseline', amends: null, head: 'Sirisena', schedule: null },
  { id: 'gze-1897-15', number: '1897/15', date: '2015-01-12', kind: 'baseline', amends: null, head: 'Sirisena', schedule: null },
  { id: 'gze-1651-20', number: '1651/20', date: '2010-04-23', kind: 'baseline', amends: null, head: 'Rajapaksa', schedule: null },
]

const titleOf = (d: Doc) => (d.kind === 'amendment' ? `Extraordinary Gazette ${d.number}: amendment to the Article 44 schedule of 2412/08` : `Extraordinary Gazette ${d.number}: subjects, functions and institutions of Ministers, Article 44(1)`)
const countInst = (d: Doc) => d.schedule?.reduce((n, m) => n + m.inst.length, 0) ?? 0

function build(d: Doc): StructurePortfolio[] {
  let page = 3
  return (d.schedule ?? []).map((m, i) => {
    const first = page
    page += Math.ceil(m.inst.length / 6) + 1
    return {
      seq: i + 1,
      doc_id: d.id,
      title: `Minister of ${m.title}`,
      short_title: m.title,
      portfolio_key: slug(m.title),
      page: first,
      functions: m.fn.map((text, k) => ({ item_no: k + 1, text })),
      institutions: m.inst.map<StructureInstitution>((text, k) => ({ item_no: k + 1, text, entity_key: entityKey(text), kind: guessKind(text), page: first + Math.floor(k / 6) })),
      laws: [],
    }
  })
}
const parsed = DOCS.filter((d) => d.kind === 'baseline' && d.schedule)
const BUILT = new Map(parsed.map((d) => [d.id, build(d)]))

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms))
const lag = () => wait(120 + Math.random() * 220)

const baselineAt = (at: string): Doc | undefined => parsed.find((d) => d.date <= at)

export const mockCivic = {
  async instruments(): Promise<Instrument[]> {
    await lag()
    return DOCS.map((d) => ({
      doc_id: d.id,
      kind: d.kind,
      effective_from: d.date,
      amends: d.amends,
      head_of_state: d.head,
      status: d.kind === 'amendment' || d.schedule ? 'validated' : 'needs_review',
      portfolios: d.schedule?.length ?? 0,
      institutions: countInst(d),
      title: titleOf(d),
      number: d.number,
    }))
  },

  async structure(at: string): Promise<Structure> {
    await lag()
    const base = baselineAt(at)
    if (!base) throw new ApiError(`No parsed schedule in force on ${at}`, 404, { problem: { code: 'not_in_coverage', earliest: parsed[parsed.length - 1].date } })
    return {
      at,
      instrument: { doc_id: base.id, effective_from: base.date, number: base.number },
      composed_of: [{ doc_id: base.id, effective_from: base.date }],
      amendments_since: DOCS.filter((d) => d.amends === base.id && d.date <= at && d.date > base.date).map((d) => ({ doc_id: d.id, effective_from: d.date, title: titleOf(d) })),
      portfolios: BUILT.get(base.id) ?? [],
    }
  },

  async diff(from: string, to: string): Promise<Diff> {
    await lag()
    const a = baselineAt(from)
    const b = baselineAt(to)
    if (!a || !b) throw new ApiError('No parsed schedule in force on one of the dates', 404, { problem: { code: 'not_in_coverage', earliest: parsed[parsed.length - 1].date } })
    const index = (id: string) => {
      const m = new Map<string, { name: string; kind: EntityKind; ref: { portfolio_key: string; title: string; short_title: string; page: number | null } }>()
      for (const p of BUILT.get(id) ?? []) for (const i of p.institutions) m.set(i.entity_key!, { name: i.text, kind: i.kind, ref: { portfolio_key: p.portfolio_key, title: p.title, short_title: p.short_title!, page: i.page } })
      return m
    }
    const A = index(a.id)
    const B = index(b.id)
    const moved: Diff['moved'] = []
    const added: Diff['added'] = []
    const removed: Diff['removed'] = []
    for (const [k, v] of B) {
      const w = A.get(k)
      if (!w) added.push({ entity_key: k, name: v.name, kind: v.kind, to: v.ref })
      else if (w.ref.portfolio_key !== v.ref.portfolio_key) moved.push({ entity_key: k, name: v.name, kind: v.kind, from: w.ref, to: v.ref })
    }
    for (const [k, v] of A) if (!B.has(k)) removed.push({ entity_key: k, name: v.name, kind: v.kind, from: v.ref })
    // a renamed ministry is not a move: drop pairs the schedule's own rename table explains
    const real = moved.filter((m) => RENAMED[m.from.short_title!] !== m.to.short_title)
    const ta = (a.schedule ?? []).map((m) => m.title)
    const tb = (b.schedule ?? []).map((m) => m.title)
    const kept: Diff['portfolios']['kept'] = ta.filter((t) => tb.includes(t)).map((t) => ({ from_title: `Minister of ${t}`, to_title: `Minister of ${t}`, from_key: slug(t), to_key: slug(t), overlap: 1, renamed: false }))
    for (const t of ta) if (RENAMED[t] && tb.includes(RENAMED[t])) kept.push({ from_title: `Minister of ${t}`, to_title: `Minister of ${RENAMED[t]}`, from_key: slug(t), to_key: slug(RENAMED[t]), overlap: 0.8, renamed: true })
    const keptFrom = new Set(kept.map((k) => k.from_title.replace('Minister of ', '')))
    const keptTo = new Set(kept.map((k) => k.to_title.replace('Minister of ', '')))
    const ref = (d: Doc) => ({ doc_id: d.id, effective_from: d.date, number: d.number })
    return {
      from: ref(a),
      to: ref(b),
      moved: real,
      added,
      removed,
      portfolios: { added: tb.filter((t) => !keptTo.has(t)).map((t) => `Minister of ${t}`), removed: ta.filter((t) => !keptFrom.has(t)).map((t) => `Minister of ${t}`), kept },
      counts: { moved: real.length, added: added.length, removed: removed.length, portfolios_added: tb.filter((t) => !keptTo.has(t)).length, portfolios_removed: ta.filter((t) => !keptFrom.has(t)).length, portfolios_renamed: kept.filter((k) => k.renamed).length },
    }
  },

  async entities(q: string): Promise<EntityHit[]> {
    await lag()
    const needle = q.trim().toLowerCase()
    const latest = BUILT.get(parsed[0].id) ?? []
    const out = new Map<string, EntityHit>()
    for (const d of parsed)
      for (const p of BUILT.get(d.id) ?? [])
        for (const i of p.institutions) {
          if (!i.text.toLowerCase().includes(needle) || out.has(i.entity_key!)) continue
          const cur = latest.find((l) => l.institutions.some((x) => x.entity_key === i.entity_key))
          out.set(i.entity_key!, { entity_key: i.entity_key!, name: i.text, kind: i.kind, current: cur ? { portfolio_title: cur.title.replace(/^Minister of /, '') } : null })
        }
    return [...out.values()].slice(0, 8)
  },

  async entity(key: string): Promise<EntityDetail> {
    await lag()
    const rows: EntityDetail['history'] = []
    let name = ''
    let kind: EntityKind = 'other'
    // oldest first, so consecutive baselines under one ministry join into one continuous row
    for (let i = parsed.length - 1; i >= 0; i--) {
      const d = parsed[i]
      for (const p of BUILT.get(d.id) ?? []) {
        const it = p.institutions.find((x) => x.entity_key === key)
        if (!it) continue
        name = it.text
        kind = it.kind
        const title = p.title.replace(/^Minister of /, '')
        const last = rows[rows.length - 1]
        if (last && last.portfolio_key === p.portfolio_key && last.valid_to) last.valid_to = i > 0 ? dayBefore(parsed[i - 1].date) : null
        else rows.push({ portfolio_title: title, portfolio_key: p.portfolio_key, valid_from: d.date, valid_to: i > 0 ? dayBefore(parsed[i - 1].date) : null, doc_id: d.id, page: it.page })
      }
    }
    if (!rows.length) throw new ApiError(`No such institution: ${key}`, 404, null)
    return { entity_key: key, name, kind, aliases: [], history: rows.reverse() }
  },

  /** A few real changes of 2458/65 (2025), as the engine reports them. */
  async changes(docId: string): Promise<AmendmentChanges> {
    await lag()
    if (docId !== 'gze-2458-65') throw new ApiError(`${docId} is not an extracted amendment`, 404, { problem: { code: 'not_found' } })
    const c = (n: number, clause: string, no: number, title: string, op: AmendmentChange['op'], col: AmendmentChange['col'], page: number, text: string, note: string, extra: Partial<AmendmentChange> = {}): AmendmentChange => ({
      change_no: n,
      clause,
      heading_no: no,
      heading_title: title,
      op,
      col,
      lang_only: null,
      page,
      text,
      applied: true,
      ok: true,
      note,
      ...extra,
    })
    return {
      doc_id: docId,
      number: '2458/65',
      effective_from: '2025-10-17',
      amends: 'gze-2412-08',
      status: 'needs_review',
      checks: [
        { check: 'changes_read', ok: true, detail: '83 change(s) read' },
        { check: 'changes_applied', ok: false, detail: '71 of 83 change(s) applied; 10 made in the Sinhala or Tamil text only; 2 to review' },
      ],
      changes: [
        c(1, '(1) (b)', 1, 'Minister of Defence', 'insert', 'institution', 2, 'In Column II thereof, by insertion of the following item immediately after item No. 25;', '2 item(s) inserted in Column II'),
        c(2, '(1) (c)', 1, 'Minister of Defence', 'omit', 'institution', 2, 'In Column II thereof, by omitting following item;', '1 item(s) omitted from Column II'),
        c(3, '(2) (e)', 2, 'Minister of Finance, Planning and Economic Development', 'insert', 'institution', 2, 'In Column II thereof, by insertion of the following item immediately after item No. 55;', '1 item(s) inserted in Column II'),
        c(4, '(4) (a)', 4, 'Minister of Agriculture, Livestock, Land, and Irrigation', 'insert', 'institution', 3, 'In Column II thereof, by insertion of the following item immediately after item No. 11;', '1 item(s) inserted in Column II'),
        c(5, '(1) (d)', 1, 'Minister of Defence', 'omit', 'law', 2, 'In Column III (only in Tamil Notification) thereof, by omitting following item;', 'made in the Tamil text only', { applied: false, lang_only: 'ta' }),
        c(6, '(8) (e)', 9, 'Minister of Health and Mass Media', 'substitute', 'law', 5, 'In Column III thereof, by substitution of following item instead of item “Homoeopathy Act, No. 07 of 2070”;', 'no item reads “Homoeopathy Act, No. 07 of 2070”, to substitute', { applied: false, ok: false }),
      ],
    }
  },

  async document(docId: string): Promise<GazetteDocument> {
    await lag()
    const d = DOCS.find((x) => x.id === docId)
    if (!d) throw new ApiError(`No such document: ${docId}`, 404, null)
    return {
      doc_id: d.id,
      number: d.number,
      issued_on: d.issued ?? d.date,
      title: titleOf(d),
      files: [{ lang: 'en', status: 'archived', pages: 40, bytes: 1_400_000, sha256: '' }],
      source_url: `https://documents.gov.lk/view/extra-gazettes/${d.number.replace('/', '-')}_E.pdf`,
    }
  },
}

function dayBefore(iso: string): string {
  const d = new Date(`${iso}T00:00:00Z`)
  d.setUTCDate(d.getUTCDate() - 1)
  return d.toISOString().slice(0, 10)
}
