import { test } from "node:test";
import assert from "node:assert/strict";
import { csvField, toCsv } from "../src/lib/csv.ts";

test("plain values pass through unquoted", () => {
  assert.equal(csvField("Kandy"), "Kandy");
  assert.equal(csvField(42), "42");
  assert.equal(csvField(0), "0");
});

test("null and undefined become empty fields", () => {
  assert.equal(csvField(null), "");
  assert.equal(csvField(undefined), "");
});

test("commas trigger quoting", () => {
  assert.equal(csvField("Colombo, Western Province"), '"Colombo, Western Province"');
});

test("embedded quotes are doubled and the field is wrapped", () => {
  assert.equal(csvField('The "Fort" area'), '"The ""Fort"" area"');
});

test("embedded newlines trigger quoting", () => {
  assert.equal(csvField("line one\nline two"), '"line one\nline two"');
});

test("Sinhala and Tamil text passes through unescaped and unmodified", () => {
  const si = "කොළඹ";
  const ta = "கொழும்பு";
  assert.equal(csvField(si), si);
  assert.equal(csvField(ta), ta);
});

test("toCsv builds a header row plus one row per record, newline-terminated", () => {
  const csv = toCsv(
    ["code", "name"],
    [
      ["20000", "Kandy"],
      ["00200", "Colombo 02"],
    ],
  );
  assert.equal(csv, "code,name\n20000,Kandy\n00200,Colombo 02\n");
});

test("round-trip: a written CSV field, when split back on the escaping rules, recovers the original value", () => {
  const original = 'Ariviyal Nagar, "Halt" — අරිවියල් නගර්, மீசாலை';
  const field = csvField(original);
  // A quoted field starts and ends with a quote; unescape by stripping the
  // wrapper and un-doubling internal quotes — the inverse of csvField.
  assert.ok(field.startsWith('"') && field.endsWith('"'));
  const recovered = field.slice(1, -1).replace(/""/g, '"');
  assert.equal(recovered, original);
});
