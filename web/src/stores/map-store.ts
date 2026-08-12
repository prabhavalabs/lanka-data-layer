import { create } from "zustand";

export interface MapViewState {
  /** [lon, lat], matching MapLibre's coordinate order. */
  center: [number, number];
  zoom: number;
}

/**
 * What the map should highlight right now. `geometry` renders a glowing
 * outline + soft fill and fits the camera to its bounds (admin boundaries,
 * fetched via `GET /v1/admin/:pcode/geometry` per docs/architecture.md §4);
 * `point` renders a pulsing marker and flies to it at zoom ~13 (places,
 * postal codes, coordinate lookups — anything that's just a lat/lon).
 */
export type MapHighlight =
  | { kind: "geometry"; feature: GeoJSON.Feature; label?: string }
  | { kind: "point"; lat: number; lon: number; label?: string }
  | null;

export interface FlyToTarget {
  lat: number;
  lon: number;
  zoom?: number;
}

interface FlyToRequest extends FlyToTarget {
  /** Distinguishes repeat requests to the same coordinates so MapView's effect re-fires even when lat/lon/zoom are unchanged. */
  requestId: number;
}

interface MapStore extends MapViewState {
  highlight: MapHighlight;
  /** Internal — MapView subscribes to this to drive the camera; call `flyTo` instead of setting it directly. */
  flyToRequest: FlyToRequest | null;
  setView: (view: Partial<MapViewState>) => void;
  /** Moves the camera to (lat, lon), keeping the current zoom unless one is given. Consumed by MapView; other features (search, lookup results, etc.) call this directly. */
  flyTo: (target: FlyToTarget) => void;
  /** Sets (or clears, with `null`) what the map highlights. MapView reacts by drawing the highlight and auto-fitting/flying the camera to it; clears on map click. */
  setHighlight: (highlight: MapHighlight) => void;
}

/** Sri Lanka, per the task brief: lat 7.5, lon 80.7, zoom 7.2. */
export const DEFAULT_MAP_VIEW: MapViewState = {
  center: [80.7, 7.5],
  zoom: 7.2,
};

let flyToCounter = 0;

/**
 * Current map camera + highlight. The map itself is the source of truth for
 * camera position once mounted (see MapView) — this store exists so the
 * camera can be read outside the map component (initial construction,
 * useMapUrlSync) and so other features can drive it (flyTo, setHighlight)
 * without reaching into MapView's internals.
 */
export const useMapStore = create<MapStore>((set) => ({
  ...DEFAULT_MAP_VIEW,
  highlight: null,
  flyToRequest: null,
  setView: (view) => set((state) => ({ ...state, ...view })),
  flyTo: (target) => set({ flyToRequest: { ...target, requestId: ++flyToCounter } }),
  setHighlight: (highlight) => set({ highlight }),
}));
