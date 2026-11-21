import { useState } from 'react'
import type { AreaProfile } from '@/api/types'
import { Legend } from '@/components/charts/chartKit'
import { ClimateBars } from '@/components/charts/ClimateBars'
import { LineChart } from '@/components/charts/LineChart'
import { Pyramid } from '@/components/charts/Pyramid'
import { AccessBars, ShareBars } from '@/components/charts/RowBars'
import { StackBar } from '@/components/charts/StackBar'
import { fmtCompact, fmtInt, fmtSigned } from '@/lib/format'
import { useTheme } from '@/lib/hooks'
import { categorical, ramp } from '@/lib/palettes'
import { ChartTitle, Fact, MiniTable, SectionCard, ViewToggle, type View } from './SectionCard'

const sum = (xs: number[]) => xs.reduce((s, x) => s + x, 0)
const pct = (v: number, d = 1) => `${v.toFixed(d)}%`

/* ------------------------------ People ------------------------------ */
export function PeopleSection({ profile }: { profile: AreaProfile }) {
  const [theme] = useTheme()
  const [view, setView] = useState<View>('chart')
  const p = ramp('people', theme)
  const male = p[3]
  const female = p[5]
  const { pyramid, popSeries, area } = profile
  const old = pyramid.slice(13)
  const young = pyramid.slice(0, 3)
  const old24 = sum(old.map((r) => r.m + r.f))
  const old12 = sum(old.map((r) => r.m0 + r.f0))
  const young24 = sum(young.map((r) => r.m + r.f))
  const young12 = sum(young.map((r) => r.m0 + r.f0))
  const first = popSeries[0]
  const last = popSeries[popSeries.length - 1]
  const growth = ((last.value - first.value) / first.value) * 100
  return (
    <SectionCard
      id="people"
      n="01"
      kicker="People"
      title={`${pct(old24)} of residents are 65 or older, up from ${pct(old12)} in ${first.year}`}
      lead={`The population ${growth >= 0 ? 'grew' : 'fell'} ${Math.abs(growth).toFixed(1)}% between the two censuses while the base of the pyramid narrowed.`}
      actions={<ViewToggle value={view} onChange={setView} label="Age structure" />}
      source={`Source: Dept. of Census and Statistics · Census ${last.year} (provisional), Census ${first.year} (final) · ${profile.meta.data_version}`}
      href={`/explore?indicator=age.65plus&area=${area.pcode}`}
    >
      <div className="grid grid-cols-1 gap-x-10 gap-y-8 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <div className="min-w-0">
          <ChartTitle sub="Share of all residents in each five-year band">Age structure by sex</ChartTitle>
          {view === 'chart' ? (
            <>
              <Legend
                className="mb-3"
                items={[
                  { label: `Male ${last.year}`, color: male },
                  { label: `Female ${last.year}`, color: female },
                  { label: `${first.year}`, color: 'var(--ink)', kind: 'outline' },
                ]}
              />
              <Pyramid rows={pyramid} male={male} female={female} yearNow={last.year} yearThen={first.year} />
            </>
          ) : (
            <MiniTable
              caption="Age structure by sex, percent of residents"
              head={['Age', `Male ${last.year}`, `Female ${last.year}`, `Male ${first.year}`, `Female ${first.year}`]}
              rows={[...pyramid].reverse().map((r) => [r.age, pct(r.m, 2), pct(r.f, 2), pct(r.m0, 2), pct(r.f0, 2)])}
            />
          )}
        </div>
        <div className="min-w-0">
          <ChartTitle sub={`${fmtSigned(growth)}% over ${last.year - first.year} years`}>
            Residents, {first.year}–{last.year}
          </ChartTitle>
          <LineChart
            data={popSeries.map((d) => ({ x: d.year, y: d.value }))}
            color={p[4]}
            unit="residents"
            label="Residents"
            yFmt={(v) => (v >= 1e6 ? `${(v / 1e6).toFixed(2)}M` : `${(v / 1e3).toFixed(v < 1e5 ? 1 : 0)}k`)}
            dots={false}
            height={176}
            ariaLabel={`Residents from ${first.year} to ${last.year}`}
          />
          <dl className="mt-6 grid grid-cols-3 gap-4 border-t border-line pt-5">
            <Fact label="65 and over" value={pct(old24)} note={`${fmtSigned(old24 - old12)} pts`} />
            <Fact label="Under 15" value={pct(young24)} note={`${fmtSigned(young24 - young12)} pts`} />
            <Fact label="Aged 15–64" value={pct(100 - old24 - young24)} note={`${fmtSigned(old12 + young12 - old24 - young24)} pts`} />
          </dl>
        </div>
      </div>
    </SectionCard>
  )
}

