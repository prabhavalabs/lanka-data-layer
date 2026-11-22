// The footer horizon: one continuous hairline that reads as a line graph and
// rises into eight Sri Lankan landmarks, set left → right in true north → south
// order. Every outline is hand-drawn in relative path commands, starting and
// ending on the baseline, so the whole skyline is a single stroke that can be
// drawn in one pass.
import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import { useInView } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const BASE = 186 // baseline y in the viewBox
const H = 232

type Part = {
  /** outline, relative commands, baseline → baseline */
  d: string
  /** extra strokes in local coordinates (origin = where the outline starts, y up is negative) */
  detail?: string
  /** quieter strokes: light beams, lamp trails, water */
  faint?: string
  dotted?: string
  label?: string
  lat?: string
  /** summit in local coordinates: gets a data point */
  apex?: [number, number]
  accent?: boolean
}

/* ---------- connectors: the graph wanders a little between sites ---------- */
const G1 = 'l10 -3l12 4l10 -2l10 1'
const G2 = 'l12 2l10 -4l14 3l8 -1'
const G3 = 'h10l10 -3l8 3h10'
const SEA = 'q9 -7 18 0t18 0t18 0t18 0t18 0t18 0'

/* ---------- Nine Arch Bridge: piers follow the valley under the deck ---------- */
const arches = (() => {
  const n = 9, x0 = 46, span = 162 / n, r = (span - 4) / 2, spring = -49 + r
  const valley = (x: number) => -52 * ((x - 127) / 88) ** 2
  let d = ''
  for (let i = 0; i < n; i++) {
    const xl = x0 + i * span + 2, xr = xl + r * 2
    const bl = Math.max(spring, Math.min(-1, valley(xl))), br = Math.max(spring, Math.min(-1, valley(xr)))
    d += `M${xl.toFixed(1)} ${bl.toFixed(1)}V${spring.toFixed(1)}a${r.toFixed(2)} ${r.toFixed(2)} 0 0 1 ${(r * 2).toFixed(2)} 0V${br.toFixed(1)}`
  }
  return d
})()

