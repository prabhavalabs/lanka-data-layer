import { useEffect, useState } from 'react'
import { cn } from '@/lib/utils'

export const SECTIONS = [
  { id: 'people', label: 'People' },
  { id: 'access', label: 'Access' },
  { id: 'land', label: 'Land' },
  { id: 'climate', label: 'Climate' },
  { id: 'elections', label: 'Elections' },
  { id: 'sub-units', label: 'Sub-units' },
] as const

/** Sticky side rail with scroll-spy (desktop only). */
export function SectionNav() {
  const [active, setActive] = useState<string>(SECTIONS[0].id)
  useEffect(() => {
    const els = SECTIONS.map((s) => document.getElementById(s.id)).filter((e): e is HTMLElement => !!e)
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) if (e.isIntersecting) setActive(e.target.id)
      },
      { rootMargin: '-25% 0px -65% 0px' },
    )
    els.forEach((e) => io.observe(e))
    return () => io.disconnect()
  }, [])
  return (
    <nav aria-label="On this page" className="sticky top-24 hidden self-start lg:block">
      <div className="t-label mb-3 pl-3.5">On this page</div>
      <ol className="border-l border-line">
        {SECTIONS.map((s, i) => {
          const on = active === s.id
          return (
            <li key={s.id}>
              <a
                href={`#${s.id}`}
                aria-current={on ? 'location' : undefined}
                className={cn(
                  'relative -ml-px flex items-center gap-2.5 border-l py-[7px] pl-3.5 text-[13px] transition-colors duration-150',
                  on ? 'border-brand font-medium text-ink' : 'border-transparent text-ink2 hover:border-line2 hover:text-ink',
                )}
              >
                <span className={cn('t-num font-mono text-[10.5px]', on ? 'text-brand-ink' : 'text-ink3')}>{String(i + 1).padStart(2, '0')}</span>
                {s.label}
              </a>
            </li>
          )
        })}
      </ol>
    </nav>
  )
}
