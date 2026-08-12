/**
 * Shapes and small constants for the universal lookup omnibox
 * (docs/architecture.md §4, `GET /v1/lookup?q=&suggest=1`). Kept separate
 * from lib/api.ts since these are specific to the lookup route, not the
 * generic client.
 */

export type LookupRowType = "place" | "postal" | "admin" | "coordinate";

/** Suggest-mode row shape: `{type, id, label, sublabel, lat, lon, pcode?}`, capped at 10. */
export interface LookupSuggestRow {
  type: LookupRowType;
  id: number | string;
  label: string;
  sublabel: string;
  lat: number | null;
  lon: number | null;
  pcode?: string;
}

/** Short badge text for each row type, shown next to every result. */
export const LOOKUP_TYPE_LABELS: Record<LookupRowType, string> = {
  place: "Place",
  postal: "Postal",
  admin: "Admin",
  coordinate: "Coordinate",
};

/**
 * Display order for grouping suggest rows in the dropdown — coordinate and
 * admin matches are usually the most deliberate/precise queries, place and
 * postal are the free-text/blended fuzzy matches.
 */
export const LOOKUP_TYPE_ORDER: LookupRowType[] = ["coordinate", "admin", "postal", "place"];

/** A unique, stable key for a suggest row (or recent search) — type + id can collide across types (e.g. numeric place id vs numeric coordinate cell id), so both are needed. */
export function lookupRowKey(row: Pick<LookupSuggestRow, "type" | "id">): string {
  return `${row.type}:${row.id}`;
}