/* ------------------------------ Access ------------------------------ */
export function AccessSection({ profile }: { profile: AreaProfile }) {
  const [theme] = useTheme()
  const a = ramp('access', theme)
  const h = profile.access[0]
  const diff = h.minutes - h.national
  return (
    <SectionCard
      id="access"
      n="02"
      kicker="Access"
      title={`A hospital is ${h.minutes.toFixed(0)} minutes away on average${Math.abs(diff) < 0.5 ? (profile.area.level === 0 ? ' by road' : ', in line with the national figure') : `, ${Math.abs(diff).toFixed(0)} ${diff < 0 ? 'fewer' : 'more'} than nationally`}`}
      lead={`${h.within30.toFixed(0)}% of residents can reach one within half an hour by road.`}
      source={`Source: Lanka Data Layer travel-time model on OSM roads, Ministry of Health facility list · 2026 · ${profile.meta.data_version}`}
      href={`/explore?indicator=access.hospital.drive_min&area=${profile.area.pcode}`}
    >
      <Legend
        className="mb-4"
        items={[
          { label: profile.area.name.en, color: a[4] },
          { label: 'National mean', color: 'var(--ink2)', kind: 'tick' },
          { label: 'Share within 30 min', color: a[2] },
        ]}
      />
      <AccessBars rows={profile.access} bar={a[4]} cell={a[2]} />
    </SectionCard>
  )
}

/* ------------------------------ Land ------------------------------ */
// identity colours: fixed slot per class, never re-ordered by value
const LAND_SLOT: Record<string, number> = { tree: 5, crop: 2, built: 4, water: 3, other: 7 }

export function LandSection({ profile }: { profile: AreaProfile }) {
  const [theme] = useTheme()
  const cat = categorical(theme)
  const b = ramp('built', theme)
  const segs = profile.land.map((l) => ({ ...l, color: cat[LAND_SLOT[l.id] ?? 7] }))
  const top = [...profile.land].sort((x, y) => y.share - x.share)[0]
  const built = profile.built
  const b75 = built[0]
  const b25 = built.find((d) => d.year === 2025) ?? built[built.length - 2]
  return (
    <SectionCard
      id="land"
      n="03"
      kicker="Land and built environment"
      title={`${top.label} is the largest land cover at ${pct(top.share, 0)}; built-up land has grown ${(b25.share / b75.share).toFixed(1)}× since ${b75.year}`}
      source={`Source: ESA WorldCover 10 m (2025) · GHSL built-up surface R2023A, 2030 is a model projection · ${profile.meta.data_version}`}
      href={`/explore?indicator=built.share&area=${profile.area.pcode}`}
    >
      <div className="grid grid-cols-1 gap-x-10 gap-y-9 lg:grid-cols-2">
        <div className="min-w-0">
          <ChartTitle sub="Share of land area, 2025">Land cover</ChartTitle>
          <StackBar segments={segs} ariaLabel={`Land cover shares: ${segs.map((s) => `${s.label} ${s.share.toFixed(0)}%`).join(', ')}`} />
          <ul className="mt-5 text-[13px]">
            {segs.map((s) => (
              <li key={s.id} className="flex items-center gap-2 border-b border-line py-[7px]">
                <span className="size-2.5 shrink-0 rounded-[2px]" style={{ background: s.color }} />
                <span className="text-ink2">{s.label}</span>
                <span className="t-num ml-auto font-medium text-ink">{pct(s.share)}</span>
              </li>
            ))}
          </ul>
        </div>
        <div className="min-w-0">
          <ChartTitle sub={`From ${pct(b75.share)} in ${b75.year} to ${pct(b25.share)} in ${b25.year}`}>Built-up share of land</ChartTitle>
          <Legend
            className="mb-2"
            items={[
              { label: 'Observed', color: b[4], kind: 'line' },
              { label: 'Modelled', color: b[4], kind: 'dashed' },
            ]}
          />
          <LineChart
            data={built.map((d) => ({ x: d.year, y: d.share }))}
            color={b[4]}
            unit="% of land"
            label="Built-up"
            yFmt={(v) => (Number.isInteger(v) ? String(v) : v.toFixed(1))}
            zero
            modelledFrom={2030}
            height={200}
            ariaLabel="Built-up share of land from 1975 to 2030; 2030 is modelled"
          />
        </div>
      </div>
    </SectionCard>
  )
}

