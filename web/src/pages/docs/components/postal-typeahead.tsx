import * as React from "react";

import { ApiError, apiGetPayload } from "@/lib/api";
import { cn } from "@/lib/utils";

const DEBOUNCE_MS = 250;
const MIN_QUERY_LEN = 2;

/** The lightweight suggest-mode row shape from GET /v1/lookup?suggest=1 (api/src/routes/lookup.ts's SuggestRow). */
export interface LookupSuggestion {
  type: "coordinate" | "postal" | "admin" | "place";
  id: number | string;
  label: string;
  sublabel: string;
  lat: number | null;
  lon: number | null;
  pcode?: string;
}

type SuggestState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "success"; rows: LookupSuggestion[] };

/**
 * Own small typeahead against `/v1/lookup?suggest=1&q=` — debounced 250ms,
 * requires ≥2 characters, cancels the in-flight request via AbortController
 * whenever the query changes again before it resolves. Deliberately
 * separate from web/src/components/search/** (owned by another concurrent
 * agent) — this is scoped to the postal-code demo only.
 *
 * Results are filtered to `type === "postal"`: /v1/lookup is a universal
 * classifier (place/postal/admin/coordinate), but this demo specifically
 * showcases /v1/postal/:code, so only postal rows are surfaced here. Note
 * the classifier's postal branch only matches a *complete* 5-digit code —
 * partial numeric input (e.g. "105") falls through to free-text matching
 * instead, which only hits postal_codes.name, not the code. Searching a
 * partial area name (e.g. "Padu") works via that same free-text path.
 */
export function usePostalSuggestions(query: string): SuggestState {
  const [state, setState] = React.useState<SuggestState>({ status: "idle" });

  React.useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < MIN_QUERY_LEN) {
      setState({ status: "idle" });
      return;
    }

    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      setState({ status: "loading" });
      apiGetPayload<LookupSuggestion[]>("/lookup", {
        params: { suggest: 1, q: trimmed },
        signal: controller.signal,
      })
        .then((rows) => setState({ status: "success", rows: rows.filter((r) => r.type === "postal") }))
        .catch((err: unknown) => {
          if (err instanceof DOMException && err.name === "AbortError") return;
          setState({ status: "error", message: err instanceof ApiError ? err.message : String(err) });
        });
    }, DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [query]);

  return state;
}

export function PostalTypeahead({
  onSelect,
  placeholder,
}: {
  onSelect: (row: LookupSuggestion) => void;
  placeholder?: string;
}) {
  const [query, setQuery] = React.useState("");
  const [open, setOpen] = React.useState(false);
  const state = usePostalSuggestions(query);

  return (
    <div className="relative">
      <input
        type="text"
        value={query}
        placeholder={placeholder ?? "Postal code or area name, e.g. 10500 or Padukka"}
        className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => window.setTimeout(() => setOpen(false), 150)}
      />

      {open && query.trim().length >= MIN_QUERY_LEN && (
        <div className="absolute z-30 mt-1 w-full overflow-hidden rounded-md border border-border bg-popover shadow-md">
          {state.status === "loading" && <div className="px-3 py-2 text-xs text-muted-foreground">Searching…</div>}
          {state.status === "error" && <div className="px-3 py-2 text-xs text-destructive">{state.message}</div>}
          {state.status === "success" && state.rows.length === 0 && (
            <div className="px-3 py-2 text-xs text-muted-foreground">
              No postal codes match &quot;{query.trim()}&quot;. Try a full 5-digit code or an area name.
            </div>
          )}
          {state.status === "success" &&
            state.rows.map((row) => (
              <button
                key={`${row.type}-${row.id}`}
                type="button"
                className={cn(
                  "flex w-full flex-col items-start gap-0.5 px-3 py-2 text-left text-sm hover:bg-accent hover:text-accent-foreground"
                )}
                // onMouseDown (not onClick) fires before the input's onBlur closes the list.
                onMouseDown={(e) => {
                  e.preventDefault();
                  setQuery(row.label);
                  setOpen(false);
                  onSelect(row);
                }}
              >
                <span className="font-mono font-medium">{row.label}</span>
                <span className="text-xs text-muted-foreground">{row.sublabel}</span>
              </button>
            ))}
        </div>
      )}
    </div>
  );
}
