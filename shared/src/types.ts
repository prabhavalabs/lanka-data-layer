/** API envelope — every Lanka Data Layer API response uses this shape. */
export interface Envelope<T> {
  success: boolean;
  message: string;
  payload: T | null;
  meta: ResponseMeta;
}

export interface ResponseMeta {
  data_version: string;
  /** Attribution for every dataset touched by this response. */
  source: SourceAttribution[];
  cursor?: string | null;
}

export interface SourceAttribution {
  name: string;
  url: string;
  license: string;
}

export type AdminLevel = 0 | 1 | 2 | 3 | 4;
export type Lang = "en" | "si" | "ta";
export type Sex = "f" | "m" | "t";

export interface AdminUnit {
  pcode: string;
  level: AdminLevel;
  name: string;
  name_si?: string | null;
  name_ta?: string | null;
  parent_pcode: string | null;
  area_km2: number | null;
  centroid: { lat: number; lon: number } | null;
}

export interface ReverseResult {
  cell_id: number;
  gnd: AdminUnit | null;
  ds_division: AdminUnit | null;
  district: AdminUnit | null;
  province: AdminUnit | null;
  postal_code: string | null;
  nearest_place: { id: number; name: string; distance_m: number } | null;
}

export type ElectionEntityKind = "ED" | "PD" | "POSTAL" | "NATIONAL";
