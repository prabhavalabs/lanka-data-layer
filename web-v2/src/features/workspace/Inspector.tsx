// The layer inspector: what a layer shows (source, period), how it draws (per type), what it leaves
// out (filter) and how it explains itself (labels, legend). Every control writes the document.
import { ChartScatter, Plus, TriangleAlert, Ungroup, X } from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { useMemo, useState, type ReactNode } from 'react'
import { adminLevelsQuery, featureMeta, gridMeta, INDICATOR_META, indicatorMeta, LEVELS, type FieldMeta, type LayerData } from '@/api/workspace'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import type { Theme } from '@/lib/hooks'
import { allLayers, clausesOf, DASH, dashKind, filterOf, isGroup, LAYER_TYPE_LABEL, resolvePeriod, type Clause, type FieldBinding, type Group, type Layer, type PeriodRef, type Source, type Stroke, type Style } from '@/lib/map-spec'
import { BIVARIATE, bivariateColors } from '@/lib/palettes'
import { extent } from '@/lib/stats'
import { cn } from '@/lib/utils'
import { useRafCallback, useWorkspace } from './context'
import { bivariateBreaks, columnAt, columnsOf, DEFAULT_FILL, filterFields, isBinding, isBivariate, layerScheme, resolveScale } from './engine/compile'
import { gridSample } from './engine/raster'
import { fmt, unitSuffix } from './format'
import { Field, NumberSlider, Picker, RampPicker, Section, Seg, TonePicker, useLive } from './inspector/controls'
import { ScaleEditor } from './inspector/ScaleEditor'
import { LayerThumb } from './LayerThumb'
import { actions, useDoc, useNode, useWs } from './store'
import { drawnFrom, useMapZoom, zoomLabel } from './zoom'

const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const CLASS_NOTE: Record<string, { label: string; variant: 'neutral' | 'warn' | 'outline'; note: string }> = {
  official: { label: 'Published', variant: 'outline', note: 'Values as published by the source' },
  estimated: { label: 'Estimate', variant: 'neutral', note: 'Modelled or derived, not counted' },
  synthetic: { label: 'Synthetic', variant: 'warn', note: 'Generated for demonstration; not real observations' },
}

/* ---------------- what the layer reads ---------------- */
function PeriodField({ layer, source, periods }: { layer: Layer; source: Extract<Source, { period: PeriodRef }>; periods: string[] }) {
  const time = useWs((s) => s.doc!.time)
  if (periods.length < 2) return <Field label="Period">{<span className="t-num font-mono text-[12px] text-ink2">{periods[0] ?? 'latest'}</span>}</Field>
  const bound = typeof source.period === 'object'
  const current = resolvePeriod(source.period, time, periods)
  const set = (period: PeriodRef) => {
    if (typeof period === 'object' && !time) actions.setTime({ cursor: current, range: [periods[0], periods[periods.length - 1]], freq: 'A', playback: { fps: 1.1, loop: true } })
    actions.setSource(layer.id, (s) => ({ ...s, period }) as Source, 'Period changed')
  }
  return (
    <>
      <Field label="Period">
        <Seg
          value={bound ? 'time' : 'fixed'}
          onChange={(v) => set(v === 'time' ? { bind: 'time', match: 'asof' } : current)}
          label="Period"
          options={[
            { value: 'time', label: 'Timeline', hint: 'Follows the map’s time cursor' },
            { value: 'fixed', label: 'Fixed', hint: 'Stays on one period while the timeline plays' },
          ]}
        />
      </Field>
      {!bound && (
        <Field label="Year">
          <Picker value={current} onChange={(p) => set(p)} label="Year" options={periods.map((p) => ({ value: p, label: p }))} />
        </Field>
      )}
    </>
  )
}

/**
 * The admin levels as Units options. A level is greyed out when there is nothing to draw at it (the
 * indicator is not published there, or the boundary tiles do not carry it), and its hint says why; one
 * that only draws from some zoom says from which.
 */
function levelOptions(admin: Record<number, number> | null | undefined, opts: { has?: (l: number) => boolean; sample: boolean }) {
  return [1, 2, 3, 4].map((l) => {
    const plural = LEVELS[l].plural.replace(/^./, (c) => c.toUpperCase())
    const tiles = !admin || l in admin
    const has = opts.has ? opts.has(l) : true
    const from = admin?.[l] ?? 0
    const hint = !tiles ? `${plural}: no boundaries for this level yet` : !has ? `${plural}: no data for this indicator` : opts.sample && l >= 3 ? `${plural}: Western Province only in the sample data` : from > 0 ? `${plural} · drawn from zoom ${from}` : plural
    return { value: String(l), label: LEVELS[l].code, disabled: !tiles || !has, hint }
  })
}

const levelWord = (l: number) => LEVELS[l].plural.replace(/^./, (c) => c.toUpperCase())
const levelName = (l: number) => LEVELS[l].name.replace(/^./, (c) => c.toUpperCase())
/** The name a boundary layer takes at another level, if it still has its level's default name ("Provinces", "Province names"). */
function boundaryName(layer: Layer, from: number, to: number): string | null {
  if (from === to) return null
  if (layer.name === levelWord(from)) return levelWord(to)
  if (layer.name === `${levelName(from)} names`) return `${levelName(to)} names`
  return null
}

/** Said when the layer is on, but the map is zoomed out past where it draws: otherwise nothing tells why it is blank. */
function ReachNotice({ layer, data, what }: { layer: Layer; data: LayerData | undefined; what: string }) {
  const zoom = useMapZoom()
  const { engine } = useWorkspace()
  const from = drawnFrom(layer, data)
  if (!layer.visible || from <= 0 || zoom >= from - 0.005) return null
  return (
    <div role="status" className="flex items-start gap-2.5 rounded-lg border border-warn/40 bg-warn/[0.07] px-2.5 py-2">
      <TriangleAlert className="mt-px size-3.5 shrink-0 text-warn" />
      <p className="min-w-0 flex-1 text-[12px] leading-[1.45] text-ink2">
        <span className="font-medium text-ink">Not drawn at this zoom.</span> {what} draw from zoom {zoomLabel(from)}; the map is at {zoomLabel(zoom)}.
      </p>
      <Button variant="outline" size="sm" onClick={() => engine.current?.zoomTo(from + 0.15)} className="h-7 shrink-0 px-2 text-[12px] max-md:h-9">
        Zoom in
      </Button>
    </div>
  )
}

