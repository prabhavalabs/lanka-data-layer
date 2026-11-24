import { GitBranch as Github, Menu, Moon, Search, Sun } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, NavLink, useLocation } from 'react-router'
import { Kbd, Logo } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetTitle, SheetTrigger } from '@/components/ui/sheet'
import { useTheme } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { useCommandPalette } from './CommandPalette'

const LINKS = [
  { to: '/explore', label: 'Explore' },
  { to: '/area/LK11', label: 'Areas', match: '/area' },
  { to: '/data', label: 'Data' },
  { to: '/government', label: 'Government' },
  { to: '/developers', label: 'Developers' },
]

/** Glass top bar. Transparent over the hero, materialises and tightens once the page scrolls. */
export function TopNav({ overlay = false }: { overlay?: boolean }) {
  const [scrolled, setScrolled] = useState(false)
  const [theme, setTheme] = useTheme()
  const { open } = useCommandPalette()
  const { pathname } = useLocation()

  useEffect(() => {
    const on = () => setScrolled(window.scrollY > 12)
    on()
    window.addEventListener('scroll', on, { passive: true })
    return () => window.removeEventListener('scroll', on)
  }, [])

  const solid = scrolled || !overlay
  return (
    <header className="pointer-events-none fixed inset-x-0 top-0 z-50 px-3 pt-[max(10px,env(safe-area-inset-top))] sm:px-5">
      <div
        className={cn(
          'pointer-events-auto relative mx-auto flex h-[46px] items-center gap-1 rounded-[14px] pr-1.5 pl-3.5 transition-[max-width] duration-500 ease-[var(--ease-out)]',
          solid ? 'max-w-[1080px]' : 'max-w-[1320px]',
        )}
      >
        {/* material layer: fades and settles in rather than snapping */}
        <div
          aria-hidden
          className={cn(
            'glass absolute inset-0 rounded-[14px] transition-[opacity,transform] duration-300 ease-[var(--ease-out)]',
            solid ? 'opacity-100' : 'scale-[0.985] opacity-0',
          )}
        />
        <Link to="/" className="relative mr-3 rounded-md" aria-label="Lanka Data Layer home">
          <Logo />
        </Link>
        <nav className="relative hidden items-center md:flex" aria-label="Primary">
          {LINKS.map((l) => {
            const active = pathname.startsWith(l.match ?? l.to)
            return (
              <NavLink
                key={l.to}
                to={l.to}
                className={cn('pressable rounded-lg px-3 py-1.5 text-[13.5px] font-medium', active ? 'bg-bg4/70 text-ink' : 'text-ink2 hover:text-ink')}
              >
                {l.label}
              </NavLink>
            )
          })}
        </nav>
        <div className="relative ml-auto flex items-center gap-1">
          <button
            type="button"
            onClick={open}
            className="pressable hidden h-8 w-[220px] items-center gap-2 rounded-lg border border-glass-line bg-bg3/50 px-2.5 text-[13px] text-ink3 hover:border-line2 hover:text-ink2 lg:flex"
          >
            <Search className="size-3.5" />
            <span className="flex-1 text-left">Search places, data…</span>
            <span className="flex gap-0.5">
              <Kbd>⌘</Kbd>
              <Kbd>K</Kbd>
            </span>
          </button>
          <Button variant="ghost" size="icon-sm" className="lg:hidden" onClick={open} aria-label="Search">
            <Search />
          </Button>
          <Button variant="ghost" size="icon-sm" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')} aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} theme`}>
            {theme === 'dark' ? <Sun /> : <Moon />}
          </Button>
          <Button variant="ghost" size="icon-sm" asChild className="hidden sm:inline-flex">
            <a href="https://github.com/prabhavalabs" target="_blank" rel="noreferrer" aria-label="Source on GitHub">
              <Github />
            </a>
          </Button>
          <Sheet>
            <SheetTrigger asChild>
              <Button variant="ghost" size="icon-sm" className="md:hidden" aria-label="Menu">
                <Menu />
              </Button>
            </SheetTrigger>
            <SheetContent side="bottom" aria-describedby={undefined}>
              <SheetTitle className="px-5 pt-5 font-mono text-xs font-normal text-ink3">Menu</SheetTitle>
              <SheetDescription className="sr-only">Site navigation</SheetDescription>
              <nav className="flex flex-col p-3 pb-5">
                {[{ to: '/', label: 'Home' }, ...LINKS].map((l) => (
                  <SheetClose asChild key={l.to}>
                    <Link to={l.to} className="pressable rounded-xl px-3 py-3 text-[22px] font-semibold tracking-[-0.03em] text-ink active:bg-bg3">
                      {l.label}
                    </Link>
                  </SheetClose>
                ))}
              </nav>
            </SheetContent>
          </Sheet>
        </div>
      </div>
    </header>
  )
}
