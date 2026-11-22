import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'

export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (cb: () => void) => {
      const m = window.matchMedia(query)
      m.addEventListener('change', cb)
      return () => m.removeEventListener('change', cb)
    },
    [query],
  )
  return useSyncExternalStore(subscribe, () => window.matchMedia(query).matches, () => false)
}

export const useReducedMotion = () => useMediaQuery('(prefers-reduced-motion: reduce)')
export const useIsDesktop = () => useMediaQuery('(min-width: 1024px)')
export const useFinePointer = () => useMediaQuery('(hover: hover) and (pointer: fine)')

/** True once the element has entered the viewport (one-shot by default). */
export function useInView<T extends Element>(opts: { once?: boolean; margin?: string; threshold?: number } = {}) {
  const { once = true, margin = '0px 0px -12% 0px', threshold = 0 } = opts
  const ref = useRef<T | null>(null)
  const [inView, setInView] = useState(false)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const io = new IntersectionObserver(
      ([e]) => {
        if (e.isIntersecting) {
          setInView(true)
          if (once) io.disconnect()
        } else if (!once) setInView(false)
      },
      { rootMargin: margin, threshold },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [once, margin, threshold])
  return [ref, inView] as const
}

/* ---------------- theme ---------------- */
export type Theme = 'dark' | 'light'
const THEME_KEY = 'ldl-theme'
const listeners = new Set<() => void>()
const readTheme = (): Theme => (document.documentElement.classList.contains('light') ? 'light' : 'dark')

function applyTheme(t: Theme) {
  const root = document.documentElement
  if (readTheme() === t) return
  root.classList.remove('dark', 'light')
  root.classList.add(t)
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', t === 'dark' ? '#0b0e12' : '#fbfbfa')
  listeners.forEach((l) => l())
}

export function setTheme(t: Theme) {
  try {
    localStorage.setItem(THEME_KEY, t)
  } catch {
    /* private mode */
  }
  applyTheme(t)
}

/** Dark-first for hero + map, light "sheet" for reading pages — until the user picks. */
export function applyRouteDefaultTheme(pathname: string) {
  let stored: string | null = null
  try {
    stored = localStorage.getItem(THEME_KEY)
  } catch {
    /* ignore */
  }
  if (stored === 'dark' || stored === 'light') return applyTheme(stored)
  applyTheme(pathname === '/' || pathname.startsWith('/explore') || pathname.startsWith('/m/') ? 'dark' : 'light')
}

export function useTheme(): [Theme, (t: Theme) => void] {
  const theme = useSyncExternalStore(
    (cb) => {
      listeners.add(cb)
      return () => listeners.delete(cb)
    },
    readTheme,
    () => 'dark' as Theme,
  )
  return [theme, setTheme]
}

/** Copy-to-clipboard with a transient "copied" flag. */
export function useCopy(timeout = 1400) {
  const [copied, setCopied] = useState(false)
  const t = useRef<number | undefined>(undefined)
  const copy = useCallback(
    (text: string) => {
      void navigator.clipboard?.writeText(text).catch(() => {})
      setCopied(true)
      window.clearTimeout(t.current)
      t.current = window.setTimeout(() => setCopied(false), timeout)
    },
    [timeout],
  )
  useEffect(() => () => window.clearTimeout(t.current), [])
  return [copied, copy] as const
}
