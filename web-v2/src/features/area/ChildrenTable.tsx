import { ArrowDown, ArrowUp, ChevronsUpDown } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import type { AreaProfile, ChildRow } from '@/api/types'
import { Sparkline } from '@/components/charts/Sparkline'
import { Eyebrow, PcodePlate, Reveal } from '@/components/common/bits'
import { fmt1, fmtInt, fmtSigned } from '@/lib/format'
import { useTheme } from '@/lib/hooks'
import { ramp } from '@/lib/palettes'
import { cn } from '@/lib/utils'
import { CHILD_LEVEL } from './AreaHeader'

type Key = 'name' | 'population' | 'density' | 'change' | 'hospitalMin'
const COLS: { key: Key; label: string; unit?: string; num: boolean }[] = [
  { key: 'name', label: 'Name', num: false },
  { key: 'population', label: 'Population', unit: '2024', num: true },
  { key: 'density', label: 'Density', unit: 'per km²', num: true },
  { key: 'change', label: 'Change', unit: '2012–24', num: true },
  { key: 'hospitalMin', label: 'Hospital', unit: 'min', num: true },
]

export function ChildrenTable({ profile }: { profile: AreaProfile }) {
  const [theme] = useTheme()
  const { area, children } = profile
  const [sort, setSort] = useState<{ key: Key; dir: 1 | -1 }>({ key: 'population', dir: -1 })
  const rows = useMemo(() => {
    const get = (r: ChildRow) => (sort.key === 'name' ? r.name.en : r[sort.key])
    return [...children].sort((a, b) => {
      const x = get(a)
      const y = get(b)
      return (typeof x === 'string' ? x.localeCompare(y as string) : x - (y as number)) * sort.dir
    })
  }, [children, sort])
  const maxDensity = Math.max(...children.map((c) => c.density))
  const navigable = area.level < 2
  const kind = CHILD_LEVEL[area.level]
  const fill = ramp('people', theme)[3]

  return (
    <section id="sub-units" aria-labelledby="sub-units-title" className="scroll-mt-24">
      <Reveal>
        <div className="flex flex-wrap items-end justify-between gap-x-8 gap-y-2">
          <div>
            <Eyebrow n="06">Sub-units</Eyebrow>
            <h2 id="sub-units-title" className="t-h3 mt-3">
              {children.length} {kind.startsWith('DS') ? kind : kind.toLowerCase()} in {area.name.en}
            </h2>
          </div>
          <p className="t-label">{navigable ? 'Select a name to open its profile' : 'DS division profiles are not published yet'}</p>
        </div>
        <div className="surface scroll-thin mt-5 max-h-[620px] overflow-auto shadow-[var(--shadow-sm)]">
          <table className="w-full min-w-[760px] border-collapse text-[13.5px]">
            <caption className="sr-only">
              {kind} of {area.name.en} with population, density, change and hospital access. Column headers sort the table.
            </caption>
            <thead>
              <tr>
                {COLS.map((c, i) => {
                  const on = sort.key === c.key
                  const Icon = !on ? ChevronsUpDown : sort.dir === 1 ? ArrowUp : ArrowDown
                  return (
                    <th
                      key={c.key}
                      scope="col"
                      aria-sort={on ? (sort.dir === 1 ? 'ascending' : 'descending') : 'none'}
                      className={cn('sticky top-0 z-10 border-b border-line2 bg-bg3 p-0 font-medium', i === 0 && 'left-0 z-20')}
                    >
                      <button
                        type="button"
                        onClick={() => setSort((s) => (s.key === c.key ? { key: c.key, dir: s.dir === 1 ? -1 : 1 } : { key: c.key, dir: c.num ? -1 : 1 }))}
                        className={cn('flex h-11 w-full items-center gap-1.5 px-4 text-[12px] whitespace-nowrap hover:text-ink', c.num && 'justify-end', on ? 'text-ink' : 'text-ink2')}
                      >
                        <span>
                          {c.label}
                          {c.unit && <span className="ml-1.5 font-normal text-ink3">{c.unit}</span>}
                        </span>
                        <Icon className={cn('size-3 shrink-0', on ? 'text-ink' : 'text-ink3')} aria-hidden />
                      </button>
                    </th>
                  )
                })}
                <th scope="col" className="sticky top-0 z-10 border-b border-line2 bg-bg3 px-4 text-right text-[12px] font-medium whitespace-nowrap text-ink2">
                  Trend <span className="ml-1 font-normal text-ink3">2012–24</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.pcode} className="group border-b border-line last:border-0 hover:bg-bg3/50">
                  <th scope="row" className="sticky left-0 z-[5] bg-bg2 px-4 py-2.5 text-left font-normal group-hover:bg-[color-mix(in_oklab,var(--bg3)_50%,var(--bg2))]">
                    <div className="flex items-center gap-3">
                      <PcodePlate code={r.pcode} size="sm" className="shrink-0" />
                      <div className="min-w-0">
                        {navigable ? (
                          <Link to={`/area/${r.pcode}`} className="link-draw font-medium text-ink hover:text-brand-ink">
                            {r.name.en}
                          </Link>
                        ) : (
                          <span className="font-medium text-ink">{r.name.en}</span>
                        )}
                        {r.name.si && (
                          <div className="truncate text-[11.5px] text-ink3">
                            <span lang="si">{r.name.si}</span>
                            {r.name.ta && (
                              <>
                                {' · '}
                                <span lang="ta">{r.name.ta}</span>
                              </>
                            )}
                          </div>
                        )}
                      </div>
                    </div>
                  </th>
                  <td className="t-num px-4 text-right text-ink">{fmtInt(r.population)}</td>
                  <td className="px-4">
                    <div className="flex items-center justify-end gap-3">
                      <span aria-hidden className="relative h-2.5 w-[72px] shrink-0 border-l border-[var(--chart-axis)]">
                        <span className="absolute inset-y-0 left-0 rounded-r-[3px]" style={{ width: `${Math.max(2, (r.density / maxDensity) * 100)}%`, background: fill }} />
                      </span>
                      <span className="t-num w-[52px] text-right text-ink">{fmtInt(r.density)}</span>
                    </div>
                  </td>
                  <td className={cn('t-num px-4 text-right font-medium whitespace-nowrap', r.change >= 0 ? 'text-ok' : 'text-bad')}>
                    <span aria-hidden className="mr-1 text-[9px]">
                      {r.change >= 0 ? '▲' : '▼'}
                    </span>
                    {fmtSigned(r.change)}%
                  </td>
                  <td className="t-num px-4 text-right text-ink2">{fmt1(r.hospitalMin)}</td>
                  <td className="px-4">
                    <Sparkline data={r.spark} width={84} height={24} className="ml-auto" />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Reveal>
    </section>
  )
}
