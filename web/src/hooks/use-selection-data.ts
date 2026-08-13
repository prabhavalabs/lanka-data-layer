import * as React from "react";
import type { AdminLevel, AdminUnit, Envelope, ReverseResult, SourceAttribution } from "@lanka-data-layer/shared";

import { apiGet, ApiError } from "@/lib/api";
import type { Selection } from "@/stores/map-store";

/** Unwraps an envelope's payload, throwing the same way apiGetPayload does when it's unexpectedly null (success:true responses from these routes always carry one). */
function unwrap<T>(envelope: Envelope<T>): T {
  if (envelope.payload === null) throw new ApiError("Unexpected null payload", 200);
  return envelope.payload;
}

/**
 * Dedupes a list of source attributions by `name`, keeping the first
 * occurrence — the same dataset never carries conflicting url/license
 * across routes, so first-seen is as good as any. Used to build the card
 * footer's running set as the core response and each secondary section's
 * response land independently.
 */
export function dedupeSources(sources: SourceAttribution[]): SourceAttribution[] {
  const seen = new Map<string, SourceAttribution>();
  for (const s of sources) {
    if (!seen.has(s.name)) seen.set(s.name, s);
  }
  return [...seen.values()];
}

/**
 * A selection's stable refetch identity — same shape for both the core and
 * secondary hooks below. Selections are plain objects rebuilt on every
 * click/search, so this keys off content, not identity: re-selecting the
 * exact same admin unit (e.g. clicking its own breadcrumb crumb) doesn't
 * refetch, but switching to a *different* one of the same type does.
 */
function selectionKey(selection: Selection): string | null {
  if (!selection) return null;
  if (selection.type === "admin") return `admin:${selection.pcode}`;
  if (selection.type === "postal") return `postal:${selection.code}`;
  if (selection.type === "place") return `place:${selection.lat.toFixed(6)},${selection.lon.toFixed(6)}`;
  return `coordinate:${selection.lat.toFixed(6)},${selection.lon.toFixed(6)}`;
}

// --- core -------------------------------------------------------------------
// What `useSelectionCore` fetches: one request per selection (admin/postal/
// reverse per type, same routes the card has always used), enough to render
// the card frame, header, and primary sections the moment it lands.

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

/** One postal code serving an admin unit — DS/GN divisions only, dominant (highest share) first, per `GET /v1/admin/:pcode`'s `postal_codes`. */
export interface AdminPostalCode {
  code: string;
  share: number;
}