function SourceSection({ layer, source, data, onRelate }: { layer: Layer; source: Source; data: LayerData | undefined; onRelate: () => void }) {
  const sample = data?.origin === 'sample'
  const admin = useQuery(adminLevelsQuery()).data
  const sampleBadge = sample && (
    <Badge variant="outline" title="Bundled sample data: the engine does not serve this source">
      Sample
    </Badge>
  )
  if (source.kind === 'indicator') {
    const meta = indicatorMeta(source.indicator, source.dims)
    const real = !!meta?.real
    // the engine's indicators for a layer drawn from the engine; sample ones for a sample layer
    const options = INDICATOR_META.filter((m) => m.levels.includes(source.level) && !!m.real === real)
    return (
      <Section title="Data" aside={<span className="flex gap-1.5">{sampleBadge}{meta && <Badge variant={CLASS_NOTE[meta.class].variant} title={CLASS_NOTE[meta.class].note}>{CLASS_NOTE[meta.class].label}</Badge>}</span>}>
        {layer.type !== 'bivariate' && (
          <Field label="Indicator" stack>
            <Picker
              value={meta?.id ?? source.indicator}
              label="Indicator"
              className="h-9 text-[13px]"
              options={options.map((m) => ({ value: m.id, label: m.label }))}
              onChange={(id) => {
                const next = indicatorMeta(id)!
                actions.setSource(layer.id, (s) => ({ ...s, indicator: next.indicator ?? id, dims: next.dims ? { ...next.dims } : undefined, period: next.periods.length > 1 ? (s as typeof source).period : 'latest' }) as Source, 'Indicator changed')
                if (layer.name === meta?.label) actions.rename(layer.id, next.label)
                // a new variable starts on its own ramp, with breaks of its own
                actions.setStyle(layer.id, (st) => (isBinding(st.fill) ? { ...st, fill: { ...st.fill, scale: { ...st.fill.scale, ramp: next.ramp, breaks: undefined, type: st.fill.scale.type === 'manual' ? 'quantile' : st.fill.scale.type } } } : st))
              }}
            />
          </Field>
        )}
        <Field label="Units">
          <Seg
            value={String(source.level)}
            label="Admin level"
            onChange={(v) => actions.setSource(layer.id, (s) => ({ ...s, level: Number(v), ...(Number(v) >= 3 && !real ? { parent: 'LK1' } : { parent: undefined }) }) as Source, 'Admin level changed')}
            options={levelOptions(real ? admin : null, { has: (l) => (meta?.levels ?? [1, 2]).includes(l), sample: !real })}
            className="[&_button]:font-mono [&_button]:text-[11px]"
          />
        </Field>
        <ReachNotice layer={layer} data={data} what={LEVELS[source.level].plural.replace(/^./, (c) => c.toUpperCase())} />
        {layer.type !== 'bivariate' && <PeriodField layer={layer} source={source} periods={meta?.periods ?? []} />}
        {meta && <p className="text-[12px] leading-[1.5] text-ink3">{meta.description}</p>}
      </Section>
    )
  }
  if (source.kind === 'grid') {
    const meta = gridMeta(source.grid)
    return (
      <Section title="Data" aside={<span className="flex gap-1.5">{sampleBadge}{meta && <Badge variant={CLASS_NOTE[meta.class].variant} title={CLASS_NOTE[meta.class].note}>{CLASS_NOTE[meta.class].label}</Badge>}</span>}>
        <Field label="Grid">
          <span className="truncate text-[12.5px] text-ink">{meta?.label ?? source.grid}</span>
        </Field>
        <PeriodField layer={layer} source={source} periods={meta?.periods ?? []} />
        {meta && <p className="text-[12px] leading-[1.5] text-ink3">{meta.description} {data?.kind === 'values' ? 'Value tiles: the 0.001° cells themselves from zoom 10, coarser summaries below.' : data?.kind === 'dem' ? 'Terrain tiles, zoom 5 to 11.' : 'Shown at about 500 m.'}</p>}
      </Section>
    )
  }
  if (source.kind === 'features') {
    const meta = featureMeta(source.dataset)
    const n = data?.kind === 'features' ? data.count ?? data.collection.features.length : 0
    return (
      <Section title="Data" aside={<span className="flex gap-1.5">{sampleBadge}{meta && <Badge variant={CLASS_NOTE[meta.class].variant} title={CLASS_NOTE[meta.class].note}>{CLASS_NOTE[meta.class].label}</Badge>}</span>}>
        <Field label="Features">
          <span className="truncate text-[12.5px] text-ink">
            {meta?.label ?? source.dataset}
            {source.featureKind ? ` · ${source.featureKind.replace(/_/g, ' ')}` : ''}
          </span>
          {n > 0 && <span className="t-label t-num shrink-0">{n.toLocaleString('en')}</span>}
        </Field>
        {data?.kind === 'features' && data.tiles && <p className="text-[12px] leading-[1.5] text-ink3">Too many features for one request: drawn from vector tiles, so it has no histogram here.</p>}
        <ReachNotice layer={layer} data={data} what={meta?.label ?? 'These features'} />
      </Section>
    )
  }
  if (source.kind === 'analysis')
    return (
      <Section title="Data">
        <p className="text-[12.5px] leading-[1.5] text-ink2">
          An output of the analysis in this map (<span className="font-mono text-[11.5px]">{source.ref}.{source.output}</span>). It recomputes when the analysis changes.
        </p>
        <Button variant="outline" size="sm" onClick={onRelate} className="w-full max-md:h-10">
          <ChartScatter />
          Open in Relate
        </Button>
      </Section>
    )
  return (
    <Section title="Data" aside={sampleBadge || undefined}>
      <Field label="Units">
        <Seg
          value={String(source.level)}
          label="Admin level"
          onChange={(v) => {
            actions.setSource(layer.id, (s) => ({ ...s, level: Number(v), parent: Number(v) >= 3 && sample ? 'LK1' : undefined }) as Source, 'Admin level changed')
            // a layer still called what its level is called follows the level, so its name says what it draws
            const next = boundaryName(layer, source.level, Number(v))
            if (next) actions.rename(layer.id, next)
          }}
          options={levelOptions(sample ? null : admin, { sample })}
          className="[&_button]:font-mono [&_button]:text-[11px]"
        />
      </Field>
      {source.kind === 'boundaries' && <ReachNotice layer={layer} data={data} what={LEVELS[source.level].plural.replace(/^./, (c) => c.toUpperCase())} />}
    </Section>
  )
}

/* ---------------- channels ---------------- */
const featureValues = (data: LayerData | undefined, field: string): number[] => (data?.kind === 'features' ? data.collection.features.map((f) => Number(f.properties?.[field])).filter(Number.isFinite) : [])
const fieldsOf = (data: LayerData | undefined): FieldMeta[] => (data?.kind === 'features' ? data.fields : [])

