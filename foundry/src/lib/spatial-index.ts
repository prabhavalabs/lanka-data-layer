/**
 * Hand-rolled grid-bucket spatial index. No external spatial library (per
 * the "no turf/spatial deps" convention set by lib/geo.ts) — just a map from
 * (bucket-x, bucket-y) to the items whose bbox overlaps that bucket. Used by
 * `cell-lookup` to avoid O(polygons) or O(points) work per cell: ~5M cells x
 * a handful of candidates per cell, instead of ~5M x thousands.
 */
import type { BBox } from "./geo.ts";

export interface GridIndexOptions {
  /** Bucket edge length, in degrees. Pick roughly the median item size. */
  cellSizeDeg: number;
}

/** Buckets bounding boxes (polygons) or single points (bbox with zero area). */
export class GridIndex<T> {
  private readonly cellSize: number;
  private readonly buckets = new Map<string, { value: T; bbox: BBox }[]>();

  constructor(opts: GridIndexOptions) {
    this.cellSize = opts.cellSizeDeg;
  }

  private key(bx: number, by: number): string {
    return `${bx},${by}`;
  }

  private bucketRange(bbox: BBox): { bx0: number; bx1: number; by0: number; by1: number } {
    return {
      bx0: Math.floor(bbox.west / this.cellSize),
      bx1: Math.floor(bbox.east / this.cellSize),
      by0: Math.floor(bbox.south / this.cellSize),
      by1: Math.floor(bbox.north / this.cellSize),
    };
  }

  /** Registers `value` (with bounding box `bbox`) into every bucket it overlaps. */
  insert(bbox: BBox, value: T): void {
    const { bx0, bx1, by0, by1 } = this.bucketRange(bbox);
    for (let bx = bx0; bx <= bx1; bx++) {
      for (let by = by0; by <= by1; by++) {
        const k = this.key(bx, by);
        let bucket = this.buckets.get(k);
        if (!bucket) {
          bucket = [];
          this.buckets.set(k, bucket);
        }
        bucket.push({ value, bbox });
      }
    }
  }

  /** Items registered in the single bucket containing (lon, lat). */
  queryPoint(lon: number, lat: number): T[] {
    const bx = Math.floor(lon / this.cellSize);
    const by = Math.floor(lat / this.cellSize);
    const bucket = this.buckets.get(this.key(bx, by));
    return bucket ? bucket.map((b) => b.value) : [];
  }

  /**
   * Items registered in the square ring of buckets at Chebyshev distance
   * exactly `radius` from (lon, lat)'s own bucket (radius 0 = queryPoint).
   * Used to expand a search outward one ring at a time without rescanning
   * already-visited buckets.
   */
  queryRing(lon: number, lat: number, radius: number): T[] {
    const cx = Math.floor(lon / this.cellSize);
    const cy = Math.floor(lat / this.cellSize);
    if (radius === 0) return this.queryPoint(lon, lat);
    const out: T[] = [];
    for (let bx = cx - radius; bx <= cx + radius; bx++) {
      for (let by = cy - radius; by <= cy + radius; by++) {
        const onRing = bx === cx - radius || bx === cx + radius || by === cy - radius || by === cy + radius;
        if (!onRing) continue;
        const bucket = this.buckets.get(this.key(bx, by));
        if (bucket) for (const b of bucket) out.push(b.value);
      }
    }
    return out;
  }

  /** Bucket edge length in degrees (exposed so callers can size distance-based stop conditions). */
  get cellSizeDeg(): number {
    return this.cellSize;
  }
}
