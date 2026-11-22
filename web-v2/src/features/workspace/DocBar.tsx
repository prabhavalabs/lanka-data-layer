// The document's own controls: its name, whether it is saved, and what can be done with it as a whole.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Copy, Ellipsis, FileJson, FilePlus, FolderOpen, ImageDown, Link2, RotateCcw, Save, Trash2, Upload } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { deleteMap, forkMap, mapsQuery, type MapSummary } from '@/api/workspace'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger, MenuShortcut } from '@/components/ui/dropdown-menu'
import type { Theme } from '@/lib/hooks'
import { assertDocument, type MapDocument } from '@/lib/map-spec'
import { cn } from '@/lib/utils'
import { ago } from './format'
import { LayerThumb } from './LayerThumb'
import { actions, useWs } from './store'

export type DocActions = {
  save: () => void
  saving: boolean
  revert: () => void
  open: (id: string) => void
  openDocument: (doc: MapDocument) => void
  blank: () => void
  /** copies a link; resolves to what to say about it */
  share: () => Promise<string | void>
  exportPng: () => void
  exportJson: () => void
  /** null when the document came from a link and has never been saved */
  origin: 'starter' | 'own' | 'link' | 'new'
}

export function DocBar({ doc, onMaps, compact = false, extra }: { doc: DocActions; onMaps: () => void; compact?: boolean; extra?: React.ReactNode }) {
  const title = useWs((s) => s.doc!.title)
  const dirty = useWs((s) => s.doc !== s.saved)
  const updated = useWs((s) => s.saved?.meta.updated_at)
  const [copied, setCopied] = useState<string | null>(null)
  const file = useRef<HTMLInputElement>(null)
  const [, force] = useState(0)
  // "saved 2 min ago" keeps itself current
  useEffect(() => {
    const id = window.setInterval(() => force((n) => n + 1), 30_000)
    return () => window.clearInterval(id)
  }, [])
  const status =
    doc.origin === 'link' ? { dot: 'bg-warn', text: 'From a link' } : doc.origin === 'new' ? { dot: 'bg-warn', text: 'Not saved' } : doc.origin === 'starter' ? (dirty ? { dot: 'bg-warn', text: 'Edited copy' } : { dot: 'bg-ink3', text: 'Starter' }) : dirty ? { dot: 'bg-warn', text: 'Unsaved' } : { dot: 'bg-ok', text: `Saved ${updated ? ago(updated) : ''}` }
  const canSave = dirty || doc.origin === 'link' || doc.origin === 'new'
  const share = async () => {
    setCopied('Making a link…')
    const said = await doc.share()
    setCopied(said || 'Link copied')
    window.setTimeout(() => setCopied(null), said && said !== 'Link copied' ? 4000 : 1600)
  }
  return (
    <div className={cn('px-3.5 pt-3 pb-2.5', compact && 'pt-2 pb-2')}>
      {!compact && (
        <div className="flex items-center gap-1">
          <input
            key={title}
            defaultValue={title}
            aria-label="Map title"
            data-sheet-nodrag
            onBlur={(e) => (e.target.value.trim() ? actions.setTitle(e.target.value) : (e.target.value = title))}
            onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
            className="-ml-1.5 min-w-0 flex-1 truncate rounded-md border border-transparent bg-transparent px-1.5 py-0.5 text-[14.5px] font-semibold tracking-[-0.015em] text-ink outline-none focus:border-line2 focus:bg-bg2/70 [@media(hover:hover)_and_(pointer:fine)]:hover:border-line2"
          />
          {extra}
        </div>
      )}
      <div className={cn('flex h-7 items-center gap-1.5 max-md:h-9', !compact && 'mt-1.5')}>
        <span className={cn('size-1.5 shrink-0 rounded-full', status.dot)} />
        <span className="t-label min-w-0 flex-1 truncate" role="status">
          {copied ?? status.text}
        </span>
        <Button variant="ghost" size="sm" onClick={onMaps} className="h-7 gap-1.5 px-2 text-[12.5px] max-md:h-9">
          <FolderOpen className="!size-3.5" />
          Maps
        </Button>
        <Button variant={canSave ? 'primary' : 'outline'} size="sm" onClick={doc.save} disabled={!canSave || doc.saving} className="h-7 gap-1.5 px-2.5 text-[12.5px] max-md:h-9">
          {canSave ? <Save className="!size-3.5" /> : <Check className="!size-3.5" />}
          {doc.saving ? 'Saving' : canSave ? 'Save' : 'Saved'}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" aria-label="Map actions" className="pressable grid size-7 shrink-0 place-items-center rounded-md text-ink3 outline-none max-md:size-10 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-[color-mix(in_oklab,var(--ink)_9%,transparent)] [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
              <Ellipsis className="size-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[212px]">
            <DropdownMenuItem onSelect={doc.save} disabled={!canSave}>
              <Save />
              {doc.origin === 'own' ? 'Save' : 'Save to your maps'}
              <MenuShortcut>⌘S</MenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={doc.revert} disabled={!dirty || doc.origin === 'link' || doc.origin === 'new'}>
              <RotateCcw />
              Revert to saved
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => void share()}>
              <Link2 />
              Copy share link
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={doc.exportPng}>
              <ImageDown />
              Export image (PNG)
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={doc.exportJson}>
              <FileJson />
              Export map document
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => file.current?.click()}>
              <Upload />
              Import map document…
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onMaps}>
              <FolderOpen />
              Open a map…
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={doc.blank}>
              <FilePlus />
              New blank map
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <input
        ref={file}
        type="file"
        accept=".json,application/json"
        className="hidden"
        onChange={async (e) => {
          const f = e.target.files?.[0]
          e.target.value = ''
          if (!f) return
          try {
            doc.openDocument(assertDocument(JSON.parse(await f.text())))
          } catch {
            actions.say('That file is not an ldl-map/1 document')
          }
        }}
      />
    </div>
  )
}

