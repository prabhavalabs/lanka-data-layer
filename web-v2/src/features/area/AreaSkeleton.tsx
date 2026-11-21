export function AreaSkeleton() {
  return (
    <div aria-busy="true" aria-label="Loading area profile">
      <div className="grid grid-cols-1 gap-x-12 gap-y-8 lg:grid-cols-[minmax(0,1fr)_268px]">
        <div>
          <div className="skeleton h-3.5 w-52" />
          <div className="skeleton mt-8 h-3 w-28" />
          <div className="skeleton mt-3 h-12 w-[min(360px,80%)]" />
          <div className="skeleton mt-4 h-6 w-48" />
          <div className="mt-8 flex gap-8 border-t border-line pt-4">
            <div className="skeleton h-8 w-36" />
            <div className="skeleton h-8 w-20" />
            <div className="skeleton h-8 w-24" />
          </div>
          <div className="mt-7 flex gap-2">
            <div className="skeleton h-9 w-32 rounded-lg" />
            <div className="skeleton h-9 w-20 rounded-lg" />
            <div className="skeleton h-9 w-24 rounded-lg" />
          </div>
        </div>
        <div className="surface p-3 max-lg:hidden">
          <div className="skeleton h-[370px] w-full" />
        </div>
      </div>
      <div className="mt-12 grid grid-cols-2 gap-px overflow-hidden rounded-[14px] border border-line bg-line lg:grid-cols-4">
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="bg-bg2 p-5">
            <div className="skeleton h-3 w-20" />
            <div className="skeleton mt-4 h-8 w-32" />
            <div className="skeleton mt-6 h-3 w-24" />
          </div>
        ))}
      </div>
      <div className="mt-16 grid grid-cols-1 gap-px overflow-hidden rounded-[14px] border border-line bg-line sm:grid-cols-2 lg:grid-cols-3">
        {[0, 1, 2, 3, 4, 5].map((i) => (
          <div key={i} className="bg-bg2 p-5">
            <div className="skeleton h-3 w-28" />
            <div className="skeleton mt-3 h-6 w-20" />
            <div className="skeleton mt-5 h-10 w-full" />
          </div>
        ))}
      </div>
      <div className="surface mt-16 p-7">
        <div className="skeleton h-3 w-24" />
        <div className="skeleton mt-4 h-6 w-[min(520px,90%)]" />
        <div className="skeleton mt-8 h-72 w-full" />
      </div>
    </div>
  )
}
