import * as React from "react";
import type { AdminLevel, AdminUnit, Envelope, ReverseResult, SourceAttribution } from "@lanka-data-layer/shared";

import { apiGet, ApiError } from "@/lib/api";
import type { Selection } from "@/stores/map-store";

/** Unwraps an envelope's payload, throwing the same way apiGetPayload does when it's unexpectedly null (success:true responses from these routes always carry one). */
function unwrap<T>(envelope: Envelope<T>): T {
  if (envelope.payload === null) throw new ApiError("Unexpected null payload", 200);
  return envelope.payload;
}

export type Sex = "f" | "m" | "t";
export type SexBreakdown = Record<Sex, number>;

export interface AdminPopulation {
  year: number;
  buckets: Record<string, SexBreakdown>;
  total: SexBreakdown | null;
}

export interface AdminStats {
  year: number;
  /** Flat key/value, e.g. `"ethnicity.sinhala": 1732530`. */
  values: Record<string, number>;
}

export interface ChildSummary {
  pcode: string;
  name: string;
}

export interface AdminSelectionData {
  type: "admin";
  unit: AdminUnit;
  level: AdminLevel;
  parentChain: AdminUnit[];
  children: ChildSummary[];
  population: AdminPopulation | null;
  stats: AdminStats | null;
  source: SourceAttribution[];
}

export interface PostalSelectionData {
  type: "postal";
  code: string;
  name: string;
  lat: number | null;
  lon: number | null;
  gnd: AdminUnit | null;
  ds_division: AdminUnit | null;
  district: AdminUnit | null;
  province: AdminUnit | null;
  source: SourceAttribution[];
}

export interface PlaceSelectionData {
  type: "place";
  label: string;
  sublabel: string;
  lat: number;
  lon: number;
  pcode?: string;
  reverse: ReverseResult;
  source: SourceAttribution[];
}

export interface CoordinateSelectionData {
  type: "coordinate";
  lat: number;
  lon: number;
  reverse: ReverseResult;
  source: SourceAttribution[];
}

export type SelectionData = AdminSelectionData | PostalSelectionData | PlaceSelectionData | CoordinateSelectionData;

export type SelectionDataState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; error: ApiError; retry: () => void }
  | { status: "success"; data: SelectionData };

interface AdminPayload {
  unit: AdminUnit;
  parent_chain: AdminUnit[];
  children: ChildSummary[];
  population?: AdminPopulation | null;
  stats?: AdminStats | null;
}

interface PostalPayload {
  code: string;
  name: string;
  lat: number | null;
  lon: number | null;
  gnd: AdminUnit | null;
  ds_division: AdminUnit | null;
  district: AdminUnit | null;
  province: AdminUnit | null;
}

/**
 * Fetches whatever `GET /v1/admin/:pcode`, `GET /v1/postal/:code`, or
 * `GET /v1/reverse` returns for the current `selection` (docs/architecture.md
 * §4 — see the route handlers this mirrors: api/src/routes/{admin,postal,
 * reverse}.ts). One request per selection change; a stale in-flight
 * request is aborted before the next one starts, same pattern as
 * useLookupSuggestions.
 */
export function useSelectionData(selection: Selection): SelectionDataState {
  const [state, setState] = React.useState<SelectionDataState>({ status: "idle" });
  const [retryToken, setRetryToken] = React.useState(0);

  // Selections are plain objects rebuilt on every click/search — key off
  // their content, not identity, so re-selecting the exact same admin unit
  // (e.g. clicking its own breadcrumb crumb) doesn't refetch, but switching
  // to a *different* one of the same type does.
  const key = selection
    ? selection.type === "admin"
      ? `admin:${selection.pcode}`
      : selection.type === "postal"
        ? `postal:${selection.code}`
        : selection.type === "place"
          ? `place:${selection.lat.toFixed(6)},${selection.lon.toFixed(6)}`
          : `coordinate:${selection.lat.toFixed(6)},${selection.lon.toFixed(6)}`
    : null;

  React.useEffect(() => {
    if (!selection) {
      setState({ status: "idle" });
      return;
    }

    setState({ status: "loading" });
    const controller = new AbortController();

    async function run(): Promise<SelectionData> {
      if (!selection) throw new Error("unreachable");
      if (selection.type === "admin") {
        const envelope = await apiGet<AdminPayload>(`/admin/${encodeURIComponent(selection.pcode)}`, {
          params: { include: "population,stats" },
          signal: controller.signal,
        });
        const payload = unwrap(envelope);
        return {
          type: "admin",
          unit: payload.unit,
          level: payload.unit.level,
          parentChain: payload.parent_chain,
          children: payload.children,
          population: payload.population ?? null,
          stats: payload.stats ?? null,
          source: envelope.meta.source,
        };
      }
      if (selection.type === "postal") {
        const envelope = await apiGet<PostalPayload>(`/postal/${encodeURIComponent(selection.code)}`, {
          signal: controller.signal,
        });
        const payload = unwrap(envelope);
        return { type: "postal", ...payload, source: envelope.meta.source };
      }
      if (selection.type === "place") {
        const envelope = await apiGet<ReverseResult>("/reverse", {
          params: { lat: selection.lat, lon: selection.lon },
          signal: controller.signal,
        });
        return {
          type: "place",
          label: selection.label,
          sublabel: selection.sublabel,
          lat: selection.lat,
          lon: selection.lon,
          pcode: selection.pcode,
          reverse: unwrap(envelope),
          source: envelope.meta.source,
        };
      }
      const envelope = await apiGet<ReverseResult>("/reverse", {
        params: { lat: selection.lat, lon: selection.lon },
        signal: controller.signal,
      });
      return {
        type: "coordinate",
        lat: selection.lat,
        lon: selection.lon,
        reverse: unwrap(envelope),
        source: envelope.meta.source,
      };
    }

    run()
      .then((data) => setState({ status: "success", data }))
      .catch((err: unknown) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
        const apiErr = err instanceof ApiError ? err : new ApiError(String(err), 0);
        setState({ status: "error", error: apiErr, retry: () => setRetryToken((t) => t + 1) });
      });

    return () => controller.abort();
    // `selection` itself is intentionally omitted — `key` is its stable
    // identity for refetch purposes (see comment above), and the branches
    // above still close over the real `selection` object for its fields.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, retryToken]);

  return state;
}
