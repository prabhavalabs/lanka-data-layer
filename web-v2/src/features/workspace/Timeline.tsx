import { Pause, Play, SkipForward } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Slider } from '@/components/ui/slider'
import { allLayers, isTimeBound, type TimeConfig } from '@/lib/map-spec'
import { cn } from '@/lib/utils'
import { actions, useWs } from './store'

/**
 * The document's time cursor. Shown only while some layer is bound to it. Stepping changes a paint
 * expression (frames carry every period); it does not refetch.
 */
export function Timeline({ reducedMotion, className, style }: { reducedMotion: boolean; className?: string; style?: React.CSSProperties }) {
  const time = useWs((s) => s.doc!.time)
  const bound = useWs((s) => (s.doc ? allLayers(s.doc.layers).filter((l) => l.visible && isTimeBound(s.doc!.sources[l.source])).length : 0))
  const [playing, setPlaying] = useState(false)
  const ref = useRef<TimeConfig | null>(time)
  ref.current = time
  useEffect(() => {
    if (!playing || !time) return
    const id = window.setInterval(() => {
      const t = ref.current
      if (!t) return
      const cur = Number(t.cursor), from = Number(t.range[0]), to = Number(t.range[1])
      if (cur >= to && !t.playback.loop) return setPlaying(false)
      actions.setCursor(String(cur >= to ? from : cur + 1))
    }, 1000 / (time.playback.fps || 1))
    return () => window.clearInterval(id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playing, time?.playback.fps])
  useEffect(() => {
    if (!bound) setPlaying(false)
  }, [bound])
  if (!time || !bound) return null
  const from = Number(time.range[0]), to = Number(time.range[1]), cur = Number(time.cursor)
  const step = () => actions.setCursor(String(cur >= to ? from : cur + 1))
  return (
    <div className={cn('glass flex h-12 w-[min(420px,calc(100vw-24px))] items-center gap-3 rounded-2xl pr-4 pl-1.5', className)} style={style} role="group" aria-label="Timeline">
      <button
        type="button"
        // reduced motion: step once instead of animating the colours
        onClick={() => (reducedMotion ? step() : setPlaying((p) => !p))}
        aria-label={reducedMotion ? 'Next year' : playing ? 'Pause' : 'Play through the years'}
        aria-pressed={reducedMotion ? undefined : playing}
        className="pressable grid size-9 shrink-0 place-items-center rounded-xl text-ink outline-none max-md:size-10 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-[color-mix(in_oklab,var(--ink)_9%,transparent)]"
      >
        {reducedMotion ? <SkipForward className="size-4" /> : playing ? <Pause className="size-4" /> : <Play className="size-4 translate-x-px" />}
      </button>
      <span className="t-num w-[46px] shrink-0 font-mono text-[19px] leading-none font-medium tracking-[-0.04em] text-ink" aria-live="off">
        {time.cursor}
      </span>
      <div className="min-w-0 flex-1">
        <Slider
          className="max-md:h-8"
          min={from}
          max={to}
          step={1}
          value={[cur]}
          aria-label="Year"
          onValueChange={([y]) => {
            setPlaying(false)
            actions.setCursor(String(y))
          }}
        />
      </div>
      <span className="t-label t-num shrink-0">
        {from}–{to}
      </span>
    </div>
  )
}