/** A colour: none, one tone, or a field through a scale. */
function ColorChannel({ value, onChange, data, scheme, allowNone = false, label }: { value: string | FieldBinding | undefined; onChange: (v: string | FieldBinding, key?: string) => void; data: LayerData | undefined; scheme: Theme; allowNone?: boolean; label: string }) {
  const fields = fieldsOf(data)
  const mode = isBinding(value) ? 'field' : value === 'none' ? 'none' : 'single'
  const field = isBinding(value) ? fields.find((f) => f.id === value.field) : undefined
  const bind = (f: FieldMeta): FieldBinding => ({ field: f.id, scale: f.kind === 'category' ? { type: 'categorical', ramp: 'ldl.cat.default' } : { type: 'quantile', classes: 5, ramp: 'ldl.seq.orange' } })
  const values = useMemo(() => (field?.kind === 'number' ? featureValues(data, field.id) : []), [data, field])
  const cats = isBinding(value) && field?.kind === 'category' ? resolveScale([], value.scale, scheme, field.categories) : null
  return (
    <>
      <Seg
        value={mode}
        label={label}
        onChange={(m) => onChange(m === 'none' ? 'none' : m === 'single' ? 'ldl.tone.ink' : bind(fields[0]))}
        options={[...(allowNone ? [{ value: 'none' as const, label: 'None' }] : []), { value: 'single' as const, label: 'One colour' }, { value: 'field' as const, label: 'By field', disabled: fields.length === 0 }]}
      />
      {mode === 'single' && <TonePicker value={typeof value === 'string' ? value : undefined} scheme={scheme} onChange={(t) => onChange(t)} label={label} extra={['paper']} />}
      {isBinding(value) && (
        <>
          <Field label="Field">
            <Picker value={value.field} label="Field" options={fields.map((f) => ({ value: f.id, label: f.label, note: f.kind === 'category' ? 'categories' : f.unit }))} onChange={(id) => onChange(bind(fields.find((f) => f.id === id)!))} />
          </Field>
          {cats?.kind === 'categorical' ? (
            <ul className="grid grid-cols-2 gap-x-3 gap-y-1.5">
              {cats.keys.map((k, i) => (
                <li key={k} className="flex min-w-0 items-center gap-2 text-[12.5px] text-ink2">
                  <span className="size-2.5 shrink-0 rounded-full" style={{ background: cats.colors[i] }} />
                  <span className="truncate capitalize">{k}</span>
                </li>
              ))}
            </ul>
          ) : field?.kind === 'number' ? (
            values.length ? (
              <ScaleEditor scale={value.scale} values={values} unit={field.unit ?? ''} scheme={scheme} smooth onChange={(scale, key) => onChange({ ...value, scale }, key)} />
            ) : (
              <Field label="Ramp">
                <RampPicker value={value.scale.ramp ?? 'ldl.seq.orange'} reverse={!!value.scale.reverse} scheme={scheme} onChange={(ramp) => onChange({ ...value, scale: { ...value.scale, ramp } })} onReverse={(reverse) => onChange({ ...value, scale: { ...value.scale, reverse } })} />
              </Field>
            )
          ) : null}
        </>
      )}
    </>
  )
}

/** A size or a width: one number, or a field mapped onto a pixel range (or a number per category). */
function SizeChannel({ value, onChange, data, min, max, step = 0.5, fallback, label, unit = 'px' }: { value: number | FieldBinding | undefined; onChange: (v: number | FieldBinding, key?: string) => void; data: LayerData | undefined; min: number; max: number; step?: number; fallback: number; label: string; unit?: string }) {
  const fields = fieldsOf(data)
  const bound = isBinding(value)
  const field = bound ? fields.find((f) => f.id === value.field) : undefined
  const [fixed, setFixed] = useLive(typeof value === 'number' ? value : fallback)
  const range = bound ? (value.scale.range ?? [min, max]) : [min, max]
  const bind = (f: FieldMeta): FieldBinding => (f.kind === 'category' ? { field: f.id, scale: { type: 'categorical', mapping: Object.fromEntries((f.categories ?? []).map((c, i, a) => [c, +(max * 0.6 - ((max * 0.6 - min) * i) / Math.max(1, a.length - 1)).toFixed(1)])) } } : { field: f.id, scale: { type: 'continuous', transform: 'sqrt', range: [Math.max(min, fallback * 0.5), Math.min(max, fallback * 2.6)] } })
  return (
    <>
      <Seg
        value={bound ? 'field' : 'fixed'}
        label={label}
        onChange={(m) => onChange(m === 'fixed' ? fallback : bind(fields.find((f) => f.kind === 'number') ?? fields[0]))}
        options={[
          { value: 'fixed', label: 'Fixed' },
          { value: 'field', label: 'By field', disabled: fields.length === 0 },
        ]}
      />
      {!bound && (
        <Field label={label}>
          <NumberSlider value={fixed} min={min} max={max} step={step} label={label} format={(v) => `${v}${unit}`} onChange={(v) => (setFixed(v), onChange(v, 'size'))} />
        </Field>
      )}
      {bound && (
        <>
          <Field label="Field">
            <Picker value={value.field} label="Field" options={fields.map((f) => ({ value: f.id, label: f.label, note: f.kind === 'category' ? 'categories' : f.unit }))} onChange={(id) => onChange(bind(fields.find((f) => f.id === id)!))} />
          </Field>
          {field?.kind === 'category' && value.scale.mapping ? (
            Object.entries(value.scale.mapping).map(([k, v]) => (
              <Field key={k} label={k}>
                <NumberSlider value={Number(v)} min={min} max={max} step={0.1} label={`${label} for ${k}`} format={(n) => `${n}${unit}`} onChange={(n) => onChange({ ...value, scale: { ...value.scale, mapping: { ...value.scale.mapping, [k]: n } } }, `size:${k}`)} />
              </Field>
            ))
          ) : (
            <Field label="Range" stack hint={`${range[0]} – ${range[1]} ${unit}`}>
              <Slider className="max-md:h-8" min={min} max={max} step={step} minStepsBetweenThumbs={1} value={range} aria-label={`${label} range`} onValueChange={([a, b]) => onChange({ ...value, scale: { ...value.scale, range: [a, b] } }, 'range')} />
            </Field>
          )}
        </>
      )}
    </>
  )
}

