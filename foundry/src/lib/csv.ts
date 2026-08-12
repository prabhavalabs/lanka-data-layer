/**
 * Minimal RFC 4180 CSV encoding for the `downloads` step. No dependency:
 * this is the only place foundry emits CSV, and the rules are small enough
 * that a library would cost more than it saves.
 *
 * Values round-trip UTF-8 as-is (Sinhala/Tamil name columns included) —
 * this only escapes the ASCII structural characters CSV cares about.
 */

export type CsvValue = string | number | null | undefined;

const NEEDS_QUOTING = /[",\n\r]/;

/** Escapes a single field: wraps in quotes and doubles embedded quotes if it contains a comma, quote, or newline. */
export function csvField(value: CsvValue): string {
  if (value === null || value === undefined) return "";
  const s = String(value);
  if (!NEEDS_QUOTING.test(s)) return s;
  return `"${s.replace(/"/g, '""')}"`;
}

function csvRow(fields: CsvValue[]): string {
  return fields.map(csvField).join(",");
}

/** Builds a full CSV document (header + rows), CRLF-free (`\n` row separator), trailing newline included. */
export function toCsv(headers: string[], rows: CsvValue[][]): string {
  const lines = [csvRow(headers), ...rows.map(csvRow)];
  return lines.join("\n") + "\n";
}
