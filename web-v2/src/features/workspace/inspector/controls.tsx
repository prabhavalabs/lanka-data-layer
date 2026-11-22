// Small parts every inspector section is built from. Consistent rhythm: 12.5 px labels on the left,
// controls on the right, 32 px rows on a pointer and 44 px under a finger.
import { ArrowLeftRight, Check, ChevronDown } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Slider } from '@/components/ui/slider'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { Theme } from '@/lib/hooks'
import { rampLabel, rampStops, SEQ_RAMPS, toneColor, TONES } from '@/lib/palettes'
import { cn } from '@/lib/utils'

export function Section({ title, aside, children, defaultOpen = true, flush = false }: { title: string; aside?: ReactNode; children: ReactNode; defaultOpen?: boolean; flush?: boolean }) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <section className="border-t border-glass-line first:border-t-0">
      <div className="flex h-10 items-center gap-2 pr-3 pl-4 max-md:h-11">
        <button type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open} className="group/sec -ml-1 flex h-full min-w-0 flex-1 items-center gap-1.5 rounded-md pl-1 text-left outline-none">
          <ChevronDown className={cn('size-3 shrink-0 text-ink3 transition-transform duration-150 ease-[var(--ease-out)]', !open && '-rotate-90')} />
          <h3 className="t-label truncate tracking-[0.08em] uppercase transition-colors duration-150 [@media(hover:hover)_and_(pointer:fine)]:group-hover/sec:text-ink2">{title}</h3>
        </button>
        {aside}
      </div>
      {open && <div className={cn('space-y-3 pb-4', !flush && 'px-4')}>{children}</div>}
    </section>
  )
}

/** A labelled control. `stack` puts the control under its label, for wide ones; a switch (`htmlFor`) gives its label the row. */
export function Field({ label, hint, children, stack = false, htmlFor }: { label: string; hint?: string; children: ReactNode; stack?: boolean; htmlFor?: string }) {
  if (htmlFor)
    return (
      <div className="flex min-h-8 items-center gap-3 max-md:min-h-11">
        <label htmlFor={htmlFor} className="min-w-0 flex-1 cursor-pointer truncate text-[12.5px] text-ink2">
          {label}
          {hint && <span className="ml-1.5 text-ink3">{hint}</span>}
        </label>
        <div className="flex shrink-0 items-center">{children}</div>
      </div>
    )
  return (
    <div className={cn(stack ? 'space-y-1.5' : 'flex min-h-8 items-center gap-3 max-md:min-h-11')}>
      <span className={cn('shrink-0 text-[12.5px] text-ink2', !stack && 'w-[84px] truncate')}>
        {label}
        {hint && <span className="ml-1.5 text-ink3">{hint}</span>}
      </span>
      <div className={cn('min-w-0', !stack && 'flex flex-1 items-center justify-end gap-2')}>{children}</div>
    </div>
  )
}

export function Seg<T extends string>({ value, onChange, options, label, className }: { value: T; onChange: (v: T) => void; options: { value: T; label: string; disabled?: boolean; hint?: string }[]; label: string; className?: string }) {
  return (
    <ToggleGroup type="single" value={value} onValueChange={(v) => v && onChange(v as T)} aria-label={label} className={cn('flex h-8 w-full max-md:h-10', className)}>
      {options.map((o) =>
        o.hint ? (
          <Tooltip key={o.value}>
            <TooltipTrigger asChild>
              <ToggleGroupItem value={o.value} disabled={o.disabled} className="px-1.5 disabled:opacity-40">
                {o.label}
              </ToggleGroupItem>
            </TooltipTrigger>
            <TooltipContent>{o.hint}</TooltipContent>
          </Tooltip>
        ) : (
          <ToggleGroupItem key={o.value} value={o.value} disabled={o.disabled} className="px-1.5 disabled:opacity-40">
            {o.label}
          </ToggleGroupItem>
        ),
      )}
    </ToggleGroup>
  )
}

/** A slider with its value beside it. The parent decides what a drag commits. */
export function NumberSlider({ value, min, max, step = 1, onChange, onCommit, format = String, label, disabled }: { value: number; min: number; max: number; step?: number; onChange: (v: number) => void; onCommit?: (v: number) => void; format?: (v: number) => string; label: string; disabled?: boolean }) {
  return (
    <>
      <Slider className="min-w-0 flex-1 max-md:h-8" min={min} max={max} step={step} value={[value]} disabled={disabled} aria-label={label} onValueChange={([v]) => onChange(v)} onValueCommit={onCommit ? ([v]) => onCommit(v) : undefined} />
      <span className="t-num w-10 shrink-0 text-right font-mono text-[11.5px] text-ink2">{format(value)}</span>
    </>
  )
}