function OutlineSection({ layer, data, scheme, bindable = false, dash = false, title = 'Outline' }: { layer: Layer; data: LayerData | undefined; scheme: Theme; bindable?: boolean; dash?: boolean; title?: string }) {
  const stroke = layer.style.stroke ?? {}
  const set = (patch: Partial<Stroke>, key?: string) => actions.setStyle(layer.id, (s) => ({ ...s, stroke: { ...s.stroke, ...patch } }), key)
  const [w, setW] = useLive(typeof stroke.width === 'number' ? stroke.width : 0.5)
  const fixedWidth = !isBinding(stroke.width)
  return (
    <Section title={title} defaultOpen={bindable}>
      {bindable ? (
        <ColorChannel value={stroke.color} onChange={(color, key) => set({ color }, key)} data={data} scheme={scheme} label="Colour" />
      ) : (
        <Field label="Colour" stack>
          <TonePicker value={typeof stroke.color === 'string' ? stroke.color : undefined} scheme={scheme} onChange={(color) => set({ color })} label="Outline colour" extra={['paper']} />
        </Field>
      )}
      {bindable ? (
        <SizeChannel value={stroke.width} onChange={(width, key) => set({ width }, key)} data={data} min={0.2} max={6} step={0.1} fallback={1.2} label="Width" />
      ) : (
        fixedWidth && (
          <Field label="Width">
            <NumberSlider value={w} min={0} max={3} step={0.1} label="Outline width" format={(v) => (v === 0 ? 'off' : `${v}px`)} onChange={(v) => (setW(v), set({ width: v }, 'width'))} />
          </Field>
        )
      )}
      {dash && (
        <>
          <Field label="Line">
            <Seg
              value={dashKind(stroke.dash)}
              onChange={(d) => set({ dash: DASH[d] })}
              label="Line style"
              options={[
                { value: 'solid', label: 'Solid' },
                { value: 'dash', label: 'Dashed' },
                { value: 'dot', label: 'Dotted' },
              ]}
            />
          </Field>
          <Field label="Casing" htmlFor={`casing-${layer.id}`} hint="a light edge">
            <Switch id={`casing-${layer.id}`} checked={!!stroke['x-casing']} onCheckedChange={(v) => set({ 'x-casing': v })} />
          </Field>
        </>
      )}
    </Section>
  )
}

/* ---------------- filter ---------------- */
function FilterSection({ layer, data }: { layer: Layer; data: LayerData | undefined }) {
  const fields = useMemo(() => filterFields(layer, data), [layer.type, data])
  const clauses = clausesOf(layer.filter)
  const write = (next: Clause[]) => actions.setFilter(layer.id, filterOf(next))
  const unused = fields.filter((f) => !clauses.some((c) => c.field === f.id))
  if (fields.length === 0 && clauses.length === 0) return null
  const domainOf = (f: FieldMeta | undefined, c: Clause): [number, number] => f?.domain ?? (c.kind === 'range' ? [Math.min(0, c.range[0]), Math.max(1, c.range[1])] : [0, 1])
  return (
    <Section
      title="Filter"
      defaultOpen={clauses.length > 0}
      aside={
        unused.length > 0 && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button type="button" aria-label="Add a filter" className="pressable grid size-7 place-items-center rounded-md text-ink3 outline-none max-md:size-10 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg4 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
                <Plus className="size-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {unused.map((f) => (
                <DropdownMenuItem key={f.id} onSelect={() => write([...clauses, f.kind === 'category' ? { field: f.id, kind: 'in', values: f.categories ?? [] } : { field: f.id, kind: 'range', range: f.domain ?? [0, 1] }])}>
                  {f.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )
      }
    >
      {clauses.length === 0 && <p className="text-[12.5px] leading-[1.5] text-ink3">Everything is drawn. Add a filter to keep only part of it.</p>}
      {clauses.map((c, i) => {
        const f = fields.find((x) => x.id === c.field)
        const remove = (
          <button type="button" onClick={() => write(clauses.filter((_, k) => k !== i))} aria-label={`Remove the ${f?.label ?? c.field} filter`} className="pressable ml-auto grid size-6 shrink-0 place-items-center rounded-md text-ink3 outline-none max-md:size-9 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg4 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
            <X className="size-3" />
          </button>
        )
        if (c.kind === 'in') {
          const all = f?.categories ?? c.values
          return (
            <div key={c.field}>
              <div className="mb-1.5 flex items-center text-[12.5px] text-ink2">
                {f?.label ?? c.field}
                {remove}
              </div>
              <div className="flex flex-wrap gap-1.5">
                {all.map((v) => {
                  const on = c.values.includes(v)
                  return (
                    <button
                      key={v}
                      type="button"
                      aria-pressed={on}
                      onClick={() => write(clauses.map((x, k) => (k === i ? { ...c, values: on ? c.values.filter((y) => y !== v) : [...c.values, v] } : x)))}
                      className={cn('pressable h-7 rounded-full border px-2.5 text-[12px] capitalize outline-none max-md:h-9', on ? 'border-ink/70 bg-ink/10 text-ink' : 'border-line2 text-ink3 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink2')}
                    >
                      {v}
                    </button>
                  )
                })}
              </div>
            </div>
          )
        }
        const [d0, d1] = domainOf(f, c)
        const lo = Math.max(d0, c.range[0]), hi = Math.min(d1, c.range[1])
        const step = (d1 - d0) / 200 || 1
        return (
          <div key={c.field}>
            <div className="flex items-center text-[12.5px] text-ink2">
              {f?.label ?? c.field}
              <span className="t-num ml-2 font-mono text-[11px] text-ink3">
                {fmt(lo, f?.unit)} – {fmt(hi, f?.unit)} {unitSuffix(f?.unit ?? '')}
              </span>
              {remove}
            </div>
            <Slider className="mt-1 max-md:h-8" min={d0} max={d1} step={step} value={[lo, hi]} aria-label={`${f?.label ?? c.field} range`} onValueChange={([a, b]) => write(clauses.map((x, k) => (k === i ? { ...c, range: [a <= d0 ? -Infinity : a, b >= d1 ? Infinity : b] as [number, number] } : x)))} />
          </div>
        )
      })}
    </Section>
  )
}

/* ---------------- per type ---------------- */
type P = { layer: Layer; source: Source; data: LayerData | undefined; scheme: Theme }
const setStyle = (layer: Layer) => (fn: (s: Style) => Style, key?: string) => actions.setStyle(layer.id, fn, key)

