import { useQuery } from '@tanstack/react-query'
import { BookOpen, Database, LineChart, Map as MapIcon, MapPin, Moon, Sun, Terminal } from 'lucide-react'
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { searchQuery } from '@/api/queries'
import type { SearchHit } from '@/api/types'
import { PcodePlate } from '@/components/common/bits'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandShortcut } from '@/components/ui/command'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { useTheme } from '@/lib/hooks'

const Ctx = createContext<{ open: () => void }>({ open: () => {} })
export const useCommandPalette = () => useContext(Ctx)

const ICON = { area: MapPin, dataset: Database, indicator: LineChart, place: MapPin } as const
const GROUP = { area: 'Areas', dataset: 'Datasets', indicator: 'Indicators', place: 'Places' } as const

export function CommandPaletteProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const [q, setQ] = useState('')
  const navigate = useNavigate()
  const [theme, setTheme] = useTheme()
  const { data: hits = [] } = useQuery({ ...searchQuery(q), enabled: open })

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setOpen((o) => !o)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const go = useCallback(
    (href: string) => {
      setOpen(false)
      setQ('')
      navigate(href)
    },
    [navigate],
  )

  const groups = useMemo(() => {
    const g: Record<string, SearchHit[]> = {}
    for (const h of hits) (g[h.kind] ??= []).push(h)
    return (['area', 'dataset', 'indicator', 'place'] as const).filter((k) => g[k]?.length).map((k) => [k, g[k].slice(0, q ? 8 : 5)] as const)
  }, [hits, q])

  const api = useMemo(() => ({ open: () => setOpen(true) }), [])

  return (
    <Ctx.Provider value={api}>
      {children}
      <Dialog open={open} onOpenChange={setOpen}>
        {/* Opened by keyboard hundreds of times a day: no enter/exit animation. */}
        <DialogContent showClose={false} className="top-[12vh] max-w-[620px] overflow-hidden p-0 !animate-none" aria-describedby={undefined}>
          <DialogTitle className="sr-only">Search Lanka Data Layer</DialogTitle>
          <DialogDescription className="sr-only">Find areas, datasets, indicators and places</DialogDescription>
          <Command shouldFilter={false} loop>
            <CommandInput value={q} onValueChange={setQ} placeholder="Search a place, p-code, postal code, dataset…" />
            <CommandList>
              <CommandEmpty>Nothing held under “{q}”. Try a district, a p-code such as LK11, or a dataset name.</CommandEmpty>
              {groups.map(([kind, items]) => {
                const Icon = ICON[kind]
                return (
                  <CommandGroup key={kind} heading={GROUP[kind]}>
                    {items.map((h) => (
                      <CommandItem key={h.kind + h.id} value={h.kind + h.id} onSelect={() => go(h.href)}>
                        <Icon />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-ink">{h.title}</span>
                          <span className="block truncate text-xs text-ink3">{h.subtitle}</span>
                        </span>
                        {h.code && (h.kind === 'area' ? <PcodePlate code={h.code} size="sm" /> : <CommandShortcut>{h.code}</CommandShortcut>)}
                      </CommandItem>
                    ))}
                  </CommandGroup>
                )
              })}
              {!q && (
                <CommandGroup heading="Go to">
                  <CommandItem value="go-explore" onSelect={() => go('/explore')}>
                    <MapIcon /> Explore map
                  </CommandItem>
                  <CommandItem value="go-data" onSelect={() => go('/data')}>
                    <Database /> Data catalogue
                  </CommandItem>
                  <CommandItem value="go-dev" onSelect={() => go('/developers')}>
                    <Terminal /> Developers: REST, MCP, CLI
                  </CommandItem>
                  <CommandItem value="go-area" onSelect={() => go('/area/LK11')}>
                    <BookOpen /> Area profile: Colombo
                  </CommandItem>
                  <CommandItem
                    value="toggle-theme"
                    onSelect={() => {
                      setTheme(theme === 'dark' ? 'light' : 'dark')
                      setOpen(false)
                    }}
                  >
                    {theme === 'dark' ? <Sun /> : <Moon />} Switch to {theme === 'dark' ? 'light' : 'dark'} theme
                  </CommandItem>
                </CommandGroup>
              )}
            </CommandList>
            <div className="flex items-center gap-4 border-t border-line px-4 py-2 font-mono text-[11px] text-ink3">
              <span>↑↓ navigate</span>
              <span>↵ open</span>
              <span>esc close</span>
              <span className="ml-auto">resolve_place · search_catalog</span>
            </div>
          </Command>
        </DialogContent>
      </Dialog>
    </Ctx.Provider>
  )
}
