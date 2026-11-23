// Export: the map as an image and its layers as data, optionally clipped to one admin area. With an area,
// the image is framed on it and cut to its outline, and each layer keeps only what lies in it. Several
// files leave as one zip, with a note of where every layer came from and under what licence.
import { useQuery } from '@tanstack/react-query'
import { Check, ChevronsUpDown, Download, FileImage, FileSpreadsheet, Loader2, MapPinned, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { apiAvailable, type Attribution } from '@/api/http'
import { LEVELS } from '@/api/workspace'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { allLayers, type MapDocument } from '@/lib/map-spec'
import { cn } from '@/lib/utils'
import { resolvedFor, shownLayers, useResolved } from './context'
import { areaShape, download, exportable, layerData, listAreas, slug, zip, type Area, type AreaShape, type DataFormat, type ExportFile } from './exportData'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  doc: MapDocument
  solo: string | null
  credits: string[]
  /** the image, framed on and cut to `area` when there is one */
  render: (area?: { shape: AreaShape; mask: boolean; name: string }) => Promise<Blob | null>
}

const offlineNow = (available: boolean | undefined) => available === false
const LEVEL_GROUP: Record<number, string> = { 1: 'Provinces', 2: 'Districts', 3: 'DS divisions' }

export function ExportDialog({ open, onOpenChange, doc, solo, credits, render }: Props) {
  const engine = useQuery({ queryKey: ['api-available'], queryFn: apiAvailable, staleTime: Infinity, enabled: open })
  const areas = useQuery({ queryKey: ['export-areas'], queryFn: listAreas, staleTime: Infinity, enabled: open && engine.data === true })
  const resolution = useResolved(doc)
  const [area, setArea] = useState<Area | null>(null)
  const [image, setImage] = useState(true)
  const [mask, setMask] = useState(true)
  const [picked, setPicked] = useState<Record<string, DataFormat | null>>({})
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  // the layers on the map, top first, and what each can leave as
  const shown = useMemo(() => shownLayers(doc.layers, solo), [doc.layers, solo])
  const rows = useMemo(
    () =>
      allLayers(doc.layers)
        .filter((l) => shown.has(l.id) && l.type !== 'label')
        .reverse()
        .map((l) => exportable(l, doc.sources[l.source], resolvedFor(l.source, resolution))),
    [doc.layers, doc.sources, shown, resolution],
  )
  useEffect(() => {
    if (open) {
      setError(null)
      setBusy(null)
    }
  }, [open])
  // a layer is exported when it has a format picked; ticking it picks its first one
  const chosen = rows.flatMap((r) => {
    const f = r.formats.length && !offlineNow(engine.data) ? picked[r.layer.id] : null
    return f ? [{ ex: r, format: f }] : []
  })
  const count = chosen.length + (image ? 1 : 0)

  const run = async () => {
    setError(null)
    try {
      const files: ExportFile[] = []
      const shape = area ? await (setBusy(`Fetching the outline of ${area.name}…`), areaShape(area.pcode)) : undefined
      if (image) {
        setBusy(area ? `Framing ${area.name} and drawing the image…` : 'Drawing the image…')
        const blob = await render(shape ? { shape, mask, name: area!.name } : undefined)
        if (blob) files.push({ name: `${slug(doc.title)}${area ? `-${slug(area.name)}` : ''}.png`, type: 'image/png', body: new Uint8Array(await blob.arrayBuffer()) })
      }
      for (const { ex, format } of chosen) {
        const source = doc.sources[ex.layer.source]
        const r = resolvedFor(ex.layer.source, resolution)
        if (!source || !r) continue
        setBusy(`Exporting ${ex.layer.name}…`)
        files.push(await layerData(ex, source, r, area, format, (done, total) => setBusy(`Exporting ${ex.layer.name}… ${done.toLocaleString('en')} of ${total.toLocaleString('en')}`)))
      }
      if (!files.length) return
      const base = `${slug(doc.title)}${area ? `-${slug(area.name)}` : ''}`
      if (files.length === 1) download(new Blob([files[0].body as BlobPart], { type: files[0].type }), files[0].name)
      else {
        const about = [
          `${doc.title}`,
          area ? `Clipped to ${area.name} (${area.pcode}, ${LEVELS[area.level].name}).` : 'Not clipped.',
          `Exported ${new Date().toISOString()} from Lanka Data Layer${resolution.resolved?.data_version ? `, data version ${resolution.resolved.data_version}` : ''}.`,
          '',
          'Sources and licences:',
          ...(resolution.resolved?.attribution?.length ? resolution.resolved.attribution.map((a) => `- ${attributionLine(a)}`) : credits.map((c) => `- ${c}`)),
          '',
          'Areas are joined by p-code. A unit layer keeps the units inside the area, or the one unit containing it.',
        ].join('\n')
        files.push({ name: 'ATTRIBUTION.txt', type: 'text/plain', body: about + '\n' })
        download(zip(files), `${base}.zip`)
      }
      onOpenChange(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  const offline = engine.data === false
  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent aria-describedby="export-desc" className="top-[9vh] flex max-h-[82vh] max-w-[560px] flex-col overflow-hidden max-md:top-0 max-md:h-[100dvh] max-md:max-h-none max-md:w-screen max-md:max-w-none max-md:rounded-none max-md:border-0">
        <div className="px-5 pt-4 pb-3.5 max-md:pt-[max(16px,env(safe-area-inset-top))]">
          <DialogTitle className="text-[15px] font-semibold tracking-[-0.015em] text-ink">Export</DialogTitle>
          <DialogDescription id="export-desc" className="mt-0.5 text-[12.5px] text-ink3">
            The map as an image and its layers as data, for the whole island or one area.
          </DialogDescription>
        </div>
        <div className="scroll-thin min-h-0 flex-1 space-y-5 overflow-y-auto border-t border-line px-5 pt-4 pb-5">
          {/* area */}
          <section>
            <h3 className="t-label mb-2 tracking-[0.08em] uppercase">Clip to</h3>
            <AreaPicker areas={areas.data} loading={areas.isLoading} disabled={offline} value={area} onChange={setArea} />
            <p className="mt-1.5 text-[12px] leading-[1.5] text-ink3">
              {offline
                ? 'Clipping needs the engine; without it the image is the current view.'
                : area
                  ? `Only what lies in ${area.name}: the image is framed on it and cut to its boundary, and each layer keeps the units and features inside it.`
                  : 'Nothing is clipped: the image is the current view, and each layer exports in full.'}
            </p>
          </section>

          {/* image */}
          <section>
            <h3 className="t-label mb-2 tracking-[0.08em] uppercase">Image</h3>
            <label className="flex min-h-10 cursor-pointer items-center gap-3 rounded-lg border border-line px-3 py-2 hover:bg-bg3/40">
              <Checkbox checked={image} onCheckedChange={(v) => setImage(v === true)} aria-label="Export the map image" />
              <FileImage className="size-4 shrink-0 text-ink3" />
              <span className="min-w-0 flex-1 text-[13px] text-ink">Map image</span>
              <span className="font-mono text-[11px] text-ink3">PNG</span>
            </label>
            {image && area && (
              <div className="mt-2 flex min-h-8 items-center gap-3 pl-1">
                <label htmlFor="export-mask" className="min-w-0 flex-1 cursor-pointer text-[12.5px] text-ink2">
                  Cut to the boundary <span className="text-ink3">transparent outside it</span>
                </label>
                <Switch id="export-mask" checked={mask} onCheckedChange={setMask} />
              </div>
            )}
          </section>

          {/* data */}
          <section>
            <h3 className="t-label mb-2 tracking-[0.08em] uppercase">Data</h3>
            {!rows.length && <p className="text-[12.5px] text-ink3">No data layers are on the map.</p>}
            <div className="space-y-1.5">
              {rows.map((r) => {
                const on = !!picked[r.layer.id]
                const fmt = picked[r.layer.id] ?? r.formats[0]
                const can = r.formats.length > 0 && !offline
                return (
                  <div key={r.layer.id} className={cn('flex min-h-10 items-center gap-3 rounded-lg border border-line px-3 py-1.5', !can && 'opacity-60')}>
                    <Checkbox
                      checked={on}
                      disabled={!can}
                      onCheckedChange={(v) => setPicked((p) => ({ ...p, [r.layer.id]: v === true ? r.formats[0] : null }))}
                      aria-label={`Export ${r.layer.name}`}
                    />
                    <FileSpreadsheet className="size-4 shrink-0 text-ink3" />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[13px] text-ink">{r.layer.name}</span>
                      {!can && <span className="block truncate text-[11.5px] text-ink3">{offline ? 'Needs the engine' : r.note}</span>}
                    </span>
                    {can && r.formats.length > 1 && (
                      <ToggleGroup
                        type="single"
                        value={fmt}
                        onValueChange={(v) => v && setPicked((p) => ({ ...p, [r.layer.id]: v as DataFormat }))}
                        aria-label={`Format for ${r.layer.name}`}
                        className="h-7 w-[148px] shrink-0"
                      >
                        {r.formats.map((f) => (
                          <ToggleGroupItem key={f} value={f} className="px-1.5 font-mono text-[11px]">
                            {f === 'csv' ? 'CSV' : 'GeoJSON'}
                          </ToggleGroupItem>
                        ))}
                      </ToggleGroup>
                    )}
                    {can && r.formats.length === 1 && <span className="shrink-0 font-mono text-[11px] text-ink3">{r.formats[0] === 'csv' ? 'CSV' : 'GeoJSON'}</span>}
                  </div>
                )
              })}
            </div>
          </section>
          {error && (
            <p role="alert" className="rounded-lg border border-bad/40 bg-bad/[0.06] px-3 py-2 text-[12.5px] leading-[1.5] text-bad">
              {error}
            </p>
          )}
        </div>
        <div className="flex items-center gap-3 border-t border-line px-5 py-3 max-md:pb-[max(12px,env(safe-area-inset-bottom))]">
          <span className="min-w-0 flex-1 truncate text-[12px] text-ink3" role="status">
            {busy ?? (count === 0 ? 'Choose something to export' : count === 1 ? 'One file' : `${count} files, as one zip with their sources and licences`)}
          </span>
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)} disabled={!!busy} className="h-8 max-md:h-10">
            Cancel
          </Button>
          <Button size="sm" variant="primary" onClick={() => void run()} disabled={!!busy || count === 0} className="h-8 gap-1.5 max-md:h-10">
            {busy ? <Loader2 className="animate-spin" /> : <Download />}
            Export
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** One source in the zip's ATTRIBUTION.txt: who, under what licence, and whether it may be passed on. */
const attributionLine = (a: Attribution) => [a.text || a.publisher, a.licence, a.licence_url, a.redistribution && `redistribution: ${a.redistribution}`].filter(Boolean).join(' · ')

function AreaPicker({ areas, loading, disabled, value, onChange }: { areas: Area[] | undefined; loading: boolean; disabled: boolean; value: Area | null; onChange: (a: Area | null) => void }) {
  const [open, setOpen] = useState(false)
  const groups = useMemo(() => [1, 2, 3].map((l) => ({ level: l, items: (areas ?? []).filter((a) => a.level === l) })), [areas])
  return (
    <div className="flex items-center gap-2">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="outline" disabled={disabled} className="h-9 min-w-0 flex-1 justify-start gap-2 px-3 text-[13px] font-normal max-md:h-11" aria-label="Area to clip to">
            <MapPinned className="size-4 shrink-0 text-ink3" />
            <span className="min-w-0 flex-1 truncate text-left">{value ? value.name : 'Whole island, no clipping'}</span>
            {value && <span className="shrink-0 font-mono text-[11px] text-ink3">{value.pcode}</span>}
            <ChevronsUpDown className="size-3.5 shrink-0 text-ink3" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-[var(--radix-popover-trigger-width)] p-0">
          <Command>
            <CommandInput placeholder="Province, district or DS division…" />
            <CommandList className="max-h-[300px]">
              <CommandEmpty>{loading ? 'Loading areas…' : 'No area by that name.'}</CommandEmpty>
              <CommandGroup>
                <CommandItem
                  value="whole island no clipping"
                  onSelect={() => {
                    onChange(null)
                    setOpen(false)
                  }}
                >
                  <Check className={cn('size-3.5', value ? 'opacity-0' : 'opacity-100')} />
                  Whole island, no clipping
                </CommandItem>
              </CommandGroup>
              {groups.map((g) =>
                g.items.length ? (
                  <CommandGroup key={g.level} heading={LEVEL_GROUP[g.level]}>
                    {g.items.map((a) => (
                      <CommandItem
                        key={a.pcode}
                        value={`${a.name} ${a.pcode}`}
                        onSelect={() => {
                          onChange(a)
                          setOpen(false)
                        }}
                      >
                        <Check className={cn('size-3.5', value?.pcode === a.pcode ? 'opacity-100' : 'opacity-0')} />
                        <span className="min-w-0 flex-1 truncate">{a.name}</span>
                        <span className="shrink-0 font-mono text-[11px] text-ink3">{a.pcode}</span>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ) : null,
              )}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {value && (
        <Button variant="ghost" size="sm" onClick={() => onChange(null)} aria-label="Clear the area" className="size-9 shrink-0 p-0 max-md:size-11">
          <X />
        </Button>
      )}
    </div>
  )
}
