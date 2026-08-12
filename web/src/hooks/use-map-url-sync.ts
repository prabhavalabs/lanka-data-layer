import * as React from "react";
import { useSearchParams } from "react-router-dom";

import { useMapStore } from "@/stores/map-store";

const DEBOUNCE_MS = 400;
const COORD_PRECISION = 5;
const ZOOM_PRECISION = 2;

/**
 * Parses a required numeric search param. Returns null when the param is
 * absent, empty, or not a finite number — note `Number(null)` and
 * `Number("")` both evaluate to `0`, so a plain `Number(...)` +
 * `isFinite` check would silently accept a missing param as "0".
 */
function parseRequiredNumberParam(searchParams: URLSearchParams, key: string): number | null {
  const raw = searchParams.get(key);
  if (raw === null || raw.trim() === "") return null;
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}

/**
 * Two-way sync between the map camera and the `lat`/`lon`/`z` search
 * params. Restores the view from the URL present at first paint (once);
 * afterwards, mirrors camera changes back to the URL, debounced, replacing
 * history so panning doesn't spam back/forward.
 */
export function useMapUrlSync(): void {
  const [searchParams, setSearchParams] = useSearchParams();
  const center = useMapStore((s) => s.center);
  const zoom = useMapStore((s) => s.zoom);
  const setView = useMapStore((s) => s.setView);

  const didRestore = React.useRef(false);

  React.useEffect(() => {
    if (didRestore.current) return;
    didRestore.current = true;

    const lat = parseRequiredNumberParam(searchParams, "lat");
    const lon = parseRequiredNumberParam(searchParams, "lon");
    const z = parseRequiredNumberParam(searchParams, "z");
    if (lat !== null && lon !== null && z !== null) {
      setView({ center: [lon, lat], zoom: z });
    }
    // Restore exactly once, from whatever the URL was on first paint.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  React.useEffect(() => {
    if (!didRestore.current) return;

    const handle = window.setTimeout(() => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set("lat", center[1].toFixed(COORD_PRECISION));
          next.set("lon", center[0].toFixed(COORD_PRECISION));
          next.set("z", zoom.toFixed(ZOOM_PRECISION));
          return next;
        },
        { replace: true }
      );
    }, DEBOUNCE_MS);

    return () => window.clearTimeout(handle);
  }, [center, zoom, setSearchParams]);
}