function MapCard({ m, current, scheme, onOpen, onFork, onDelete }: { m: MapSummary; current: boolean; scheme: Theme; onOpen: () => void; onFork: () => void; onDelete?: () => void }) {
  const [confirm, setConfirm] = useState(false)
  return (
    <div className={cn('group/card relative flex flex-col rounded-xl border bg-bg2/40 transition-colors duration-150', current ? 'border-ink/50' : 'border-line2 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/70')}>
      <button type="button" onClick={onOpen} className="pressable flex min-h-[112px] flex-1 flex-col rounded-xl p-3.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-brand/60">
        <span className="flex items-center gap-1">
          {m.preview.slice(0, 6).map((l) => (
            <LayerThumb key={l.id} node={l} scheme={scheme} />
          ))}
          {current && <span className="ml-auto rounded-full bg-ink/10 px-2 py-0.5 font-mono text-[10px] text-ink2">open</span>}
        </span>
        <span className="mt-3 block text-[14px] leading-tight font-semibold tracking-[-0.015em] text-ink">{m.title}</span>
        {m.description && <span className="mt-1 line-clamp-2 text-[12.5px] leading-[1.45] text-ink2">{m.description}</span>}
        <span className="t-label mt-auto block pt-2.5">
          {m.layers} layers{m.analyses ? ` · ${m.analyses} ${m.analyses === 1 ? 'analysis' : 'analyses'}` : ''}
          {m.starter ? '' : ` · ${ago(m.updated_at)}`}
        </span>
      </button>
      <div className="absolute right-2 bottom-2 flex items-center gap-0.5">
        {confirm ? (
          <>
            <button type="button" onClick={() => setConfirm(false)} className="pressable h-7 rounded-md px-2 text-[12px] text-ink2 outline-none max-md:h-9">
              Keep
            </button>
            <button type="button" onClick={onDelete} className="pressable h-7 rounded-md bg-bad-soft px-2 text-[12px] font-medium text-bad outline-none max-md:h-9">
              Delete
            </button>
          </>
        ) : (
          <>
            <button type="button" onClick={onFork} aria-label={`Duplicate ${m.title}`} title="Duplicate" className="pressable grid size-7 place-items-center rounded-md text-ink3 outline-none max-md:size-9 [@media(hover:hover)_and_(pointer:fine)]:opacity-0 [@media(hover:hover)_and_(pointer:fine)]:group-hover/card:opacity-100 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg4 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink focus-visible:opacity-100">
              <Copy className="size-3.5" />
            </button>
            {onDelete && (
              <button type="button" onClick={() => setConfirm(true)} aria-label={`Delete ${m.title}`} title="Delete" className="pressable grid size-7 place-items-center rounded-md text-ink3 outline-none max-md:size-9 [@media(hover:hover)_and_(pointer:fine)]:opacity-0 [@media(hover:hover)_and_(pointer:fine)]:group-hover/card:opacity-100 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bad-soft [@media(hover:hover)_and_(pointer:fine)]:hover:text-bad focus-visible:opacity-100">
                <Trash2 className="size-3.5" />
              </button>
            )}
          </>
        )}
      </div>
    </div>
  )
}

