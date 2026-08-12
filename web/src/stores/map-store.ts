import { create } from "zustand";

export interface MapViewState {
  /** [lon, lat], matching MapLibre's coordinate order. */
  center: [number, number];
  zoom: number;
}

interface MapStore extends MapViewState {
  setView: (view: Partial<MapViewState>) => void;
}

/** Sri Lanka, per the task brief: lat 7.5, lon 80.7, zoom 7.2. */
export const DEFAULT_MAP_VIEW: MapViewState = {
  center: [80.7, 7.5],
  zoom: 7.2,
};

/**
 * Current map camera. The map itself is the source of truth once mounted
 * (see MapView) — this store exists so the camera can be read outside the
 * map component (initial construction) and mirrored to the URL for
 * shareable links (see useMapUrlSync).
 */
export const useMapStore = create<MapStore>((set) => ({
  ...DEFAULT_MAP_VIEW,
  setView: (view) => set((state) => ({ ...state, ...view })),
}));
