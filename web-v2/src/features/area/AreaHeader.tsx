import { ChevronRight, Download, GitCompareArrows, Map as MapIcon } from 'lucide-react'
import { Fragment } from 'react'
import { Link, useNavigate } from 'react-router'
import type { AreaProfile } from '@/api/types'
import { LocatorMap } from '@/components/charts/LocatorMap'
import { PcodePlate } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { fmtCoord, fmtInt } from '@/lib/format'

export const LEVEL = ['Country', 'Province', 'District', 'DS division', 'GN division'] as const
export const CHILD_LEVEL = ['Provinces', 'Districts', 'DS divisions', 'GN divisions', 'Sub-units'] as const

function download(name: string, mime: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: mime }))
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.click()
  window.setTimeout(() => URL.revokeObjectURL(url), 500)
}

export function AreaHeader({ profile }: { profile: AreaProfile }) {
  const { area, breadcrumb, meta, children } = profile
  const navigate = useNavigate()
  // [lon, lat]; tolerate antipodal centroids from rings wound the non-d3 way
  const [lon, lat] = area.centroid[1] < 0 ? [area.centroid[0] + 180, -area.centroid[1]] : area.centroid
  const csv = () =>
    download(
      `${area.pcode}-profile-${meta.data_version}.csv`,
      'text/csv',
      ['pcode,name,population_2024,density_per_km2,change_2012_2024_pct,hospital_min', ...children.map((c) => [c.pcode, `"${c.name.en}"`, c.population, c.density.toFixed(1), c.change.toFixed(2), c.hospitalMin.toFixed(1)].join(','))].join('\n'),
    )
  const geojson = () =>
    download(
      `${area.pcode}.geojson`,
      'application/geo+json',
      JSON.stringify({ type: 'FeatureCollection', features: [{ type: 'Feature', properties: { pcode: area.pcode, name_en: area.name.en, data_version: meta.data_version }, geometry: { type: 'Point', coordinates: [+lon.toFixed(6), +lat.toFixed(6)] } }] }, null, 2),
    )

  return (
    <header className="grid grid-cols-1 gap-x-12 gap-y-8 lg:grid-cols-[minmax(0,1fr)_268px]">
      <div className="min-w-0">
        <nav aria-label="Breadcrumb" className="flex flex-wrap items-center gap-1 font-mono text-[12px] text-ink3">
          <Link to="/explore" className="rounded-sm hover:text-ink">
            Areas
          </Link>
          {breadcrumb.map((b, i) => (
            <Fragment key={b.pcode}>
              <ChevronRight className="size-3 opacity-60" aria-hidden />
              {i === breadcrumb.length - 1 ? (
                <span aria-current="page" className="text-ink2">
                  {b.name.en}
                </span>
              ) : (
                <Link to={`/area/${b.pcode}`} className="rounded-sm hover:text-ink">
                  {b.name.en}
                </Link>
              )}
            </Fragment>
          ))}
        </nav>

        <div className="mt-7 flex items-center gap-3 font-mono text-[11.5px] tracking-[0.06em] text-ink3 uppercase">
          <span>{LEVEL[area.level]}</span>
          <span className="h-px w-8 bg-line2" />
          <span>ADM{area.level}</span>
        </div>
        <div className="mt-2.5 flex flex-wrap items-center gap-x-5 gap-y-3">
          <h1 className="t-h1">{area.name.en}</h1>
          <PcodePlate code={area.pcode} size="lg" copyable />
        </div>
        <p className="mt-3 flex flex-wrap items-baseline gap-x-3 gap-y-1 text-[1.2rem] text-ink2">
          <span lang="si">{area.name.si}</span>
          <span aria-hidden className="text-line2">
            /
          </span>
          <span lang="ta">{area.name.ta}</span>
        </p>

        <dl className="mt-7 flex flex-wrap gap-x-8 gap-y-3 border-t border-line pt-4 font-mono text-[12.5px]">
          {(
            [
              ['centroid', fmtCoord(lat, lon)],
              ['area', `${fmtInt(area.areaKm2 || profile.headline.find((h) => h.id === 'area.land')?.value || 0)} km²`],
              ['data_version', meta.data_version],
            ] as const
          ).map(([k, v]) => (
            <div key={k}>
              <dt className="text-[10.5px] tracking-[0.04em] text-ink3">{k}</dt>
              <dd className="t-num mt-0.5 text-ink">{v}</dd>
            </div>
          ))}
        </dl>

        <div className="mt-7 flex flex-wrap items-center gap-2">
          <Button asChild>
            <Link to={`/explore?area=${area.pcode}`}>
              <MapIcon />
              Open in map
            </Link>
          </Button>
          <Button variant="outline" onClick={csv}>
            <Download />
            CSV
          </Button>
          <Button variant="outline" onClick={geojson}>
            <Download />
            GeoJSON
          </Button>
          <Button variant="ghost" asChild>
            <Link to={`/explore?area=${area.pcode}&compare=1`}>
              <GitCompareArrows />
              Compare
            </Link>
          </Button>
        </div>
      </div>

      <figure className="surface ticks self-start p-3 shadow-[var(--shadow-sm)] max-lg:mx-auto max-lg:w-full max-lg:max-w-[268px]">
        <figcaption className="flex items-center justify-between px-1 pb-2 font-mono text-[10.5px] text-ink3">
          <span>LOCATOR</span>
          <span>click a district</span>
        </figcaption>
        <div className="hairline-grid overflow-hidden rounded-md border border-line bg-bg">
          <LocatorMap pcode={area.pcode} onPick={(p) => navigate(`/area/${p}`)} graticule />
        </div>
      </figure>
    </header>
  )
}
