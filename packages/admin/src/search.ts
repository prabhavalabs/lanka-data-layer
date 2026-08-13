import type { AdminUnit } from "./types.ts";
import { allUnits } from "./store.ts";

const DEFAULT_LIMIT = 10;

/** Lower rank is a better match: exact (0) < prefix (1) < substring (2). */
function bestMatchRank(needle: string, unit: AdminUnit): number | null {
  let best: number | null = null;
  for (const name of [unit.name, unit.nameSi, unit.nameTa]) {
    if (!name) continue;
    const lower = name.toLowerCase();
    let rank: number | null = null;
    if (lower === needle) rank = 0;
    else if (lower.startsWith(needle)) rank = 1;
    else if (lower.includes(needle)) rank = 2;
    if (rank !== null && (best === null || rank < best)) best = rank;
  }
  return best;
}

/**
 * Case-insensitive search across name_en / name_si / name_ta. Exact match
 * ranks above prefix match above substring match; ties break alphabetically
 * by English name for a deterministic order.
 */
export function searchByName(q: string, opts?: { level?: number; limit?: number }): AdminUnit[] {
  const needle = q.trim().toLowerCase();
  if (!needle) return [];
  const limit = opts?.limit ?? DEFAULT_LIMIT;
  const level = opts?.level;

  const scored: { unit: AdminUnit; rank: number }[] = [];
  for (const unit of allUnits()) {
    if (level !== undefined && unit.level !== level) continue;
    const rank = bestMatchRank(needle, unit);
    if (rank !== null) scored.push({ unit, rank });
  }

  scored.sort((a, b) => a.rank - b.rank || a.unit.name.localeCompare(b.unit.name));
  return scored.slice(0, limit).map((s) => s.unit);
}
