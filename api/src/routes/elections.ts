import type { Hono } from "hono";
import type Database from "better-sqlite3";
import type { ElectionEntityKind } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotFoundError } from "../lib/errors.ts";

interface ElectionRow {
  id: string;
  type: string | null;
  year: number | null;
  date: string | null;
  label: string | null;
}

interface ElectionResultRow {
  election_id: string;
  entity_id: string;
  electors: number | null;
  polled: number | null;
  valid: number | null;
  rejected: number | null;
  winner_party: string | null;
  winner_votes: number | null;
  results: string;
}

interface ElectionEntityRow {
  entity_id: string;
  kind: ElectionEntityKind;
  name: string | null;
  ed_id: string | null;
}

interface ElectionPartyRow {
  code: string;
  candidate: string | null;
  name: string | null;
  color: string | null;
}

export function mountElectionsRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/elections", (c) => {
    const rows = prepared<ElectionRow>(
      db,
      "SELECT id, type, year, date, label FROM elections ORDER BY year DESC, id",
    ).all();
    const meta = buildMeta(db, ["elections"]);
    return c.json(ok(rows, meta));
  });

  app.get("/v1/elections/:id/results/:entity", (c) => {
    const electionId = c.req.param("id");
    const entityId = c.req.param("entity");

    const resultRow = prepared<ElectionResultRow>(
      db,
      `SELECT election_id, entity_id, electors, polled, valid, rejected, winner_party, winner_votes, results
       FROM election_results WHERE election_id = ? AND entity_id = ?`,
    ).get(electionId, entityId);
    if (!resultRow) {
      throw new NotFoundError(`no results for election "${electionId}" entity "${entityId}"`);
    }

    const election =
      prepared<ElectionRow>(db, "SELECT id, type, year, date, label FROM elections WHERE id = ?").get(electionId) ??
      null;
    const entity =
      prepared<ElectionEntityRow>(
        db,
        "SELECT entity_id, kind, name, ed_id FROM election_entities WHERE entity_id = ?",
      ).get(entityId) ?? null;

    let resultsByCode: Record<string, number> = {};
    try {
      resultsByCode = JSON.parse(resultRow.results) as Record<string, number>;
    } catch {
      resultsByCode = {};
    }

    const partyStmt = prepared<ElectionPartyRow>(
      db,
      "SELECT code, candidate, name, color FROM election_parties WHERE election_id = ? AND code = ?",
    );
    const parties = Object.entries(resultsByCode)
      .map(([code, votes]) => {
        const party = partyStmt.get(electionId, code);
        return {
          code,
          votes,
          candidate: party?.candidate ?? null,
          name: party?.name ?? null,
          color: party?.color ?? null,
        };
      })
      .sort((a, b) => b.votes - a.votes);

    const payload = {
      election: election ? { id: election.id, type: election.type, year: election.year, date: election.date, label: election.label } : null,
      entity: entity ? { id: entity.entity_id, kind: entity.kind, name: entity.name, ed_id: entity.ed_id } : null,
      electors: resultRow.electors,
      polled: resultRow.polled,
      valid: resultRow.valid,
      rejected: resultRow.rejected,
      winner_party: resultRow.winner_party,
      winner_votes: resultRow.winner_votes,
      parties,
    };

    const meta = buildMeta(db, ["elections"]);
    return c.json(ok(payload, meta));
  });
}
