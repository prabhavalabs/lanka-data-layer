/** A single Sri Lankan postal code / post office. */
export interface PostalCode {
  /** 5-digit postal code. */
  code: string;
  /** Post office / place name. */
  name: string;
  /** District-level (level 2) admin p-code the office falls in, when known. */
  adminPcode: string | null;
  /** Coordinate of the office, when known. */
  location: { lat: number; lon: number } | null;
}

/** Provenance entry for a dataset bundled with this package. */
export interface Source {
  name: string;
  url: string;
  license: string;
}
