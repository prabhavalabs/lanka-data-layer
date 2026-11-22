import { useQuery } from '@tanstack/react-query'
import { ArrowRight, ArrowUpRight, Landmark as LandmarkIcon, Moon, Search, Ship, Sun, TrainFront, UsersRound } from 'lucide-react'
import { Component, lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type ReactNode } from 'react'
import { Link } from 'react-router'
import { landmarksQuery, statsQuery } from '@/api/queries'
import type { Landmark } from '@/api/types'
import { Kbd, PcodePlate } from '@/components/common/bits'
import { useCommandPalette } from '@/components/shell/CommandPalette'
import { Button } from '@/components/ui/button'
import { fmtCoord } from '@/lib/format'
import { useInView, useMediaQuery, useReducedMotion, useTheme, type Theme } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import type { Focus } from './camera'
import { HeroLoader } from './HeroLoader'
import type { HeroLayers } from './HeroScene'
import { portKey, type Port } from './maritimeData'
import { SCENE_THEMES } from './sceneTheme'
import type { CardPlacement, SafeRect } from './stage'

const HeroScene = lazy(() => import('./HeroScene'))

/**
 * Design-review switch (dev only): `/?hero=loading` holds the loader, `/?hero=fallback` shows the no-WebGL poster,
 * `/?hero=glfail` refuses the renderer its context (the real failure path), `/?hero=poster` shows the scene alone,
 * with no copy or controls over it, for rendering the poster stills.
 */
const REVIEW = import.meta.env.DEV ? new URLSearchParams(window.location.search).get('hero') : null
const BARE = REVIEW === 'poster'
/** Dev only: `?focus=sigiriya[:hover|pin|tour]` (or `port:colombo`) holds one card and its camera move, for reviewing placements. */
const HOLD = import.meta.env.DEV
  ? (() => {
      const [id, kind = 'hover'] = (new URLSearchParams(window.location.search).get('focus') ?? '').split(/:(?=hover|pin|tour)/)
      return id ? ({ id, kind } as Focus) : null
    })()
  : null
/** Dev only: `?view=…` parks the camera for a close-up (see HeroScene), so the tour stays out of the picture. */
const STILL = import.meta.env.DEV && (new URLSearchParams(window.location.search).has('view') || !!HOLD)
/** Dev only: `?layers=landmarks,rail` starts with exactly those layers on, for review shots. */
const START_LAYERS = import.meta.env.DEV ? new URLSearchParams(window.location.search).get('layers')?.split(',') : undefined
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

/** What sits behind the scene before it paints: the scene's own horizon haze and a faint survey grid, so the hand-over is a fade and not a change of colour. */
function Backdrop({ dim }: { dim: boolean }) {
  return (
    <div className={cn('absolute inset-0 overflow-hidden transition-opacity duration-700', dim ? 'opacity-0' : 'opacity-100')} aria-hidden>
      <div className="absolute inset-0 bg-[radial-gradient(120%_80%_at_70%_58%,color-mix(in_oklab,var(--hero-haze)_78%,var(--bg))_0%,var(--bg)_64%)]" />
      <div className="hairline-grid absolute inset-0 [mask-image:radial-gradient(70%_60%_at_68%_55%,black,transparent)]" />
    </div>
  )
}

/**
 * Static poster: a still of the scene itself, one per theme and per framing (the island beside the headline, or
 * under it). Only shown when WebGL is unavailable or the scene fails. The stills are rendered from the live scene
 * by scripts/build-poster.mjs (`pnpm poster`), so they carry the same island, landmarks and sea.
 */
function Poster({ theme }: { theme: Theme }) {
  // the same test the camera rig applies to the canvas: the hero is as wide as the window and between 660 and 1080 px tall
  const wide = useSyncExternalStore(
    (cb) => {
      window.addEventListener('resize', cb)
      return () => window.removeEventListener('resize', cb)
    },
    () => window.innerWidth / Math.min(1080, Math.max(660, window.innerHeight)) > 1.15,
    () => true,
  )
  const [loaded, setLoaded] = useState(false)
  return (
    <img
      src={`/hero/poster-${theme}-${wide ? 'wide' : 'tall'}.webp`}
      alt="A relief model of Sri Lanka with its landmarks, ports and shipping lanes"
      decoding="async"
      draggable={false}
      onLoad={() => setLoaded(true)}
      // the island is what must survive a crop: right of centre on wide screens, low on tall ones
      className={cn('absolute inset-0 size-full object-cover transition-opacity duration-700 ease-out select-none', wide ? '[object-position:74%_60%]' : '[object-position:50%_78%]', loaded ? 'opacity-100' : 'opacity-0')}
    />
  )
}