function ChoroplethSections({ layer, source, data, scheme }: P) {
  const time = useWs((s) => s.doc!.time)
  const binding = isBinding(layer.style.fill) ? layer.style.fill : DEFAULT_FILL
  const frame = data?.kind === 'frame' ? data : null
  const { values, unit, estimates } = useMemo(() => {
    if (!frame) return { values: [] as number[], unit: '', estimates: false }
    const cols = columnsOf(frame, binding.field, source)
    const col = columnAt(frame, binding.field, source, time).col
    const pool = (binding.scale.across ?? 'all_periods') === 'all_periods' ? cols.flatMap((c) => c.values) : (col?.values ?? [])
    return { values: pool.filter(num), unit: col?.unit ?? '', estimates: !!col?.basis?.some((b) => b === 'estimated') && col.class !== 'estimated' }
  }, [frame, binding.field, binding.scale.across, source, time])
  const patch = setStyle(layer)
  return (
    <>
      <Section title="Colour" aside={<span className="t-label">{unitSuffix(unit)}</span>}>
        {values.length ? <ScaleEditor scale={binding.scale} values={values} unit={unit} scheme={scheme} onChange={(scale, key) => patch((s) => ({ ...s, fill: { ...binding, scale } }), key)} /> : <div className="skeleton h-[68px]" />}
        {frame && columnsOf(frame, binding.field, source).length > 1 && (
          <Field label="Breaks" htmlFor={`across-${layer.id}`} hint="same for every year">
            <Switch id={`across-${layer.id}`} checked={(binding.scale.across ?? 'all_periods') === 'all_periods'} onCheckedChange={(v) => patch((s) => ({ ...s, fill: { ...binding, scale: { ...binding.scale, across: v ? 'all_periods' : 'period' } } }))} />
          </Field>
        )}
        {estimates && (
          <Field label="Estimates" htmlFor={`hatch-${layer.id}`} hint="hatch modelled units">
            <Switch id={`hatch-${layer.id}`} checked={layer.style.uncertainty === 'hatch'} onCheckedChange={(v) => patch((s) => ({ ...s, uncertainty: v ? 'hatch' : 'off' }))} />
          </Field>
        )}
      </Section>
      <OutlineSection layer={layer} data={data} scheme={scheme} />
    </>
  )
}

function BivariateSections({ layer, source, data, scheme }: P) {
  const time = useWs((s) => s.doc!.time)
  const frame = data?.kind === 'frame' ? data : null
  const fill = isBivariate(layer.style.fill) ? layer.style.fill.bivariate : null
  const b = frame ? bivariateBreaks(layer, frame, source, time) : null
  const patch = setStyle(layer)
  if (!fill) return null
  const own = source.kind === 'indicator'
  const options = own ? INDICATOR_META.filter((m) => m.levels.includes(source.level)).map((m) => ({ value: m.id, label: m.label })) : []
  const setVar = (axis: 'x' | 'y', id: string) => {
    if (source.kind !== 'indicator') return
    const other = axis === 'x' ? fill.y.field : fill.x.field
    actions.setSource(layer.id, (s) => ({ ...s, indicator: axis === 'x' ? id : other, indicators: axis === 'x' ? [id, other] : [other, id] }) as Source, 'Variable changed')
    patch((s) => ({ ...s, fill: { bivariate: { ...fill, [axis]: { field: id, scale: { type: 'quantile', classes: 3 } } } } }))
  }
  return (
    <>
      <Section title="Variables">
        {(['x', 'y'] as const).map((axis) => (
          <Field key={axis} label={axis === 'x' ? 'Across (x)' : 'Up (y)'} stack>
            {own ? <Picker value={fill[axis].field} label={`${axis} variable`} className="h-9 text-[13px]" options={options} onChange={(id) => setVar(axis, id)} /> : <div className="flex h-9 items-center rounded-lg border border-line bg-bg3/40 px-2.5 text-[13px] text-ink2">{b?.[axis].label ?? '…'}</div>}
            {b && (
              <div className="t-num mt-1 font-mono text-[10.5px] text-ink3">
                thirds at {fmt(b[axis === 'x' ? 'bx' : 'by'][0], b[axis].unit)} and {fmt(b[axis === 'x' ? 'bx' : 'by'][1], b[axis].unit)} {unitSuffix(b[axis].unit)}
              </div>
            )}
          </Field>
        ))}
      </Section>
      <Section title="Palette">
        <div role="radiogroup" aria-label="Bivariate palette" className="grid grid-cols-3 gap-2">
          {BIVARIATE.map((p) => {
            const cs = bivariateColors(p.name, scheme)
            const on = fill.palette === p.name
            return (
              <button key={p.name} type="button" role="radio" aria-checked={on} onClick={() => patch((s) => ({ ...s, fill: { bivariate: { ...fill, palette: p.name } } }))} className={cn('pressable flex flex-col items-center gap-2 rounded-xl border p-2.5 outline-none', on ? 'border-ink/60 bg-ink/[0.06]' : 'border-line2 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/60')}>
                <span className="grid size-12 grid-cols-3 grid-rows-3 overflow-hidden rounded-[5px]">
                  {[6, 7, 8, 3, 4, 5, 0, 1, 2].map((i) => (
                    <span key={i} style={{ background: cs[i] }} />
                  ))}
                </span>
                <span className={cn('text-[11.5px] leading-tight', on ? 'text-ink' : 'text-ink2')}>{p.label}</span>
              </button>
            )
          })}
        </div>
        <Field label="Estimates" htmlFor={`hatch-${layer.id}`} hint="hatch modelled units">
          <Switch id={`hatch-${layer.id}`} checked={layer.style.uncertainty === 'hatch'} onCheckedChange={(v) => patch((s) => ({ ...s, uncertainty: v ? 'hatch' : 'off' }))} />
        </Field>
      </Section>
      <OutlineSection layer={layer} data={data} scheme={scheme} />
    </>
  )
}

