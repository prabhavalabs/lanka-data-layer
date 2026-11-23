import { ArrowDownToLine, ArrowUpToLine, ChevronRight, CircleDot, Copy, Ellipsis, Eye, EyeOff, FolderPlus, GripVertical, Group as GroupIcon, Lock, LockOpen, Map as MapIcon, Pencil, Plus, Redo2, ScanSearch, Trash2, Undo2, Ungroup } from 'lucide-react'
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ComponentType, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger, MenuShortcut } from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { Theme } from '@/lib/hooks'
import { BLEND_MODES, containsNode, isGroup, LAYER_TYPE_LABEL, parentOf, type BasemapStyle, type BlendMode, type LayerNode, type MapDocument } from '@/lib/map-spec'
import { cn } from '@/lib/utils'
import { useRafCallback, useWorkspace } from './context'
import { LayerThumb } from './LayerThumb'
import { drawnFrom, useMapZoom, zoomLabel } from './zoom'
import { actions, getWs, rowsOf, useNode, useWs, type Row } from './store'

const INDENT = 16
const HOVER = '[@media(hover:hover)_and_(pointer:fine)]:hover:bg-[color-mix(in_oklab,var(--ink)_9%,transparent)] [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink'
const iconBtn = cn('pressable grid size-7 shrink-0 place-items-center rounded-md text-ink3 outline-none max-md:size-10 [&_svg]:size-[15px]', HOVER)

/* ---------------- one row ---------------- */
type RowProps = {
  row: Row
  /** drawn from the bundled sample data, not the engine */
  sample?: boolean
  /** the engine's reason the layer could not be drawn */
  failed?: string
  /** the zoom the layer starts drawing at, when the map is zoomed out past it: nothing of it is on screen */
  reach?: number
  selected: boolean
  solo: boolean
  /** another node is soloed and this one is not part of it */
  muted: boolean
  renaming: boolean
  dragging: boolean
  into: boolean
  focusable: boolean
  scheme: Theme
  touch: boolean
  onRename: (id: string | null) => void
  onZoom: (id: string) => void
  onReach: (zoom: number) => void
}
type ItemKit = { Item: ComponentType<{ children: ReactNode; onSelect?: () => void; disabled?: boolean; destructive?: boolean }>; Separator: ComponentType }
function RowMenu({ node, kit, onRename, onZoom }: { node: LayerNode; kit: ItemKit; onRename: (id: string) => void; onZoom: (id: string) => void }) {
  const { Item, Separator } = kit
  const group = isGroup(node)
  const top = (toTop: boolean) => {
    const doc = getWs().doc
    if (!doc) return
    const parent = parentOf(doc.layers, node.id) ?? null
    actions.move(node.id, parent?.id ?? null, toTop ? (parent ? parent.children : doc.layers).length : 0)
  }
  return (
    <>
      <Item onSelect={() => onRename(node.id)} disabled={node.locked}>
        <Pencil />
        Rename
        <MenuShortcut>F2</MenuShortcut>
      </Item>
      <Item onSelect={() => actions.duplicate(node.id)}>
        <Copy />
        Duplicate
        <MenuShortcut>⌘D</MenuShortcut>
      </Item>
      {!group && (
        <Item onSelect={() => onZoom(node.id)}>
          <ScanSearch />
          Zoom to layer
        </Item>
      )}
      <Separator />
      {group ? (
        <Item onSelect={() => actions.ungroup(node.id)} disabled={node.locked}>
          <Ungroup />
          Ungroup
        </Item>
      ) : (
        <Item onSelect={() => actions.group(node.id)}>
          <GroupIcon />
          Group
          <MenuShortcut>⌘G</MenuShortcut>
        </Item>
      )}
      <Item onSelect={() => top(true)}>
        <ArrowUpToLine />
        Bring to front
      </Item>
      <Item onSelect={() => top(false)}>
        <ArrowDownToLine />
        Send to back
      </Item>
      <Separator />
      <Item onSelect={() => actions.solo(node.id)}>
        <CircleDot />
        Solo
      </Item>
      <Item onSelect={() => actions.toggleLock(node.id)}>
        {node.locked ? <LockOpen /> : <Lock />}
        {node.locked ? 'Unlock' : 'Lock'}
      </Item>
      <Separator />
      <Item onSelect={() => actions.remove(node.id)} disabled={node.locked} destructive>
        <Trash2 />
        Delete
        <MenuShortcut>⌫</MenuShortcut>
      </Item>
    </>
  )
}
const DROPDOWN: ItemKit = { Item: DropdownMenuItem, Separator: DropdownMenuSeparator }
const CONTEXT: ItemKit = { Item: ContextMenuItem, Separator: ContextMenuSeparator }

