import { Folder, FolderOpen } from 'lucide-react'
import type { Theme } from '@/lib/hooks'
import { dashKind, isGroup, type FieldBinding, type LayerNode } from '@/lib/map-spec'
import { bivariateColors, categorical, paperColor, rampClasses, toneColor } from '@/lib/palettes'
import { cn } from '@/lib/utils'
import { isBinding, isBivariate, layerScheme } from './engine/compile'

/** Colours a binding will draw with, known from the style alone (no data needed, so hidden layers have a thumb too). */
function swatches(b: FieldBinding, scheme: Theme, n = 4): string[] {
  const s = b.scale
  if (s.type === 'categorical') return s.mapping ? Object.values(s.mapping).filter((v): v is string => typeof v === 'string' && !/^[\d.]+$/.test(v)).map((v) => toneColor(v, scheme)).slice(0, n) : categorical(scheme).slice(0, n)
  return rampClasses(s.type === 'diverging' ? 'ldl.div.blue-orange' : (s.ramp ?? 'ldl.seq.blue'), scheme, n, s.reverse)
}

/** A 20 px picture of what the layer draws: its type and its colours at a glance. */
export function LayerThumb({ node, scheme: mapScheme, className }: { node: LayerNode; scheme: Theme; className?: string }) {
  const scheme = isGroup(node) ? mapScheme : layerScheme(node.blend, mapScheme)
  const frame = cn('relative grid size-5 shrink-0 place-items-center overflow-hidden rounded-[5px] ring-1 ring-[color-mix(in_oklab,var(--ink)_14%,transparent)] ring-inset', className)
  if (isGroup(node)) {
    const Icon = node.collapsed ? Folder : FolderOpen
    return (
      <span className={cn(frame, 'text-ink2 ring-0')} aria-hidden>
        <Icon className="size-4" strokeWidth={1.6} />
      </span>
    )
  }
  const st = node.style
  const paper = paperColor(scheme)
  const ground = scheme === 'dark' ? '#131922' : '#f3f4f3'
  const fill = st.fill
  const strokeC = typeof st.stroke?.color === 'string' ? toneColor(st.stroke.color, scheme) : isBinding(st.stroke?.color) ? swatches(st.stroke.color, scheme)[1] : toneColor('ink', scheme)
  let body: React.ReactNode
  switch (node.type) {
    case 'choropleth':
    case 'raster': {
      const cs = isBinding(fill) ? swatches(fill, scheme) : rampClasses('ldl.seq.blue', scheme, 4)
      body = (
        <span className="flex size-full">
          {cs.map((c, i) => (
            <span key={i} className="flex-1" style={{ background: c }} />
          ))}
        </span>
      )
      break
    }
    case 'bivariate': {
      const cs = bivariateColors(isBivariate(fill) ? fill.bivariate.palette : 'ldl.biv.blue-orange', scheme)
      body = (
        <span className="grid size-full grid-cols-3 grid-rows-3">
          {[6, 7, 8, 3, 4, 5, 0, 1, 2].map((i) => (
            <span key={i} style={{ background: cs[i] }} />
          ))}
        </span>
      )
      break
    }
    case 'hillshade':
      body = <span className="size-full" style={{ background: `linear-gradient(135deg, ${scheme === 'dark' ? '#55606c' : '#ffffff'} 0 46%, ${scheme === 'dark' ? '#0a0d11' : '#8c98a4'} 54% 100%)` }} />
      break
    case 'heatmap': {
      const cs = rampClasses(st.ramp ?? 'ldl.seq.orange', scheme, 3, st.reverse)
      body = <span className="size-full" style={{ background: `radial-gradient(circle at 42% 46%, ${cs[2]} 0 18%, ${cs[1]} 42%, ${cs[0]}55 66%, ${ground} 86%)` }} />
      break
    }
    case 'polygon': {
      const f = fill === 'none' || fill === undefined ? 'transparent' : isBinding(fill) ? swatches(fill, scheme)[0] : toneColor(fill as string, scheme)
      body = (
        <span className="grid size-full place-items-center" style={{ background: ground }}>
          <span className="size-3 rounded-[3px]" style={{ background: f, boxShadow: `inset 0 0 0 1.5px ${strokeC}`, opacity: f === 'transparent' ? 1 : 0.9 }} />
        </span>
      )
      break
    }
    case 'line':
      body = (
        <svg viewBox="0 0 20 20" className="size-full" style={{ background: ground }}>
          <path d="M2 14c4-9 6 3 9-3s4-4 7-5" fill="none" stroke={strokeC} strokeWidth="1.8" strokeLinecap="round" strokeDasharray={dashKind(st.stroke?.dash) === 'dash' ? '4 2.4' : dashKind(st.stroke?.dash) === 'dot' ? '0.5 3' : undefined} />
        </svg>
      )
      break
    case 'point':
    case 'cluster': {
      const cs = isBinding(fill) ? swatches(fill, scheme, 3) : Array(3).fill(toneColor(typeof fill === 'string' ? fill : 'ink', scheme))
      body = (
        <svg viewBox="0 0 20 20" className="size-full" style={{ background: ground }}>
          {node.type === 'cluster' ? (
            <circle cx="10" cy="10" r="5.5" fill={cs[0]} stroke={paper} strokeWidth="1" />
          ) : (
            <>
              <circle cx="6.5" cy="12.5" r="3" fill={cs[cs.length - 1]} stroke={paper} strokeWidth="0.8" />
              <circle cx="13" cy="7" r="2.4" fill={cs[Math.min(1, cs.length - 1)]} stroke={paper} strokeWidth="0.8" />
              <circle cx="14" cy="14.2" r="1.6" fill={cs[0]} stroke={paper} strokeWidth="0.8" />
            </>
          )}
        </svg>
      )
      break
    }
    default:
      body = (
        <span className="grid size-full place-items-center text-[10px] leading-none font-semibold tracking-[-0.02em] text-ink" style={{ background: ground }}>
          Aa
        </span>
      )
  }
  return (
    <span className={frame} aria-hidden>
      {body}
    </span>
  )
}