/**
 * A card pinned to something in the scene. The scene decides where it hangs (clear of the
 * other landmarks, inside the free part of the canvas); this draws it there, with a hairline
 * back to what it describes. It scales in from the point it hangs by, so it reads as coming
 * out of its subject, and glides if the placement changes while it is open.
 */
function PinnedCard({ at, tint, children }: { at: CardPlacement; tint: string; children: ReactNode }) {
  const dx = at.x - at.lx, dy = at.y - at.ly
  return (
    <div className="pointer-events-none absolute top-0 left-0">
      <div
        aria-hidden
        className="absolute top-0 left-0 h-px origin-left [animation:overlay-in_320ms_var(--ease-out)]"
        style={{ width: Math.hypot(dx, dy), transform: `translate(${at.lx}px, ${at.ly}px) rotate(${Math.atan2(dy, dx)}rad)`, background: `linear-gradient(to right, transparent, ${tint})` }}
      />
      <div className="absolute top-0 left-0 transition-transform duration-500 ease-[var(--ease-out)] motion-reduce:transition-none" style={{ transform: `translate(${at.x}px, ${at.y}px) translate(${-at.ax * 100}%, ${-at.ay * 100}%)` }}>
        {/* no fill mode: an animation that stays applied keeps the card in its own compositing group, and the glass would have nothing behind it to blur */}
        <div className="[animation:pop-in_260ms_var(--ease-out)]" style={{ transformOrigin: `${at.ax * 100}% ${at.ay * 100}%` } as CSSProperties}>
          {children}
        </div>
      </div>
    </div>
  )
}

function LandmarkCard({ lm, at }: { lm: Landmark; at: CardPlacement }) {
  return (
    <PinnedCard at={at} tint="var(--ink2)">
      <div className="glass w-[236px] rounded-[14px] p-3.5 text-left max-sm:p-3">
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
        <dl className="mt-3 grid grid-cols-3 gap-2 border-t border-glass-line pt-2.5 max-sm:mt-2.5 max-sm:pt-2">
          {lm.facts.map((f) => (
            <div key={f.label} className="min-w-0">
              <dt className="truncate text-[10.5px] text-ink3">{f.label}</dt>
              <dd className="t-num mt-0.5 truncate text-[12.5px] font-medium text-ink">{f.value}</dd>
            </div>
          ))}
        </dl>
        {/* phones: the island has to share the screen with the headline, so the card gives up its last row */}
        <div className="mt-2.5 flex items-center justify-between font-mono text-[10.5px] text-ink3 max-sm:hidden">
          <span>{fmtCoord(lm.lat, lm.lon)}</span>
          <span>{lm.district}</span>
        </div>
      </div>
    </PinnedCard>
  )
}

function PortCard({ port, at }: { port: Port; at: CardPlacement }) {
  return (
    <PinnedCard at={at} tint="var(--brand-ink)">
      <div className="glass w-[252px] rounded-[14px] p-3.5 text-left max-sm:p-3">
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
        <dl className="mt-3 grid grid-cols-3 gap-2 border-t border-glass-line pt-2.5 max-sm:mt-2.5 max-sm:pt-2">
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
        <div className="mt-2.5 flex items-center justify-between font-mono text-[10.5px] text-ink3 max-sm:hidden">
          <span>{fmtCoord(port.lat, port.lon)}</span>
          <span>mock AIS</span>
        </div>
      </div>
    </PinnedCard>
  )
}

const TOUR = ['sigiriya', 'lotus-tower', 'temple-tooth', 'ruwanwelisaya', 'sri-pada', 'nallur', 'trinco', 'nine-arch', 'galle-fort']
const EXAMPLES = ['Colombo', 'LK2127', '10250', '6.93, 79.84']
/** the pointer has left a landmark: wait this long before the camera goes home, in case it is on its way to the next one */
const RELEASE_MS = 900
/** a clicked or tapped landmark holds the camera this long, then lets it go */
const PIN_MS = 12000
/** the tour waits this long after the visitor last touched the scene */
const TOUR_IDLE_MS = 9000

