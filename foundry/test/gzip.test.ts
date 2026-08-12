import { test } from "node:test";
import assert from "node:assert/strict";
import { gzipBuffer, gunzipToText } from "../src/lib/gzip.ts";

test("gzip then gunzip recovers plain ASCII text exactly", () => {
  const text = '{"type":"FeatureCollection","features":[]}';
  const gz = gzipBuffer(text);
  assert.equal(gunzipToText(gz), text);
});

test("gzip then gunzip recovers multi-byte (Sinhala/Tamil) text exactly", () => {
  const text = JSON.stringify({ name_si: "කොළඹ", name_ta: "கொழும்பு" });
  const gz = gzipBuffer(text);
  assert.equal(gunzipToText(gz), text);
});

test("gzipped output is actually compressed (starts with the gzip magic number)", () => {
  const text = "a".repeat(10_000);
  const gz = gzipBuffer(text);
  assert.equal(gz[0], 0x1f);
  assert.equal(gz[1], 0x8b);
  assert.ok(gz.length < text.length, "gzip output should be smaller than 10,000 repeated bytes");
});

test("round-trips a realistic GeoJSON FeatureCollection without data loss", () => {
  const fc = {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        properties: { id: "node/61602781", name: "Kumarakanda", name_si: null, name_ta: null, kind: "station" },
        geometry: { type: "Point", coordinates: [80.124287, 6.112449] },
      },
    ],
  };
  const text = JSON.stringify(fc);
  const gz = gzipBuffer(text);
  const recovered = JSON.parse(gunzipToText(gz));
  assert.deepEqual(recovered, fc);
});