/* ------------------------------ Climate ------------------------------ */
export function ClimateSection({ profile }: { profile: AreaProfile }) {
  const [theme] = useTheme()
  const [view, setView] = useState<View>('chart')
  const w = ramp('water', theme)
  const warm = ramp('built', theme)[4]
  const c = profile.climate
  const wet = [...c].sort((x, y) => y.rain - x.rain)[0]
  const dry = [...c].sort((x, y) => x.rain - y.rain)[0]
  const total = sum(c.map((d) => d.rain))
  const normal = sum(c.map((d) => (d.lo + d.hi) / 2))
  const outside = c.filter((d) => d.rain < d.lo || d.rain > d.hi).length
  return (
    <SectionCard
      id="climate"
      n="04"
      kicker="Climate"
      title={`${wet.month} is the wettest month at ${fmtInt(wet.rain)} mm; ${dry.month} the driest at ${fmtInt(dry.rain)} mm`}
      lead={outside ? `${outside} of 12 months fell outside the 1991–2020 normal range.` : 'Every month fell inside the 1991–2020 normal range.'}
      actions={<ViewToggle value={view} onChange={setView} label="Climate" />}
      source={`Source: CHIRPS v2.0 rainfall, ERA5-Land 2 m temperature · 2025 against 1991–2020 normals · ${profile.meta.data_version}`}
      href={`/explore?indicator=rain.annual&area=${profile.area.pcode}`}
    >
      <div className="grid grid-cols-1 gap-x-10 gap-y-8 lg:grid-cols-[minmax(0,1fr)_190px]">
        <div className="min-w-0">
          {view === 'chart' ? (
            <>
              <Legend
                className="mb-3"
                items={[
                  { label: 'Rainfall 2025', color: w[4] },
                  { label: 'Normal range 1991–2020', color: 'var(--ink3)', kind: 'band' },
                  { label: 'Mean temperature', color: warm, kind: 'line' },
                ]}
              />
              <ClimateBars data={c} rain={w[4]} temp={warm} />
            </>
          ) : (
            <MiniTable
              caption="Monthly rainfall, normal range and mean temperature"
              head={['Month', 'Rainfall, mm', 'Normal low', 'Normal high', 'Temp, °C']}
              rows={c.map((d) => [d.month, fmtInt(d.rain), fmtInt(d.lo), fmtInt(d.hi), d.temp.toFixed(1)])}
            />
          )}
        </div>
        <dl className="grid grid-cols-3 content-start gap-x-4 gap-y-6 border-line max-lg:border-t max-lg:pt-5 lg:grid-cols-1 lg:border-l lg:pl-7">
          <Fact label="Annual rainfall" value={`${fmtInt(total)} mm`} note={`${fmtSigned(((total - normal) / normal) * 100)}% vs normal`} />
          <Fact label="Wettest month" value={wet.month} note={`${fmtInt(wet.rain)} mm`} />
          <Fact label="Mean temp" value={`${(sum(c.map((d) => d.temp)) / 12).toFixed(1)} °C`} note={`${Math.min(...c.map((d) => d.temp)).toFixed(1)}–${Math.max(...c.map((d) => d.temp)).toFixed(1)} °C`} />
        </dl>
      </div>
    </SectionCard>
  )
}

/* ------------------------------ Elections ------------------------------ */
export function ElectionsSection({ profile }: { profile: AreaProfile }) {
  const [theme] = useTheme()
  const cat = categorical(theme)
  const e = profile.elections
  const rows = e.map((r, i) => ({ id: r.party, label: r.party, share: r.share, value: r.votes, color: r.party === 'Others' ? cat[7] : cat[i] }))
  const ranked = e.filter((r) => r.party !== 'Others').sort((x, y) => y.share - x.share)
  const total = sum(e.map((r) => r.votes))
  return (
    <SectionCard
      id="elections"
      n="05"
      kicker="Elections"
      title={`${ranked[0].party} led with ${pct(ranked[0].share)} of valid votes, ${(ranked[0].share - ranked[1].share).toFixed(1)} points ahead of ${ranked[1].party}`}
      source={`Source: Election Commission of Sri Lanka · Parliamentary election 2024, results by polling division · ${profile.meta.data_version}`}
      href="/data?theme=elections"
      hrefLabel="See election datasets"
    >
      <div className="grid grid-cols-1 gap-x-10 gap-y-8 lg:grid-cols-[minmax(0,1fr)_190px]">
        <div className="min-w-0">
          <ShareBars rows={rows} valueFmt={fmtInt} valueLabel="Votes" />
          <p className="mt-4 border-l-2 border-line2 pl-3 text-[12.5px] leading-relaxed text-ink2">
            Results for the containing polling divisions. Polling divisions do not nest exactly inside administrative units, so totals are apportioned by registered electors.
          </p>
        </div>
        <dl className="grid grid-cols-3 content-start gap-x-4 gap-y-6 border-line max-lg:border-t max-lg:pt-5 lg:grid-cols-1 lg:border-l lg:pl-7">
          <Fact label="Valid votes" value={fmtCompact(total)} note={fmtInt(total)} />
          <Fact label="Lead" value={`${(ranked[0].share - ranked[1].share).toFixed(1)} pts`} note={`${ranked[0].party} over ${ranked[1].party}`} />
          <Fact label="Parties shown" value={String(ranked.length)} note="plus others" />
        </dl>
      </div>
    </SectionCard>
  )
}
