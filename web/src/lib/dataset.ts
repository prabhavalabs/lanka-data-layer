/**
 * Shape of a row from the `datasets` table (docs/architecture.md §2),
 * as returned by `GET /v1/datasets`.
 *
 * This mirrors the SQLite schema, not a type from @lanka-data-layer/shared — the
 * shared package does not yet export API response types for this route.
 * If/when it does, prefer that export over this local copy.
 */
export interface Dataset {
  id: string;
  title: string;
  category: string;
  description: string | null;
  source_name: string;
  source_url: string;
  license: string;
  feature_count: number | null;
  download_path: string | null;
}
