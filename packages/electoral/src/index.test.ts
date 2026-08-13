import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DATA_VERSION,
  SOURCES,
  listElections,
  getElection,
  listEntities,
  getEntity,
  listParties,
  getResult,
  resultsForElection,
  resultsForEntity,
} from "./index.ts";

test("DATA_VERSION and SOURCES are populated", () => {
  assert.match(DATA_VERSION, /^\d{8}\.\d+$/);
  assert.ok(SOURCES.length > 0);
  assert.ok(SOURCES.every((s) => s.name && s.url && s.license));
});

test("listElections includes a known election with a plausible year", () => {
  const elections = listElections();
  assert.ok(elections.length >= 4);
  const parl2024 = elections.find((e) => e.id === "parl-2024");
  assert.ok(parl2024);
  assert.equal(parl2024?.type, "parliamentary");
  assert.equal(parl2024?.year, 2024);
  assert.equal(parl2024?.date, "2024-11-14");
});

test("listElections is sorted most recent year first", () => {
  const years = listElections().map((e) => e.year);
  for (let i = 1; i < years.length; i++) {
    assert.ok(years[i - 1]! >= years[i]!);
  }
});

test("getElection resolves a real id and returns null for an unknown one", () => {
  assert.deepEqual(getElection("parl-2024"), {
    id: "parl-2024",
    type: "parliamentary",
    year: 2024,
    date: "2024-11-14",
    label: "2024 Parliamentary",
  });
  assert.equal(getElection("no-such-election"), null);
});

test("listEntities covers every entity kind and getEntity resolves the national total", () => {
  const entities = listEntities();
  assert.equal(entities.length, 205);
  const kinds = new Set(entities.map((e) => e.kind));
  assert.deepEqual([...kinds].sort(), ["ED", "NATIONAL", "PD", "POSTAL"]);

  const lk = getEntity("LK");
  assert.deepEqual(lk, { id: "LK", kind: "NATIONAL", name: "Sri Lanka", edId: null });

  const ed = getEntity("EC-01");
  assert.equal(ed?.kind, "ED");
  assert.equal(ed?.edId, "EC-01");

  assert.equal(getEntity("no-such-entity"), null);
});

test("listParties returns per-election party records", () => {
  const parties = listParties();
  assert.equal(parties.length, 16);
  const npp2024 = parties.find((p) => p.electionId === "parl-2024" && p.code === "NPP");
  assert.ok(npp2024);
  assert.equal(npp2024?.name, "National People's Power");
  assert.match(npp2024?.color ?? "", /^#[0-9a-f]{6}$/i);
});

test("getResult: a real (election, entity) pair — hand-verified against the source numbers", () => {
  const result = getResult("parl-2024", "EC-01");
  assert.ok(result);
  assert.equal(result?.electors, 1765351);
  assert.equal(result?.polled, 1211738);
  assert.equal(result?.valid, 1149125);
  assert.equal(result?.rejected, 62613);

  // turnout = polled / electors, 4dp, within (0, 1]
  assert.equal(result?.turnout, 0.6864);
  assert.ok(result!.turnout! > 0 && result!.turnout! <= 1);

  // winner matches the top party by votes
  assert.deepEqual(result?.winner, { party: "NPP", votes: 788636, share: 0.6863 });
  const topByVotes = [...result!.parties].sort((a, b) => b.votes - a.votes)[0];
  assert.equal(topByVotes?.party, result?.winner?.party);
  assert.equal(topByVotes?.votes, result?.winner?.votes);

  // parties sum to no more than valid votes (the source lists top parties only, not every candidate)
  const sum = result!.parties.reduce((s, p) => s + p.votes, 0);
  assert.ok(sum <= result!.valid);

  // parties sorted desc by votes
  for (let i = 1; i < result!.parties.length; i++) {
    assert.ok(result!.parties[i - 1]!.votes >= result!.parties[i]!.votes);
  }
});

test("getResult returns null for unknown election or entity ids", () => {
  assert.equal(getResult("no-such-election", "EC-01"), null);
  assert.equal(getResult("parl-2024", "no-such-entity"), null);
  assert.equal(getResult("no-such-election", "no-such-entity"), null);
});

test("resultsForElection: 205 rows for a real election, empty array for an unknown one", () => {
  const rows = resultsForElection("parl-2024");
  assert.equal(rows.length, 205);
  assert.ok(rows.every((r) => typeof r.entityId === "string"));
  assert.deepEqual(resultsForElection("no-such-election"), []);
});

test("resultsForEntity: 4 rows for the national total, empty array for an unknown entity", () => {
  const rows = resultsForEntity("LK");
  assert.equal(rows.length, 4);
  assert.ok(rows.every((r) => typeof r.electionId === "string"));
  assert.deepEqual(resultsForEntity("no-such-entity"), []);
});

test("resultsForElection / resultsForEntity are consistent with getResult", () => {
  const direct = getResult("parl-2024", "EC-01");
  const viaElection = resultsForElection("parl-2024").find((r) => r.entityId === "EC-01");
  const viaEntity = resultsForEntity("EC-01").find((r) => r.electionId === "parl-2024");

  assert.ok(direct && viaElection && viaEntity);
  assert.equal(viaElection?.electors, direct?.electors);
  assert.equal(viaElection?.turnout, direct?.turnout);
  assert.deepEqual(viaElection?.winner, direct?.winner);
  assert.deepEqual(viaEntity?.parties, direct?.parties);
});
