import "@/components/explore/fonts";

import { MapView } from "@/components/map/map-view";
import { useMapUrlSync } from "@/hooks/use-map-url-sync";

/**
 * Route-level "Explore" surface. App.tsx (out of scope to edit for this
 * task) still wraps every route in AppShell's sidebar+header chrome, so
 * this page escapes it visually with `fixed inset-0` — filling the actual
 * viewport instead of just the shell's <main> content area — rather than
 * the app shell being changed to stop wrapping it. z-40 keeps it above the
 * shell's sidebar/header (which have no z-index of their own, so DOM order
 * alone wouldn't guarantee this page paints over them) while staying below
 * anything the app might stack higher later (e.g. a future modal).
 */
export function MapPage() {
  useMapUrlSync();

  return (
    <div className="fixed inset-0 z-40 overflow-hidden" style={{ backgroundColor: "var(--bg, #07090C)" }}>
      <MapView />
    </div>
  );
}