export interface AdminSelectionData {
  type: "admin";
  unit: AdminUnit;
  level: AdminLevel;
  parentChain: AdminUnit[];
  children: ChildSummary[];
  population: AdminPopulation | null;
  stats: AdminStats | null;
  postalCodes: AdminPostalCode[];
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
  postal_codes?: AdminPostalCode[];
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
 *
 * This is the "core" query in the card's progressive-loading architecture:
 * it alone renders the card frame, header, and primary sections.
 * `useSelectionSecondary` below fires once this resolves and fills in the
 * rest.
 */
export function useSelectionCore(selection: Selection): SelectionDataState {
  const [state, setState] = React.useState<SelectionDataState>({ status: "idle" });
  const [retryToken, setRetryToken] = React.useState(0);

  const key = selectionKey(selection);

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
          postalCodes: payload.postal_codes ?? [],
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
    // identity for refetch purposes (see selectionKey above), and the
    // branches above still close over the real `selection` object for its
    // fields.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, retryToken]);

  return state;
}

// --- secondary ----------------------------------------------------------------
// What `useSelectionSecondary` fetches: everything the core query doesn't
// cover, dispatched once the core lands and staggered so the card visibly
// fills in section by section rather than jumping straight to "done".

/** A named place near a point, with its distance — the shape `ReverseResult.nearest_place` and the "AREA CONTEXT" / "GND context" secondary sections share. */
export interface NearestPlaceInfo {
  name: string;
  distance_m: number;
}

/** The four admin levels a point resolves to — same shape `ReverseResult` carries, factored out so both the core admin hierarchy section and the postal "GND context" secondary section can build a breadcrumb from it. */
export interface AdminChain {
  province: AdminUnit | null;
  district: AdminUnit | null;
  ds_division: AdminUnit | null;
  gnd: AdminUnit | null;
}

/** "AREA CONTEXT" — population within 5km of a point, plus (admin selections only) the nearest named place to that point. Used by every selection type; `nearestPlace` is only ever populated for admin (see `areaContextJob`). */
export interface AreaContextSecondaryData {
  kind: "area-context";
  populationRadius: number | null;
  nearestPlace: NearestPlaceInfo | null;
}

/** Postal-only: fills in the administrative hierarchy (and/or a nearest-place hint) when the core `/v1/postal/:code` response couldn't resolve one — see `buildSecondaryJobs`'s postal branch. */
export interface GndContextSecondaryData {
  kind: "gnd-context";
  chain: AdminChain;
  nearestPlace: NearestPlaceInfo | null;
}

export type SecondaryData = AreaContextSecondaryData | GndContextSecondaryData;

export interface SecondarySection {
  key: string;
  status: "loading" | "ready";
  /** Only set once `status` is "ready". */
  data: SecondaryData | null;
  /** Only set once `status` is "ready" — this section's own response's attribution, per the task brief's per-section source tag. */
  source: SourceAttribution[];
}

interface SecondaryJobResult {
  data: SecondaryData;
  source: SourceAttribution[];
}

interface SecondaryJob {
  key: string;
  run: (signal: AbortSignal) => Promise<SecondaryJobResult | null>;
}

/**
 * Fetches `path` and returns its payload + source attribution, or `null` on
 * *any* failure — a 404, a network error, an abort, a null payload. Every
 * secondary section is best-effort enrichment (per the task brief: 404s and
 * errors render nothing, never an error block), so every failure mode
 * collapses to the same "this section doesn't get to exist" signal.
 */
async function fetchEnrichment<T>(
  path: string,
  params: Record<string, string | number | boolean | undefined>,
  signal: AbortSignal
): Promise<{ payload: T; source: SourceAttribution[] } | null> {
  try {
    const envelope = await apiGet<T>(path, { params, signal });
    if (envelope.payload === null) return null;
    return { payload: envelope.payload, source: envelope.meta.source };
  } catch {
    return null;
  }
}

const AREA_CONTEXT_RADIUS_KM = 5;

interface PopulationPayload {
  radius: { radius_km: number; population: number; cell_count: number } | null;
}

/**
 * "AREA CONTEXT": `GET /v1/population?radius=5` always; `GET /v1/reverse`
 * for the nearest named place too when `includeNearestPlace` (admin
 * selections only — postal/place/coordinate selections already show a
 * nearest place or don't need one, see `buildSecondaryJobs`). Both calls
 * run in parallel rather than one waiting on the other, since they're
 * independent facts about the same point; the section itself still reads
 * as one unit that becomes ready once both have settled (or been dropped).
 */
function areaContextJob(key: string, lat: number, lon: number, includeNearestPlace: boolean): SecondaryJob {
  return {
    key,
    run: async (signal) => {
      const [pop, nearest] = await Promise.all([
        fetchEnrichment<PopulationPayload>("/population", { lat, lon, radius: AREA_CONTEXT_RADIUS_KM }, signal),
        includeNearestPlace ? fetchEnrichment<ReverseResult>("/reverse", { lat, lon }, signal) : Promise.resolve(null),
      ]);

      const populationRadius = pop?.payload.radius?.population ?? null;
      const nearestPlace = nearest?.payload.nearest_place
        ? { name: nearest.payload.nearest_place.name, distance_m: nearest.payload.nearest_place.distance_m }
        : null;
      if (populationRadius === null && nearestPlace === null) return null;

      return {
        data: { kind: "area-context", populationRadius, nearestPlace },
        source: dedupeSources([...(pop?.source ?? []), ...(nearest?.source ?? [])]),
      };
    },
  };
}

/**
 * Postal-only "GND context": re-runs `GET /v1/reverse` at the postal
 * record's own point when the core response's admin chain came back empty
 * (see `buildSecondaryJobs`) — picks up a chain and/or a nearest-place
 * fallback the core `/v1/postal/:code` route doesn't attempt on its own.
 * Drops silently if this still resolves to nothing.
 */
function gndContextJob(key: string, lat: number, lon: number): SecondaryJob {
  return {
    key,
    run: async (signal) => {
      const result = await fetchEnrichment<ReverseResult>("/reverse", { lat, lon }, signal);
      if (!result) return null;

      const { gnd, ds_division, district, province, nearest_place } = result.payload;
      const chain: AdminChain = { gnd, ds_division, district, province };
      const hasChain = gnd !== null || ds_division !== null || district !== null || province !== null;
      const nearestPlace = nearest_place ? { name: nearest_place.name, distance_m: nearest_place.distance_m } : null;
      if (!hasChain && !nearestPlace) return null;

      return { data: { kind: "gnd-context", chain, nearestPlace }, source: result.source };
    },
  };
}

/** The ordered list of secondary jobs for a resolved core selection — see selection-sections.tsx's per-type build notes in the task brief this mirrors. Order here is dispatch order (staggered ~150ms apart by the hook below). */
function buildSecondaryJobs(data: SelectionData): SecondaryJob[] {
  if (data.type === "admin") {
    if (!data.unit.centroid) return [];
    return [areaContextJob("area-context", data.unit.centroid.lat, data.unit.centroid.lon, true)];
  }
  if (data.type === "postal") {
    if (data.lat == null || data.lon == null) return [];
    const jobs = [areaContextJob("area-context", data.lat, data.lon, false)];
    const chainMissing = !data.gnd && !data.ds_division && !data.district && !data.province;
    if (chainMissing) jobs.push(gndContextJob("gnd-context", data.lat, data.lon));
    return jobs;
  }
  if (data.type === "place") {
    return [areaContextJob("area-context", data.lat, data.lon, false)];
  }
  return [areaContextJob("area-context", data.lat, data.lon, false)];
}

const SECONDARY_STAGGER_MS = 150;

/**
 * Progressive enrichment: the sections `useSelectionCore` doesn't cover.
 * Fires once `core` lands successfully, one job per applicable secondary
 * section (`buildSecondaryJobs`), each dispatched `SECONDARY_STAGGER_MS`
 * apart so they visibly trickle in rather than popping in all at once —
 * every job still runs independently (nothing waits for an earlier job to
 * finish, just to *start* later), so a slow section never blocks a fast
 * one behind it.
 *
 * Every returned section starts "loading" the moment the job list is known
 * (immediately after core resolves) so its skeleton is visible right away;
 * it either flips to "ready" or is dropped from the list entirely (see
 * `fetchEnrichment` — best-effort, no error state for enrichment data).
 * Selection changes (and unmount) abort every in-flight job and cancel any
 * pending dispatch timers via the effect's cleanup.
 */
export function useSelectionSecondary(selection: Selection, core: SelectionDataState): SecondarySection[] {
  const [sections, setSections] = React.useState<SecondarySection[]>([]);
  const key = selectionKey(selection);

  React.useEffect(() => {
    if (core.status !== "success") {
      setSections([]);
      return;
    }

    const jobs = buildSecondaryJobs(core.data);
    setSections(jobs.map((job) => ({ key: job.key, status: "loading", data: null, source: [] })));
    if (jobs.length === 0) return;

    const controller = new AbortController();
    let cancelled = false;
    const timers = jobs.map((job, i) =>
      setTimeout(() => {
        if (cancelled) return;
        job.run(controller.signal).then((result) => {
          if (cancelled) return;
          setSections((prev) => {
            if (!result) return prev.filter((s) => s.key !== job.key);
            return prev.map((s) =>
              s.key === job.key ? { key: job.key, status: "ready", data: result.data, source: result.source } : s
            );
          });
        });
      }, i * SECONDARY_STAGGER_MS)
    );

    return () => {
      cancelled = true;
      controller.abort();
      timers.forEach(clearTimeout);
    };
    // `core` is read via closure the same way `selection` is in
    // useSelectionCore above — `key` plus `core.status` (loading -> success,
    // or a Retry replaying error -> success) is the stable trigger for
    // "the core payload this should build jobs from just changed", not
    // `core`'s object identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, core.status]);

  return sections;
}
