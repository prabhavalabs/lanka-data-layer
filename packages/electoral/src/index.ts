import raw from "../data/electoral.json" with { type: "json" };

/**
 * @lanka-data-layer/electoral — offline Sri Lankan election results.
 *
 * Covers four elections (2024 parliamentary, 2015/2019/2024 presidential)
 * across every electoral district, polling division, postal-vote entity,
 * and the national total. The full dataset is bundled at build time —
 * there is no network access, no filesystem read at runtime, and nothing
 * to configure.
 */

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/** One of the four elections this dataset covers. */
export interface Election {
  id: string;
  type: string;
  year: number;
  date: string;
  label: string;
}

/**
 * `ED` — electoral district, `PD` — polling division, `POSTAL` — a
 * district's postal-vote entity, `NATIONAL` — the country-wide total (`LK`).
 */
export type ElectoralEntityKind = "ED" | "PD" | "POSTAL" | "NATIONAL";

/** A place votes are counted for: an electoral district, polling division,
 * postal-vote entity, or the national total. */
export interface ElectoralEntity {
  id: string;
  kind: ElectoralEntityKind;
  name: string;
  /** The parent electoral district's id, or null for the national entity. */
  edId: string | null;
}

/** A party's identity within one election — code, candidate, display name,
 * and chart color. Not every party code that appears in a `Result` has a
 * matching entry here: this list covers the notable contesting parties,
 * while `Result.parties` includes every code the source recorded, including
 * minor and independent candidates with no metadata beyond their code. */
export interface Party {
  electionId: string;
  code: string;
  candidate: string;
  name: string;
  color: string;
}

/** One party's share of a single result. */
export interface PartyResult {
  party: string;
  votes: number;
  /** Fraction of `valid` votes, 4 decimal places. */
  share: number;
}

/** The winning party of a result, with its share of the valid vote. */
export interface Winner {
  party: string;
  votes: number;
  /** Fraction of `valid` votes, 4 decimal places. */
  share: number;
}

/** Full result breakdown for one (election, entity) pair. */
export interface Result {
  electors: number;
  polled: number;
  valid: number;
  rejected: number;
  /** `polled / electors`, 4 decimal places, or null if `electors` is 0. */
  turnout: number | null;
  winner: Winner | null;
  /** Every party the source recorded for this result, descending by votes. */
  parties: PartyResult[];
}

/** A `Result` tagged with the entity it belongs to. */
export interface ResultForElection extends Result {
  entityId: string;
}

/** A `Result` tagged with the election it belongs to. */
export interface ResultForEntity extends Result {
  electionId: string;
}

export interface SourceAttribution {
  name: string;
  url: string;
  license: string;
}

// ---------------------------------------------------------------------------
// Raw data shape (as emitted by the foundry)
// ---------------------------------------------------------------------------

/** entities are tuples, not objects: [id, kind, name, edId]. */
type RawEntity = [id: string, kind: ElectoralEntityKind, name: string, edId: string | null];

/** parties are tuples too: [electionId, code, candidate, name, color]. */
type RawParty = [electionId: string, code: string, candidate: string, name: string, color: string];

interface RawResult {
  electors: number;
  polled: number;
  valid: number;
  rejected: number;
  winner_party: string | null;
  winner_votes: number | null;
  parties: [code: string, votes: number][];
}

interface RawData {
  data_version: string;
  generated: string;
  sources: SourceAttribution[];
  elections: Election[];
  entities: RawEntity[];
  parties: RawParty[];
  results: Record<string, Record<string, RawResult>>;
}

const data = raw as unknown as RawData;

/** The dataset's build version (`YYYYMMDD.N`), matching the foundry's
 * `manifest.json`. Bump-tracks the underlying artifact — compare it if
 * you need to detect a stale cached copy of this package. */
export const DATA_VERSION: string = data.data_version;

/** Source attribution for every election result in this package. */
export const SOURCES: SourceAttribution[] = data.sources;

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

/** Rounds to `decimals` places, guarding the classic binary-float overshoot
 * (plain `Math.round(1.005 * 100) / 100` undershoots to 1). */
