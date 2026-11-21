import { ArrowUpRight, Info } from 'lucide-react'
import { Link } from 'react-router'
import type { AreaProfile } from '@/api/types'
import { CodeBlock } from '@/components/common/CodeBlock'
import { Eyebrow, LicenceLight, PcodePlate, Reveal } from '@/components/common/bits'
import { API, SITE } from '@/lib/site'
import { LEVEL } from './AreaHeader'

export function PostalAndNeighbours({ profile }: { profile: AreaProfile }) {
  const { area, postal, neighbours } = profile
  return (
    <Reveal className="grid grid-cols-1 gap-x-12 gap-y-10 lg:grid-cols-2">
      <section aria-labelledby="postal-title">
        <Eyebrow>Postal codes</Eyebrow>
        <h2 id="postal-title" className="mt-3 text-[15px] font-semibold text-ink">
          Main post offices in {area.name.en}
        </h2>
        <ul className="mt-4 flex flex-wrap gap-2">
          {postal.map((p) => (
            <li key={p.code} className="inline-flex h-8 items-center gap-2 rounded-lg border border-line bg-bg2 pr-3 pl-1 text-[13px] text-ink2">
              <span className="t-num rounded-[5px] bg-bg3 px-1.5 py-0.5 font-mono text-[11.5px] font-medium text-ink">{p.code}</span>
              {p.name}
            </li>
          ))}
        </ul>
      </section>
      <section aria-labelledby="neighbours-title">
        <Eyebrow>Neighbours</Eyebrow>
        <h2 id="neighbours-title" className="mt-3 text-[15px] font-semibold text-ink">
          Nearest {area.level === 0 ? 'provinces' : `${LEVEL[area.level].toLowerCase()}s`}
        </h2>
        <ul className="mt-4 flex flex-wrap gap-2">
          {neighbours.map((n) => (
            <li key={n.pcode}>
              <Link to={`/area/${n.pcode}`} className="pressable inline-flex h-8 items-center gap-2 rounded-lg border border-line bg-bg2 pr-3 pl-1 text-[13px] font-medium text-ink hover:border-ink3/60 hover:bg-bg3">
                <PcodePlate code={n.pcode} size="sm" />
                {n.name}
              </Link>
            </li>
          ))}
        </ul>
      </section>
    </Reveal>
  )
}

const LIMITS = [
  '2024 census figures are provisional and will be revised when the final tables are published.',
  'Travel times are modelled on the road network at free-flow speeds; real journeys at peak hours take longer.',
  'Land cover is classified from 10 m satellite imagery; narrow features such as roads and canals are under-counted.',
  'Election results are reported by polling division and apportioned to this unit, so small differences from official totals are expected.',
]

export function SourcesAndLimits({ profile }: { profile: AreaProfile }) {
  const { area, meta } = profile
  const url = `${SITE}/area/${area.pcode}`
  const cite = `Lanka Data Layer (2026). ${area.name.en} ${LEVEL[area.level].toLowerCase()} profile, ${area.pcode}. Data version ${meta.data_version}. ${url}`
  const embed = `<iframe src="${url}/embed?v=${meta.data_version}"\n  width="100%" height="520" loading="lazy"\n  title="${area.name.en} — Lanka Data Layer"></iframe>`
  return (
    <Reveal as="section" className="surface ticks shadow-[var(--shadow-sm)]">
      <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)]">
        <div className="p-5 sm:p-7">
          <Eyebrow>Sources and limits</Eyebrow>
          <h2 className="t-h3 mt-3">Where these numbers come from, and where they stop</h2>
          <ul className="mt-5 divide-y divide-line border-y border-line">
            {meta.source.map((s) => (
              <li key={s.title} className="flex items-start justify-between gap-4 py-3">
                <div className="min-w-0">
                  <a href={s.url} target="_blank" rel="noreferrer" className="group inline-flex items-center gap-1 text-[14px] font-medium text-ink hover:text-brand-ink">
                    <span className="link-draw">{s.title}</span>
                    <ArrowUpRight className="size-3.5 shrink-0 text-ink3 group-hover:text-brand-ink" />
                  </a>
                  <div className="mt-0.5 text-[12.5px] text-ink2">{s.publisher}</div>
                </div>
              </li>
            ))}
          </ul>
          <div className="mt-5 flex flex-col gap-2.5">
            {meta.licences.map((l) => (
              <div key={l.id} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[12.5px] text-ink2">
                <LicenceLight status={l.status} />
                <span className="font-mono text-[11.5px] text-ink">{l.id}</span>
                <span className="min-w-0">{l.attribution}</span>
              </div>
            ))}
          </div>
          <p className="mt-5 flex items-start gap-2 rounded-lg border border-line bg-bg3/60 px-3 py-2.5 text-[12.5px] leading-relaxed text-ink2">
            <Info className="mt-0.5 size-3.5 shrink-0 text-ink3" />
            <span>
              <span className="font-medium text-ink">Completeness: {meta.completeness.status}.</span> {meta.completeness.note ?? 'All indicators on this page are available for this unit.'}
            </span>
          </p>
          <h3 className="mt-7 text-[13px] font-medium text-ink">Read these numbers with care</h3>
          <ul className="mt-2.5 space-y-2 text-[13.5px] leading-relaxed text-ink2">
            {LIMITS.map((l) => (
              <li key={l} className="flex gap-2.5">
                <span aria-hidden className="mt-[9px] h-px w-3 shrink-0 bg-ink3" />
                {l}
              </li>
            ))}
          </ul>
        </div>
        <div className="min-w-0 border-line p-5 max-lg:border-t sm:p-7 lg:border-l">
          <Eyebrow>Embed / cite</Eyebrow>
          <h3 className="mt-3 text-[15px] font-semibold text-ink">Take this page with you</h3>
          <p className="mt-1.5 text-[13.5px] text-ink2">Pinned to data version {meta.data_version}, so a citation keeps pointing at the numbers you read.</p>
          <CodeBlock className="mt-5 [&_pre]:break-words [&_pre]:whitespace-pre-wrap" lang="text" title="cite" code={cite} />
          <CodeBlock className="mt-3" lang="text" title="embed.html" code={embed} />
          <CodeBlock className="mt-3" lang="bash" title="api" code={`curl ${API}/areas/${area.pcode}`} />
        </div>
      </div>
    </Reveal>
  )
}
