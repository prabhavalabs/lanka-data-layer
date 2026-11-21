import { useQuery } from '@tanstack/react-query'
import { ArrowRight, ArrowUpRight, Landmark as LandmarkIcon, Moon, Search, Ship, Sun, TrainFront, UsersRound } from 'lucide-react'
import { Component, lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { landmarksQuery, statsQuery } from '@/api/queries'
import type { Landmark } from '@/api/types'
import { Kbd, PcodePlate } from '@/components/common/bits'
import { useCommandPalette } from '@/components/shell/CommandPalette'
import { Button } from '@/components/ui/button'
import { geo } from '@/lib/geo'
import { fmtCoord } from '@/lib/format'
import { useInView, useMediaQuery, useReducedMotion, useTheme } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { HeroLoader } from './HeroLoader'
import type { HeroLayers } from './HeroScene'
import type { Port } from './maritimeData'

const HeroScene = lazy(() => import('./HeroScene'))

/** Design-review switch (dev only): `/?hero=loading` holds the loader, `/?hero=fallback` shows the no-WebGL poster. */
const REVIEW = import.meta.env.DEV ? new URLSearchParams(window.location.search).get('hero') : null
/** The loader stays up at least this long so it reads as a beat, not a flicker. */
const MIN_LOADER_MS = 850

function webglAvailable() {
  try {
    const c = document.createElement('canvas')
    return !!(c.getContext('webgl2') || c.getContext('webgl'))
  } catch {
    return false
  }
}

class Boundary extends Component<{ onError: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() {
    return { failed: true }
  }
  componentDidCatch() {
    this.props.onError()
  }
  render() {
    return this.state.failed ? null : this.props.children
  }
}

/** What sits behind the scene before it paints: a brand-tinted sky and a faint survey grid. */
function Backdrop({ dim }: { dim: boolean }) {
  return (
    <div className={cn('absolute inset-0 overflow-hidden transition-opacity duration-700', dim ? 'opacity-0' : 'opacity-100')} aria-hidden>
      <div className="absolute inset-0 bg-[radial-gradient(120%_80%_at_70%_60%,color-mix(in_oklab,var(--brand)_16%,var(--bg))_0%,var(--bg)_62%)]" />
      <div className="hairline-grid absolute inset-0 [mask-image:radial-gradient(70%_60%_at_68%_55%,black,transparent)]" />
    </div>
  )
}

/** Static poster: the real coastline as SVG. Only shown when WebGL is unavailable or the scene fails. */
function Poster() {
  const { data } = useQuery({ queryKey: ['geo', 'country'], queryFn: geo.country, staleTime: Infinity })
  const d = useMemo(() => {
    if (!data) return ''
    // equirectangular with cos(lat) correction, fitted to a 323 × 520 box
    const s = 520 / 4.15
    const x = (lon: number) => (lon - 79.45) * Math.cos((7.85 * Math.PI) / 180) * s
    const y = (lat: number) => (9.95 - lat) * s
    return data.rings.map((r) => 'M' + r.map(([lon, lat]) => `${x(lon).toFixed(1)} ${y(lat).toFixed(1)}`).join('L') + 'Z').join('')
  }, [data])
  return (
    <svg viewBox="0 0 323 520" className="absolute top-[52%] left-1/2 h-[74%] -translate-x-1/2 -translate-y-1/2 [animation:fade-up_700ms_var(--ease-out)_both] lg:left-[69%]" aria-hidden>
      <defs>
        <linearGradient id="poster-land" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#7fbf5c" />
          <stop offset="0.55" stopColor="#2f7a45" />
          <stop offset="1" stopColor="#1d5236" />
        </linearGradient>
        <filter id="poster-glow" x="-30%" y="-30%" width="160%" height="160%">
          <feGaussianBlur stdDeviation="9" />
        </filter>
      </defs>
      <path d={d} fill="#2b9aa0" opacity="0.4" filter="url(#poster-glow)" />
      <path d={d} fill="url(#poster-land)" stroke="#e6d3a3" strokeWidth="1.2" strokeLinejoin="round" />
    </svg>
  )
}

function LandmarkCard({ lm }: { lm: Landmark }) {
  return (
    <div className="pointer-events-none flex w-0 flex-col items-center">
      <div className="flex -translate-y-full flex-col items-center [animation:fade-up_260ms_var(--ease-out)]">
        <div className="glass w-[236px] rounded-[14px] p-3.5 text-left">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="truncate font-mono text-[10.5px] text-ink3">{lm.kind}</div>
              <div className="mt-0.5 text-[15px] leading-tight font-semibold tracking-[-0.015em] text-ink">{lm.name.en}</div>
            </div>
            <PcodePlate code={lm.pcode} size="sm" />
          </div>
          <div className="mt-1.5 flex flex-wrap gap-x-2 text-[12.5px] leading-snug text-ink2">
            <span lang="si">{lm.name.si}</span>
            <span className="text-ink3">·</span>
            <span lang="ta">{lm.name.ta}</span>
          </div>
          <dl className="mt-3 grid grid-cols-3 gap-2 border-t border-glass-line pt-2.5">
            {lm.facts.map((f) => (
              <div key={f.label} className="min-w-0">
                <dt className="truncate text-[10.5px] text-ink3">{f.label}</dt>
                <dd className="t-num mt-0.5 truncate text-[12.5px] font-medium text-ink">{f.value}</dd>
              </div>
            ))}
          </dl>
          <div className="mt-2.5 flex items-center justify-between font-mono text-[10.5px] text-ink3">
            <span>{fmtCoord(lm.lat, lm.lon)}</span>
            <span>{lm.district}</span>
          </div>
        </div>
        <div className="h-7 w-px bg-gradient-to-b from-[var(--ink2)] to-transparent" />
      </div>
    </div>
  )
}

function PortCard({ port }: { port: Port }) {
  return (
    <div className="pointer-events-none flex w-0 flex-col items-center">
      <div className="flex -translate-y-full flex-col items-center [animation:fade-up_200ms_var(--ease-out)]">
        <div className="glass w-[252px] rounded-[14px] p-3.5 text-left">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="truncate font-mono text-[10.5px] text-ink3">Port · {port.role}</div>
              <div className="mt-0.5 text-[15px] leading-tight font-semibold tracking-[-0.015em] text-ink">{port.name.en}</div>
            </div>
            <PcodePlate code={port.locode} size="sm" />
          </div>
          <div className="mt-1.5 flex flex-wrap gap-x-2 text-[12.5px] leading-snug text-ink2">
            <span lang="si">{port.name.si}</span>
            <span className="text-ink3">·</span>
            <span lang="ta">{port.name.ta}</span>
          </div>
          <dl className="mt-3 grid grid-cols-3 gap-2 border-t border-glass-line pt-2.5">
            {[
              ['In port', String(port.inPort)],
              ['Arrivals 24 h', String(port.arrivals24h)],
              ['Draught', port.draught],
            ].map(([label, value]) => (
              <div key={label} className="min-w-0">
                <dt className="truncate text-[10.5px] text-ink3">{label}</dt>
                <dd className="t-num mt-0.5 truncate text-[12.5px] font-medium text-ink">{value}</dd>
              </div>
            ))}
          </dl>
          <div className="mt-2.5 flex items-center justify-between font-mono text-[10.5px] text-ink3">
            <span>{fmtCoord(port.lat, port.lon)}</span>
            <span>mock AIS</span>
          </div>
        </div>
        <div className="h-6 w-px bg-gradient-to-b from-[var(--brand-ink)] to-transparent" />
      </div>
    </div>
  )
}

const TOUR = ['sigiriya', 'lotus-tower', 'temple-tooth', 'ruwanwelisaya', 'sri-pada', 'nallur', 'trinco', 'nine-arch', 'galle-fort']
const EXAMPLES = ['Colombo', 'LK2127', '10250', '6.93, 79.84']

export function Hero() {
  const [theme, setTheme] = useTheme()
  const mood = theme === 'light' ? 'day' : 'dusk'
  const reduced = useReducedMotion()
  const coarse = useMediaQuery('(pointer: coarse)')
  const narrow = useMediaQuery('(max-width: 767px)')
  const { open } = useCommandPalette()
  const { data: landmarks = [] } = useQuery(landmarksQuery())
  const { data: stats } = useQuery(statsQuery())

  // Maritime is on from the start: it gives the open sea in the foreground something to do and never covers land.
  const [layers, setLayers] = useState<HeroLayers>({ landmarks: true, maritime: true, population: false, rail: false })
  const [active, setActive] = useState<string | null>(null)
  const [port, setPort] = useState<string | null>(null)
  const [stage, setStage] = useState(0)
  const [ready, setReady] = useState(false)
  const [loaderGone, setLoaderGone] = useState(false)
  const [failed, setFailed] = useState(() => REVIEW === 'fallback' || !webglAvailable())
  const mountedAt = useRef(performance.now())
  const lastTouch = useRef(0)
  const [ref, visible] = useInView<HTMLElement>({ once: false, margin: '0px' })
  const [tabVisible, setTabVisible] = useState(true)

  const lowPower = useMemo(() => coarse || narrow || (navigator.hardwareConcurrency ?? 8) <= 4, [coarse, narrow])

  useEffect(() => {
    const on = () => setTabVisible(document.visibilityState === 'visible')
    document.addEventListener('visibilitychange', on)
    return () => document.removeEventListener('visibilitychange', on)
  }, [])

  // guided tour of the landmarks until the visitor takes over
  useEffect(() => {
    if (!ready || !layers.landmarks || !visible || !tabVisible || narrow) return
    if (reduced) {
      setActive((a) => a ?? 'sigiriya')
      return
    }
    let i = active ? TOUR.indexOf(active) : -1
    const first = window.setTimeout(() => setActive((a) => a ?? TOUR[(i = 0)]), 900)
    const id = window.setInterval(() => {
      if (performance.now() - lastTouch.current < 9000) return
      i = (i + 1) % TOUR.length
      setActive(TOUR[i])
    }, 5200)
    return () => {
      window.clearTimeout(first)
      window.clearInterval(id)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, layers.landmarks, visible, tabVisible, reduced, narrow])

  const onActive = useCallback((id: string | null) => {
    lastTouch.current = performance.now()
    if (id) setActive(id)
  }, [])
  const onSelect = useCallback((id: string) => {
    lastTouch.current = performance.now()
    setActive(id)
  }, [])
  // first frame is on the glass: hand over from the loader, but never so fast that it only flickers
  const onReady = useCallback(() => {
    window.setTimeout(() => setReady(true), Math.max(0, MIN_LOADER_MS - (performance.now() - mountedAt.current)))
  }, [])
  const onError = useCallback(() => setFailed(true), [])
  const onStage = useCallback((s: number) => setStage((cur) => Math.max(cur, s)), [])
  const onPort = useCallback((id: string | null) => {
    lastTouch.current = performance.now()
    setPort(id)
  }, [])
  const renderCard = useCallback((lm: Landmark) => <LandmarkCard lm={lm} />, [])
  const renderPortCard = useCallback((p: Port) => <PortCard port={p} />, [])
  // keep the loader mounted through its exit, then drop it so nothing keeps spinning under the scene
  useEffect(() => {
    if (!ready) return
    const id = window.setTimeout(() => setLoaderGone(true), 800)
    return () => window.clearTimeout(id)
  }, [ready])

  const toggle = (k: keyof HeroLayers) => {
    if (k === 'maritime') setPort(null)
    setLayers((l) => {
      lastTouch.current = performance.now()
      return { ...l, [k]: !l[k] }
    })
  }

  const chips: { k: keyof HeroLayers; label: string; Icon: typeof Sun }[] = [
    { k: 'landmarks', label: 'Landmarks', Icon: LandmarkIcon },
    { k: 'maritime', label: 'Maritime', Icon: Ship },
    { k: 'population', label: 'Population', Icon: UsersRound },
    { k: 'rail', label: 'Rail', Icon: TrainFront },
  ]

  return (
    <section ref={ref} className="relative isolate h-[100svh] max-h-[1080px] min-h-[660px] overflow-hidden bg-bg" aria-label="Sri Lanka, live">
      {/* scene */}
      <div className="absolute inset-0">
        <Backdrop dim={ready && !failed} />
        {failed && <Poster />}
        {!failed && REVIEW !== 'loading' && (
          <div className={cn('absolute inset-0 transition-opacity duration-1000 ease-out', ready ? 'opacity-100' : 'opacity-0')}>
            <Boundary onError={onError}>
              <Suspense fallback={null}>
                <HeroScene
                  landmarks={landmarks}
                  mood={mood}
                  layers={layers}
                  active={port ? null : active}
                  onActive={onActive}
                  onSelect={onSelect}
                  renderCard={renderCard}
                  port={port}
                  onPort={onPort}
                  renderPortCard={renderPortCard}
                  onStage={onStage}
                  revealed={ready}
                  animate={!reduced}
                  lowPower={lowPower}
                  paused={!visible || !tabVisible}
                  onReady={onReady}
                  onError={onError}
                />
              </Suspense>
            </Boundary>
          </div>
        )}
        {!failed && !loaderGone && <HeroLoader stage={REVIEW === 'loading' ? 2 : stage} done={ready} hold={REVIEW === 'loading' ? 64 : undefined} />}
      </div>

      {/* legibility: a soft wash behind the headline, and a fade into the page below */}
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-[linear-gradient(to_bottom,color-mix(in_oklab,var(--bg)_82%,transparent)_0%,transparent_46%)] lg:bg-[linear-gradient(100deg,color-mix(in_oklab,var(--bg)_78%,transparent)_0%,color-mix(in_oklab,var(--bg)_38%,transparent)_30%,transparent_56%)]" />
      <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-40 bg-gradient-to-b from-transparent to-[var(--bg)]" />

      {/* copy */}
      <div className="pointer-events-none relative z-10 mx-auto flex h-full max-w-[1320px] flex-col px-5 pt-[calc(var(--nav-h)+22px)] sm:px-8 lg:justify-center lg:pt-0 lg:pb-16">
        <div className="max-w-[600px] [&>*]:[animation:fade-up_700ms_var(--ease-out)_both]">
          <Link
            to="/data"
            className="pressable pointer-events-auto inline-flex items-center gap-2 rounded-full border border-glass-line bg-[var(--glass)] py-1 pr-2.5 pl-2 font-mono text-[11.5px] text-ink2 backdrop-blur-md hover:text-ink"
          >
            <span className="relative grid size-2 place-items-center">
              <span className="absolute size-2 rounded-full bg-ok opacity-60 [animation:pulse-dot_2.4s_ease-in-out_infinite]" />
              <span className="size-1.5 rounded-full bg-ok" />
            </span>
            data_version {stats?.dataVersion ?? '········.·'}
            <span className="text-ink3">·</span>
            {stats?.datasets ?? '··'} datasets
            <ArrowRight className="size-3" />
          </Link>
          <h1 className="t-display mt-4 [animation-delay:60ms] sm:mt-5">
            Every Sri Lankan fact has a place, a source and a download.
          </h1>
          <p className="t-lead mt-3.5 max-w-[46ch] [animation-delay:120ms] sm:mt-5">
            Open geo-data for the whole island, keyed to official p-codes from province to GN division.
            <span className="hidden sm:inline"> Read it as maps and area profiles, or take it through the API, the MCP server and the <code className="font-mono text-[0.92em] text-ink">lanka</code> CLI.</span>
          </p>

          <button
            type="button"
            onClick={open}
            className="pressable glass pointer-events-auto mt-5 flex h-12 w-full sm:mt-7 sm:h-[52px] max-w-[520px] items-center gap-3 rounded-[14px] px-4 text-left [animation-delay:180ms] hover:border-[var(--ink3)]"
          >
            <Search className="size-[18px] shrink-0 text-ink3" />
            <span className="flex-1 truncate text-[15px] text-ink3">Search a place, p-code or postal code</span>
            <span className="hidden gap-0.5 sm:flex">
              <Kbd>⌘</Kbd>
              <Kbd>K</Kbd>
            </span>
          </button>
          <div className="pointer-events-auto mt-3 hidden flex-wrap items-center gap-1.5 [animation-delay:220ms] sm:flex">
            <span className="mr-1 text-[12.5px] text-ink3">Try</span>
            {EXAMPLES.map((e) => (
              <button key={e} type="button" onClick={open} className="pressable rounded-md border border-glass-line bg-[var(--glass)] px-2 py-0.5 font-mono text-[11.5px] text-ink2 backdrop-blur-md hover:text-ink">
                {e}
              </button>
            ))}
          </div>

          <div className="pointer-events-auto mt-4 flex flex-wrap gap-2.5 [animation-delay:280ms] sm:mt-8 max-sm:[&>a]:h-10 max-sm:[&>a]:px-3.5 max-sm:[&>a]:text-sm">
            <Button size="lg" variant="brand" asChild>
              <Link to="/explore">
                Explore the map <ArrowRight />
              </Link>
            </Button>
            <Button size="lg" variant="glass" asChild>
              <Link to="/developers">
                API, MCP and CLI <ArrowUpRight />
              </Link>
            </Button>
          </div>
        </div>
      </div>

      {/* instrument readout + layer controls */}
      <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 mx-auto flex max-w-[1320px] items-end justify-between gap-4 px-5 pb-[max(20px,env(safe-area-inset-bottom))] sm:px-8 sm:pb-7">
        <div className="hidden font-mono text-[11px] leading-relaxed text-ink3 md:block">
          <div>LK · 7.8731°N 80.7718°E · 65,610 km²</div>
          <div>Real coastline, landmarks at true coordinates. Relief is stylised.</div>
        </div>
        <div className="glass pointer-events-auto flex max-w-full items-center gap-0.5 overflow-x-auto rounded-[13px] p-1" role="group" aria-label="Hero layers">
          {chips.map(({ k, label, Icon }) => (
            <button
              key={k}
              type="button"
              aria-pressed={layers[k]}
              title={label}
              onClick={() => toggle(k)}
              disabled={failed}
              className={cn(
                'pressable flex h-8 shrink-0 items-center gap-1.5 rounded-[9px] px-2.5 text-[12.5px] font-medium disabled:opacity-40',
                layers[k] ? 'bg-ink text-bg' : 'text-ink2 hover:bg-bg4/60 hover:text-ink',
              )}
            >
              <Icon className="size-3.5" />
              {/* phones: only the layers that are on keep their label, so the whole bar fits */}
              <span className={cn(!layers[k] && 'max-sm:sr-only')}>{label}</span>
            </button>
          ))}
          <span className="mx-1 h-4 w-px shrink-0 bg-glass-line" />
          <button
            type="button"
            onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
            className="pressable flex h-8 shrink-0 items-center gap-1.5 rounded-[9px] px-2.5 text-[12.5px] font-medium text-ink2 hover:bg-bg4/60 hover:text-ink"
            aria-label={`Switch to ${mood === 'dusk' ? 'day' : 'dusk'}`}
          >
            {mood === 'dusk' ? <Moon className="size-3.5" /> : <Sun className="size-3.5" />}
            <span className="max-sm:sr-only">{mood === 'dusk' ? 'Dusk' : 'Day'}</span>
          </button>
        </div>
      </div>
    </section>
  )
}