export function Hero() {
  const [theme, setTheme] = useTheme()
  const mood = theme === 'light' ? 'day' : 'night'
  const reduced = useReducedMotion()
  const coarse = useMediaQuery('(pointer: coarse)')
  const narrow = useMediaQuery('(max-width: 767px)')
  const { open } = useCommandPalette()
  const { data: landmarks = [] } = useQuery(landmarksQuery())
  const { data: stats } = useQuery(statsQuery())

  // Maritime is on from the start: it gives the open sea in the foreground something to do and never covers land.
  const [layers, setLayers] = useState<HeroLayers>(() =>
    START_LAYERS
      ? { landmarks: START_LAYERS.includes('landmarks'), maritime: START_LAYERS.includes('maritime'), population: START_LAYERS.includes('population'), rail: START_LAYERS.includes('rail') }
      : { landmarks: true, maritime: true, population: false, rail: false },
  )
  const [active, setActive] = useState<string | null>(null)
  const [port, setPort] = useState<string | null>(null)
  // What the camera leans toward. Separate from `active`: a card can stay up after the camera has gone home.
  const [focus, setFocus] = useState<Focus | null>(null)
  const focusRef = useRef(focus)
  focusRef.current = focus
  const [stage, setStage] = useState(0)
  const [ready, setReady] = useState(false)
  const [loaderGone, setLoaderGone] = useState(false)
  const [failed, setFailed] = useState(() => REVIEW === 'fallback' || !webglAvailable())
  const mountedAt = useRef(performance.now())
  const lastTouch = useRef(0)
  const release = useRef(0)
  const [ref, visible] = useInView<HTMLElement>({ once: false, margin: '0px' })
  const copy = useRef<HTMLDivElement>(null)
  const bar = useRef<HTMLDivElement>(null)
  const [tabVisible, setTabVisible] = useState(true)

  // the poster stills are always rendered at full quality, whatever the window they are rendered in
  const lowPower = useMemo(() => !BARE && (coarse || narrow || (navigator.hardwareConcurrency ?? 8) <= 4), [coarse, narrow])

  useEffect(() => {
    const on = () => setTabVisible(document.visibilityState === 'visible')
    document.addEventListener('visibilitychange', on)
    return () => document.removeEventListener('visibilitychange', on)
  }, [])

  useEffect(() => {
    if (!ready || !HOLD) return
    if (HOLD.id.startsWith('port:')) setPort(HOLD.id.slice(5))
    else setActive(HOLD.id)
    setFocus(HOLD)
  }, [ready])
  // guided tour of the landmarks until the visitor takes over. Each stop is a card and a slow lean of the camera.
  useEffect(() => {
    if (!ready || !layers.landmarks || !visible || !tabVisible || narrow || STILL) return
    if (reduced) {
      setActive((a) => a ?? 'sigiriya')
      return
    }
    let i = active ? TOUR.indexOf(active) : -1
    const show = (id: string) => {
      setActive(id)
      setFocus({ id, kind: 'tour' })
    }
    const first = window.setTimeout(() => {
      if (focusRef.current) return
      show(active ?? TOUR[(i = 0)])
    }, 900)
    const id = window.setInterval(() => {
      const f = focusRef.current
      // never take the camera from a visitor who is pointing at something
      if (performance.now() - lastTouch.current < TOUR_IDLE_MS || (f && f.kind !== 'tour')) return
      i = (i + 1) % TOUR.length
      show(TOUR[i])
    }, 5200)
    return () => {
      window.clearTimeout(first)
      window.clearInterval(id)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, layers.landmarks, visible, tabVisible, reduced, narrow])

  const letGo = useCallback((only?: (f: Focus) => boolean) => setFocus((f) => (f && (!only || only(f)) ? null : f)), [])
  // hover: the card is immediate, the camera leans in behind it and goes home a beat after the pointer leaves
  const onActive = useCallback(
    (id: string | null, touch?: boolean) => {
      lastTouch.current = performance.now()
      if (touch) return
      window.clearTimeout(release.current)
      if (id) {
        setActive(id)
        setFocus((f) => (f?.id === id && f.kind === 'pin' ? f : { id, kind: 'hover' }))
      } else release.current = window.setTimeout(() => letGo((f) => f.kind === 'hover'), RELEASE_MS)
    },
    [letGo],
  )
  // click or tap: hold the camera there. A second one lets it go; on touch that also puts the card away.
  const onSelect = useCallback((id: string, touch?: boolean) => {
    lastTouch.current = performance.now()
    window.clearTimeout(release.current)
    const held = focusRef.current?.id === id && focusRef.current.kind === 'pin'
    setPort(null)
    setActive(held && touch ? null : id)
    setFocus(held ? null : { id, kind: 'pin' })
  }, [])
  const onPort = useCallback(
    (id: string | null, pin?: boolean) => {
      lastTouch.current = performance.now()
      window.clearTimeout(release.current)
      setPort(id)
      if (id) setFocus({ id: portKey(id), kind: pin ? 'pin' : 'hover' })
      else release.current = window.setTimeout(() => letGo((f) => f.id.startsWith('port:')), RELEASE_MS)
    },
    [letGo],
  )
  // a click or tap on open water: everything the visitor opened is put away
  const onMiss = useCallback(() => {
    lastTouch.current = performance.now()
    window.clearTimeout(release.current)
    setPort(null)
    const pinned = focusRef.current?.kind === 'pin'
    letGo((f) => f.kind !== 'tour')
    // touch has no hover to bring a card back, so a pinned card goes with the camera
    if (pinned && window.matchMedia('(pointer: coarse)').matches) setActive(null)
  }, [letGo])
  // a pin is not for ever: after a while, or when the hero leaves the screen, the camera goes home
  useEffect(() => {
    if (focus?.kind !== 'pin') return
    const away = () => {
      letGo((f) => f.kind === 'pin')
      setPort(null)
      if (window.matchMedia('(pointer: coarse)').matches) setActive(null)
    }
    if (!visible) return away()
    const id = window.setTimeout(away, PIN_MS)
    const key = (e: KeyboardEvent) => e.key === 'Escape' && away()
    window.addEventListener('keydown', key)
    return () => {
      window.clearTimeout(id)
      window.removeEventListener('keydown', key)
    }
  }, [focus, visible, letGo])
  useEffect(() => () => window.clearTimeout(release.current), [])

  // The part of the canvas a card may open into: clear of the nav, the headline and the layer bar.
  // Read each time a card opens, so it always matches the layout of the moment.
  const getSafe = useCallback((): SafeRect | null => {
    const sec = ref.current
    if (!sec) return null
    const s = sec.getBoundingClientRect()
    const c = copy.current?.getBoundingClientRect(), b = bar.current?.getBoundingClientRect()
    const m = 14
    const nav = 64
    const bottom = (b ? b.top - s.top : s.height) - m
    // beside the headline on wide layouts, under it when the copy is stacked above the island
    const beside = window.matchMedia('(min-width: 1024px)').matches
    let top = !beside && c ? c.bottom - s.top + 10 : nav + m
    // a short screen: better a card over the last row of the copy, for as long as it is open, than one that sits on the landmark it describes
    if (bottom - top < 236) top = Math.min(top, Math.max(nav + m, bottom - 236))
    return { left: beside && c ? c.right - s.left + 24 : m, top, right: s.width - m, bottom }
  }, [ref])

  // first frame is on the glass: hand over from the loader, but never so fast that it only flickers
  const onReady = useCallback(() => {
    window.setTimeout(() => setReady(true), Math.max(0, MIN_LOADER_MS - (performance.now() - mountedAt.current)))
  }, [])
  const onError = useCallback(() => setFailed(true), [])
  const onStage = useCallback((s: number) => setStage((cur) => Math.max(cur, s)), [])
  const renderCard = useCallback((lm: Landmark, at: CardPlacement) => <LandmarkCard lm={lm} at={at} />, [])
  const renderPortCard = useCallback((p: Port, at: CardPlacement) => <PortCard port={p} at={at} />, [])
  // keep the loader mounted through its exit, then drop it so nothing keeps spinning under the scene
  useEffect(() => {
    if (!ready) return
    const id = window.setTimeout(() => setLoaderGone(true), 800)
    return () => window.clearTimeout(id)
  }, [ready])

  const toggle = (k: keyof HeroLayers) => {
    if (k === 'maritime') {
      setPort(null)
      letGo((f) => f.id.startsWith('port:'))
    }
    if (k === 'landmarks') letGo((f) => !f.id.startsWith('port:'))
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
        {failed && <Poster theme={theme} />}
        {!failed && REVIEW !== 'loading' && (
          <div className={cn('absolute inset-0 transition-opacity duration-1000 ease-out', ready ? 'opacity-100' : 'opacity-0')}>
            <Boundary onError={onError}>
              <Suspense fallback={null}>
                <HeroScene
                  landmarks={landmarks}
                  mood={mood}
                  layers={layers}
                  active={port ? null : active}
                  focus={focus}
                  getSafe={getSafe}
                  onActive={onActive}
                  onSelect={onSelect}
                  renderCard={renderCard}
                  port={port}
                  onPort={onPort}
                  renderPortCard={renderPortCard}
                  onMiss={onMiss}
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

      {!BARE && (
        <>
          {/* legibility: the page colour under the nav, a soft wash behind the headline, and a fade into the page below */}
          <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 hidden h-[26%] bg-[linear-gradient(to_bottom,color-mix(in_oklab,var(--bg)_72%,transparent)_0%,transparent_100%)] lg:block" />
          <div aria-hidden className="pointer-events-none absolute inset-0 bg-[linear-gradient(to_bottom,color-mix(in_oklab,var(--bg)_82%,transparent)_0%,transparent_46%)] lg:bg-[linear-gradient(100deg,color-mix(in_oklab,var(--bg)_78%,transparent)_0%,color-mix(in_oklab,var(--bg)_38%,transparent)_30%,transparent_56%)]" />
          <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-gradient-to-b from-transparent to-[var(--bg)] sm:h-40" />

          {/* copy */}
          <div className="pointer-events-none relative z-10 mx-auto flex h-full max-w-[1320px] flex-col px-5 pt-[calc(var(--nav-h)+22px)] max-[389px]:px-4 sm:px-8 lg:justify-center lg:pt-0 lg:pb-16">
            <div ref={copy} className="max-w-[600px] [&>*]:[animation:fade-up_700ms_var(--ease-out)_both]">
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

              <div className="pointer-events-auto mt-4 flex flex-wrap gap-2.5 [animation-delay:280ms] sm:mt-8 max-sm:[&>a]:h-10 max-sm:[&>a]:px-3.5 max-sm:[&>a]:text-sm max-[389px]:gap-1.5 max-[389px]:[&>a]:gap-1 max-[389px]:[&>a]:px-2.5 max-[389px]:[&>a]:text-[13px] max-[389px]:[&>a]:tracking-[-0.01em]">
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
              {/* the caption answers for whatever is on the island: with the population layer up, it is the legend */}
              {layers.population && !failed ? (
                <div key="pop" className="[animation:overlay-in_320ms_var(--ease-out)]">
                  {/* a needle in the layer's own tone: identity is the mark beside the words, never the colour of the words */}
                  <span className="relative -top-[5px] mr-2 inline-flex flex-col items-center" style={{ color: SCENE_THEMES[theme].population.ramp[theme === 'dark' ? 2 : 1] }} aria-hidden>
                    <span className="size-[5px] rounded-full bg-current" />
                    <span className="h-[7px] w-px bg-current" />
                  </span>
                  Population of the largest places. Needle height and ring area follow residents.
                </div>
              ) : (
                <div key="base">Real coastline and rivers. Landmarks are enlarged; relief is stylised.</div>
              )}
            </div>
            <div ref={bar} className="glass pointer-events-auto flex max-w-full items-center gap-0.5 overflow-x-auto rounded-[13px] p-1" role="group" aria-label="Hero layers">
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
                aria-label={`Switch to ${mood === 'night' ? 'day' : 'night'}`}
              >
                {mood === 'night' ? <Moon className="size-3.5" /> : <Sun className="size-3.5" />}
                <span className="max-sm:sr-only">{mood === 'night' ? 'Night' : 'Day'}</span>
              </button>
            </div>
          </div>
        </>
      )}
    </section>
  )
}