export function Picker<T extends string>({ value, onChange, options, label, className, placeholder }: { value: T | undefined; onChange: (v: T) => void; options: { value: T; label: string; note?: string }[]; label: string; className?: string; placeholder?: string }) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as T)}>
      <SelectTrigger aria-label={label} className={cn('h-8 text-[12.5px] max-md:h-10', className)}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value} textValue={o.label} className="max-md:py-2.5">
            {o.label}
            {o.note && <span className="ml-2 text-ink3">{o.note}</span>}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

export const rampCss = (stops: string[]) => `linear-gradient(to right, ${stops.join(', ')})`
/** Ramp chooser: the ramps as they will draw on this map, and a reverse switch. */
export function RampPicker({ value, reverse, scheme, onChange, onReverse, diverging = false }: { value: string; reverse: boolean; scheme: Theme; onChange: (ramp: string) => void; onReverse: (r: boolean) => void; diverging?: boolean }) {
  const [open, setOpen] = useState(false)
  const ramps = [...SEQ_RAMPS.map((r) => r.name as string), ...(diverging ? ['ldl.div.blue-orange'] : [])]
  return (
    <div className="flex w-full items-center gap-1.5">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button type="button" aria-label={`Colour ramp: ${rampLabel(value)}`} className="pressable flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg border border-line2 bg-bg2/60 pr-2 pl-1.5 outline-none max-md:h-10 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/60">
            <span className="h-4 min-w-0 flex-1 rounded-[4px]" style={{ background: rampCss(rampStops(value, scheme, reverse)) }} />
            <span className="shrink-0 text-[12.5px] text-ink">{rampLabel(value)}</span>
            <ChevronDown className="size-3.5 shrink-0 text-ink3" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-[248px] p-1.5">
          {ramps.map((r) => (
            <button
              key={r}
              type="button"
              onClick={() => {
                onChange(r)
                setOpen(false)
              }}
              className="pressable flex h-9 w-full items-center gap-2.5 rounded-lg px-2 outline-none max-md:h-11 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg4"
            >
              <span className="h-4 flex-1 rounded-[4px]" style={{ background: rampCss(rampStops(r, scheme, reverse)) }} />
              <span className="w-[74px] text-left text-[12.5px] text-ink2">{rampLabel(r)}</span>
              <Check className={cn('size-3.5 text-ink', r !== value && 'opacity-0')} />
            </button>
          ))}
        </PopoverContent>
      </Popover>
      <Tooltip>
        <TooltipTrigger asChild>
          <button type="button" onClick={() => onReverse(!reverse)} aria-label="Reverse the ramp" aria-pressed={reverse} className={cn('pressable grid size-8 shrink-0 place-items-center rounded-lg border border-line2 text-ink3 outline-none max-md:size-10', reverse ? 'bg-bg4 text-ink' : 'bg-bg2/60 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink')}>
            <ArrowLeftRight className="size-3.5" />
          </button>
        </TooltipTrigger>
        <TooltipContent>Reverse</TooltipContent>
      </Tooltip>
    </div>
  )
}

/** Named tones for a single colour: they follow the basemap's scheme, so a line never vanishes on a theme change. */
export function TonePicker({ value, scheme, onChange, label, extra = [] }: { value: string | undefined; scheme: Theme; onChange: (tone: string) => void; label: string; extra?: string[] }) {
  const current = value?.replace('ldl.tone.', '')
  return (
    <div role="radiogroup" aria-label={label} className="flex flex-wrap items-center gap-1.5">
      {[...TONES, ...extra].map((t) => (
        <Tooltip key={t}>
          <TooltipTrigger asChild>
            <button
              type="button"
              role="radio"
              aria-checked={current === t}
              aria-label={t}
              onClick={() => onChange(`ldl.tone.${t}`)}
              className={cn('pressable grid size-6 place-items-center rounded-full outline-none max-md:size-9', current === t ? 'ring-2 ring-ink ring-offset-2 ring-offset-bg2' : 'ring-1 ring-[color-mix(in_oklab,var(--ink)_18%,transparent)] ring-inset')}
              style={{ background: toneColor(t, scheme) }}
            />
          </TooltipTrigger>
          <TooltipContent className="capitalize">{t}</TooltipContent>
        </Tooltip>
      ))}
    </div>
  )
}

/** A slider value that follows the pointer locally and only tells the document when it stops (or every frame, if asked). */
export function useLive<T>(value: T): [T, (v: T) => void, () => void] {
  const [live, setLive] = useState<T | null>(null)
  useEffect(() => setLive(null), [value])
  return [live ?? value, setLive, () => setLive(null)]
}
