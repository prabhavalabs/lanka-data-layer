import raw from "../data/postal.json" with { type: "json" };
import type { PostalCode, Source } from "./types.ts";

/**
 * Row shape emitted by the foundry: a flat tuple per postal code, positional
 * to keep the bundled JSON small. Kept in sync with
 * packages/postal/data/postal.json.
 */
type CodeRow = [
  code: string,
  name: string,
  adminPcode: string | null,
  lat: number | null,
  lon: number | null,
];

/** [code, share] pairs, share = fraction of the division's land cells the code covers. */
type DivisionEntry = [code: string, share: number];

interface PostalData {
  data_version: string;
  generated: string;
  sources: Source[];
  codes: CodeRow[];
  division_codes: Record<string, DivisionEntry[]>;
}

const data = raw as unknown as PostalData;

export const DATA_VERSION: string = data.data_version;
export const CODE_COUNT: number = data.codes.length;
export const SOURCES: Source[] = data.sources;

function rowToCode(row: CodeRow): PostalCode {
  const [code, name, adminPcode, lat, lon] = row;
  return {
    code,
    name,
    adminPcode,
    location: lat !== null && lon !== null ? { lat, lon } : null,
  };
}

export interface DivisionShare {
  code: string;
  share: number;
}

export interface CodeShare {
  pcode: string;
  share: number;
}

let byCode: Map<string, PostalCode> | null = null;
let reverseIndex: Map<string, CodeShare[]> | null = null;

/** Builds the code lookup map on first use so import cost stays near-zero. */
function getByCode(): Map<string, PostalCode> {
  if (byCode) return byCode;
  byCode = new Map();
  for (const row of data.codes) {
    const c = rowToCode(row);
    byCode.set(c.code, c);
  }
  return byCode;
}

/** Builds the reverse (code -> divisions) index on first use. */
function getReverseIndex(): Map<string, CodeShare[]> {
  if (reverseIndex) return reverseIndex;
  reverseIndex = new Map();
  for (const [pcode, entries] of Object.entries(data.division_codes)) {
    for (const [code, share] of entries) {
      const existing = reverseIndex.get(code);
      if (existing) existing.push({ pcode, share });
      else reverseIndex.set(code, [{ pcode, share }]);
    }
  }
  return reverseIndex;
}

export function lookupCode(code: string): PostalCode | null {
  return getByCode().get(code.trim()) ?? null;
}

export function allCodes(): IterableIterator<PostalCode> {
  return getByCode().values();
}

export function lookupDivision(pcode: string): DivisionShare[] {
  const entries = data.division_codes[pcode.trim().toUpperCase()];
  if (!entries) return [];
  return entries.map(([code, share]) => ({ code, share }));
}

export function lookupReverse(code: string): CodeShare[] {
  const entries = getReverseIndex().get(code.trim());
  return entries ? [...entries] : [];
}
