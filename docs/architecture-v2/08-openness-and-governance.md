# 08 — Openness and governance

## 1. What is licensed how

| Thing | Licence | Why |
|---|---|---|
| Engine (`engine/`), infra, tools | **Apache-2.0** (today: MIT in `engine/Cargo.toml` and the root `LICENSE`) | Explicit patent grant; the standard choice for infrastructure that governments and NGOs adopt; compatible with the reference platform's Apache-2.0 code when ideas or helpers are ported (with NOTICE). Relicensing MIT → Apache-2.0 is possible because the code has one copyright holder — **needs the owner's decision** (open question Q1). |
| Web app | follows the web track | out of scope here |
| The standard (`spec/`), method pages (`docs/methods/`), documentation | **CC BY 4.0** | Others must be free to implement and quote it |
| JSON Schemas, OpenAPI document, codelists, country profile | **CC0 1.0** | Schemas and code lists should carry no attribution friction |
| Registry metadata (`registry/sources`, `datasets`, `recipes`, `connectors`, `templates`, `crosswalks`) | **CC BY 4.0** | It is our curated work; attribution is reasonable |
| Data we merely relay | the source's licence, per dataset | We cannot grant more than we received |
| Data we derive | computed from inputs ([02 §8](02-standard-model.md#8-licences-attribution-redistribution)); where every input is open/attribution: **CC BY 4.0**; where OSM geometry is redistributed: **ODbL 1.0** | |
| Name "Lanka Data Layer" and logo | reserved (trademark-style policy in `TRADEMARK.md`): forks must use their own name; "conforms to Lanka Data Layer Standard x.y" is allowed for deployments passing the conformance suite | Prevents a fork with different data being mistaken for this one |

Every repository directory that differs from the root licence carries its own `LICENSE` file; `REUSE.toml` (REUSE 3 spec) makes it machine-checkable in CI.

## 2. Licence honesty for data

Today 86 of 174 sources and 136 of 263 datasets are `permission_unclear` (government publications with no stated reuse terms), and 122 of the 148 parity rows depend on them. The platform's position:

1. **Say so everywhere.** Licence status is a first-class field in the catalogue, in `meta.attribution[]` on every response, at the top of every dataset README, and as a filter (`?licence_status=open,attribution`).
2. **Relay, attribute, do not repackage.** `permission_unclear` data is served through the API and per-dataset downloads with the publisher's name and source URL, is excluded from aggregate packs and the npm/CLI bundles, and its raw documents are never redistributed (hash + URL only).
3. **Ask.** A permission request goes to each publisher (DCS, CBSL, Ministry of Finance, Ministry of Health, NWSDB, CEB, PUCSL, NTC, BOI, RDA, UDA). `registry/permissions/<publisher>.yaml` records date sent, contact, reply, and the resulting status; a granted permission upgrades the licence entry and, at the next release, the redistribution tier.
4. **Remove on request**, fast (§6).

## 3. The spec

`spec/` is the adoptable part, versioned separately from the engine:

```
spec/
  README.md                      what the standard is, conformance classes, how to claim conformance
  1.0/standard.md                the normative text (from docs/architecture-v2/02-standard-model.md)
  1.0/schemas/*.json             observation, dataset card, release manifest, source, dataset, connector, recipe, template, profile
  1.0/codelists/*.csv            basis, status, flags (bit registry), freq, extraction methods
  1.0/openapi/v2.json            the REST contract
  1.0/conformance/               request/assertion suite run by `lanka conformance <base-url>`
  CHANGELOG.md
```

The standard is deliberately small: identifiers, the observation model, licence/provenance metadata, release manifest, and the HTTP contract. It does not mandate Postgres or Rust; another implementation that passes the conformance suite conforms.

## 4. Contribution model

Contributions that do not need a maintainer to write code are the ones to optimise for.

| Contribution | Needs | Review |
|---|---|---|
| **New source or dataset (registry only)** | `registry/sources/<id>.yaml`, `registry/datasets/<id>.yaml` | `registry-ci` validates schema, references, licence fields, limits present. One maintainer approval. |
| **New connector, declarative** | `registry/connectors/<id>.yaml` + fixtures in `engine/crates/lanka-ingest/tests/fixtures/<id>/` + expected rows | CI runs `lanka connector test`; maintainer checks the licence and the `expect:` block. No Rust. |
| **New PDF template / new edition** | `registry/templates/<source>/<table>.yaml` + one test page set | CI runs the template tests. |
| **New code connector** | Rust module + golden test | Normal code review; must use the SDK traits, no network outside `discover`, no database access. |
| **New recipe** | `registry/recipes/<id>.yaml` + `registry/datasets/derived-*.yaml` with limits + method page | `lanka recipe check` + QA gates; lands as `experimental`; promoted to `stable` after two clean releases and a maintainer sign-off on the validation score. |
| **New method** | Rust in `lanka-enrich/src/methods/` with unit, property, golden and (where applicable) reference tests + `docs/methods/<id>.md` | Two approvals, one from someone who did not write it; statistical methods need a cited reference and a reference-implementation comparison. |
| **Alias / crosswalk fix** | row in `registry/crosswalks/*.csv` with a source for the claim | One approval; CI checks weights and parent constraints. |
| **Data correction report** | issue template: indicator, geo, period, value shown, value expected, evidence URL | Triage within 7 days; if the source is wrong, the dataset gets a `limits` note and a `SOURCE_AMBIGUOUS`/`SOURCE_INCONSISTENT` flag — the platform does not "fix" official numbers. |
| **Translation (si/ta labels)** | registry label edits | Native-speaker reviewer; no machine translation merged unreviewed. |

Supporting files: `CONTRIBUTING.md` (the table above with examples), `GOVERNANCE.md` (who can merge what; maintainers listed; decisions by lazy consensus, the owner breaks ties), `CODE_OF_CONDUCT.md`, `SECURITY.md` (private reporting address, 90-day disclosure), issue and PR templates, a DCO sign-off requirement (`Signed-off-by`) instead of a CLA.

**Review of data is separate from review of code.** A merged connector produces versions that still go through the publication class gate; a contributor cannot publish numbers by merging a PR. Reviewers of class B/C versions are named in `LANKA_REVIEWERS` and recorded on each version.

## 5. Versioning and deprecation

Five things are versioned, each with its own rule:

| Thing | Scheme | Compatibility promise |
|---|---|---|
| **Standard** (`spec/`) | `MAJOR.MINOR` | MINOR adds optional fields, codes, flags, conformance classes. MAJOR may remove or redefine; the previous MAJOR's text and schemas stay published. |
| **HTTP API** | path major (`/v1`, `/v2`) + `meta.contract_version` | Within a major: additive only (new endpoints, fields, enum values documented as open sets). A breaking change is a new major. The previous major is served for **≥ 12 months** after the new one is declared stable, with `Deprecation` and `Sunset` headers and a notice in `/v2/status`. |
| **Engine** | SemVer, git tags `v0.x` → `v1.0` when `/v2` is declared stable | CLI flags and config variables follow SemVer from 1.0. |
| **Data** | `data_version` (`YYYYMMDD.N`) + per-dataset `dsv` | Releases are immutable and addressable forever by manifest; files under retention rules ([01](01-layers.md#retention)). |
| **Identifiers** | — | Never reused. An indicator, dataset, codelist code or unit code is **deprecated, not deleted**: `deprecated: {since, replaced_by, reason}` in the registry; the API keeps answering for it (with `meta.deprecations[]`) for ≥ 12 months and returns `410` with the replacement afterwards. A p-code change follows the upstream COD-AB edition and always ships with a crosswalk. |
| **Methods / recipes** | SemVer per method | MAJOR = outputs change for identical inputs → new derived dataset versions with a changelog entry that says "method change", and `status = break` at the first affected period where a series is discontinuous. |

Changelog obligations: every release has `CHANGELOG.md` with, per changed dataset, rows added/removed/changed, the reason class (`new_edition | revision | correction | method_change | boundary_change | withdrawal`), and for corrections a sentence a journalist can quote.

Breaking changes to the country's reference geography (a new COD-AB edition, a GN recode) are announced one release ahead in `/v2/status` and `CHANGELOG.md` and applied in a release whose manifest sets `geography_changed: true`.

## 6. Takedown

For permission-unclear sources, and for any rights-holder or privacy concern.

**Contact.** A published address (`takedown@<domain>`) and a form linked from every dataset page, the API docs and `LICENSE-DATA.md`. The request must identify the dataset(s) and the requester's standing.

**Process and clock.**

| Step | Within | Action |
|---|---|---|
| 1. Acknowledge | 2 working days | Log in `ops.takedowns`; reply with the reference number |
| 2. Suspend | 2 working days for a publisher's own data; immediately for anything that looks like personal data | `lanka takedown <dataset> --suspend --ref <n>`: sets `catalog.datasets.suspended = true`; the API returns `451 Unavailable For Legal Reasons` with the reference; downloads return 451; the next (immediate) release omits the dataset. Suspension is reversible. |
| 3. Decide | 10 working days | Options: restore (permission granted or claim unfounded), restrict (move to `link` tier: metadata + pointer to the source only), or withdraw |
| 4. Withdraw | on decision | `lanka takedown <dataset> --withdraw`: versions → `withdrawn`; distribution files replaced by `WITHDRAWN.txt`; rows deleted from `data.observations`/`obs_provenance`/`geo.features` (the one sanctioned hard delete); extracts and raw blobs deleted; `ops.artifacts` rows kept with sha256 + URL + `purged_at` so the audit trail shows what existed; nginx cache purged for the dataset's URL prefixes; backups age out under normal retention (stated in the reply) |
| 5. Cascade | same release | Derived datasets that used it are recomputed without it by the train, or, if it was a required input, are themselves suspended and listed in the takedown record |
| 6. Record | — | Public transparency log `/v2/takedowns` (dataset, date, requester category, outcome) — no personal details |

```sql
CREATE TABLE ops.takedowns (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  received_at timestamptz NOT NULL, requester_category text NOT NULL,     -- publisher | rights_holder | individual | other
  dataset_ids text[] NOT NULL, state text NOT NULL CHECK (state IN ('received','suspended','restored','restricted','withdrawn','rejected')),
  decided_at timestamptz, public_note text, private_note text
);
```

**Design consequences that make takedown cheap** (and are why it is designed now): one partition per dataset (delete = `DROP`/`TRUNCATE PARTITION`); distributions keyed by dataset/dsv (delete = remove a directory); licence tier computed through derivations (cascade is a graph walk); permission-unclear data never enters aggregate packs or npm bundles (nothing to recall from third-party mirrors beyond per-dataset files).

**Personal data.** The platform publishes aggregates and public-facility features only. Registry rule: a dataset with any indicator below a minimum cell count (default 10 persons/households) at ADM4 must declare a suppression rule; sources containing named individuals (other than public office holders in their official role, e.g. `parliament-members`) are rejected at registration.

## 7. Self-hosting and forking

### 7.1 Self-host Sri Lanka's layer (a mirror or a private instance)

```
git clone … && cd lanka-data-layer
cp infra/v2/.env.example infra/v2/.env        # passwords
docker compose -f infra/v2/docker-compose.yml up -d
lanka db migrate && lanka registry sync
lanka pull full --release current --into-db   # load a published release instead of re-ingesting 174 sources
lanka serve
```

`lanka pull --into-db` restores a release from its public distributions (Parquet → COPY), so a mirror needs no upstream access, no review work, and gets byte-identical dsvs. Mirrors may add private sources in their own registry overlay (`LANKA_REGISTRY_DIR` accepts a list; later directories override/extend earlier ones).

Minimum footprint for a read-only mirror: 2 vCPU / 4 GB / 40 GB **[A]**.

### 7.2 Fork for another country

Everything country-specific is in two places.

```yaml
# country/lk/profile.yaml
id: lk
name: { en: Sri Lanka, si: ශ්‍රී ලංකාව, ta: இலங்கை }
iso3166: { alpha2: LK, alpha3: LKA }
site: { base_url: https://lanka-data-layer.prabhavalabs.com }
languages: [en, si, ta]                       # first = mandatory fallback
timezone: Asia/Colombo
currency: LKR
grid:                                         # the canonical grid; formula is fixed, parameters are not
  lat_max: 10.000
  lon_min: 79.400
  step_deg: 0.001
  rows: 4200
  cols: 2700
admin:
  scheme: admin
  pcode_pattern: "^LK[0-9]*$"
  levels:
    - { level: 0, name: { en: Country } }
    - { level: 1, name: { en: Province, si: පළාත, ta: மாகாணம் } }
    - { level: 2, name: { en: District } }
    - { level: 3, name: { en: Divisional Secretariat division }, short: DSD }
    - { level: 4, name: { en: Grama Niladhari division }, short: GND }
  reference_boundary_set: cod-ab-lka-2022
metric_crs: EPSG:5235
calendars:
  fiscal: { start_month: 1 }
  agri:   { start_month: 9 }
  seasons: [ { id: Maha, start: "09-01" }, { id: Yala, start: "04-01" } ]
schemes: [ed, pd, la, moh, police, basin, station]
suppression: { min_count_adm4: 10 }
```

plus `registry/` (sources, datasets, connectors, templates, recipes, crosswalks, codelists).

A fork:

1. writes `country/<iso>/profile.yaml` (grid bbox/step, p-code pattern, levels, languages, CRS, calendars);
2. starts an empty `registry/` with its COD-AB source (the `ckan` driver + `cod_ab` connector work for any OCHA COD-AB country), WorldPop, OSM and the international API sources — these ~40 global sources are shipped as **registry templates** in `registry-templates/global/` with `{{iso3}}` placeholders, so a new country has population, boundaries, roads, facilities, land cover, terrain, climate and WDI on day one;
3. reuses every method and recipe that only depends on those (zonal population, accessibility, flood exposure, land cover — the "open inputs only" products);
4. adds national sources incrementally.

What keeps this honest: the engine contains no `LK` literal (the `CHECK (pcode ~ '^LK[0-9]*$')` in 0001 moves to a profile-driven validator; `BOUNDARY_DATASET` in `lanka-core/src/lib.rs` becomes profile data), and the CI **fork test** builds a release for a synthetic country ([07 §8](07-operations.md#8-testing-strategy)). Crate and binary names stay `lanka*` — renaming is cosmetic and a fork can alias the binary; the standard's name is what generalises.

Explicit non-goal: a multi-country deployment (one database serving several countries). A fork is one country per deployment; cross-country comparison is a client-side concern over two conforming APIs.

## 8. Project governance, briefly

- **Maintainers**: listed in `GOVERNANCE.md`; today one or two people. The design assumes that and keeps every recurring duty scriptable: class-B review (~3–4 person-days/month), registry PR review, the quarterly restore drill, permission follow-ups.
- **Decision records**: architecture decisions are ADRs in `docs/architecture-v2/adr/`; data-policy decisions (a suppression rule, a tier change, a method promoted to stable) are recorded in the registry file they affect, with the PR as the record.
- **Independence of numbers**: no sponsor, publisher or contributor gets a private path to change a published value; all values come from sources or recipes in the public registry.
- **Sustainability**: the running cost is one VPS and an off-site backup target; the design has no per-request or per-GB metered dependency.
