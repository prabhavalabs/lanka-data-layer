import type { PostalCode } from "./types.ts";
import { allCodes } from "./store.ts";

const DEFAULT_LIMIT = 10;

/** Lower rank is a better match: exact (0) < prefix (1) < substring (2). */
function matchRank(needle: string, name: string): number | null {
  const lower = name.toLowerCase();
  if (lower === needle) return 0;
  if (lower.startsWith(needle)) return 1;
  if (lower.includes(needle)) return 2;
  return null;
}

/**
 * Case-insensitive search over post office / place names. Exact match ranks
 * above prefix match above substring match; ties break alphabetically for a
 * deterministic order. Defaults to the top 10 results.
 */
export function searchOffices(q: string, opts?: { limit?: number }): PostalCode[] {
  const needle = q.trim().toLowerCase();
  if (!needle) return [];
  const limit = opts?.limit ?? DEFAULT_LIMIT;

  const scored: { code: PostalCode; rank: number }[] = [];
  for (const c of allCodes()) {
    const rank = matchRank(needle, c.name);
    if (rank !== null) scored.push({ code: c, rank });
  }

  scored.sort((a, b) => a.rank - b.rank || a.code.name.localeCompare(b.code.name));
  return scored.slice(0, limit).map((s) => s.code);
}