const LayerRow = memo(function LayerRow({ row, sample, failed, reach, selected, solo, muted, renaming, dragging, into, focusable, scheme, touch, onRename, onZoom, onReach }: RowProps) {
  const { node, depth, hidden, lockedBy } = row
  const group = isGroup(node)
  const blend = !group && node.blend !== 'normal' ? BLEND_MODES.find((b) => b.id === node.blend)?.tag : null
  const off = hidden || muted
  // visible, but the map is zoomed out past where it draws: dimmed like a hidden one, with a way in
  const far = !off && reach !== undefined
  const input = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (renaming) input.current?.select()
  }, [renaming])
  const commit = () => {
    const v = input.current?.value ?? ''
    if (v.trim() && v !== node.name) actions.rename(node.id, v)
    onRename(null)
  }
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild disabled={touch}>
        <div
          role="treeitem"
          aria-level={depth + 1}
          aria-selected={selected}
          aria-expanded={group ? !node.collapsed : undefined}
          aria-label={`${node.name}, ${group ? 'group' : LAYER_TYPE_LABEL[node.type]}${node.visible ? '' : ', hidden'}${node.locked ? ', locked' : ''}`}
          tabIndex={focusable ? 0 : -1}
          data-row={node.id}
          data-selected={selected}
          data-dragging={dragging}
          data-into={into}
          className="ws-row group/row relative flex h-[34px] cursor-default items-center gap-1.5 rounded-lg pr-1 outline-none select-none focus-visible:ring-2 focus-visible:ring-brand/60 max-md:h-11"
          style={{ paddingLeft: 6 + depth * INDENT }}
        >
          {touch && (
            <span data-grip data-sheet-nodrag className="-ml-1 grid h-full w-6 shrink-0 cursor-grab touch-none place-items-center text-ink3" aria-hidden>
              <GripVertical className="size-3.5" />
            </span>
          )}
          {group ? (
            <button type="button" tabIndex={-1} onClick={() => actions.toggleCollapsed(node.id)} aria-label={node.collapsed ? 'Expand group' : 'Collapse group'} className={cn('grid size-5 shrink-0 place-items-center rounded text-ink3 max-md:size-8', HOVER)}>
              <ChevronRight className={cn('size-3.5 transition-transform duration-150 ease-[var(--ease-out)]', !node.collapsed && 'rotate-90')} />
            </button>
          ) : (
            // a layer inside a group lines up under the group's folder, past its chevron
            depth > 0 && <span className="w-1 shrink-0" />
          )}
          <LayerThumb node={node} scheme={scheme} className={cn('transition-opacity duration-150', (off || far) && 'opacity-40')} />
          {renaming ? (
            <input
              ref={input}
              defaultValue={node.name}
              aria-label="Layer name"
              data-sheet-nodrag
              onBlur={commit}
              onKeyDown={(e) => {
                e.stopPropagation()
                if (e.key === 'Enter') commit()
                if (e.key === 'Escape') onRename(null)
              }}
              className="-my-1 min-w-0 flex-1 rounded-md border border-line2 bg-bg2 px-1.5 py-1 text-[13px] text-ink outline-none focus:border-brand"
            />
          ) : (
            <span className={cn('min-w-0 flex-1 truncate text-[13px] leading-none tracking-[-0.005em] transition-colors duration-150', off || far ? 'text-ink3' : selected ? 'text-ink' : 'text-ink2', group && 'font-medium')}>{node.name}</span>
          )}
          {!renaming && (
            <>
              {failed && (
                <span title={failed} className="shrink-0 rounded-[4px] border border-bad/50 px-1 font-mono text-[9px] leading-[14px] tracking-[0.04em] text-bad">
                  FAILED
                </span>
              )}
              {far && !failed && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      tabIndex={-1}
                      onClick={(e) => {
                        e.stopPropagation()
                        onReach(reach)
                      }}
                      aria-label={`Not drawn at this zoom. Zoom in to ${zoomLabel(reach)} to see ${node.name}`}
                      className="pressable shrink-0 rounded-[4px] border border-warn/50 px-1 font-mono text-[9px] leading-[14px] tracking-[0.04em] text-warn hover:bg-warn/10"
                    >
                      Z{zoomLabel(reach)}+
                    </button>
                  </TooltipTrigger>
                  <TooltipContent>Drawn from zoom {zoomLabel(reach)}. Click to zoom in.</TooltipContent>
                </Tooltip>
              )}
              {sample && !failed && (
                <span title="Sample data: the engine does not serve this source" className="shrink-0 rounded-[4px] border border-dashed border-line2 px-1 font-mono text-[9px] leading-[14px] tracking-[0.04em] text-ink3">
                  SAMPLE
                </span>
              )}
              {blend && <span className="shrink-0 rounded-[4px] border border-line2 px-1 font-mono text-[9px] leading-[14px] tracking-[0.04em] text-ink3">{blend}</span>}
              {node.opacity < 0.995 && <span className="t-num shrink-0 font-mono text-[10.5px] text-ink3">{Math.round(node.opacity * 100)}%</span>}
              <span className="flex shrink-0 items-center">
                {!touch && (
                  <>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <button type="button" tabIndex={-1} data-on={solo} onClick={() => actions.solo(node.id)} aria-label={solo ? 'Stop soloing' : 'Solo: show only this'} aria-pressed={solo} className={cn(iconBtn, 'ws-row-act', solo && 'text-brand-ink')}>
                          <CircleDot />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent>{solo ? 'Stop soloing' : 'Solo'}</TooltipContent>
                    </Tooltip>
                    <button type="button" tabIndex={-1} data-on={!!node.locked} onClick={() => actions.toggleLock(node.id)} aria-label={node.locked ? 'Unlock' : 'Lock'} aria-pressed={!!node.locked} className={cn(iconBtn, 'ws-row-act', (node.locked || lockedBy) && 'text-ink2')}>
                      {node.locked || lockedBy ? <Lock /> : <LockOpen />}
                    </button>
                  </>
                )}
                {touch && (node.locked || lockedBy) && <Lock className="mr-1 size-3.5 text-ink3" />}
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button type="button" tabIndex={-1} aria-label={`More actions for ${node.name}`} className={cn(iconBtn, 'ws-row-act data-[state=open]:!opacity-100')}>
                      <Ellipsis />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <RowMenu node={node} kit={DROPDOWN} onRename={onRename} onZoom={onZoom} />
                  </DropdownMenuContent>
                </DropdownMenu>
                <button
                  type="button"
                  tabIndex={-1}
                  onClick={(e) => (e.altKey ? actions.solo(node.id) : actions.setVisible(node.id, !node.visible))}
                  aria-label={node.visible ? 'Hide' : 'Show'}
                  aria-pressed={node.visible}
                  className={cn(iconBtn, node.visible ? (hidden ? 'text-ink3/60' : 'text-ink2') : 'text-ink3/70')}
                >
                  {node.visible ? <Eye /> : <EyeOff />}
                </button>
              </span>
            </>
          )}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <RowMenu node={node} kit={CONTEXT} onRename={onRename} onZoom={onZoom} />
      </ContextMenuContent>
    </ContextMenu>
  )
})

