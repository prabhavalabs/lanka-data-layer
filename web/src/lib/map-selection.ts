import { apiGetPayload, apiGetRaw } from "@/lib/api";
import { useMapStore } from "@/stores/map-store";

/**
 * Selects an admin unit by pcode: sets the store's `selection` immediately
 * (so SelectionCard can start its own /v1/admin/:pcode fetch and show a
 * loading state right away) and best-effort fetches the unit's boundary
 * geometry to draw the map highlight. Shared by MapView's admin-polygon
 * click handler and SelectionCard's breadcrumb/hierarchy links.
 *
 * Omnibox has its own richer version of this same flow (with an inline
 * "couldn't load that boundary" error banner on a failed geometry fetch),
 * so it isn't a caller of this helper — the geometry outline is secondary
 * furniture here, not something either of these two callers has a banner
 * slot for; if it fails, the card still renders with a point-less admin
 * outline missing but its data intact.
 */
export function selectAdminUnit(pcode: string, label?: string): void {
  useMapStore.getState().setSelection({ type: "admin", pcode, label });
  apiGetRaw<GeoJSON.Feature>(`/admin/${encodeURIComponent(pcode)}/geometry`)
    .then((feature) => useMapStore.getState().setHighlight({ kind: "geometry", feature, label }))
    .catch(() => {
      // Swallow — see doc comment above.
    });
}

/**
 * Selects a postal code: sets `selection` immediately, then fetches the
 * record (for lat/lon, since callers here — a chain link on a place/
 * coordinate card — usually only have the code itself) to place a point
 * highlight. Mirrors `selectAdminUnit` above.
 */
export function selectPostalCode(code: string, label?: string): void {
  useMapStore.getState().setSelection({ type: "postal", code, label });
  apiGetPayload<{ code: string; name: string; lat: number | null; lon: number | null }>(
    `/postal/${encodeURIComponent(code)}`
  )
    .then((row) => {
      if (row.lat != null && row.lon != null) {
        useMapStore.getState().setHighlight({ kind: "point", lat: row.lat, lon: row.lon, label: label ?? row.name });
      }
    })
    .catch(() => {
      // Best-effort — the card does its own fetch and shows its own error state if that fails too.
    });
}
