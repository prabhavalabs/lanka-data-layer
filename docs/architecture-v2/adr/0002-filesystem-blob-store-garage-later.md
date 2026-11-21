# ADR 0002 — Filesystem blob store behind a trait; Garage when a threshold is crossed

Status: proposed · 2026-11-21 · Detail: [01](../01-layers.md#object-store-decision-filesystem-now-garage-when-a-threshold-is-crossed)

## Context

Raw artifacts, extracts, grid arrays, tiles and distributions are files. No Cloudflare R2; any object storage must be self-hostable. Today there is one host and one disk.

## Decision

`/srv/lanka/store` on local NVMe, content-addressed for `raw/` and `extract/`, immutable named keys elsewhere, accessed only through the `BlobStore` trait (`FsStore`). nginx serves `datasets/`, `releases/`, `tiles/` directly. An `S3Store` implementation targets **Garage** and is built only when a trigger fires: a second host needs blobs, the store exceeds 100 GB, or downloads exceed 30 % of bandwidth.

## Consequences

- Zero extra processes now; `sendfile`, `mmap` and restic work directly.
- Code never assumes a path except through `local_path()`, which returns `None` on S3; grids are then synced to a local cache.
- Durability on one disk depends entirely on the off-site backup (ADR 0016 exposes backup freshness).

## Alternatives rejected

- **MinIO now**: a daemon on the same disk adds a failure mode and no durability; its community distribution has narrowed (to re-verify when WP-23 starts).
- **SeaweedFS**: three roles (master, volume, filer) for a problem we do not have.
- **Garage from day one**: correct later, premature on one node.
- **Files in Postgres (bytea / large objects)**: bloats backups and WAL, no range requests, no mmap.