function RasterSections({ layer, data, scheme }: P) {
  const grid = data?.kind === 'grid' || data?.kind === 'values' ? data : null
  const binding = isBinding(layer.style.fill) ? layer.style.fill : { field: 'value', scale: { type: 'continuous' as const, ramp: 'ldl.seq.blue' } }
  // value tiles carry a decoded overview of the island for this; the sample grid is sampled directly
  const all = useMemo(() => (!grid ? [] : grid.kind === 'grid' ? gridSample(grid.grid) : Array.from(grid.sample)), [grid])
  const categorical = binding.scale.type === 'categorical'
  const [d0, d1] = useMemo(() => extent(all), [all])
  const range = layer.style.range ?? [null, null]
  const inRange = useMemo(() => all.filter((v) => v >= (range[0] ?? -Infinity) && v <= (range[1] ?? Infinity)), [all, range[0], range[1]])
  const patch = setStyle(layer)
  const live = useRafCallback((r: [number | null, number | null]) => patch((s) => ({ ...s, range: r }), 'range'))
  const [shown, setShown, endShown] = useLive<[number, number]>([range[0] ?? d0, range[1] ?? d1])
  // square-root spacing: most grids are skewed, and the low end is where a mask is set
  const span = d1 - d0 || 1
  const to = (v: number) => ((v - d0) / span) ** 0.5 * 1000
  const from = (t: number) => d0 + (t / 1000) ** 2 * span
  return (
    <>
      <Section title="Colour" aside={<span className="t-label">{unitSuffix(grid?.unit ?? '')}</span>}>
        {categorical ? (
          <p className="text-[12px] leading-[1.5] text-ink3">Each class in its published colour; the key lists them.</p>
        ) : grid && inRange.length ? (
          <ScaleEditor scale={binding.scale} values={inRange} unit={grid.unit} scheme={scheme} smooth onChange={(scale, key) => patch((s) => ({ ...s, fill: { ...binding, scale } }), key)} />
        ) : (
          <div className="skeleton h-[68px]" />
        )}
      </Section>
      {!categorical && <Section title="Range" aside={grid && <span className="t-label t-num">{fmt(shown[0], grid.unit)} – {fmt(shown[1], grid.unit)}</span>}>
        <Slider
          className="max-md:h-8"
          min={0}
          max={1000}
          step={1}
          minStepsBetweenThumbs={8}
          disabled={!grid}
          aria-label="Value range drawn"
          value={[to(shown[0]), to(shown[1])]}
          onValueChange={([a, b]) => {
            const next: [number, number] = [from(a), from(b)]
            setShown(next)
            live([a <= 0 ? null : +next[0].toPrecision(3), b >= 1000 ? null : +next[1].toPrecision(3)])
          }}
          onValueCommit={endShown}
        />
        <p className="text-[12px] leading-[1.5] text-ink3">Cells outside the range are not drawn, so what is below shows through.</p>
      </Section>}
      <Section title="Rendering">
        <Field label="Cells">
          <Seg
            value={layer.style.resample ?? 'linear'}
            onChange={(r) => patch((s) => ({ ...s, resample: r }))}
            label="Resampling"
            options={[
              { value: 'linear', label: 'Smooth', hint: 'Interpolated between cell centres' },
              { value: 'nearest', label: 'Crisp', hint: 'Every cell as a square' },
            ]}
          />
        </Field>
        {grid?.kind === 'grid' ? (
          <Field label="Relief" htmlFor={`relief-${layer.id}`} hint="shade with the terrain">
            <Switch id={`relief-${layer.id}`} checked={!!layer.style['x-relief']} onCheckedChange={(v) => patch((s) => ({ ...s, 'x-relief': v }))} />
          </Field>
        ) : (
          <p className="text-[12px] leading-[1.5] text-ink3">For relief, add Terrain relief above this layer with a multiply blend.</p>
        )}
      </Section>
    </>
  )
}

function PointSections({ layer, data, scheme }: P) {
  const patch = setStyle(layer)
  const clustered = layer.type === 'cluster'
  const [radius, setRadius] = useLive(layer.style.radius ?? 44)
  return (
    <>
      {!clustered && (
        <>
          <Section title="Colour">
            <ColorChannel value={layer.style.fill as string | FieldBinding | undefined} onChange={(fill, key) => patch((s) => ({ ...s, fill }), key)} data={data} scheme={scheme} label="Colour" />
          </Section>
          <Section title="Size">
            <SizeChannel value={layer.style.size} onChange={(size, key) => patch((s) => ({ ...s, size }), key)} data={data} min={1} max={18} fallback={4} label="Radius" />
          </Section>
        </>
      )}
      <Section title="Clustering">
        <Field label="Cluster" htmlFor={`cluster-${layer.id}`} hint="counts that split as you zoom">
          <Switch id={`cluster-${layer.id}`} checked={clustered} onCheckedChange={(v) => actions.setType(layer.id, v ? 'cluster' : 'point', v ? { ...layer.style, radius: 44, aggregate: 'count', fill: typeof layer.style.fill === 'string' ? layer.style.fill : 'ldl.tone.teal' } : undefined)} />
        </Field>
        {clustered && (
          <>
            <Field label="Reach">
              <NumberSlider value={radius} min={20} max={90} step={2} label="Cluster radius" format={(v) => `${v}px`} onChange={setRadius} onCommit={(v) => patch((s) => ({ ...s, radius: v }))} />
            </Field>
            <Field label="Colour" stack>
              <TonePicker value={typeof layer.style.fill === 'string' ? layer.style.fill : undefined} scheme={scheme} onChange={(fill) => patch((s) => ({ ...s, fill }))} label="Cluster colour" />
            </Field>
          </>
        )}
      </Section>
      {!clustered && <OutlineSection layer={layer} data={data} scheme={scheme} />}
    </>
  )
}

function HeatSections({ layer, data, scheme }: P) {
  const patch = setStyle(layer)
  const [radius, setRadius] = useLive(layer.style.radius ?? 22)
  const [intensity, setIntensity] = useLive(layer.style.intensity ?? 1)
  const weights = fieldsOf(data).filter((f) => f.kind === 'number')
  return (
    <Section title="Density">
      <Field label="Radius">
        <NumberSlider value={radius} min={6} max={60} step={1} label="Kernel radius" format={(v) => `${v}px`} onChange={(v) => (setRadius(v), patch((s) => ({ ...s, radius: v }), 'radius'))} />
      </Field>
      <Field label="Intensity">
        <NumberSlider value={intensity} min={0.2} max={3} step={0.1} label="Intensity" format={(v) => `${v.toFixed(1)}×`} onChange={(v) => (setIntensity(v), patch((s) => ({ ...s, intensity: v }), 'intensity'))} />
      </Field>
      <Field label="Weight">
        <Picker value={layer.style.weight ?? '∅'} label="Weight field" options={[{ value: '∅', label: 'Each point counts once' }, ...weights.map((f) => ({ value: f.id, label: f.label }))]} onChange={(w) => patch((s) => ({ ...s, weight: w === '∅' ? null : w }))} />
      </Field>
      <Field label="Ramp">
        <RampPicker value={layer.style.ramp ?? 'ldl.seq.orange'} reverse={!!layer.style.reverse} scheme={scheme} onChange={(ramp) => patch((s) => ({ ...s, ramp }))} onReverse={(reverse) => patch((s) => ({ ...s, reverse }))} />
      </Field>
    </Section>
  )
}

function PolygonSections({ layer, data, scheme }: P) {
  const patch = setStyle(layer)
  return (
    <>
      <Section title="Fill">
        <ColorChannel value={layer.style.fill as string | FieldBinding | undefined} onChange={(fill, key) => patch((s) => ({ ...s, fill }), key)} data={data} scheme={scheme} allowNone label="Fill" />
      </Section>
      <OutlineSection layer={layer} data={data} scheme={scheme} bindable />
    </>
  )
}