/* ---------------- the tree, with drag-to-reorder ---------------- */
type Drop = { parentId: string | null; index: number; y: number; depth: number; into: string | null }

function LayerTree({ onAdd }: { onAdd: () => void }) {
  const layers = useWs((s) => s.doc!.layers)
  const selected = useWs((s) => s.selected)
  const solo = useWs((s) => s.solo)
  const { scheme, engine, desktop, data, failed } = useWorkspace()
  const rows = useMemo(() => rowsOf(layers), [layers])
  const [renaming, setRenaming] = useState<string | null>(null)
  const [drag, setDrag] = useState<{ id: string; into: string | null } | null>(null)
  const list = useRef<HTMLDivElement>(null)
  const line = useRef<HTMLDivElement>(null)
  const ghost = useRef<HTMLDivElement>(null)
  const live = useRef<{ id: string; x0: number; y0: number; active: boolean; pointer: number; drop: Drop | null; raf: number; y: number; grabY: number } | null>(null)
  const rowsRef = useRef(rows)
  rowsRef.current = rows
  const touch = !desktop

  const soloNode = solo ? rows.find((r) => r.node.id === solo)?.node : null
  const inSolo = (r: Row) => !soloNode || containsNode(soloNode, r.node.id) || containsNode(r.node, soloNode.id)
  const onZoom = useCallback((id: string) => engine.current?.fitLayer(id), [engine])
  const onReach = useCallback((z: number) => engine.current?.zoomTo(z + 0.15), [engine])
  const zoom = useMapZoom()
  const reachOf = (node: LayerNode, z: number) => {
    if (isGroup(node)) return undefined
    const from = drawnFrom(node, data.get(node.id))
    return from > 0 && z < from - 0.005 ? from : undefined
  }

  /** Where a node dropped at this pointer position would go. */
  const dropAt = (clientX: number, clientY: number, dragId: string): Drop | null => {
    const el = list.current
    const doc = getWs().doc
    if (!el || !doc) return null
    const dragged = rowsRef.current.find((r) => r.node.id === dragId)?.node
    if (!dragged) return null
    // rows that belong to the dragged node are not places it can go
    const cand = rowsRef.current.filter((r) => !containsNode(dragged, r.node.id))
    const box = el.getBoundingClientRect()
    const rects = cand.map((r) => el.querySelector<HTMLElement>(`[data-row="${CSS.escape(r.node.id)}"]`)?.getBoundingClientRect())
    // over the middle of a group: into it
    for (let i = 0; i < cand.length; i++) {
      const rc = rects[i]
      const n = cand[i].node
      if (!rc || !isGroup(n)) continue
      if (clientY > rc.top + rc.height * 0.28 && clientY < rc.bottom - rc.height * 0.28) return { parentId: n.id, index: n.children.length, y: 0, depth: cand[i].depth + 1, into: n.id }
    }
    let g = 0
    while (g < cand.length && rects[g] && clientY > rects[g]!.top + rects[g]!.height / 2) g++
    const above = cand[g - 1], below = cand[g]
    const y = (above ? rects[g - 1]!.bottom : (rects[g]?.top ?? box.top)) - box.top + el.scrollTop
    if (!above) return { parentId: null, index: doc.layers.length, y, depth: 0, into: null }
    const min = below ? below.depth : 0
    const max = above.depth + (isGroup(above.node) && !above.node.collapsed ? 1 : 0)
    const depth = Math.max(min, Math.min(max, Math.round((clientX - box.left - 30) / INDENT)))
    if (depth === above.depth + 1 && isGroup(above.node)) return { parentId: above.node.id, index: above.node.children.length, y, depth, into: null }
    // the row, or the group it is inside, that the new node goes directly below
    let ref: Row = above
    while (ref.depth > depth && ref.parent) ref = cand.find((r) => r.node.id === ref.parent!.id) ?? ref
    const siblings = ref.parent ? ref.parent.children : doc.layers
    return { parentId: ref.parent?.id ?? null, index: siblings.findIndex((n) => n.id === ref.node.id), y, depth, into: null }
  }
  const paint = (clientX: number, clientY: number) => {
    const d = live.current
    if (!d) return
    const drop = dropAt(clientX, clientY, d.id)
    d.drop = drop
    if (ghost.current) ghost.current.style.transform = `translate3d(${Math.round(list.current!.getBoundingClientRect().left + 6)}px, ${Math.round(clientY - d.grabY)}px, 0) scale(1.015)`
    if (line.current) {
      line.current.style.opacity = drop && !drop.into ? '1' : '0'
      if (drop && !drop.into) {
        line.current.style.transform = `translate3d(0, ${Math.round(drop.y - 1)}px, 0)`
        line.current.style.left = `${10 + drop.depth * INDENT}px`
      }
    }
    setDrag((cur) => (cur && cur.into === (drop?.into ?? null) ? cur : { id: d.id, into: drop?.into ?? null }))
  }
  const finish = (commit: boolean) => {
    const d = live.current
    live.current = null
    if (!d) return
    cancelAnimationFrame(d.raf)
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onCancel)
    window.removeEventListener('keydown', onEscape, true)
    if (d.active) {
      setDrag(null)
      document.body.style.cursor = ''
      if (commit && d.drop) actions.move(d.id, d.drop.parentId, d.drop.index)
    }
  }
  const onMove = (e: PointerEvent) => {
    const d = live.current
    if (!d || e.pointerId !== d.pointer) return
    if (!d.active) {
      if (Math.hypot(e.clientX - d.x0, e.clientY - d.y0) < 5) return
      d.active = true
      document.body.style.cursor = 'grabbing'
      setDrag({ id: d.id, into: null })
      // near either end of the list, scroll it under the pointer
      const tick = () => {
        const el = list.current
        const cur = live.current
        if (!el || !cur) return
        const box = el.getBoundingClientRect()
        const edge = 36
        const v = cur.y < box.top + edge ? -(box.top + edge - cur.y) : cur.y > box.bottom - edge ? cur.y - (box.bottom - edge) : 0
        if (v) el.scrollTop += Math.max(-14, Math.min(14, v * 0.35))
        cur.raf = requestAnimationFrame(tick)
      }
      d.raf = requestAnimationFrame(tick)
    }
    e.preventDefault()
    d.y = e.clientY
    paint(e.clientX, e.clientY)
  }
  const onUp = () => finish(true)
  const onCancel = () => finish(false)
  const onEscape = (e: globalThis.KeyboardEvent) => {
    if (e.key !== 'Escape') return
    e.stopPropagation()
    finish(false)
  }
  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || live.current) return
    const target = e.target as HTMLElement
    const rowEl = target.closest<HTMLElement>('[data-row]')
    if (!rowEl || target.closest('button, input, [role="menu"]')) return
    // a finger scrolls the list; it reorders only from the grip
    if (e.pointerType !== 'mouse' && !target.closest('[data-grip]')) return
    const id = rowEl.dataset.row!
    const row = rowsRef.current.find((r) => r.node.id === id)
    if (!row || row.node.locked || row.lockedBy) return
    live.current = { id, x0: e.clientX, y0: e.clientY, active: false, pointer: e.pointerId, drop: null, raf: 0, y: e.clientY, grabY: e.clientY - rowEl.getBoundingClientRect().top }
    window.addEventListener('pointermove', onMove, { passive: false })
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onCancel)
    window.addEventListener('keydown', onEscape, true)
  }
  useEffect(() => () => finish(false), [])

  const focusRow = (id: string) => requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-row="${CSS.escape(id)}"]`)?.focus())
  /** Alt+arrow: one step up or down the stack, into and out of groups on the way. */
  const nudge = (row: Row, up: boolean) => {
    const doc = getWs().doc
    if (!doc || row.node.locked) return
    const siblings = row.parent ? row.parent.children : doc.layers
    const i = siblings.findIndex((n) => n.id === row.node.id)
    const next = siblings[up ? i + 1 : i - 1]
    let ok: boolean
    if (next && isGroup(next) && !next.collapsed && !isGroup(row.node)) ok = actions.move(row.node.id, next.id, up ? 0 : next.children.length)
    else if (next) ok = actions.move(row.node.id, row.parent?.id ?? null, up ? i + 2 : i - 1)
    else if (row.parent) {
      const outer = parentOf(doc.layers, row.parent.id) ?? null
      const at = (outer ? outer.children : doc.layers).findIndex((n) => n.id === row.parent!.id)
      ok = actions.move(row.node.id, outer?.id ?? null, up ? at + 1 : at)
    } else ok = false
    if (ok) {
      actions.say(`${row.node.name} moved ${up ? 'up' : 'down'}`)
      focusRow(row.node.id)
    }
  }
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>('[data-row]')
    if (!el || (e.target as HTMLElement).tagName === 'INPUT') return
    const i = rows.findIndex((r) => r.node.id === el.dataset.row)
    const row = rows[i]
    if (!row) return
    const mod = e.metaKey || e.ctrlKey
    const go = (r: Row | undefined) => {
      if (!r) return
      actions.select(r.node.id)
      focusRow(r.node.id)
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (e.altKey) nudge(row, e.key === 'ArrowUp')
      else go(rows[i + (e.key === 'ArrowDown' ? 1 : -1)])
    } else if (e.key === 'ArrowRight' && isGroup(row.node)) {
      e.preventDefault()
      if (row.node.collapsed) actions.toggleCollapsed(row.node.id)
      else go(rows[i + 1])
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault()
      if (isGroup(row.node) && !row.node.collapsed) actions.toggleCollapsed(row.node.id)
      else if (row.parent) go(rows.find((r) => r.node.id === row.parent!.id))
    } else if (e.key === 'Enter' || e.key === 'F2') {
      e.preventDefault()
      if (!row.node.locked) setRenaming(row.node.id)
    } else if (e.key === ' ') {
      e.preventDefault()
      actions.setVisible(row.node.id, !row.node.visible)
    } else if ((e.key === 'Delete' || e.key === 'Backspace') && !row.node.locked) {
      e.preventDefault()
      const next = rows[i + 1] ?? rows[i - 1]
      actions.remove(row.node.id)
      if (next) go(next)
    } else if (mod && e.key.toLowerCase() === 'd') {
      e.preventDefault()
      actions.duplicate(row.node.id)
    } else if (mod && e.key.toLowerCase() === 'g') {
      e.preventDefault()
      actions.group(row.node.id)
    }
  }
  const onClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement
    const el = target.closest<HTMLElement>('[data-row]')
    if (!el) return void (target === e.currentTarget && actions.select(null))
    if (target.closest('button, input')) return
    actions.select(el.dataset.row!)
  }
  const onDoubleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement
    const el = target.closest<HTMLElement>('[data-row]')
    if (!el || target.closest('button, input')) return
    const row = rows.find((r) => r.node.id === el.dataset.row)
    if (row && !row.node.locked) setRenaming(row.node.id)
  }

  const dragged = drag ? rows.find((r) => r.node.id === drag.id) : null
  if (rows.length === 0)
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-6 pb-8 text-center">
        <p className="text-[13px] leading-[1.5] text-ink2">This map has no layers yet. Add a dataset, an indicator or a grid from the catalogue.</p>
        <Button variant="outline" size="sm" onClick={onAdd}>
          <Plus />
          Add layer
        </Button>
      </div>
    )
  return (
    <div
      ref={list}
      role="tree"
      aria-label="Layers, top to bottom"
      data-reordering={drag ? '' : undefined}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      className="scroll-thin relative overscroll-contain px-1.5 py-1.5 md:min-h-0 md:flex-1 md:overflow-y-auto"
    >
      {rows.map((row, i) => (
        <LayerRow
          key={row.node.id}
          row={row}
          sample={data.get(row.node.id)?.origin === 'sample'}
          failed={failed.get(row.node.id)}
          reach={reachOf(row.node, zoom)}
          selected={row.node.id === selected}
          solo={row.node.id === solo}
          muted={!inSolo(row)}
          renaming={renaming === row.node.id}
          dragging={!!dragged && containsNode(dragged.node, row.node.id)}
          into={drag?.into === row.node.id}
          focusable={selected ? row.node.id === selected : i === 0}
          scheme={scheme}
          touch={touch}
          onRename={setRenaming}
          onZoom={onZoom}
          onReach={onReach}
        />
      ))}
      <div ref={line} className="ws-drop" style={{ opacity: 0 }} aria-hidden />
      {dragged && (
        <div ref={ghost} className="ws-ghost flex h-[34px] items-center gap-2 px-2.5 max-md:h-11" style={{ width: (list.current?.clientWidth ?? 280) - 12 }} aria-hidden>
          <LayerThumb node={dragged.node} scheme={scheme} />
          <span className="truncate text-[13px] text-ink">{dragged.node.name}</span>
        </div>
      )}
    </div>
  )
}

/* ---------------- blend and opacity of the selection ---------------- */
function SelectionBar() {
  const selected = useWs((s) => s.selected)
  const node = useNode(selected)
  const { engine } = useWorkspace()
  const [live, setLive] = useState<number | null>(null)
  const preview = useRafCallback((id: string, v: number) => engine.current?.previewOpacity(id, v))
  const dragging = useRef(false)
  useEffect(() => setLive(null), [selected])
  const group = !!node && isGroup(node)
  const locked = !node || !!node.locked
  const pct = live ?? Math.round((node?.opacity ?? 1) * 100)
  return (
    <div className={cn('flex items-center gap-2.5 px-3 pb-2.5 transition-opacity duration-150', !node && 'opacity-45')}>
      {group || !node ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="flex h-8 w-[104px] shrink-0 items-center rounded-lg border border-line bg-bg3/40 px-2.5 text-[12.5px] text-ink3 max-md:h-10">{group ? 'Group' : 'Normal'}</span>
          </TooltipTrigger>
          {group && <TooltipContent>A group is composited as one image; blend its layers individually</TooltipContent>}
        </Tooltip>
      ) : (
        <Select value={node.blend} onValueChange={(v) => actions.setBlend(node.id, v as BlendMode)} disabled={locked}>
          <SelectTrigger aria-label="Blend mode" className="h-8 w-[104px] shrink-0 px-2.5 text-[12.5px] max-md:h-10">
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="min-w-[236px]">
            {BLEND_MODES.map((b) => (
              <SelectItem key={b.id} value={b.id} textValue={b.label} className="max-md:py-2.5">
                {b.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
      <Slider
        className="min-w-0 flex-1 max-md:h-8"
        min={0}
        max={100}
        step={1}
        value={[pct]}
        disabled={locked}
        aria-label="Opacity"
        onValueChange={([v]) => {
          if (!node) return
          dragging.current = true
          setLive(v)
          preview(node.id, v / 100)
        }}
        onValueCommit={([v]) => {
          if (!node) return
          dragging.current = false
          preview.cancel()
          actions.setOpacity(node.id, v / 100)
          engine.current?.previewOpacity(node.id, null)
          setLive(null)
        }}
      />
      <span className="t-num w-9 shrink-0 text-right font-mono text-[11.5px] text-ink2">{pct}%</span>
    </div>
  )
}

/* ---------------- the basemap, pinned under the stack ---------------- */
const BASEMAPS: { id: BasemapStyle; label: string }[] = [
  { id: 'ldl-auto', label: 'Auto' },
  { id: 'ldl-light', label: 'Light' },
  { id: 'ldl-dark', label: 'Dark' },
  { id: 'none', label: 'None' },
]
function BasemapRow() {
  const basemap = useWs((s) => s.doc!.basemap)
  const style = BASEMAPS.find((b) => b.id === basemap.style)?.label ?? 'Auto'
  const set = (patch: Partial<MapDocument['basemap']>) => actions.setBasemap(patch)
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button type="button" className={cn('pressable flex h-10 w-full items-center gap-2 border-t border-glass-line px-3.5 text-left outline-none max-md:h-12', HOVER)}>
          <span className="grid size-5 shrink-0 place-items-center text-ink3">
            <MapIcon className="size-4" strokeWidth={1.6} />
          </span>
          <span className="text-[13px] text-ink2">Basemap</span>
          <span className="t-label ml-auto truncate">
            {style.toLowerCase()} · labels {basemap.labels === 'top' ? 'on top' : basemap.labels === 'bottom' ? 'under data' : 'off'}
          </span>
        </button>
      </PopoverTrigger>
      <PopoverContent side="top" align="start" className="w-[296px] p-3.5">
        <div className="t-label mb-2 tracking-[0.08em] uppercase">Style</div>
        <ToggleGroup type="single" value={basemap.style} onValueChange={(v) => v && set({ style: v as BasemapStyle })} aria-label="Basemap style" className="flex h-9 w-full">
          {BASEMAPS.map((b) => (
            <ToggleGroupItem key={b.id} value={b.id}>
              {b.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <p className="mt-2 text-[12px] leading-[1.45] text-ink3">Auto follows the app theme. Data colours follow the basemap, so a ramp reads the same on either.</p>
        <div className="t-label mt-4 mb-2 tracking-[0.08em] uppercase">Place and road names</div>
        <ToggleGroup type="single" value={basemap.labels} onValueChange={(v) => v && set({ labels: v as MapDocument['basemap']['labels'] })} aria-label="Basemap labels" className="flex h-9 w-full">
          <ToggleGroupItem value="top">On top</ToggleGroupItem>
          <ToggleGroupItem value="bottom">Under data</ToggleGroupItem>
          <ToggleGroupItem value="off">Off</ToggleGroupItem>
        </ToggleGroup>
        <label htmlFor="ws-coast" className="mt-4 flex cursor-pointer items-center gap-3">
          <span className="min-w-0 flex-1">
            <span className="block text-[13px] text-ink">Cut areas at the coastline</span>
            <span className="mt-0.5 block text-[12px] leading-[1.4] text-ink3">Draws the basemap's water over the area layers at the bottom of the stack.</span>
          </span>
          <Switch id="ws-coast" checked={basemap['x-coast'] !== false} onCheckedChange={(v) => set({ 'x-coast': v })} />
        </label>
      </PopoverContent>
    </Popover>
  )
}

/* ---------------- the panel ---------------- */
export function LayersPanel({ header, onAdd }: { header: ReactNode; onAdd: () => void }) {
  const count = useWs((s) => (s.doc ? rowsOf(s.doc.layers).length : 0))
  const canUndo = useWs((s) => s.past.length > 0)
  const canRedo = useWs((s) => s.future.length > 0)
  const solo = useWs((s) => s.solo)
  const soloName = useWs((s) => (s.solo && s.doc ? rowsOf(s.doc.layers).find((r) => r.node.id === s.solo)?.node.name : null))
  const said = useWs((s) => s.said)
  return (
    <div className="flex flex-col md:min-h-0 md:flex-1">
      {header}
      <div className="flex items-center gap-0.5 border-t border-glass-line pt-2 pr-2 pb-1.5 pl-3.5">
        <h2 className="t-label tracking-[0.08em] uppercase">Layers</h2>
        <span className="t-label t-num ml-1.5 text-ink2">{count}</span>
        <span className="ml-auto flex items-center">
          <Tooltip>
            <TooltipTrigger asChild>
              <button type="button" onClick={actions.undo} disabled={!canUndo} aria-label="Undo" className={cn(iconBtn, 'disabled:pointer-events-none disabled:opacity-35')}>
                <Undo2 />
              </button>
            </TooltipTrigger>
            <TooltipContent>Undo · ⌘Z</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <button type="button" onClick={actions.redo} disabled={!canRedo} aria-label="Redo" className={cn(iconBtn, 'disabled:pointer-events-none disabled:opacity-35')}>
                <Redo2 />
              </button>
            </TooltipTrigger>
            <TooltipContent>Redo · ⇧⌘Z</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <button type="button" onClick={actions.addGroup} aria-label="New group" className={iconBtn}>
                <FolderPlus />
              </button>
            </TooltipTrigger>
            <TooltipContent>New group</TooltipContent>
          </Tooltip>
          <Button variant="outline" size="sm" onClick={onAdd} className="ml-1.5 h-7 gap-1 rounded-md px-2 text-[12.5px] max-md:h-9 max-md:px-3">
            <Plus className="!size-3.5" />
            Add
          </Button>
        </span>
      </div>
      <SelectionBar />
      {solo && (
        <div className="ws-fade mx-3 mb-2 flex items-center gap-2 rounded-lg bg-brand-soft py-1 pr-1 pl-2.5 text-[12px] text-ink">
          <CircleDot className="size-3.5 shrink-0 text-brand-ink" />
          <span className="min-w-0 flex-1 truncate">Showing only “{soloName}”</span>
          <button type="button" onClick={() => actions.solo(null)} className={cn('pressable h-6 shrink-0 rounded-md px-2 text-[12px] font-medium text-ink2 max-md:h-8', HOVER)}>
            Show all
          </button>
        </div>
      )}
      <div className="flex flex-col border-t border-glass-line md:min-h-0 md:flex-1">
        <LayerTree onAdd={onAdd} />
      </div>
      <BasemapRow />
      <div className="sr-only" role="status" aria-live="polite">
        {said}
      </div>
    </div>
  )
}
