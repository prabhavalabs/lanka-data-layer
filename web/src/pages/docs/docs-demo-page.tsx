import * as React from "react";
import type { AdminUnit } from "@lanka-data-layer/shared";

import { Separator } from "@/components/ui/separator";
import { ApiError, apiGetPayload } from "@/lib/api";
import { DemoMap, type DemoMapPoint } from "@/pages/docs/components/demo-map";
import { JsonViewer } from "@/pages/docs/components/json-viewer";
import { PostalTypeahead, type LookupSuggestion } from "@/pages/docs/components/postal-typeahead";

/** GET /v1/postal/:code's payload shape (api/README.md's "/v1/postal/:code response shape"). */
interface PostalRecord {
  code: string;
  name: string;
  lat: number | null;
  lon: number | null;
  gnd: AdminUnit | null;
  ds_division: AdminUnit | null;
  district: AdminUnit | null;
  province: AdminUnit | null;
}

type DemoState =
  | { status: "empty" }
  | { status: "loading"; code: string }
  | { status: "error"; code: string; message: string }
  | { status: "success"; code: string; record: PostalRecord; geometry: GeoJSON.Feature | null };

/**
 * GET /v1/admin/:pcode/geometry is deliberately NOT the {success,message,
 * payload,meta} envelope (see docs-spec.ts's admin-geometry entry) — it's a
 * bare GeoJSON Feature, so this fetches it directly rather than through
 * apiGet/apiGetPayload (which unwrap an envelope that doesn't exist here).
 * A missing geometry (404) degrades to "no boundary to draw", not an error
 * — plenty of pcodes don't have an admin_geometry row.
 */
async function fetchAdminGeometry(pcode: string, signal: AbortSignal): Promise<GeoJSON.Feature | null> {
  const res = await fetch(`/v1/admin/${encodeURIComponent(pcode)}/geometry`, { signal });
  if (!res.ok) return null;
  return (await res.json()) as GeoJSON.Feature;
}

export function DocsDemoPage() {
  const [state, setState] = React.useState<DemoState>({ status: "empty" });

  const handleSelect = React.useCallback((row: LookupSuggestion) => {
    const code = String(row.id);
    const controller = new AbortController();
    setState({ status: "loading", code });

    (async () => {
      const record = await apiGetPayload<PostalRecord>(`/postal/${encodeURIComponent(code)}`, { signal: controller.signal });
      const pcode = record.gnd?.pcode;
      const geometry = pcode ? await fetchAdminGeometry(pcode, controller.signal) : null;
      setState({ status: "success", code, record, geometry });
    })().catch((err: unknown) => {
      if (err instanceof DOMException && err.name === "AbortError") return;
      setState({ status: "error", code, message: err instanceof ApiError ? err.message : String(err) });
    });

    return () => controller.abort();
  }, []);

  const point: DemoMapPoint | null =
    state.status === "success" && state.record.lat != null && state.record.lon != null
      ? { lat: state.record.lat, lon: state.record.lon, label: `${state.record.code} · ${state.record.name}` }
      : null;
  const geometry = state.status === "success" ? state.geometry : null;

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-6">
      <div>
        <h1 className="text-xl font-semibold">Postal lookup demo</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Search a postal code or area name below. Selecting a result calls{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">GET /v1/postal/:code</code>, flies the map
          to its coordinates, and — when the record resolves to an admin GN division —{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">GET /v1/admin/:pcode/geometry</code> to draw
          its boundary. One query, answered and visualized in one place.
        </p>
      </div>

      <PostalTypeahead onSelect={handleSelect} />

      <Separator />

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="flex flex-col gap-2">
          <h2 className="text-sm font-semibold">Map</h2>
          <DemoMap point={point} geometry={geometry} />
          {state.status === "success" && (
            <p className="text-xs text-muted-foreground">
              {geometry
                ? `Boundary glow: ${state.record.gnd?.name} (${state.record.gnd?.pcode}).`
                : "No admin_geometry row for this record's GN division — showing the point marker only."}
            </p>
          )}
        </div>

        <div className="flex flex-col gap-2">
          <h2 className="text-sm font-semibold">
            {state.status === "success" || state.status === "loading" || state.status === "error"
              ? `GET /v1/postal/${state.code}`
              : "Raw response"}
          </h2>
          {state.status === "empty" && (
            <p className="rounded-md border border-dashed border-border p-4 text-sm text-muted-foreground">
              Pick a result above to see the API&apos;s raw JSON here.
            </p>
          )}
          {state.status === "loading" && <p className="text-sm text-muted-foreground">Loading…</p>}
          {state.status === "error" && (
            <p className="rounded-md border border-dashed border-destructive/40 p-4 text-sm text-destructive">{state.message}</p>
          )}
          {state.status === "success" && <JsonViewer value={state.record} />}
        </div>
      </div>
    </div>
  );
}