const PARTS: Part[] = [
  { d: 'h30' },
  // Palk Strait swell, with the shoals of Adam's Bridge trailing off to the west
  { d: SEA, faint: 'M4 9q9 -6 18 0t18 0t18 0t18 0t18 0M30 17q9 -5 18 0t18 0t18 0' },
  { d: G1 },
  {
    label: 'Nallur Kovil', lat: '9.6745°N', apex: [30, -111],
    // five diminishing tiers under a barrel-vaulted crown
    d: 'v-28h5v-15h4v-14h4v-13h4v-12h3v-10h-3v-4q13 -16 26 0v4h-3v10h3v12h4v13h4v14h4v15h5v28',
    detail: 'M30 -104v-7M24 -102v-5M36 -102v-5M24 0v-14q6 -8 12 0v14M27 -31v-8h6v8M27.500 -46v-7h5v7M28 -60v-6h4v6M28.500 -73v-5h3v5M20 -92h20',
  },
  { d: G2 },
  {
    label: 'Ruwanwelisaya', lat: '8.3500°N', apex: [64, -121],
    // terraces, bubble dome, square harmika, spire
    d: 'v-5h8v-5h5v-4h5v-4h4c-6 -30 12 -54 34 -56v-10h4l4 -36l4 36h4v10c22 2 40 26 34 56h4v4h5v4h5v5h8v5',
    detail: 'M56 -74h16M26 -27q38 6 76 0M29 -36q35 5 70 0M61.300 -96h5.400M62.200 -104h3.600M63 -112h2',
  },
  { d: G3 },
  {
    label: 'Sigiriya', lat: '7.9570°N', apex: [73, -86],
    // a sheer, flat-topped block of rock standing clear of the forest
    d: 'q14 -2 22 -14l3 -18c-3 -14 -4 -30 3 -44l6 -7l16 -3l18 2l14 -2l16 3l10 2l5 6c6 14 2 30 5 43l4 18q8 12 24 14',
    // rain streaks down the face, the gallery ledge, the palace footings on top
    detail: 'M45 -82v20M58 -84v31M72 -82v17M87 -83v27M100 -79v19M29 -41l52 -5M56 -86v-3h11v3M78 -85v-2h7',
    faint: 'M5 -2q3 -9 7 -1q3 -8 7 0M124 -8q3 -8 7 -1q4 -7 8 3M36 -22q5 -8 10 -1M98 -22q5 -8 10 -1',
  },
  { d: G1 },
  {
    label: 'Temple of the Tooth', lat: '7.2936°N', apex: [55, -93],
    // cloud wall, the octagonal Pattirippuwa under its two-pitch Kandyan roof, then the shrine
    d: 'v-9q4 -5 8 0q4 -5 8 0q4 -5 8 0q4 -5 8 0v-23h5v-18h-8l13 -8v-3l13 -22v-10v10l13 22v3l13 8h-8v18h5v8l9 -7h7l9 -16h30l9 16h7l9 7v24',
    detail: 'M37 -35h36M43 -50v15M49 -50v15M55 -50v15M61 -50v15M67 -50v15M40 -6v-11q3 -5 6 0v11M52 -6v-11q3 -5 6 0v11M64 -6v-11q3 -5 6 0v11M78 -24h80M112 -47l6 -8l6 8M94 -24v18M108 -24v18M128 -24v18M142 -24v18',
    faint: 'M-6 7q5 -4 10 0t10 0t10 0M40 12q5 -4 10 0t10 0t10 0t10 0',
  },
  { d: G2 },
  {
    label: 'Lotus Tower', lat: '6.9271°N', apex: [22, -154],
    d: 'v-7h8v-5h6l3 -84l-9 -7c-3 -14 4 -24 12 -29l2 -22l2 22c8 5 15 15 12 29l-9 7l3 84h6v5h8v7',
    detail: 'M17 -96h10M15 -105c1 -10 3 -18 7 -26M29 -105c-1 -10 -3 -18 -7 -26M8.500 -108h27M14 -12h16',
    faint: 'M22 -18v-70',
  },
  { d: G3 },
  {
    label: 'Nine Arch Bridge', lat: '6.8768°N', apex: [127, -58],
    // the line itself is the hillside and the deck; the arches hang beneath it
    d: 'c12 -2 18 -14 24 -34c4 -10 8 -20 18 -24h170c10 4 14 14 18 24c6 20 12 32 24 34',
    detail: `M44 -52h166${arches}M92 -58v-6h14v6M108 -58v-6h14v6M124 -58v-6h12l4 6`,
    faint: 'M58 -30Q127 22 196 -30',
  },
  { d: G1 },
  {
    label: 'Sri Pada', lat: '6.8096°N', apex: [103, -142], accent: true,
    d: 'l18 -12l10 2l17 -24l9 -3l14 -28l7 2l12 -34l5 -4l7 -30h2v-5h4v5h2l8 22l7 4l12 34l10 4l14 28l9 -2l12 22l15 8l12 11',
    faint: 'M45 -34l8 12M75 -63l7 16M115 -109l4 14M134 -71l-4 14M158 -39l-5 11',
    // the pilgrim path, lit through the season's nights
    dotted: 'M84 -3l12 -26l-7 -15l11 -24l-5 -14l7 -24l1 -24',
  },
  { d: G2 },
  {
    label: 'Galle Lighthouse', lat: '6.0246°N', apex: [33, -104],
    // rampart, tapering tower, gallery, lantern; a palm leans over the bastion
    d: 'l4 -16h18l3 -56h-4v-4h5v-12q1 -7 7 -9v-7v7q6 2 7 9v12h5v4h-4l3 56h40v6h14l4 10',
    detail: 'M28 -78v-9M33 -78v-10M38 -78v-9M30.500 -16v-8q2.500 -4 5 0v8M23.600 -44h18.800M64 -16q-3 -18 3 -34M67 -50q-8 -8 -16 -2M67 -50q8 -8 16 -2M67 -50q-4 -10 -12 -10M67 -50q6 -10 13 -8M67 -50q0 -8 3 -12',
    faint: 'M42 -85l52 -12M42 -82l52 6',
  },
  { d: 'h20' },
  { d: SEA, faint: 'M10 9q9 -6 18 0t18 0t18 0t18 0t18 0M40 17q9 -5 18 0t18 0t18 0' },
  { d: 'h30' },
]

/* ---------- measure the relative outlines: net advance + approximate ink length ---------- */
function measure(d: string) {
  let x = 0, y = 0, len = 0
  for (const m of d.matchAll(/([a-z])([^a-z]*)/g)) {
    const n = (m[2].match(/-?\d*\.?\d+/g) ?? []).map(Number)
    const seg = (dx: number, dy: number, k = 1) => {
      len += Math.hypot(dx, dy) * k
      x += dx
      y += dy
    }
    switch (m[1]) {
      case 'h': n.forEach((v) => seg(v, 0)); break
      case 'v': n.forEach((v) => seg(0, v)); break
      case 'l': for (let i = 0; i < n.length; i += 2) seg(n[i], n[i + 1]); break
      case 't': for (let i = 0; i < n.length; i += 2) seg(n[i], n[i + 1], 1.12); break
      case 'q': for (let i = 0; i < n.length; i += 4) seg(n[i + 2], n[i + 3], 1.12); break
      case 'c': for (let i = 0; i < n.length; i += 6) seg(n[i + 4], n[i + 5], 1.12); break
      case 'a': for (let i = 0; i < n.length; i += 7) seg(n[i + 5], n[i + 6], 1.3); break
    }
  }
  return { w: x, dy: y, len }
}

