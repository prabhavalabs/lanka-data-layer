import { Pause, Play, RotateCcw, Search, SkipForward } from 'lucide-react'
import { useMemo, type ReactNode } from 'react'
import type { Indicator, LicenceStatus, ThemeInfo } from '@/api/types'
import { Kbd, LicenceLight } from '@/components/common/bits'
import { useCommandPalette } from '@/components/shell/CommandPalette'
import { Checkbox } from '@/components/ui/checkbox'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { fmtBare, LEVEL_PLURAL, rangeBounds, unitSuffix, type LayerKey, type LayerState, type Level, type LicenceFilter, type MapModel } from './model'

export function Section({ title, aside, children, className }: { title: string; aside?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn('border-t border-glass-line px-4 py-4 first:border-t-0', className)}>
      <div className="mb-3 flex h-4 items-center justify-between gap-3">
        <h3 className="t-label tracking-[0.08em] uppercase">{title}</h3>
        {aside}
      </div>
      {children}
    </section>
  )
}

/** A button dressed as a search field; the real search lives in the ⌘K palette. */
export function SearchField({ className }: { className?: string }) {
  const { open } = useCommandPalette()
  return (
    <button
      type="button"
      onClick={open}
      aria-label="Search areas, indicators and datasets"
      className={cn(
        'pressable flex h-9 min-w-0 flex-1 items-center gap-2 rounded-lg border border-line2 bg-bg2/55 px-2.5 text-[13px] text-ink3 max-md:h-11 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/60 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink2',
        className,
      )}
    >
      <Search className="size-3.5 shrink-0" />
      <span className="flex-1 truncate text-left">Search areas, indicators…</span>
      <span className="flex gap-0.5">
        <Kbd>⌘</Kbd>
        <Kbd>K</Kbd>
      </span>
    </button>
  )
}

const LICENCES: LicenceStatus[] = ['open', 'attribution', 'unclear']
const LAYERS: { key: LayerKey; label: string; note: string }[] = [
  { key: 'rail', label: 'Rail', note: 'Lines in service' },
  { key: 'rivers', label: 'Rivers', note: 'Main channels' },
  { key: 'protected', label: 'Protected areas', note: 'Parks and reserves' },
  { key: 'places', label: 'Places', note: '25 largest towns' },
]

