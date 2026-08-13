import * as React from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { apiGetRaw } from "@/lib/api";
import { LOOKUP_TYPE_LABELS, LOOKUP_TYPE_ORDER, lookupRowKey, type LookupRowType, type LookupSuggestRow } from "@/lib/lookup";
import { cn } from "@/lib/utils";
import { useLookupSuggestions } from "@/hooks/use-lookup-suggestions";
import { useRecentSearches } from "@/hooks/use-recent-searches";
import { useMapStore } from "@/stores/map-store";

const ACTION_ERROR_TIMEOUT_MS = 5000;

/** Suggest rows bucketed by LOOKUP_TYPE_ORDER, empty buckets dropped — drives the dropdown's grouped sections. */
function groupByType(rows: LookupSuggestRow[]): { type: LookupRowType; rows: LookupSuggestRow[] }[] {
  const byType = new Map<LookupRowType, LookupSuggestRow[]>();
  for (const row of rows) {
    const bucket = byType.get(row.type);
    if (bucket) bucket.push(row);
    else byType.set(row.type, [row]);
  }
  return LOOKUP_TYPE_ORDER.map((type) => ({ type, rows: byType.get(type) ?? [] })).filter((g) => g.rows.length > 0);
}

function Spinner({ className }: { className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn("inline-block size-3.5 animate-spin rounded-full border-2 border-muted-foreground/25 border-t-foreground", className)}
    />
  );
}

function SearchIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden="true" className={className}>
      <circle cx="9" cy="9" r="6" stroke="currentColor" strokeWidth="1.6" />
      <path d="M17 17l-3.8-3.8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

interface ResultRowProps {
  row: LookupSuggestRow;
  active: boolean;
  onHover: () => void;
  onSelect: () => void;
}

function ResultRow({ row, active, onHover, onSelect }: ResultRowProps) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={active}
      onMouseEnter={onHover}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onSelect}
      className={cn(
        "flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-sm transition-colors",
        active ? "bg-accent text-accent-foreground" : "hover:bg-muted"
      )}
    >
      <span className="flex min-w-0 flex-col">
        <span className="truncate font-medium">{row.label}</span>
        <span className="truncate text-xs text-muted-foreground">{row.sublabel}</span>
      </span>
      <span className="shrink-0 rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
        {LOOKUP_TYPE_LABELS[row.type]}
      </span>
    </button>
  );
}

/**
 * Universal search omnibox — the header's `⌘K` command palette over
 * `GET /v1/lookup?q=&suggest=1` (docs/architecture.md §4). Selecting a
 * result drives the map via mapStore: places/postal codes/coordinates get
 * a point highlight straight from the suggestion's lat/lon; admin areas
 * fetch their boundary from `GET /v1/admin/:pcode/geometry` (a bare
 * GeoJSON Feature, not the usual envelope — see api/src/routes/geometry.ts)
 * and get a geometry highlight. MapView (map-view.tsx) owns what happens
 * once a highlight is set — flying/fitting the camera, drawing the marker
 * or outline — this component only ever calls `setHighlight`.
 */
