import { populationRampT } from "@/components/minimap/ramp";
import { scatterPoints } from "@/components/minimap/circle";
import { tileBoundsFeature } from "@/components/minimap/tile-math";
import { geometryBounds } from "@/lib/geojson-bounds";

export type MiniMapView = "lookup" | "reverse" | "postal" | "admin" | "geometry" | "population" | "grid" | "elections" | "tiles";

export interface MiniMapMarker {
  lon: number;
  lat: number;
  label?: string;
  primary?: boolean;
}

export interface MiniMapChip {
  lon: number;
  lat: number;
  text: string;
}

export interface MiniMapRampDot {
  lon: number;
  lat: number;
  t: number;
}

export interface MiniMapScene {
  view: MiniMapView;
  center: [number, number];
  zoom: number;
  markers: MiniMapMarker[];
  chips: MiniMapChip[];
  boundary?: GeoJSON.Feature | null;
  /** Set when the boundary isn't in the response itself and needs a follow-up GET /v1/admin/:pcode/geometry — mirrors docs-demo-page.tsx's existing pattern. */
  boundaryPcode?: string;
  circle?: { lon: number; lat: number; radiusKm: number; dashed: boolean };
  rampDots: MiniMapRampDot[];
  isGrid?: boolean;
  fillColor?: string;
  tileRect?: GeoJSON.Feature | null;
}

/**
 * Every docs-spec slug's MiniMap view — null means "this endpoint has no
 * map" (catalog/liveness/binary routes). `tiles` is null here deliberately:
 * the design's mock illustrates a per-tile `/v1/tiles/{layer}/{z}/{x}/{y}`
 * endpoint with real z/x/y path params (see tile-math.ts, kept for that
 * shape), but our actual `/v1/tiles/:file` is a static whole-archive PMTiles
 * passthrough with no coordinates in it at all — there's nothing honest to
 * draw from a live request to it, so the Try panel skips the map section
 * for this endpoint rather than show a "send to see it" hint that can never
 * be fulfilled.
 */
export const ENDPOINT_MINIMAP_VIEW: Record<string, MiniMapView | null> = {
  health: null,
  datasets: null,
  tiles: null,
  lookup: "lookup",
  search: "lookup",
  reverse: "reverse",
  "postal-code": "postal",
  "postal-point": "postal",
  "admin-unit": "admin",
  "admin-geometry": "geometry",
  population: "population",
  "population-grid": "grid",
  elections: null,
  "election-results": "elections",
};

