/** A single node in Sri Lanka's administrative hierarchy. */
export interface AdminUnit {
  /** Official p-code (OCHA COD-AB). The only stable join key — never join on names. */
  pcode: string;
  /** 0 country, 1 province, 2 district, 3 DS division, 4 GN division. */
  level: 0 | 1 | 2 | 3 | 4;
  /** English name. */
  name: string;
  /** Sinhala name, when available. */
  nameSi: string | null;
  /** Tamil name, when available. */
  nameTa: string | null;
  /** p-code of the parent unit, or null for the country root. */
  parentPcode: string | null;
  /** Area in square kilometers, when available. */
  areaKm2: number | null;
  /** Centroid coordinate, when available. */
  centroid: { lat: number; lon: number } | null;
}

/** Provenance entry for a dataset bundled with this package. */
export interface Source {
  name: string;
  url: string;
  license: string;
}
