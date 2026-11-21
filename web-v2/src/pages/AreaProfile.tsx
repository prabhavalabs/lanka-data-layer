import { useQuery } from '@tanstack/react-query'
import { Link, useParams } from 'react-router'
import { areaProfileQuery } from '@/api/queries'
import { Button } from '@/components/ui/button'
import { PostalAndNeighbours, SourcesAndLimits } from '@/features/area/Appendix'
import { AreaHeader } from '@/features/area/AreaHeader'
import { AreaSkeleton } from '@/features/area/AreaSkeleton'
import { ChildrenTable } from '@/features/area/ChildrenTable'
import { HeadlineStrip } from '@/features/area/HeadlineStrip'
import { RankSection } from '@/features/area/RankSection'
import { SectionNav } from '@/features/area/SectionNav'
import { AccessSection, ClimateSection, ElectionsSection, LandSection, PeopleSection } from '@/features/area/sections'

export default function AreaProfile() {
  const { pcode = 'LK' } = useParams()
  const { data, isPending, isError, refetch } = useQuery(areaProfileQuery(pcode.toUpperCase()))

  return (
    <div className="mx-auto max-w-[1240px] px-5 pt-28 pb-24 sm:px-8">
      {isPending ? (
        <AreaSkeleton />
      ) : isError || !data ? (
        <div className="surface mx-auto mt-10 max-w-md p-8 text-center">
          <h1 className="t-h3">This area could not be loaded</h1>
          <p className="mt-2 text-sm text-ink2">
            No profile came back for <span className="font-mono text-ink">{pcode}</span>.
          </p>
          <div className="mt-5 flex justify-center gap-2">
            <Button variant="outline" onClick={() => void refetch()}>
              Try again
            </Button>
            <Button asChild>
              <Link to="/area/LK">Sri Lanka</Link>
            </Button>
          </div>
        </div>
      ) : (
        <article key={data.area.pcode}>
          <AreaHeader profile={data} />
          <div className="mt-12">
            <HeadlineStrip profile={data} />
          </div>
          <div className="mt-16">
            <RankSection profile={data} />
          </div>
          <div className="mt-16 grid gap-x-10 lg:grid-cols-[132px_minmax(0,1fr)]">
            <SectionNav />
            <div className="flex min-w-0 flex-col gap-8">
              <PeopleSection profile={data} />
              <AccessSection profile={data} />
              <LandSection profile={data} />
              <ClimateSection profile={data} />
              <ElectionsSection profile={data} />
              <div className="mt-8">
                <ChildrenTable profile={data} />
              </div>
            </div>
          </div>
          <div className="mt-20">
            <PostalAndNeighbours profile={data} />
          </div>
          <div className="mt-16">
            <SourcesAndLimits profile={data} />
          </div>
        </article>
      )}
    </div>
  )
}
