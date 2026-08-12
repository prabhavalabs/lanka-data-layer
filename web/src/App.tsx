import { Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/layout/app-shell";
import { DatasetsPage } from "@/pages/datasets-page";
import { ElectionsPage } from "@/pages/elections-page";
import { MapPage } from "@/pages/map-page";
import { DocsDemoPage } from "@/pages/docs/docs-demo-page";
import { DocsEndpointPage } from "@/pages/docs/docs-endpoint-page";
import { DocsIndexPage } from "@/pages/docs/docs-index-page";
import { DocsLayout } from "@/pages/docs/docs-layout";

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Navigate to="/map" replace />} />
        <Route path="map" element={<MapPage />} />
        <Route path="datasets" element={<DatasetsPage />} />
        <Route path="elections" element={<ElectionsPage />} />
        <Route path="docs" element={<DocsLayout />}>
          <Route index element={<DocsIndexPage />} />
          <Route path="demo" element={<DocsDemoPage />} />
          <Route path=":slug" element={<DocsEndpointPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/map" replace />} />
      </Route>
    </Routes>
  );
}
