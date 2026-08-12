import * as React from "react";

import { ApiError, apiGetPayload } from "@/lib/api";
import { looksLikeCoordinatePair } from "@/lib/coord-query";
import type { LookupSuggestRow } from "@/lib/lookup";

const DEBOUNCE_MS = 250;
const MIN_QUERY_LENGTH = 2;

export type LookupSuggestState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  | { status: "success"; rows: LookupSuggestRow[] };

/** Whether `query` is substantial enough to search — the normal 2-char floor, or (regardless of length) anything that already parses as a full coordinate pair. */
function isSearchable(query: string): boolean {
  return query.length >= MIN_QUERY_LENGTH || looksLikeCoordinatePair(query);
}

/**
 * Debounced (250ms) `GET /v1/lookup?q=&suggest=1` for the omnibox. Fires
 * from 2+ characters, or immediately for a full coordinate pair regardless
 * of length. Cancels the in-flight request (both the pending timer and the
 * fetch itself, via AbortController) whenever `query` changes again before
 * it resolves, so a slow response for a stale query can never overwrite a
 * newer one.
 */
export function useLookupSuggestions(query: string): LookupSuggestState {
  const [state, setState] = React.useState<LookupSuggestState>({ status: "idle" });

  React.useEffect(() => {
    const trimmed = query.trim();
    if (!isSearchable(trimmed)) {
      setState({ status: "idle" });
      return;
    }

    setState({ status: "loading" });
    const controller = new AbortController();

    const timer = window.setTimeout(() => {
      apiGetPayload<LookupSuggestRow[]>("/lookup", {
        params: { q: trimmed, suggest: 1 },
        signal: controller.signal,
      })
        .then((rows) => setState({ status: "success", rows }))
        .catch((err: unknown) => {
          if (err instanceof DOMException && err.name === "AbortError") return;
          const apiErr = err instanceof ApiError ? err : new ApiError(String(err), 0);
          // not_in_coverage (coordinate outside the Sri Lanka grid) isn't a
          // failure worth alarming the user over — it's just an empty
          // result, same as any other query with no matches.
          if (apiErr.status === 404) {
            setState({ status: "success", rows: [] });
            return;
          }
          setState({ status: "error", error: apiErr });
        });
    }, DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [query]);

  return state;
}
