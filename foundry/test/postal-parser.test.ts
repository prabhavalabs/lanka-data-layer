import { test } from "node:test";
import assert from "node:assert/strict";
import { parsePostalTsv, dedupeByCode } from "../src/lib/postal-parser.ts";

const SAMPLE = [
  "LK\t20000\tKandy\tCentral Province\t29\tKandy\tKY\t\t\t7.2955\t80.6356\t4",
  "LK\t20032\tHalloluwa\tCentral Province\t29\tKandy\tKY\t\t\t7.2031\t80.3386\t4",
  "LK\t00200\tColombo 02\tWestern Province\t28\tColombo\tCO\t\t\t6.9271\t79.8612\t4",
  "LK\t00200\tColombo 02 (dup)\tWestern Province\t28\tColombo\tCO\t\t\t6.9271\t79.8612\t4",
].join("\n");

test("parses tab-separated rows into structured objects", () => {
  const rows = parsePostalTsv(SAMPLE);
  assert.equal(rows.length, 4);
  assert.deepEqual(rows[0], {
    country: "LK",
    postalCode: "20000",
    placeName: "Kandy",
    admin1Name: "Central Province",
    admin1Code: "29",
    admin2Name: "Kandy",
    admin2Code: "KY",
    admin3Name: "",
    admin3Code: "",
    lat: 7.2955,
    lon: 80.6356,
    accuracy: 4,
  });
});

test("skips blank lines", () => {
  const rows = parsePostalTsv(SAMPLE + "\n\n\n");
  assert.equal(rows.length, 4);
});

test("throws on malformed rows (too few columns)", () => {
  assert.throws(() => parsePostalTsv("LK\t20000\tKandy"));
});

test("throws on malformed rows (bad lat/lon)", () => {
  assert.throws(() =>
    parsePostalTsv("LK\t20000\tKandy\tCentral Province\t29\tKandy\tKY\t\t\tNaN\t80.6356\t4"),
  );
});

test("dedupeByCode keeps first occurrence and drops later duplicates", () => {
  const rows = parsePostalTsv(SAMPLE);
  const deduped = dedupeByCode(rows);
  assert.equal(deduped.length, 3);
  const codes = deduped.map((r) => r.postalCode);
  assert.deepEqual(codes, ["20000", "20032", "00200"]);
  assert.equal(deduped[2]!.placeName, "Colombo 02");
});
