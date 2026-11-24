import { lazy, Suspense, useEffect, useLayoutEffect } from 'react'
import { Navigate, Outlet, Route, Routes, useLocation } from 'react-router'
import { CommandPaletteProvider } from '@/components/shell/CommandPalette'
import { Footer } from '@/components/shell/Footer'
import { TopNav } from '@/components/shell/TopNav'
import { TooltipProvider } from '@/components/ui/tooltip'
import { applyRouteDefaultTheme } from '@/lib/hooks'
import Landing from '@/pages/Landing'

const Explore = lazy(() => import('@/pages/Explore'))
const AreaProfile = lazy(() => import('@/pages/AreaProfile'))
const Catalogue = lazy(() => import('@/pages/Catalogue'))
const DatasetPage = lazy(() => import('@/pages/Dataset'))
const Government = lazy(() => import('@/pages/Government'))
const Developers = lazy(() => import('@/pages/Developers'))
const SharedMap = lazy(() => import('@/pages/SharedMap'))
const Terms = lazy(() => import('@/pages/Terms'))
const EmbedMap = lazy(() => import('@/pages/SharedMap').then((m) => ({ default: m.EmbedMap })))

function PageFallback() {
  return (
    <div className="mx-auto max-w-[1240px] px-5 pt-32 sm:px-8">
      <div className="skeleton h-10 w-72" />
      <div className="skeleton mt-4 h-5 w-[min(520px,90%)]" />
      <div className="skeleton mt-10 h-72 w-full" />
    </div>
  )
}

function Shell() {
  const { pathname, hash } = useLocation()
  const immersive = pathname.startsWith('/explore') || pathname.startsWith('/m/')
  useLayoutEffect(() => applyRouteDefaultTheme(pathname), [pathname])
  useEffect(() => {
    if (hash) document.getElementById(hash.slice(1))?.scrollIntoView()
    else window.scrollTo(0, 0)
  }, [pathname, hash])
  return (
    <>
      <a href="#main" className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-[100] focus:rounded-lg focus:bg-ink focus:px-3 focus:py-2 focus:text-bg">
        Skip to content
      </a>
      <TopNav overlay={pathname === '/' || immersive} />
      <main id="main">
        <Suspense fallback={<PageFallback />}>
          <Outlet />
        </Suspense>
      </main>
      {!immersive && <Footer />}
    </>
  )
}

export function App() {
  return (
    <TooltipProvider>
      <CommandPaletteProvider>
        <Routes>
          <Route
            path="embed/m/:id"
            element={
              <Suspense fallback={null}>
                <EmbedMap />
              </Suspense>
            }
          />
          <Route element={<Shell />}>
            <Route index element={<Landing />} />
            <Route path="explore" element={<Explore />} />
            <Route path="m/:id" element={<SharedMap />} />
            <Route path="area/:pcode" element={<AreaProfile />} />
            <Route path="data" element={<Catalogue />} />
            <Route path="data/:id" element={<DatasetPage />} />
            <Route path="government" element={<Government />} />
            <Route path="developers" element={<Developers />} />
            <Route path="terms" element={<Terms />} />
            <Route path="takedown" element={<Navigate to="/terms#takedown" replace />} />
            <Route path="*" element={<Landing />} />
          </Route>
        </Routes>
      </CommandPaletteProvider>
    </TooltipProvider>
  )
}
