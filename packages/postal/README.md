# @lanka-data-layer/postal

Offline lookup library for Sri Lankan postal codes, cross-referenced against
administrative p-codes.

The full dataset (1,833 postal codes, plus a two-way mapping between them
and DS/GN admin divisions) is bundled into the package at build time. There
is no network call, no filesystem read, and no runtime dependency: `import`
it and it works, in Node 18+, in the browser, or on an edge runtime.
Everything after the first lookup call runs off in-memory indexes built
lazily on first use, so the import itself stays cheap.

## Install

```bash
npm install @lanka-data-layer/postal
```

## Quick start

```ts
import {
  getPostalCode,
  searchOffices,
  codesForDivision,
  divisionsForCode,
  nearestPostalCode,
} from "@lanka-data-layer/postal";

getPostalCode("61162");
// { code: "61162", name: "Pothuwatawana", adminPcode: "LK62",
//   location: { lat: 7.358483, lon: 79.922411 } }

searchOffices("galle");
// exact name matches rank above prefix matches above substring matches,
// searched case-insensitively

codesForDivision("LK1103");
// postal codes serving that DS division, e.g.
// [{ code: "01500", share: 0.7394 }, { code: "01300", share: 0.6389 }, …]
// share = fraction of the division's land cells assigned to that code,
// highest share first

divisionsForCode("01500");
// the reverse lookup: every admin division that code serves,
// highest share first

nearestPostalCode(6.9271, 79.8612);
// [{ code: "00200", name: "Slave Island", adminPcode: "LK11",
//    location: { lat: …, lon: … }, distanceKm: 1.21 }]
// haversine distance, closest first; pass { limit } for more than one match
```

### The `PostalCode` type

```ts
interface PostalCode {
  code: string;
  name: string;
  adminPcode: string | null; // district-level (level 2) admin p-code
  location: { lat: number; lon: number } | null;
}
```

### API

| Function | Description |
|---|---|
| `getPostalCode(code)` | Single postal code, or `null`. |
| `searchOffices(q, opts?)` | Name search over post office / place names. `opts.limit` caps result count (default 10). |
| `codesForDivision(pcode)` | Postal codes serving an admin division, by descending share. |
| `divisionsForCode(code)` | Admin divisions a postal code serves, by descending share — the reverse of `codesForDivision`. |
| `nearestPostalCode(lat, lon, opts?)` | Nearest postal codes to a coordinate, closest first. `opts.limit` caps result count (default 1). Distances are haversine kilometers, rounded to 2dp. |
| `DATA_VERSION` | Foundry data release this build was generated from. |
| `CODE_COUNT` | Total number of postal codes bundled (1,833). |
| `SOURCES` | Provenance array — see below. |

## Data

| Source | License |
|---|---|
| GeoNames | CC BY 4.0 |

The exact source list bundled with your installed version is also available
at runtime as `SOURCES`, and its release is stamped in `DATA_VERSION`
(format `YYYYMMDD.N`). Postal codes, coordinates and admin cross-references
are regenerated periodically from the upstream source by the Lanka Data
Layer foundry — the offline ETL pipeline in the
[monorepo](https://github.com/prabhavalabs/lanka-data-layer) this package is
extracted from. To pick up a new data release, upgrade the package; there is
no separate data download step.

Gzipped install size is roughly 160KB (per module format — `import` and
`require` each ship their own bundle with the dataset inlined).

## Part of Lanka Data Layer

This package is a standalone extract of the postal dataset that powers
[Lanka Data Layer](https://github.com/prabhavalabs/lanka-data-layer), an
open geo-data platform for Sri Lanka. If you need more than offline lookups
— reverse geocoding, boundary geometry, population and census indicators,
election results — see the hosted API and interactive docs at
[lanka-data-layer.prabhavalabs.com](https://lanka-data-layer.prabhavalabs.com).

### Development

This package is built from within the `lanka-data-layer` monorepo and has
no dependencies or devDependencies of its own; build tooling (`tsup`,
`typescript`) is provided by the workspace root. See the repository for
build instructions.

## License

MIT © Prabhava Labs