/* ---------- inverse of the draw easing, so details appear as the pen passes them ---------- */
const DRAW_MS = 3200
// cubic-bezier(0.77, 0, 0.175, 1) = --ease-in-out
const EASE = Array.from({ length: 61 }, (_, i) => {
  const t = i / 60, u = 1 - t
  return [3 * u * u * t * 0.77 + 3 * u * t * t * 0.175 + t * t * t, 3 * u * t * t + t * t * t] as const
})
function timeAtProgress(p: number) {
  // find the bezier parameter where y = p, return its x (elapsed time fraction)
  for (let i = 1; i < EASE.length; i++) {
    if (EASE[i][1] >= p) {
      const [x0, y0] = EASE[i - 1], [x1, y1] = EASE[i]
      return x0 + ((x1 - x0) * (p - y0)) / (y1 - y0 || 1)
    }
  }
  return 1
}

function build() {
  let x = 0, len = 0
  let line = `M0 ${BASE}`
  const placed: (Part & { x: number; w: number; at: number })[] = []
  for (const p of PARTS) {
    const m = measure(p.d)
    if (import.meta.env.DEV && Math.abs(m.dy) > 0.01) console.warn('[skyline] outline does not return to the baseline', p.label ?? p.d, m.dy)
    placed.push({ ...p, x, w: m.w, at: len + m.len * 0.5 })
    line += p.d
    x += m.w
    len += m.len
  }
  return { line, width: x, parts: placed.map((p) => ({ ...p, delay: Math.round(timeAtProgress(p.at / len) * DRAW_MS) })) }
}

export function FooterSkyline({ className }: { className?: string }) {
  const { line, width, parts } = useMemo(build, [])
  const [ref, inView] = useInView<HTMLDivElement>({ once: false, margin: '0px 0px -8% 0px' })
  // Draw once, and only after the horizon has stayed on screen for a beat: lazy routes
  // flash the footer into view before their content arrives and pushes it down.
  const [drawn, setDrawn] = useState(false)
  useEffect(() => {
    if (!inView || drawn) return
    const id = window.setTimeout(() => setDrawn(true), 320)
    return () => window.clearTimeout(id)
  }, [inView, drawn])
  const split = `${((BASE / H) * 100).toFixed(2)}%`

  return (
    <div
      ref={ref}
      data-in={drawn}
      aria-hidden
      className={cn('skyline pointer-events-none relative overflow-hidden select-none', className)}
      // sky above the horizon, the footer's ground tint below it
      style={{ background: `linear-gradient(to bottom, transparent ${split}, color-mix(in oklab, var(--bg2) 40%, transparent) ${split})` }}
    >
      <svg
        viewBox={`0 0 ${width} ${H}`}
        fill="none"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="relative left-1/2 block w-[max(100%,1060px)] max-w-none -translate-x-[65.4%] [mask-image:linear-gradient(to_right,transparent,black_7%,black_93%,transparent)] sm:-translate-x-1/2"
      >
        {/* graph furniture: level lines and a tick under each site */}
        <g className="skyline-grid" stroke="currentColor">
          {[50, 100, 150].map((h) => (
            <path key={h} d={`M0 ${BASE - h}H${width}`} strokeDasharray="1 7" />
          ))}
          {parts.map((p) => p.label && <path key={p.label} d={`M${p.x + p.w / 2} ${BASE + 4}v5`} />)}
        </g>

        <path className="skyline-line" d={line} pathLength={1} stroke="currentColor" />

        {parts.map((p, i) =>
          p.detail || p.faint || p.dotted || p.label ? (
            <g key={i} className="skyline-part" style={{ '--d': `${p.delay}ms` } as CSSProperties}>
              <g transform={`translate(${p.x} ${BASE})`} stroke="currentColor">
                {p.detail && <path className="skyline-detail" d={p.detail} />}
                {p.faint && <path className="skyline-faint" d={p.faint} />}
                {p.dotted && <path className="skyline-faint" d={p.dotted} strokeDasharray="0.1 5" />}
                {p.apex &&
                  (p.accent ? (
                    <>
                      <circle cx={p.apex[0]} cy={p.apex[1]} r={7} className="skyline-halo" />
                      <circle cx={p.apex[0]} cy={p.apex[1]} r={2.6} className="skyline-point-accent" />
                    </>
                  ) : (
                    <circle cx={p.apex[0]} cy={p.apex[1]} r={2.1} className="skyline-point" />
                  ))}
              </g>
              {p.label && (
                <g className="skyline-label max-sm:hidden" transform={`translate(${p.x + p.w / 2} ${BASE})`}>
                  <text y={24} textAnchor="middle">{p.label}</text>
                  <text y={38} textAnchor="middle" className="skyline-lat">{p.lat}</text>
                </g>
              )}
            </g>
          ) : null,
        )}
      </svg>
    </div>
  )
}