function LayerGlyph({ kind }: { kind: LayerKey }) {
  return (
    <svg viewBox="0 0 20 12" className="h-3 w-5 shrink-0 text-ink2" aria-hidden>
      {kind === 'rail' && <path d="M1 6h18" stroke="currentColor" strokeWidth="1.4" strokeDasharray="4 2" fill="none" />}
      {kind === 'rivers' && <path d="M1 8c3-6 6 2 9-2s5-2 9-3" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" fill="none" />}
      {kind === 'protected' && <rect x="2.5" y="1.5" width="15" height="9" rx="2" stroke="currentColor" strokeWidth="1" strokeDasharray="2 2" fill="currentColor" fillOpacity="0.14" />}
      {kind === 'places' && <circle cx="10" cy="6" r="2.4" fill="currentColor" stroke="var(--bg2)" strokeWidth="1" />}
    </svg>
  )
}

export type ControlsProps = {
  indicators: Indicator[] | undefined
  themes: ThemeInfo[] | undefined
  licenceOf: (ind: Indicator) => LicenceStatus | undefined
  licences: LicenceFilter
  onLicence: (status: LicenceStatus, on: boolean) => void
  indicator: Indicator | undefined
  onIndicator: (id: string) => void
  level: Level
  onLevel: (level: Level) => void
  year: number
  onYear: (year: number) => void
  playing: boolean
  onPlay: () => void
  reducedMotion: boolean
  model: MapModel | null
  range: [number, number]
  onRange: (range: [number, number]) => void
  layers: LayerState
  onLayer: (key: LayerKey, on: boolean) => void
}

export function Controls(p: ControlsProps) {
  const { indicators, themes, indicator, model } = p
  const visible = useMemo(() => (indicators ?? []).filter((i) => p.licences[p.licenceOf(i) ?? 'open']), [indicators, p.licences, p.licenceOf])
  const groups = useMemo(() => {
    const order = themes?.map((t) => t.id) ?? []
    const ids = [...new Set(visible.map((i) => i.theme))].sort((a, b) => order.indexOf(a) - order.indexOf(b))
    return ids.map((id) => ({ id, label: themes?.find((t) => t.id === id)?.label ?? id, items: visible.filter((i) => i.theme === id) }))
  }, [visible, themes])
  const counts = useMemo(() => {
    const c: Record<LicenceStatus, number> = { open: 0, attribution: 0, unclear: 0 }
    for (const i of indicators ?? []) c[p.licenceOf(i) ?? 'open']++
    return c
  }, [indicators, p.licenceOf])
  const checked = LICENCES.filter((s) => p.licences[s]).length

  const years = indicator?.years ?? []
  const [lo, hi] = rangeBounds(model, p.range)
  const active = p.range[0] > 0 || p.range[1] < 100
  const inRange = model ? model.data.values.filter((v) => v >= lo && v <= hi).length : 0
  const unit = model?.indicator.unit ?? ''

  return (
    <div>
      <Section title="Indicator" aside={indicator && <span className="t-label t-num">{unitSuffix(indicator.unit)}</span>}>
        {indicator ? (
          <>
            <Select value={indicator.id} onValueChange={p.onIndicator}>
              <SelectTrigger aria-label="Indicator" className="h-10 text-[13.5px] font-medium max-md:h-11">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {groups.map((g) => (
                  <SelectGroup key={g.id}>
                    <SelectLabel className="tracking-[0.06em] uppercase">{g.label}</SelectLabel>
                    {g.items.map((i) => (
                      <SelectItem key={i.id} value={i.id} className="max-md:py-2.5">
                        {i.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
            <p className="mt-2.5 text-[12.5px] leading-[1.5] text-ink2">{indicator.description}</p>
          </>
        ) : (
          <>
            <div className="skeleton h-10 w-full" />
            <div className="skeleton mt-3 h-3.5 w-11/12" />
            <div className="skeleton mt-1.5 h-3.5 w-7/12" />
          </>
        )}
      </Section>

      <Section title="Admin level" aside={<span className="t-label t-num">{model ? `${model.data.pcodes.length} ${LEVEL_PLURAL[model.data.level]}` : ''}</span>}>
        <ToggleGroup
          type="single"
          value={String(p.level)}
          onValueChange={(v) => (v === '1' || v === '2') && p.onLevel(Number(v) as Level)}
          aria-label="Administrative level"
          className="flex h-9 w-full max-md:h-11"
        >
          <ToggleGroupItem value="1" className="font-mono text-[11.5px]">
            ADM1
          </ToggleGroupItem>
          <ToggleGroupItem value="2" className="font-mono text-[11.5px]">
            ADM2
          </ToggleGroupItem>
          {['ADM3', 'ADM4'].map((l) => (
            <Tooltip key={l}>
              <TooltipTrigger asChild>
                <ToggleGroupItem value={l} aria-disabled className="cursor-not-allowed font-mono text-[11.5px] text-ink3/70 active:!scale-100">
                  {l}
                </ToggleGroupItem>
              </TooltipTrigger>
              <TooltipContent>Not in this mockup</TooltipContent>
            </Tooltip>
          ))}
        </ToggleGroup>
      </Section>

      <Section title="Year" aside={years.length > 0 && <span className="t-label t-num">{`${years[0]}–${years[years.length - 1]}`}</span>}>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={p.onPlay}
            disabled={years.length < 2}
            aria-label={p.reducedMotion ? 'Next year' : p.playing ? 'Pause' : 'Play through years'}
            aria-pressed={p.reducedMotion ? undefined : p.playing}
            className="pressable grid size-10 shrink-0 place-items-center rounded-full border border-line2 bg-bg2/60 text-ink max-md:size-11 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/60 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3"
          >
            {p.reducedMotion ? <SkipForward className="size-4" /> : p.playing ? <Pause className="size-4" /> : <Play className="size-4 translate-x-px" />}
          </button>
          <div className="t-num font-mono text-[30px] leading-none font-medium tracking-[-0.04em] text-ink" aria-live="off">
            {years.length ? p.year : '—'}
          </div>
          {p.playing && <span className="t-label ml-auto">playing</span>}
        </div>
        <Slider
          className="mt-3.5 max-md:h-8"
          min={years[0] ?? 0}
          max={years[years.length - 1] ?? 1}
          step={1}
          value={[p.year]}
          disabled={years.length < 2}
          onValueChange={([y]) => p.onYear(y)}
          aria-label="Year"
        />
        <div className="mt-1 flex justify-between" aria-hidden>
          {years.map((y, i) => (
            <span key={y} className={cn('h-1 w-px', y === p.year ? 'bg-ink' : i % 2 === 0 ? 'bg-ink3/70' : 'bg-line2')} />
          ))}
        </div>
      </Section>

      <Section
        title="Value range"
        aside={
          model && (
            <span className="t-label t-num text-ink2">
              {fmtBare(Number.isFinite(lo) ? lo : model.min, unit)} – {fmtBare(Number.isFinite(hi) ? hi : model.max, unit)}
            </span>
          )
        }
      >
        {/* one tick per area, in its class colour: the distribution the slider cuts through */}
        <div className="relative mx-[7px] h-3.5" aria-hidden>
          {model?.data.values.map((v, i) => (
            <span
              key={model.data.pcodes[i]}
              className="absolute top-0 h-full w-[2px] -translate-x-1/2 rounded-full transition-opacity duration-150"
              style={{
                left: `${((v - model.min) / (model.max - model.min || 1)) * 100}%`,
                background: model.colors[model.byPcode.get(model.data.pcodes[i])?.cls ?? 0],
                opacity: v >= lo && v <= hi ? 1 : 0.22,
              }}
            />
          ))}
        </div>
        <Slider className="mt-1 max-md:h-8" min={0} max={100} step={1} minStepsBetweenThumbs={1} value={p.range} disabled={!model} onValueChange={(v) => p.onRange([v[0], v[1]])} aria-label="Value range" />
        <div className="mt-2 flex h-5 items-center justify-between text-[12px] text-ink2">
          <span className="t-num">{model ? `${inRange} of ${model.data.pcodes.length} ${LEVEL_PLURAL[model.data.level]} in range` : ' '}</span>
          {active && (
            <button type="button" onClick={() => p.onRange([0, 100])} className="pressable inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-ink2 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
              <RotateCcw className="size-3" />
              Reset
            </button>
          )}
        </div>
      </Section>

      <Section title="Source licence">
        <div className="-my-1 flex flex-col">
          {LICENCES.map((s) => {
            const id = `lic-${s}`
            const last = p.licences[s] && checked === 1
            return (
              <label key={s} htmlFor={id} className={cn('flex min-h-9 cursor-pointer items-center gap-2.5 max-md:min-h-11', last && 'cursor-not-allowed')}>
                <Checkbox id={id} checked={p.licences[s]} disabled={last} onCheckedChange={(v) => p.onLicence(s, v === true)} />
                <LicenceLight status={s} />
                <span className="t-label t-num ml-auto">{counts[s]}</span>
              </label>
            )
          })}
        </div>
      </Section>

      <Section title="Layers">
        <div className="-my-1 flex flex-col">
          {LAYERS.map((l) => {
            const id = `layer-${l.key}`
            return (
              <label key={l.key} htmlFor={id} className="flex min-h-9 cursor-pointer items-center gap-2.5 max-md:min-h-11">
                <LayerGlyph kind={l.key} />
                <span className="text-[13px] text-ink">{l.label}</span>
                <span className="truncate text-[12px] text-ink3">{l.note}</span>
                <Switch id={id} className="ml-auto" checked={p.layers[l.key]} onCheckedChange={(v) => p.onLayer(l.key, v)} />
              </label>
            )
          })}
        </div>
      </Section>
    </div>
  )
}
