import * as React from "react";

import { apiGetPayload } from "@/lib/api";
import type { PopulationCell } from "@/components/map/population-layer";

export interface PopulationGridPayload {
  res: number;
  count: number;
  cells: PopulationCell[];
}

/**
 * Fetches `GET /v1/population/grid?res=0.02` (docs/architecture.md §4)
 * lazily — only once `enabled` first goes true (i.e. the population-3d
 * layer gets toggled on) — and caches the result for the component's
 * lifetime so re-toggling doesn't re-fetch ~14k rows every time.
 */
export function usePopulationGrid(enabled: boolean): { data: PopulationGridPayload | null; error: Error | null } {
  const [data, setData] = React.useState<PopulationGridPayload | null>(null);
  const [error, setError] = React.useState<Error | null>(null);
  const fetchedRef = React.useRef(false);

  React.useEffect(() => {
    if (!enabled || fetchedRef.current) return;
    fetchedRef.current = true;

    const controller = new AbortController();
    apiGetPayload<PopulationGridPayload>("/population/grid", {
      params: { res: 0.02 },
      signal: controller.signal,
    })
      .then(setData)
      .catch((err: unknown) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
        fetchedRef.current = false; // allow a retry on the next toggle
        setError(err as Error);
      });

    return () => controller.abort();
  }, [enabled]);

  return { data, error };
}