const ISLAND_CENTER: [number, number] = [80.7, 7.5];

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function num(v: unknown): number | null {
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

/** Digs a lat/lon pair out of a lookup/search result row, tolerant of both the full-mode `{type,score,payload:{lat,lon}}` shape and the flat suggest-mode / search shape. */
function rowLatLon(row: unknown): { lat: number; lon: number } | null {
  if (!isRecord(row)) return null;
  const direct = num(row.lat);
  const directLon = num(row.lon);
  if (direct !== null && directLon !== null) return { lat: direct, lon: directLon };
  const payload = row.payload;
  if (isRecord(payload)) {
    const lat = num(payload.lat);
    const lon = num(payload.lon);
    if (lat !== null && lon !== null) return { lat, lon };
  }
  return null;
}

function rowLabel(row: unknown): string | undefined {
  if (!isRecord(row)) return undefined;
  if (typeof row.label === "string") return row.label;
  const payload = row.payload;
  if (isRecord(payload) && typeof payload.name === "string") return payload.name;
  if (typeof row.name === "string") return row.name;
  return undefined;
}

/**
 * Builds a MiniMapScene from a live Try-panel response (or the home page's
 * live /v1/population/grid fetch). `requestValues` covers the endpoints
 * whose payload doesn't echo back the coordinates the caller sent
 * (population, postal-point, tiles) — the same values the playground/curl
 * line already built the request from.
 */
export function buildMiniMapScene(slug: string, payload: unknown, requestValues: Record<string, string> = {}): MiniMapScene | null {
  const view = ENDPOINT_MINIMAP_VIEW[slug];
  if (!view) return null;

  switch (view) {
    case "lookup": {
      const rows = Array.isArray(payload) ? payload : [];
      const located = rows.map((r) => ({ ll: rowLatLon(r), label: rowLabel(r) })).filter((r) => r.ll !== null);
      if (located.length === 0) return null;
      const [best, ...rest] = located;
      return {
        view,
        center: [best.ll!.lon, best.ll!.lat],
        zoom: 10.5,
        markers: [
          { lon: best.ll!.lon, lat: best.ll!.lat, label: best.label, primary: true },
          ...rest.slice(0, 5).map((r) => ({ lon: r.ll!.lon, lat: r.ll!.lat, label: r.label })),
        ],
        chips: best.label ? [{ lon: best.ll!.lon, lat: best.ll!.lat, text: best.label }] : [],
        rampDots: [],
      };
    }

    case "reverse": {
      const lat = (isRecord(payload) ? num(payload.lat) : null) ?? num(Number(requestValues.lat));
      const lon = (isRecord(payload) ? num(payload.lon) : null) ?? num(Number(requestValues.lon));
      if (lat === null || lon === null) return null;
      const gnd = isRecord(payload) && isRecord(payload.gnd) ? payload.gnd : null;
      const province = isRecord(payload) && isRecord(payload.province) ? payload.province : null;
      const chainLabel = [gnd?.name, province?.name].filter((v) => typeof v === "string").join(" → ");
      return {
        view,
        center: [lon, lat],
        zoom: 11,
        markers: [{ lon, lat, primary: true }],
        chips: [
          { lon, lat, text: `${lat.toFixed(4)}, ${lon.toFixed(4)}` },
          ...(chainLabel ? [{ lon, lat, text: chainLabel }] : []),
        ],
        rampDots: [],
        boundaryPcode: typeof gnd?.pcode === "string" ? gnd.pcode : undefined,
      };
    }

    case "postal": {
      let lat = isRecord(payload) ? num(payload.lat) : null;
      let lon = isRecord(payload) ? num(payload.lon) : null;
      if (lat === null || lon === null) {
        lat = num(Number(requestValues.lat));
        lon = num(Number(requestValues.lon));
      }
      if (lat === null || lon === null) return null;
      const code = isRecord(payload) && typeof payload.code === "string" ? payload.code : requestValues.code;
      const name = isRecord(payload) && typeof payload.name === "string" ? payload.name : undefined;
      return {
        view,
        center: [lon, lat],
        zoom: 11.5,
        markers: [{ lon, lat, primary: true }],
        chips: [{ lon, lat, text: [code, name].filter(Boolean).join(" · ") }],
        circle: { lon, lat, radiusKm: 1.2, dashed: true },
        rampDots: [],
      };
    }

    case "admin": {
      if (!isRecord(payload)) return null;
      const unit = isRecord(payload.unit) ? payload.unit : null;
      const centroid = unit && isRecord(unit.centroid) ? unit.centroid : null;
      const lat = num(centroid?.lat);
      const lon = num(centroid?.lon);
      if (lat === null || lon === null) return null;
      return {
        view,
        center: [lon, lat],
        zoom: 9.5,
        markers: [{ lon, lat, primary: true }],
        chips: [{ lon, lat, text: [unit?.pcode, unit?.name].filter((v) => typeof v === "string").join(" · ") }],
        rampDots: [],
        boundaryPcode: typeof unit?.pcode === "string" ? unit.pcode : undefined,
      };
    }

    case "geometry": {
      // admin-geometry's response IS the bare Feature (responseKind "geojson") — no envelope to unwrap.
      const feature = isRecord(payload) && payload.type === "Feature" ? (payload as unknown as GeoJSON.Feature) : null;
      if (!feature?.geometry) return null;
      const bounds = geometryBounds(feature.geometry);
      if (!bounds) return null;
      const [w, s, e, n] = bounds;
      const center: [number, number] = [(w + e) / 2, (s + n) / 2];
      const props = isRecord(feature.properties) ? feature.properties : {};
      const vertices = typeof props.vertices === "number" ? `${props.vertices} vertices` : undefined;
      return {
        view,
        center,
        zoom: 9.5,
        markers: [],
        chips: vertices ? [{ lon: center[0], lat: center[1], text: vertices }] : [],
        boundary: feature,
        rampDots: [],
      };
    }

    case "population": {
      let lat = num(Number(requestValues.lat));
      let lon = num(Number(requestValues.lon));
      if (isRecord(payload)) {
        // The point/radius payload never echoes lat/lon back, so requestValues (what the form actually sent) is the source of truth here.
        lat = lat ?? num(Number(requestValues.lat));
        lon = lon ?? num(Number(requestValues.lon));
      }
      if (lat === null || lon === null) return null;
      const radiusInfo = isRecord(payload) && isRecord(payload.radius) ? payload.radius : null;
      const radiusKm = num(radiusInfo?.radius_km) ?? num(Number(requestValues.radius)) ?? 5;
      const pointInfo = isRecord(payload) && isRecord(payload.point) ? payload.point : null;
      const pointPop = num(pointInfo?.population) ?? 0;
      const dots = scatterPoints(lat, lon, radiusKm, 16).map((p) => ({
        lon: p.lon,
        lat: p.lat,
        t: Math.max(0.1, Math.min(1, populationRampT(pointPop, 5000) * (0.6 + p.t * 0.4))),
      }));
      return {
        view,
        center: [lon, lat],
        zoom: 11,
        markers: [{ lon, lat, primary: true }],
        chips: [{ lon, lat, text: `r = ${radiusKm} km` }],
        circle: { lon, lat, radiusKm, dashed: false },
        rampDots: dots,
      };
    }

    case "grid": {
      if (!isRecord(payload) || !Array.isArray(payload.cells)) return null;
      const cells = payload.cells as unknown[];
      const maxDots = 3000;
      const step = Math.max(1, Math.ceil(cells.length / maxDots));
      const dots: MiniMapRampDot[] = [];
      let maxPop = 1;
      for (const c of cells) {
        if (Array.isArray(c) && typeof c[2] === "number" && c[2] > maxPop) maxPop = c[2];
      }
      for (let i = 0; i < cells.length; i += step) {
        const c = cells[i];
        if (!Array.isArray(c) || c.length < 3) continue;
        const [lat, lon, pop] = c as [number, number, number];
        if (typeof lat !== "number" || typeof lon !== "number") continue;
        dots.push({ lat, lon, t: populationRampT(pop, Math.min(maxPop, 40000)) });
      }
      return {
        view,
        center: ISLAND_CENTER,
        zoom: 6.5,
        markers: [],
        chips: [],
        rampDots: dots,
        isGrid: true,
      };
    }

    case "elections": {
      if (!isRecord(payload)) return null;
      const entity = isRecord(payload.entity) ? payload.entity : null;
      const parties = Array.isArray(payload.parties) ? payload.parties : [];
      const winner = isRecord(parties[0]) ? parties[0] : null;
      const turnout = num(payload.turnout_pct);
      const fillColor = typeof winner?.color === "string" ? winner.color : undefined;
      const winnerLine = [winner?.code, num(winner?.pct) !== null ? `${winner?.pct}%` : null].filter(Boolean).join(" · ");
      return {
        view,
        center: ISLAND_CENTER,
        zoom: 6.8,
        markers: [{ lon: ISLAND_CENTER[0], lat: ISLAND_CENTER[1], primary: true }],
        chips: [
          { lon: ISLAND_CENTER[0], lat: ISLAND_CENTER[1] + 0.35, text: typeof entity?.name === "string" ? entity.name : "" },
          {
            lon: ISLAND_CENTER[0],
            lat: ISLAND_CENTER[1] - 0.35,
            text: winnerLine || (turnout !== null ? `turnout ${turnout}%` : ""),
          },
        ].filter((c) => c.text),
        rampDots: [],
        fillColor,
      };
    }

    case "tiles": {
      const z = Number(requestValues.z);
      const x = Number(requestValues.x);
      const y = Number(requestValues.y);
      const feature = tileBoundsFeature(z, x, y);
      if (!feature?.geometry || feature.geometry.type !== "Polygon") return null;
      const bounds = geometryBounds(feature.geometry);
      const center: [number, number] = bounds ? [(bounds[0] + bounds[2]) / 2, (bounds[1] + bounds[3]) / 2] : ISLAND_CENTER;
      return {
        view,
        center,
        zoom: Math.max(5, Math.min(10, z - 1)),
        markers: [],
        chips: [{ lon: center[0], lat: center[1], text: `z${z} · x${x} · y${y}` }],
        rampDots: [],
        tileRect: feature,
      };
    }

    default:
      return null;
  }
}
