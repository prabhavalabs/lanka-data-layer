import { ArrowRight } from 'lucide-react'
import { Link, useNavigate } from 'react-router'
import type { AreaProfile, RankItem } from '@/api/types'
import { Legend } from '@/components/charts/chartKit'
import { RankStrip } from '@/components/charts/RankStrip'
import { Eyebrow, Reveal } from '@/components/common/bits'
import { fmt1, fmtInt } from '@/lib/format'
import { LEVEL } from './AreaHeader'

const ordinal = (n: number) => {
  const s = ['th', 'st', 'nd', 'rd']
  const v = n % 100
  return `${n}${s[(v - 20) % 10] ?? s[v] ?? s[0]}`
}
export const rankFmt = (unit: string) => (v: number) => (unit === '%' ? `${v.toFixed(1)}%` : unit === 'min' ? fmt1(v) : fmtInt(v))

function RankCell({ item, profile }: { item: RankItem; profile: AreaProfile }) {
  const navigate = useNavigate()
  const { area } = profile
  const fmt = rankFmt(item.unit)
  return (
    <div className="flex min-w-0 flex-col bg-bg2 p-4 sm:p-5">
      <div className="flex items-start justify-between gap-3">
        <h3 className="text-[13px] font-medium text-ink">{item.label}</h3>
        <Link to={`/explore?indicator=${item.indicator}&area=${area.pcode}`} aria-label={`Open ${item.label} on the map`} className="pressable -m-1.5 grid size-7 shrink-0 place-items-center rounded-md text-ink3 hover:bg-bg3 hover:text-ink">
          <ArrowRight className="size-3.5" />
        </Link>
      </div>
      <p className="mt-2 flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <span className="t-num text-[22px] leading-none font-semibold tracking-[-0.03em] text-ink">{fmt(item.value)}</span>
        {item.unit !== '%' && <span className="text-[12px] text-ink2">{item.unit}</span>}
        <span className="t-num ml-auto font-mono text-[11px] text-ink3">{area.level === 0 ? 'national value' : `${ordinal(item.rank)} highest of ${item.of}`}</span>
      </p>
      <div className="mt-3">
        <RankStrip points={item.siblings} current={area.pcode} value={item.value} national={item.national} fmt={fmt} label={item.label} onPick={(p) => navigate(`/area/${p}`)} />
      </div>
    </div>
  )
}

export function RankSection({ profile }: { profile: AreaProfile }) {
  const { area } = profile
  const sib = area.level === 0 ? 'province' : LEVEL[area.level].toLowerCase()
  return (
    <section aria-labelledby="ranks-title">
      <Reveal>
        <div className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3">
          <div>
            <Eyebrow>Standing</Eyebrow>
            <h2 id="ranks-title" className="t-h3 mt-3">
              {area.level === 0 ? 'How the provinces spread around the national figure' : `How ${area.name.en} sits among ${profile.ranks[0]?.of ?? ''} ${sib}s`}
            </h2>
          </div>
          <Legend
            items={[
              { label: area.level === 0 ? 'Sri Lanka' : area.name.en, color: 'var(--brand)', kind: 'dot' },
              { label: `Other ${sib}s`, color: 'color-mix(in oklab, var(--ink3) 55%, transparent)', kind: 'dot' },
              ...(area.level === 0 ? [] : [{ label: 'National', color: 'var(--ink2)', kind: 'tick' as const }]),
            ]}
          />
        </div>
        <div className="mt-5 grid grid-cols-1 gap-px overflow-hidden rounded-[14px] border border-line bg-line shadow-[var(--shadow-sm)] sm:grid-cols-2 lg:grid-cols-3">
          {profile.ranks.map((r) => (
            <RankCell key={r.indicator} item={r} profile={profile} />
          ))}
        </div>
      </Reveal>
    </section>
  )
}
