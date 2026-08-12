/**
 * Classifies an election result entity id into its kind, per
 * docs/architecture.md §2 `election_entities.kind`.
 *
 * Entity id shapes seen in the source data (Election Commission of Sri
 * Lanka, via nuuuwan/lk_elections):
 *   "LK"       -> whole-country total                 -> NATIONAL
 *   "EC-01"    -> electoral district                   -> ED
 *   "EC-01A"   -> polling division within an ED         -> PD
 *   "EC-01P"   -> postal votes for an ED (the "P" is an -> POSTAL
 *                 ordinary-looking single-letter suffix,
 *                 so it must be checked before the
 *                 general PD pattern or it would be
 *                 misclassified as a polling division)
 */
export type ElectionEntityKind = "NATIONAL" | "ED" | "PD" | "POSTAL";

const ED_RE = /^EC-\d+$/;
const POSTAL_RE = /^EC-\d+P$/;
const PD_RE = /^EC-\d+[A-Z]$/;

export function classifyEntityKind(entityId: string): ElectionEntityKind {
  if (entityId === "LK") return "NATIONAL";
  if (ED_RE.test(entityId)) return "ED";
  // Must run before the PD check: POSTAL ids also match PD_RE.
  if (POSTAL_RE.test(entityId)) return "POSTAL";
  if (PD_RE.test(entityId)) return "PD";
  throw new Error(`Unrecognized election entity id: ${entityId}`);
}

/** The electoral district id an entity belongs to, or null for NATIONAL. */
export function edIdOf(entityId: string, kind: ElectionEntityKind): string | null {
  if (kind === "NATIONAL") return null;
  if (kind === "ED") return entityId;
  // PD ("EC-01A") and POSTAL ("EC-01P") both hang off their ED ("EC-01").
  const match = /^(EC-\d+)[A-Z]$/.exec(entityId);
  if (!match) throw new Error(`Cannot derive ed_id from entity id: ${entityId}`);
  return match[1]!;
}
