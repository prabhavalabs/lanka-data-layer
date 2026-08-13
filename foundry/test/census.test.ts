import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildDsMatchIndex,
  matchDistrict,
  matchDs,
  normalizeName,
  parseCensusSheet,
  type CellValue,
} from "../src/lib/census.ts";

// A miniature of the real sheet layout: trilingual titles, a two-row header,
// blank spacers, then country -> district -> DS rows (values from column C).
const MATRIX: CellValue[][] = [
  [null, "A5 : Population by sex, age"],
  [],
  [null, "District and DS Division", "මුළු පුද්ගලයින් ගණන\nTotal number of persons", "Male"],
  [],
  [null, "ශ්‍රී ලංකා இலங்கை Sri Lanka", 100, 48],
  [],
  [null, "කොළඹ දිස්ත්‍රික්කය\nகொழும்பு மாவட்டம்\nColombo District", 60, 29],
  [null, "කොළඹ\nகொழும்பு\nColombo", 35, 17],
  [null, "ජා-ඇල\nஜா-எல\nJa-Ela", 25, 12],
  [],
  [null, "ගම්පහ දිස්ත්‍රික්කය\nகம்பஹா மாவட்டம்\nGampaha District", 40, 19],
  [null, "මීගමුව\nநீர்கொழும்பு\nNegombo", 40, 19],
];

test("parseCensusSheet extracts country/district/ds rows in order", () => {
  const rows = parseCensusSheet(MATRIX, 2);
  assert.deepEqual(
    rows.map((r) => [r.kind, r.nameEn, ...r.values]),
    [
      ["country", "Sri Lanka", 100, 48],
      ["district", "Colombo District", 60, 29],
      ["ds", "Colombo", 35, 17],
      ["ds", "Ja-Ela", 25, 12],
      ["district", "Gampaha District", 40, 19],
      ["ds", "Negombo", 40, 19],
    ],
  );
});

test("parseCensusSheet skips titles, headers, and blank spacers", () => {
  // The header row has a string in column C, the title row has nothing there.
  const rows = parseCensusSheet(MATRIX, 2);
  assert.equal(rows.length, 6);
});

test("parseCensusSheet throws when a data row is missing an expected value column", () => {
  const bad: CellValue[][] = [[null, "සිංහල\nதமிழ்\nSomewhere", 10, null]];
  assert.throws(() => parseCensusSheet(bad, 2), /Somewhere/);
});

test("normalizeName strips case, spaces, and punctuation", () => {
  assert.equal(normalizeName("Ja-Ela"), "jaela");
  assert.equal(normalizeName("Ja Ela"), "jaela");
  assert.equal(normalizeName("Sri Jayawardanapura Kotte"), "srijayawardanapurakotte");
  assert.equal(normalizeName("Island South (Velanai)"), "islandsouthvelanai");
});

const INDEX = buildDsMatchIndex([
  { properties: { adm2_name: "Colombo", adm2_pcode: "LK11", adm3_name: "Ja Ela", adm3_pcode: "LK1146" } },
  { properties: { adm2_name: "Kalutara", adm2_pcode: "LK13", adm3_name: "Matugama", adm3_pcode: "LK1330" } },
  { properties: { adm2_name: "Monaragala", adm2_pcode: "LK82", adm3_name: "Monaragala", adm3_pcode: "LK8212" } },
]);

test("matchDistrict strips the District suffix and applies aliases", () => {
  assert.equal(matchDistrict(INDEX, "Colombo District"), "LK11");
  // Census spells the district "Moneragala"; COD-AB has "Monaragala".
  assert.equal(matchDistrict(INDEX, "Moneragala District"), "LK82");
  assert.equal(matchDistrict(INDEX, "Atlantis District"), null);
});

test("matchDs matches by normalized name within the district", () => {
  // "Ja-Ela" (census) vs "Ja Ela" (COD-AB) — same normalized form.
  assert.equal(matchDs(INDEX, "LK11", "Ja-Ela"), "LK1146");
  // Wrong district: no cross-district matches, ever.
  assert.equal(matchDs(INDEX, "LK13", "Ja-Ela"), null);
});

test("matchDs applies the curated spelling aliases", () => {
  // "Mathugama" (census) -> "Matugama" (COD-AB).
  assert.equal(matchDs(INDEX, "LK13", "Mathugama"), "LK1330");
  // The Moneragala DS division inside Monaragala district.
  assert.equal(matchDs(INDEX, "LK82", "Moneragala"), "LK8212");
  assert.equal(matchDs(INDEX, "LK13", "Nowhere"), null);
});
