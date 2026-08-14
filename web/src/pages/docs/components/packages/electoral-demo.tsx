import * as React from "react";
import { getResult, listElections, listEntities } from "@lanka-data-layer/electoral";

import { EmptyHint, ShareBar, Stat } from "@/pages/docs/components/packages/shared";

const selectClass =
  "h-9 min-w-0 flex-1 rounded-[7px] border border-border bg-bg px-2.5 font-mono text-[12px] text-ink outline-none focus-visible:border-brand";

function formatPct(value: number | null): string {
  return value == null ? "—" : `${(value * 100).toFixed(1)}%`;
}

/**
 * Live `@lanka-data-layer/electoral` demo: pick an election and an entity
 * (electoral district, polling division, postal-vote bucket, or the
 * national total), then render getResult()'s turnout/winner/party-share
 * breakdown as a table with vote-share bars. Defaults to the most recent
 * election and the national ("LK") entity.
 */
export function ElectoralDemo() {
  const elections = React.useMemo(() => listElections(), []);
  const entities = React.useMemo(() => listEntities(), []);

  const [electionId, setElectionId] = React.useState(elections[0]?.id ?? "");
  const [entityId, setEntityId] = React.useState(() => entities.find((e) => e.kind === "NATIONAL")?.id ?? entities[0]?.id ?? "");

  const result = React.useMemo(() => getResult(electionId, entityId), [electionId, entityId]);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap gap-2">
        <select value={electionId} onChange={(e) => setElectionId(e.target.value)} className={selectClass} aria-label="Election">
          {elections.map((e) => (
            <option key={e.id} value={e.id}>
              {e.label}
            </option>
          ))}
        </select>
        <select value={entityId} onChange={(e) => setEntityId(e.target.value)} className={selectClass} aria-label="Entity">
          {entities.map((e) => (
            <option key={e.id} value={e.id}>
              {e.name} · {e.kind}
            </option>
          ))}
        </select>
      </div>

      {!result && <EmptyHint>No recorded result for this election/entity pairing.</EmptyHint>}

      {result && (
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <Stat label="Turnout" value={formatPct(result.turnout)} />
            <Stat label="Electors" value={result.electors.toLocaleString()} />
            <Stat label="Valid votes" value={result.valid.toLocaleString()} />
            <Stat label="Winner" value={result.winner ? result.winner.party : "—"} />
          </div>

          <div className="flex flex-col gap-2">
            {result.parties.map((p) => (
              <ShareBar
                key={p.party}
                label={p.party}
                share={p.share}
                sublabel={`${p.votes.toLocaleString()} votes`}
                tone={result.winner?.party === p.party ? "brand" : "default"}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