function round(value: number, decimals: number): number {
  const factor = 10 ** decimals;
  return Math.round((value + Number.EPSILON) * factor) / factor;
}
const round4 = (value: number) => round(value, 4);

// ---------------------------------------------------------------------------
// Lazy indexes — built on first use, then memoized
// ---------------------------------------------------------------------------

let electionsCache: Election[] | null = null;
let electionIndex: Map<string, Election> | null = null;

/** All four elections, sorted most recent first (ties broken by id). */
export function listElections(): Election[] {
  if (!electionsCache) {
    electionsCache = [...data.elections].sort((a, b) => b.year - a.year || a.id.localeCompare(b.id));
  }
  return electionsCache;
}

function getElectionIndex(): Map<string, Election> {
  if (!electionIndex) {
    electionIndex = new Map(listElections().map((e) => [e.id, e]));
  }
  return electionIndex;
}

/** Looks up one election by id (e.g. `"parl-2024"`), or null if unknown. */
export function getElection(id: string): Election | null {
  return getElectionIndex().get(id) ?? null;
}

let entitiesCache: ElectoralEntity[] | null = null;
let entityIndex: Map<string, ElectoralEntity> | null = null;

/** Every electoral entity: districts, polling divisions, postal-vote
 * entities, and the national total — 205 rows. */
export function listEntities(): ElectoralEntity[] {
  if (!entitiesCache) {
    entitiesCache = data.entities.map(([id, kind, name, edId]) => ({ id, kind, name, edId }));
  }
  return entitiesCache;
}

function getEntityIndex(): Map<string, ElectoralEntity> {
  if (!entityIndex) {
    entityIndex = new Map(listEntities().map((e) => [e.id, e]));
  }
  return entityIndex;
}

/** Looks up one entity by id (e.g. `"EC-01"`, `"LK"`), or null if unknown. */
export function getEntity(id: string): ElectoralEntity | null {
  return getEntityIndex().get(id) ?? null;
}

let partiesCache: Party[] | null = null;

/** Every party record across all four elections — 16 rows. A party that
 * contested more than one election appears once per election, since its
 * candidate, display name, and color can all differ between them. */
export function listParties(): Party[] {
  if (!partiesCache) {
    partiesCache = data.parties.map(([electionId, code, candidate, name, color]) => ({
      electionId,
      code,
      candidate,
      name,
      color,
    }));
  }
  return partiesCache;
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

function computeResult(raw: RawResult): Result {
  const turnout = raw.electors > 0 ? round4(raw.polled / raw.electors) : null;
  const winner =
    raw.winner_party != null && raw.winner_votes != null
      ? { party: raw.winner_party, votes: raw.winner_votes, share: raw.valid > 0 ? round4(raw.winner_votes / raw.valid) : 0 }
      : null;
  const parties: PartyResult[] = raw.parties
    .map(([party, votes]) => ({ party, votes, share: raw.valid > 0 ? round4(votes / raw.valid) : 0 }))
    .sort((a, b) => b.votes - a.votes);

  return {
    electors: raw.electors,
    polled: raw.polled,
    valid: raw.valid,
    rejected: raw.rejected,
    turnout,
    winner,
    parties,
  };
}

/** The result for one (election, entity) pair, or null if either id is
 * unknown or the pair has no recorded result. */
export function getResult(electionId: string, entityId: string): Result | null {
  const raw = data.results[electionId]?.[entityId];
  return raw ? computeResult(raw) : null;
}

/** Every entity's result for one election, each tagged with `entityId`.
 * Empty array for an unknown election id. */
export function resultsForElection(electionId: string): ResultForElection[] {
  const rows = data.results[electionId];
  if (!rows) return [];
  return Object.entries(rows).map(([entityId, raw]) => ({ entityId, ...computeResult(raw) }));
}

/** One entity's result across every election it appears in, each tagged
 * with `electionId`. Empty array for an unknown entity id. */
export function resultsForEntity(entityId: string): ResultForEntity[] {
  const out: ResultForEntity[] = [];
  for (const [electionId, rows] of Object.entries(data.results)) {
    const raw = rows[entityId];
    if (raw) out.push({ electionId, ...computeResult(raw) });
  }
  return out;
}