/** The most layers a split makes: past this a legend says it better than a list of layers. */
const SPLIT_MAX = 24
/** The colour a layer draws a category in now, so a split keeps the picture as it was. */
function colourBinding(layer: Layer): FieldBinding | undefined {
  const c = layer.type === 'line' ? layer.style.stroke?.color : layer.type === 'polygon' && (layer.style.fill === 'none' || layer.style.fill === undefined) ? layer.style.stroke?.color : layer.style.fill
  return isBinding(c) ? c : undefined
}

/** Split a layer into one layer per category: each can then be recoloured, hidden or labelled on its own. */
function SplitSection({ layer, data }: { layer: Layer; data: LayerData | undefined }) {
  const { scheme } = useWorkspace()
  const bound = colourBinding(layer)
  const fields = fieldsOf(data).filter((f) => f.kind === 'category' && (f.categories?.length ?? 0) >= 2)
  const [field, setField] = useState<string | undefined>(undefined)
  const pick = field && fields.some((f) => f.id === field) ? field : bound && fields.some((f) => f.id === bound.field) ? bound.field : fields[0]?.id
  const meta = fields.find((f) => f.id === pick)
  const parts = useMemo(() => {
    if (!meta) return []
    // keep the colours the layer draws in now when it is coloured by this field; else the categorical palette
    const scale = bound?.field === meta.id && bound.scale.type === 'categorical' ? bound.scale : { type: 'categorical' as const, ramp: 'ldl.cat.default' }
    const r = resolveScale([], scale, scheme, meta.categories)
    const colourOf = (v: string) => (r.kind === 'categorical' ? (r.colors[r.keys.indexOf(v)] ?? r.other) : '#888888')
    const features = data?.kind === 'features' ? data.collection.features : []
    return (meta.categories ?? []).map((value) => {
      // a value whose features all share one name is called by it (a route by "CM01 Makumbura to Pettah")
      const names = new Set(features.filter((f) => String(f.properties?.[meta.id]) === value).map((f) => f.properties?.name).filter((n): n is string => typeof n === 'string' && !!n))
      return { value, name: names.size === 1 ? [...names][0] : value.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase()), color: colourOf(value) }
    })
  }, [meta, bound, scheme, data])
  if (!fields.length) return null
  const tooMany = parts.length > SPLIT_MAX
  return (
    <Section title="Sub-layers" defaultOpen={false}>
      <p className="text-[12px] leading-[1.5] text-ink3">One layer per value, grouped under this one, so each can have its own colour, labels and visibility.</p>
      <Field label="Split by">
        <Picker value={pick} label="Split by field" options={fields.map((f) => ({ value: f.id, label: f.label }))} onChange={setField} />
      </Field>
      {parts.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {parts.slice(0, 12).map((p) => (
            <span key={p.value} className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-line px-1.5 py-0.5 text-[11.5px] text-ink2">
              <span className="size-2 shrink-0 rounded-full" style={{ background: p.color }} />
              <span className="truncate">{p.name}</span>
            </span>
          ))}
          {parts.length > 12 && <span className="text-[11.5px] text-ink3">+{parts.length - 12} more</span>}
        </div>
      )}
      <Button variant="outline" size="sm" disabled={!meta || tooMany || parts.length < 2} onClick={() => meta && actions.split(layer.id, meta.id, parts)} className="w-full max-md:h-10">
        <Ungroup className="rotate-180" />
        {tooMany ? `${parts.length} values: too many to split` : `Split into ${parts.length} layers`}
      </Button>
    </Section>
  )
}

/** The field a layer keeps one value of (`route_id = CM02`, as a split's parts do): its natural label. */
function keptField(f: Layer['filter']): string | undefined {
  if (!f) return undefined
  if (f.op === '=' && typeof f.args[0] === 'object') return f.args[0].property
  if (f.op === 'and') return keptField(f.args[f.args.length - 1])
  return undefined
}

function LabelSection({ layer, data }: { layer: Layer; data: LayerData | undefined }) {
  const label = layer.style.label
  // a layer of one value is called by it ("CM02"): short enough to run along a winding line, where a long name does not fit
  const kept = keptField(layer.filter)
  const fallback = kept ? `{${kept}}` : '{name}'
  const patch = setStyle(layer)
  const [size, setSize] = useLive(label?.size ?? 11)
  const fields = [{ value: '{name}', label: 'Name' }, ...(data?.kind === 'frame' || data?.kind === 'boundaries' ? [{ value: '{pcode}', label: 'P-code' }] : fieldsOf(data).map((f) => ({ value: `{${f.id}}`, label: f.label })))]
  const own = layer.type === 'label'
  return (
    <Section title="Labels" defaultOpen={own || !!label}>
      {!own && (
        <Field label="Show" htmlFor={`label-${layer.id}`}>
          <Switch id={`label-${layer.id}`} checked={!!label} onCheckedChange={(v) => patch((s) => ({ ...s, label: v ? { text: fallback, size: 11 } : undefined }))} />
        </Field>
      )}
      {(own || label) && (
        <>
          <Field label="Text">
            <Picker value={label?.text ?? '{name}'} label="Label text" options={fields} onChange={(text) => patch((s) => ({ ...s, label: { size: 11, ...s.label, text } }))} />
          </Field>
          <Field label="Size">
            <NumberSlider value={size} min={8} max={18} step={0.5} label="Label size" format={(v) => `${v}px`} onChange={(v) => (setSize(v), patch((s) => ({ ...s, label: { text: '{name}', ...s.label, size: v } }), 'label'))} />
          </Field>
          <Field label="Halo" htmlFor={`halo-${layer.id}`}>
            <Switch id={`halo-${layer.id}`} checked={label?.halo !== 'none'} onCheckedChange={(v) => patch((s) => ({ ...s, label: { text: '{name}', size: 11, ...s.label, halo: v ? undefined : 'none' } }))} />
          </Field>
        </>
      )}
    </Section>
  )
}

function LegendSection({ layer }: { layer: Layer }) {
  return (
    <Section title="Legend" defaultOpen={false}>
      <Field label="Show" htmlFor={`legend-${layer.id}`}>
        <Switch id={`legend-${layer.id}`} checked={layer.legend.show} onCheckedChange={(show) => actions.setLegend(layer.id, { ...layer.legend, show })} />
      </Field>
      <Field label="Title" stack>
        <input
          value={layer.legend.title ?? ''}
          placeholder={layer.name}
          aria-label="Legend title"
          data-sheet-nodrag
          onChange={(e) => actions.setLegend(layer.id, { ...layer.legend, title: e.target.value || null })}
          className="h-8 w-full rounded-lg border border-line2 bg-bg2/60 px-2.5 text-[12.5px] text-ink outline-none placeholder:text-ink3 focus:border-ink3 max-md:h-10"
        />
      </Field>
    </Section>
  )
}