export function MapsDialog({ open, onOpenChange, doc, scheme }: { open: boolean; onOpenChange: (open: boolean) => void; doc: DocActions; scheme: Theme }) {
  const client = useQueryClient()
  const currentId = useWs((s) => s.doc?.id)
  const { data: maps } = useQuery({ ...mapsQuery(), enabled: open })
  const fork = useMutation({ mutationFn: (id: string) => forkMap(id, scheme), onSuccess: (copy) => (void client.invalidateQueries({ queryKey: ['maps'] }), copy?.id && pick(copy.id)) })
  const remove = useMutation({ mutationFn: deleteMap, onSuccess: () => client.invalidateQueries({ queryKey: ['maps'] }) })
  const pick = (id: string) => {
    onOpenChange(false)
    doc.open(id)
  }
  const starters = maps?.filter((m) => m.starter) ?? []
  const own = maps?.filter((m) => !m.starter) ?? []
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby="maps-desc" className="top-[9vh] flex max-h-[82vh] max-w-[760px] flex-col overflow-hidden max-md:top-0 max-md:h-[100dvh] max-md:max-h-none max-md:w-screen max-md:max-w-none max-md:rounded-none max-md:border-0">
        <div className="px-5 pt-4 pb-3.5 max-md:pt-[max(16px,env(safe-area-inset-top))]">
          <DialogTitle className="text-[15px] font-semibold tracking-[-0.015em] text-ink">Maps</DialogTitle>
          <DialogDescription id="maps-desc" className="mt-0.5 text-[12.5px] text-ink3">
            A map is one document: its layers, their styling and its analyses. Open one, or duplicate it to start your own.
          </DialogDescription>
        </div>
        <div className="scroll-thin min-h-0 flex-1 overflow-y-auto border-t border-line px-5 pt-4 pb-5">
          <h3 className="t-label mb-2.5 tracking-[0.08em] uppercase">Starters</h3>
          <div className="grid grid-cols-2 gap-3 max-md:grid-cols-1">
            {!maps && Array.from({ length: 4 }, (_, i) => <div key={i} className="skeleton h-[128px] rounded-xl" />)}
            {starters.map((m) => (
              <MapCard key={m.id} m={m} current={m.id === currentId} scheme={scheme} onOpen={() => pick(m.id)} onFork={() => fork.mutate(m.id)} />
            ))}
          </div>
          <div className="mt-6 mb-2.5 flex items-center">
            <h3 className="t-label tracking-[0.08em] uppercase">Your maps</h3>
            <span className="t-label t-num ml-1.5 text-ink2">{own.length}</span>
            <Button
              variant="outline"
              size="sm"
              className="ml-auto h-7 gap-1.5 px-2.5 text-[12.5px] max-md:h-9"
              onClick={() => {
                onOpenChange(false)
                doc.blank()
              }}
            >
              <FilePlus className="!size-3.5" />
              New blank map
            </Button>
          </div>
          {maps && own.length === 0 ? (
            <p className="rounded-xl border border-dashed border-line2 px-4 py-6 text-center text-[12.5px] leading-[1.5] text-ink3">
              Nothing saved yet. Maps you save are kept in this browser; a share link carries the whole map with it.
            </p>
          ) : (
            <div className="grid grid-cols-2 gap-3 max-md:grid-cols-1">
              {own.map((m) => (
                <MapCard
                  key={m.id}
                  m={m}
                  current={m.id === currentId}
                  scheme={scheme}
                  onOpen={() => pick(m.id)}
                  onFork={() => fork.mutate(m.id)}
                  onDelete={() => {
                    remove.mutate(m.id)
                    if (m.id === currentId) doc.open('')
                  }}
                />
              ))}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