export function Omnibox() {
  const navigate = useNavigate();
  const location = useLocation();
  const setHighlight = useMapStore((s) => s.setHighlight);
  const setSelection = useMapStore((s) => s.setSelection);
  const { recents, addRecent, clearRecents } = useRecentSearches();

  const [query, setQuery] = React.useState("");
  const [open, setOpen] = React.useState(false);
  const [activeIndex, setActiveIndex] = React.useState(-1);
  const [actionError, setActionError] = React.useState<string | null>(null);

  const inputRef = React.useRef<HTMLInputElement | null>(null);
  const containerRef = React.useRef<HTMLDivElement | null>(null);

  const suggestState = useLookupSuggestions(query);
  const trimmedQuery = query.trim();
  const showRecents = trimmedQuery === "";

  const groupedSuggestions = React.useMemo(
    () => (suggestState.status === "success" ? groupByType(suggestState.rows) : []),
    [suggestState]
  );

  const flatRows: LookupSuggestRow[] = showRecents ? recents : groupedSuggestions.flatMap((g) => g.rows);

  // A fresh query invalidates whatever was highlighted by the previous one.
  React.useEffect(() => {
    setActiveIndex(-1);
  }, [query]);

  // ⌘K / Ctrl+K focuses the omnibox from anywhere in the app.
  React.useEffect(() => {
    function onWindowKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    }
    window.addEventListener("keydown", onWindowKeyDown);
    return () => window.removeEventListener("keydown", onWindowKeyDown);
  }, []);

  // Close the dropdown on an outside click (but not on blur alone — a
  // result's onMouseDown already prevents default, and clicks inside the
  // dropdown are inside containerRef anyway).
  React.useEffect(() => {
    if (!open) return;
    function onPointerDown(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  // Auto-dismiss the "couldn't load that boundary" banner.
  React.useEffect(() => {
    if (!actionError) return;
    const timer = window.setTimeout(() => setActionError(null), ACTION_ERROR_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [actionError]);

  async function selectRow(row: LookupSuggestRow) {
    addRecent(row);
    setQuery("");
    setOpen(false);
    setActiveIndex(-1);
    inputRef.current?.blur();

    if (location.pathname !== "/map") navigate("/map");

    if (row.type === "admin") {
      if (!row.pcode) {
        setActionError(`"${row.label}" doesn't have a boundary to show.`);
        return;
      }
      // Set selection right away — SelectionCard starts its own
      // /v1/admin/:pcode fetch immediately rather than waiting on the
      // boundary geometry below, which is only for the highlight outline.
      setSelection({ type: "admin", pcode: row.pcode, label: row.label });
      try {
        const feature = await apiGetRaw<GeoJSON.Feature>(`/admin/${encodeURIComponent(row.pcode)}/geometry`);
        setHighlight({ kind: "geometry", feature, label: row.label });
      } catch (err) {
        if (err instanceof DOMException && err.name === "AbortError") return;
        setActionError(`Couldn't load the boundary for "${row.label}".`);
      }
      return;
    }

    if (row.lat === null || row.lon === null) {
      setActionError(`"${row.label}" doesn't have a location to show.`);
      return;
    }
    setHighlight({ kind: "point", lat: row.lat, lon: row.lon, label: row.label });
    if (row.type === "postal") {
      setSelection({ type: "postal", code: String(row.id), label: row.label });
    } else if (row.type === "place") {
      setSelection({ type: "place", id: row.id, label: row.label, sublabel: row.sublabel, lat: row.lat, lon: row.lon, pcode: row.pcode });
    } else {
      setSelection({ type: "coordinate", lat: row.lat, lon: row.lon, label: row.label });
    }
  }

  function onInputKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (flatRows.length === 0) return;
      setOpen(true);
      setActiveIndex((i) => (i + 1) % flatRows.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (flatRows.length === 0) return;
      setOpen(true);
      setActiveIndex((i) => (i - 1 + flatRows.length) % flatRows.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = flatRows[activeIndex] ?? flatRows[0];
      if (row) void selectRow(row);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      inputRef.current?.blur();
    }
  }

  function renderSuggestionsBody() {
    if (suggestState.status === "idle") {
      return <div className="px-3 py-6 text-center text-sm text-muted-foreground">Keep typing to search…</div>;
    }
    if (suggestState.status === "loading" && groupedSuggestions.length === 0) {
      return (
        <div className="flex items-center justify-center gap-2 px-3 py-6 text-sm text-muted-foreground">
          <Spinner /> Searching…
        </div>
      );
    }
    if (suggestState.status === "error") {
      return <div className="px-3 py-6 text-center text-sm text-destructive">{suggestState.error.message}</div>;
    }
    if (groupedSuggestions.length === 0) {
      return <div className="px-3 py-6 text-center text-sm text-muted-foreground">No results for “{trimmedQuery}”.</div>;
    }

    let rowIndex = 0;
    return groupedSuggestions.map((group) => (
      <div key={group.type}>
        <div className="bg-popover px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {LOOKUP_TYPE_LABELS[group.type]}
        </div>
        {group.rows.map((row) => {
          const idx = rowIndex++;
          return (
            <ResultRow
              key={lookupRowKey(row)}
              row={row}
              active={idx === activeIndex}
              onHover={() => setActiveIndex(idx)}
              onSelect={() => void selectRow(row)}
            />
          );
        })}
      </div>
    ));
  }

  function renderRecentsBody() {
    if (recents.length === 0) {
      return (
        <div className="px-3 py-6 text-center text-sm text-muted-foreground">
          Search places, postal codes, admin areas, or coordinates.
        </div>
      );
    }
    return (
      <div>
        <div className="flex items-center justify-between px-3 py-1.5">
          <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">Recent</span>
          <button
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={clearRecents}
            className="text-[11px] font-medium text-muted-foreground hover:text-foreground"
          >
            Clear
          </button>
        </div>
        {recents.map((row, idx) => (
          <ResultRow
            key={lookupRowKey(row)}
            row={row}
            active={idx === activeIndex}
            onHover={() => setActiveIndex(idx)}
            onSelect={() => void selectRow(row)}
          />
        ))}
      </div>
    );
  }

  return (
    <div ref={containerRef} className="relative w-full max-w-md">
      <div className="relative">
        <SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={onInputKeyDown}
          placeholder="Search places, postal codes, coordinates…"
          role="combobox"
          aria-expanded={open}
          aria-controls="omnibox-listbox"
          aria-autocomplete="list"
          autoComplete="off"
          spellCheck={false}
          className="h-9 w-full rounded-md border border-input bg-background py-2 pl-8 pr-12 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
        />
        <div className="pointer-events-none absolute right-2 top-1/2 flex -translate-y-1/2 items-center">
          {suggestState.status === "loading" ? (
            <Spinner />
          ) : (
            <kbd className="rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">⌘K</kbd>
          )}
        </div>
      </div>

      {open && (
        <div
          id="omnibox-listbox"
          role="listbox"
          className="absolute left-0 right-0 top-full z-50 mt-1.5 max-h-96 overflow-y-auto rounded-lg border border-border bg-popover py-1 text-popover-foreground shadow-lg"
        >
          {showRecents ? renderRecentsBody() : renderSuggestionsBody()}
        </div>
      )}

      {actionError && !open && (
        <div className="absolute left-0 right-0 top-full z-40 mt-1.5 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {actionError}
        </div>
      )}
    </div>
  );
}