/* ---------------- shell ---------------- */
function Header({ node, scheme, children }: { node: Layer | Group; scheme: Theme; children: ReactNode }) {
  return (
    <div className="flex items-start gap-3 px-4 pt-3.5 pb-3.5">
      <LayerThumb node={node} scheme={scheme} className="mt-0.5 size-8 rounded-lg" />
      <div className="min-w-0 flex-1">
        <input
          key={node.id}
          defaultValue={node.name}
          aria-label="Layer name"
          data-sheet-nodrag
          disabled={node.locked}
          onBlur={(e) => (e.target.value.trim() ? actions.rename(node.id, e.target.value) : (e.target.value = node.name))}
          onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
          className="-mx-1.5 -my-0.5 w-[calc(100%+12px)] truncate rounded-md border border-transparent bg-transparent px-1.5 py-0.5 text-[14.5px] font-semibold tracking-[-0.015em] text-ink outline-none focus:border-line2 focus:bg-bg2/70 [@media(hover:hover)_and_(pointer:fine)]:hover:border-line2"
        />
        <div className="t-label mt-1.5 truncate">{children}</div>
      </div>
    </div>
  )
}

function GroupInspector({ group, scheme }: { group: Group; scheme: Theme }) {
  const { engine } = useWorkspace()
  const [live, setLive, end] = useLive(Math.round(group.opacity * 100))
  const preview = useRafCallback((v: number) => engine.current?.previewOpacity(group.id, v))
  const layers = allLayers([group])
  return (
    <>
      <Header node={group} scheme={scheme}>
        GROUP · {layers.length} {layers.length === 1 ? 'layer' : 'layers'}
      </Header>
      <Section title="Group">
        <Field label="Opacity">
          <NumberSlider
            value={live}
            min={0}
            max={100}
            label="Group opacity"
            format={(v) => `${v}%`}
            onChange={(v) => (setLive(v), preview(v / 100))}
            onCommit={(v) => {
              preview.cancel()
              actions.setOpacity(group.id, v / 100)
              engine.current?.previewOpacity(group.id, null)
              end()
            }}
          />
        </Field>
        <p className="text-[12px] leading-[1.5] text-ink3">A group fades as one image: its layers do not show through each other. Blend modes are set on the layers inside.</p>
        <Button variant="outline" size="sm" onClick={() => actions.ungroup(group.id)} disabled={group.locked} className="max-md:h-10">
          <Ungroup />
          Ungroup
        </Button>
      </Section>
    </>
  )
}

export function Inspector({ onRelate }: { onRelate: () => void }) {
  const selected = useWs((s) => s.selected)
  const node = useNode(selected)
  const doc = useDoc()
  const { data, failed, scheme: mapScheme, engine, tick } = useWorkspace()
  void tick
  if (!node || !doc)
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-8 pb-10 text-center">
        <p className="text-[13.5px] font-medium text-ink">No layer selected</p>
        <p className="text-[12.5px] leading-[1.5] text-ink3">Select a layer to change what it shows and how it is drawn. Click it in the list, or on the map.</p>
      </div>
    )
  if (isGroup(node)) return <GroupInspector group={node} scheme={mapScheme} />
  const source = doc.sources[node.source]
  const d = data.get(node.id)
  const scheme = layerScheme(node.blend, mapScheme)
  // the engine's refusal first: a layer it could not resolve draws nothing at all
  const problem = failed.get(node.id) ?? engine.current?.problem(node.id)
  const p: P = { layer: node, source, data: d, scheme }
  const id = source?.kind === 'indicator' ? source.indicator : source?.kind === 'grid' ? source.grid : source?.kind === 'features' ? source.dataset : source?.kind === 'analysis' ? `${source.ref}.${source.output}` : source ? `adm${source.level}` : '—'
  return (
    <div className={cn(node.locked && 'pointer-events-none opacity-60')} aria-disabled={node.locked}>
      <Header node={node} scheme={mapScheme}>
        <span className="uppercase">{LAYER_TYPE_LABEL[node.type]}</span> · {id}
      </Header>
      {node.locked && <p className="mx-4 mb-3 rounded-lg bg-bg3/60 px-3 py-2 text-[12px] text-ink2">Locked. Unlock the layer to edit it.</p>}
      {problem && (
        <p className="mx-4 mb-3 flex gap-2 rounded-lg bg-warn-soft px-3 py-2 text-[12px] leading-[1.45] text-ink">
          <TriangleAlert className="mt-px size-3.5 shrink-0 text-warn" />
          {problem}
        </p>
      )}
      {source && <SourceSection layer={node} source={source} data={d} onRelate={onRelate} />}
      {source && node.type === 'choropleth' && <ChoroplethSections {...p} />}
      {source && node.type === 'bivariate' && <BivariateSections {...p} />}
      {source && node.type === 'raster' && <RasterSections {...p} />}
      {node.type === 'hillshade' && <HillshadeSection layer={node} />}
      {source && (node.type === 'point' || node.type === 'cluster') && <PointSections {...p} />}
      {source && node.type === 'heatmap' && <HeatSections {...p} />}
      {source && node.type === 'polygon' && <PolygonSections {...p} />}
      {node.type === 'line' && <OutlineSection layer={node} data={d} scheme={scheme} bindable dash title="Line" />}
      <FilterSection layer={node} data={d} />
      {(node.type === 'line' || node.type === 'polygon' || node.type === 'point') && <SplitSection layer={node} data={d} />}
      {(node.type === 'choropleth' || node.type === 'bivariate' || node.type === 'point' || node.type === 'line' || node.type === 'polygon' || node.type === 'label') && <LabelSection layer={node} data={d} />}
      {node.type !== 'label' && node.type !== 'hillshade' && <LegendSection layer={node} />}
    </div>
  )
}

function HillshadeSection({ layer }: { layer: Layer }) {
  const [z, setZ] = useLive(layer.style.exaggeration ?? 1)
  return (
    <Section title="Relief">
      <Field label="Exaggeration">
        <NumberSlider value={z} min={0.5} max={3} step={0.5} label="Vertical exaggeration" format={(v) => `${v}×`} onChange={setZ} onCommit={(v) => actions.setStyle(layer.id, (s) => ({ ...s, exaggeration: v }))} />
      </Field>
      <p className="text-[12px] leading-[1.5] text-ink3">Lit from the north-west. Set the layer to Multiply to shade whatever is below it.</p>
    </Section>
  )
}
