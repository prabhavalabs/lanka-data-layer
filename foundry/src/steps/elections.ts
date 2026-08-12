import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { classifyEntityKind, edIdOf } from "../lib/election-entities.ts";

export const name = "elections";

interface PartyRef {
  party: string;
  votes: number;
  pct?: number;
}
interface EntityResult {
  valid: number;
  rejected: number;
  polled: number;
  electors: number;
  turnoutPct?: number;
  winner?: PartyRef;
  runnerUp?: PartyRef;
  margin?: number;
  topParties?: PartyRef[];
  otherVotes?: number;
}
interface ElectionSource {
  id: string;
  type: string;
  year: number;
  date: string;
  label: string;
  parties: Record<string, { candidate: string; name: string; color: string }>;
  entities: Record<string, EntityResult>;
}
interface GeoFeature {
  properties: { id: string; name: string };
}
interface FeatureCollection {
  features: GeoFeature[];
}

const ELECTION_FILES = ["election-pres-2024.json", "election-pres-2019.json", "election-pres-2015.json", "election-parl-2024.json"];

async function readJson<T>(relPath: string): Promise<T> {
  return JSON.parse(await readFile(rawPath(relPath), "utf8")) as T;
}

/**
 * elections + election_parties + election_entities + election_results.
 * Entity kind classification (ED/PD/POSTAL/NATIONAL) is in
 * lib/election-entities.ts, unit-tested separately. election_entities is
 * the same 205-entity set across all four source files, so it is built
 * once and upserted; election_results carries one row per (election, entity).
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const eds = await readJson<FeatureCollection>("geo/electoral-divisions.geojson");
  const pds = await readJson<FeatureCollection>("geo/polling-divisions.geojson");
  const edNameById = new Map(eds.features.map((f) => [f.properties.id, f.properties.name]));
  const pdNameById = new Map(pds.features.map((f) => [f.properties.id, f.properties.name]));

  const insertElection = db.prepare(`
    INSERT INTO elections (id, type, year, date, label) VALUES (@id, @type, @year, @date, @label)
    ON CONFLICT(id) DO UPDATE SET type=excluded.type, year=excluded.year, date=excluded.date, label=excluded.label
  `);
  const insertParty = db.prepare(`
    INSERT INTO election_parties (election_id, code, candidate, name, color)
    VALUES (@election_id, @code, @candidate, @name, @color)
    ON CONFLICT(election_id, code) DO UPDATE SET candidate=excluded.candidate, name=excluded.name, color=excluded.color
  `);
  const insertEntity = db.prepare(`
    INSERT INTO election_entities (entity_id, kind, name, ed_id) VALUES (@entity_id, @kind, @name, @ed_id)
    ON CONFLICT(entity_id) DO UPDATE SET kind=excluded.kind, name=excluded.name, ed_id=excluded.ed_id
  `);
  const insertResult = db.prepare(`
    INSERT INTO election_results (election_id, entity_id, electors, polled, valid, rejected, winner_party, winner_votes, results)
    VALUES (@election_id, @entity_id, @electors, @polled, @valid, @rejected, @winner_party, @winner_votes, @results)
    ON CONFLICT(election_id, entity_id) DO UPDATE SET
      electors=excluded.electors, polled=excluded.polled, valid=excluded.valid, rejected=excluded.rejected,
      winner_party=excluded.winner_party, winner_votes=excluded.winner_votes, results=excluded.results
  `);

  let elections = 0;
  let parties = 0;
  let results = 0;
  const seenEntityIds = new Set<string>();

  const tx = db.transaction((source: ElectionSource) => {
    insertElection.run({ id: source.id, type: source.type, year: source.year, date: source.date, label: source.label });
    elections++;

    for (const [code, p] of Object.entries(source.parties)) {
      insertParty.run({ election_id: source.id, code, candidate: p.candidate, name: p.name, color: p.color });
      parties++;
    }

    for (const [entityId, result] of Object.entries(source.entities)) {
      const kind = classifyEntityKind(entityId);
      const edId = edIdOf(entityId, kind);
      let entityName: string;
      if (kind === "NATIONAL") entityName = "Sri Lanka";
      else if (kind === "ED") entityName = edNameById.get(entityId) ?? entityId;
      else if (kind === "PD") entityName = pdNameById.get(entityId) ?? entityId;
      else {
        // POSTAL: no direct source name — derive from the parent ED's name.
        const edName = edId ? (edNameById.get(edId) ?? edId) : entityId;
        entityName = `${edName} Postal`;
      }
      insertEntity.run({ entity_id: entityId, kind, name: entityName, ed_id: edId });
      seenEntityIds.add(entityId);

      insertResult.run({
        election_id: source.id,
        entity_id: entityId,
        electors: result.electors ?? null,
        polled: result.polled ?? null,
        valid: result.valid ?? null,
        rejected: result.rejected ?? null,
        winner_party: result.winner?.party ?? null,
        winner_votes: result.winner?.votes ?? null,
        results: JSON.stringify(result),
      });
      results++;
    }
  });

  for (const file of ELECTION_FILES) {
    const source = await readJson<ElectionSource>(`data/${file}`);
    tx(source);
  }

  log(
    `elections: upserted ${elections} elections, ${parties} election_parties, ` +
      `${seenEntityIds.size} election_entities, ${results} election_results`,
  );
}
