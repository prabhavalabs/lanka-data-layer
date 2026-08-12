import { Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/layout/app-shell";
import { DatasetsPage } from "@/pages/datasets-page";
import { ElectionsPage } from "@/pages/elections-page";
import { HomePage } from "@/pages/home-page";
import { MapPage } from "@/pages/map-page";
import { DocsDemoPage } from "@/pages/docs/docs-demo-page";
import { DocsEndpointPage } from "@/pages/docs/docs-endpoint-page";

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<HomePage />} />
        {/* Docs home now lives at "/" — the old /docs index just redirects there. */}
        <Route path="docs" element={<Navigate to="/" replace />} />
        <Route path="docs/demo" element={<DocsDemoPage />} />
        <Route path="docs/:slug" element={<DocsEndpointPage />} />
        <Route path="map" element={<MapPage />} />
        <Route path="datasets" element={<DatasetsPage />} />
        <Route path="elections" element={<ElectionsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
