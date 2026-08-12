import * as React from "react";

import { lookupRowKey, type LookupSuggestRow } from "@/lib/lookup";

const STORAGE_KEY = "geopub.omnibox.recent-searches";
const MAX_RECENTS = 6;

function readStoredRecents(): LookupSuggestRow[] {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? (parsed as LookupSuggestRow[]) : [];
  } catch {
    // Corrupt or inaccessible storage (private browsing, quota, hand-edited
    // value) — behave as if there's simply no history yet.
    return [];
  }
}

function writeStoredRecents(rows: LookupSuggestRow[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(rows));
  } catch {
    // Best-effort — a full/blocked localStorage shouldn't break selection.
  }
}

/**
 * Last 6 omnibox selections, persisted to localStorage so they survive a
 * reload. Most-recent-first; re-selecting an existing entry moves it to the
 * front instead of duplicating it.
 */
export function useRecentSearches(): {
  recents: LookupSuggestRow[];
  addRecent: (row: LookupSuggestRow) => void;
  clearRecents: () => void;
} {
  const [recents, setRecents] = React.useState<LookupSuggestRow[]>(() => readStoredRecents());

  const addRecent = React.useCallback((row: LookupSuggestRow) => {
    setRecents((prev) => {
      const deduped = prev.filter((r) => lookupRowKey(r) !== lookupRowKey(row));
      const next = [row, ...deduped].slice(0, MAX_RECENTS);
      writeStoredRecents(next);
      return next;
    });
  }, []);

  const clearRecents = React.useCallback(() => {
    setRecents([]);
    writeStoredRecents([]);
  }, []);

  return { recents, addRecent, clearRecents };
}
