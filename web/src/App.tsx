import * as React from "react";
import { Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/layout/app-shell";
import { DatasetsPage } from "@/pages/datasets-page";
import { ElectionsPage } from "@/pages/elections-page";
import { HomePage } from "@/pages/home-page";
import { MapPage } from "@/pages/map-page";
import { DocsDemoPage } from "@/pages/docs/docs-demo-page";
import { DocsEndpointPage } from "@/pages/docs/docs-endpoint-page";

// Lazy at the route level: the four npm packages showcased on this page
// (@lanka-data-layer/{admin,postal,electoral,census}) inline ~2.4MB of
// data between them. Loading the page component via dynamic import — with
// every package imported only from inside this module or its children —
// keeps that data out of the main bundle entirely; it only downloads when
// someone actually visits /docs/packages.
const DocsPackagesPage = React.lazy(() => import("@/pages/docs/docs-packages-page"));

function DocsPackagesFallback() {
  return (
    <div className="flex h-full items-center justify-center">
      <span className="size-5 animate-spin rounded-full border-2 border-line2 border-t-brand" aria-hidden="true" />
    </div>
  );
}

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<HomePage />} />
        {/* Docs home now lives at "/" — the old /docs index just redirects there. */}
        <Route path="docs" element={<Navigate to="/" replace />} />
        <Route path="docs/demo" element={<DocsDemoPage />} />
        <Route
          path="docs/packages"
          element={
            <React.Suspense fallback={<DocsPackagesFallback />}>
              <DocsPackagesPage />
            </React.Suspense>
          }
        />
        <Route path="docs/:slug" element={<DocsEndpointPage />} />
        <Route path="map" element={<MapPage />} />
        <Route path="datasets" element={<DatasetsPage />} />
        <Route path="elections" element={<ElectionsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
