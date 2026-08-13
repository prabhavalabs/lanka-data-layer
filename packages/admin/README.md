# @lanka-data-layer/admin

Offline lookup library for Sri Lanka's administrative hierarchy — provinces,
districts, DS divisions and GN divisions — keyed by official OCHA COD-AB
p-codes.

The full dataset (14,417 units, in English, Sinhala and Tamil) is bundled
into the package at build time. There is no network call, no filesystem
read, and no runtime dependency: `import` it and it works, in Node 18+, in
the browser, or on an edge runtime. Everything after the first lookup call
runs off in-memory indexes built lazily on first use, so the import itself
stays cheap.

## Install

```bash
npm install @lanka-data-layer/admin
```

## Quick start

```ts
import {
  getUnit,
  getChildren,
  getParentChain,
  unitsAtLevel,
  searchByName,
} from "@lanka-data-layer/admin";

getUnit("LK11");
// { pcode: "LK11", level: 2, name: "Colombo District",
//   nameSi: "කොළඹ දිස්ත්‍රික්ක", nameTa: "கொழும்பு மாவட்டம்",
//   parentPcode: "LK1", areaKm2: 687.11,
//   centroid: { lat: 6.848501, lon: 80.022807 } }

getUnit("lk11"); // pcode lookup is case-insensitive

getChildren("LK1");
// the districts of Western Province: LK11, LK12, LK13

getParentChain("LK1103005");
// [Sri Lanka, Western Province, Colombo District] — root first,
// excludes LK1103005 itself

unitsAtLevel(2);
// all 25 districts

searchByName("colombo");
// exact name matches rank above prefix matches above substring matches,
// searched case-insensitively across English, Sinhala and Tamil names

searchByName("gampaha", { level: 2, limit: 5 });
```

### The `AdminUnit` type

```ts
interface AdminUnit {
  pcode: string;
  level: 0 | 1 | 2 | 3 | 4; // country, province, district, DS division, GN division
  name: string;
  nameSi: string | null;
  nameTa: string | null;
  parentPcode: string | null;
  areaKm2: number | null;
  centroid: { lat: number; lon: number } | null;
}
```

### API

| Function | Description |
|---|---|
| `getUnit(pcode)` | Single unit by p-code, or `null`. Case-insensitive. |
| `getChildren(pcode)` | Direct children of a unit. |
| `getParentChain(pcode)` | Ancestors from the country root down, excluding the unit itself. |
| `unitsAtLevel(level)` | Every unit at a given level (0–4). |
| `searchByName(q, opts?)` | Name search across all three languages. `opts.level` restricts to one admin level, `opts.limit` caps result count (default 10). |
| `DATA_VERSION` | Foundry data release this build was generated from. |
| `UNIT_COUNT` | Total number of units bundled (14,417). |
| `SOURCES` | Provenance array — see below. |

## Data

| Source | License |
|---|---|
| geoBoundaries + OCHA COD-AB Sri Lanka | CC BY 3.0 IGO / CC BY-IGO |

The exact source list bundled with your installed version is also available
at runtime as `SOURCES`, and its release is stamped in `DATA_VERSION`
(format `YYYYMMDD.N`). Boundaries, names and p-codes are regenerated
periodically from the upstream sources by the Lanka Data Layer foundry — the
offline ETL pipeline in the [monorepo](https://github.com/prabhavalabs/lanka-data-layer)
this package is extracted from. To pick up a new data release, upgrade the
package; there is no separate data download step.

Gzipped install size is roughly 400KB (per module format — `import` and
`require` each ship their own bundle with the dataset inlined).

## Part of Lanka Data Layer

This package is a standalone extract of the admin-hierarchy dataset that
powers [Lanka Data Layer](https://github.com/prabhavalabs/lanka-data-layer),
an open geo-data platform for Sri Lanka. If you need more than offline
lookups — reverse geocoding, boundary geometry, population and census
indicators, postal/admin cross-references, election results — see the
hosted API and interactive docs at
[lanka-data-layer.prabhavalabs.com](https://lanka-data-layer.prabhavalabs.com).

### Development

This package is built from within the `lanka-data-layer` monorepo and has
no dependencies or devDependencies of its own; build tooling (`tsup`,
`typescript`) is provided by the workspace root. See the repository for
build instructions.

## License

MIT © Prabhava Labs
