import { useQuery } from '@tanstack/react-query'
import { geoMercator, geoPath } from 'd3-geo'
import { useMemo } from 'react'
import { geo, type AdminCollection } from '@/lib/geo'
import { cn } from '@/lib/utils'

/** Static SVG locator: the island with one unit (or one province's districts) picked out. No map library. */
export function LocatorMap({
  pcode,
  width = 220,
  height = 330,
  className,
  graticule = true,
  onPick,
}: {
  pcode?: string
  width?: number
  height?: number
  className?: string
  graticule?: boolean
  onPick?: (pcode: string) => void
}) {
  const { data } = useQuery({ queryKey: ['geo', 'adm2'], queryFn: geo.adm2, staleTime: Infinity })
  const drawn = useMemo(() => {
    if (!data) return null
    const projection = geoMercator().fitExtent(
      [
        [10, 10],
        [width - 10, height - 10],
      ],
      data as AdminCollection,
    )
    const path = geoPath(projection)
    const ticks: { x?: number; y?: number; label: string }[] = []
    for (const lon of [80, 81]) ticks.push({ x: projection([lon, 7])![0], label: `${lon}°E` })
    for (const lat of [6, 7, 8, 9]) ticks.push({ y: projection([80, lat])![1], label: `${lat}°N` })
    return { shapes: data.features.map((f) => ({ pcode: f.properties.pcode, name: f.properties.name_en, d: path(f) ?? '' })), ticks }
  }, [data, width, height])

  const isHit = (p: string) => !!pcode && pcode !== 'LK' && (p === pcode || p.startsWith(pcode) || pcode.startsWith(p))
  return (
    <svg viewBox={`0 0 ${width} ${height}`} className={cn('h-auto w-full', className)} role="img" aria-label={pcode ? `Locator map highlighting ${pcode}` : 'Map of Sri Lanka'}>
      {graticule &&
        drawn?.ticks.map((t) =>
          t.x !== undefined ? (
            <g key={t.label}>
              <line x1={t.x} x2={t.x} y1={0} y2={height} stroke="var(--grid-line)" />
              <text x={t.x + 3} y={height - 3} fontSize={7.5} fill="var(--ink3)" fontFamily="var(--font-mono)">
                {t.label}
              </text>
            </g>
          ) : (
            <g key={t.label}>
              <line x1={0} x2={width} y1={t.y} y2={t.y} stroke="var(--grid-line)" />
              <text x={3} y={t.y! - 3} fontSize={7.5} fill="var(--ink3)" fontFamily="var(--font-mono)">
                {t.label}
              </text>
            </g>
          ),
        )}
      {drawn?.shapes.map((s) => (
        <path
          key={s.pcode}
          d={s.d}
          fill={isHit(s.pcode) ? 'var(--brand)' : 'var(--bg4)'}
          stroke={isHit(s.pcode) ? 'var(--brand)' : 'var(--bg2)'}
          strokeWidth={0.8}
          strokeLinejoin="round"
          className={onPick ? 'cursor-pointer transition-[fill] duration-150 hover:fill-[var(--ink3)]' : undefined}
          onClick={onPick ? () => onPick(s.pcode) : undefined}
        >
          <title>{s.name}</title>
        </path>
      ))}
      {!drawn && <rect x={width * 0.3} y={height * 0.1} width={width * 0.4} height={height * 0.8} rx={30} fill="var(--bg3)" />}
    </svg>
  )
}
