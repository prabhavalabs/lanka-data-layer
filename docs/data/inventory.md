# Data inventory and gap to developing-country.org

Status as of 2026-11-21. Companion files: [`dc-parity.csv`](dc-parity.csv) (one row per dc dataset or layer, with costing), [`value-added.md`](value-added.md) (products beyond dc), [`../../registry/coverage-vs-dc.md`](../../registry/coverage-vs-dc.md) (dataset mapping).

**Reference platform checked live on 2026-11-21.** `developing-country.org/sitemap.xml` lists the same 140 `/data` slugs as the 2026-11-21 crawl. Nothing has been added or removed. `explore.developing-country.org/api/catalog` (generated 2026-08-31) lists **160 layers**. 146 of them hang off the 140 datasets; for example, `demographics` alone backs the `province`, `dsd`, `gnd` and `demographics` layers. The other **14 are explore-only** (no `/data` page): `roads`, `water`, `terrain`, `climate`, `sun`, `stepper`, `clinic_moh`, `courthouse`, `silent_zone`, `noise_ordinance`, `prospects`, `analyses`, `data_views` and `regions`. dc marks `clinic_moh` and `courthouse` as "not ingested". The last four are their users' saved state, not data. `dc-parity.csv` therefore has **154 rows**: 140 datasets plus 14 explore-only layers. The `dc_layers` column lists every explore layer behind each dataset, so all 160 layers are accounted for.

---

## (a) What we have today

### v1 (production, `lanka.sqlite`, data_version 20260813.8)

| Dataset | What | Rows / features | Geo level | Source, licence |
|---|---|---|---|---|
| admin-units | Admin hierarchy + geometry | 14,417 units (1 / 9 / 25 / 339 / 14,043) | ADM0-ADM4 | geoBoundaries + OCHA COD-AB, CC BY-IGO |
| population-2023 | Population by sex x 5-year age | 17,819 rows | ADM0-ADM2 (1,225 units) | HDX COD-PS, CC BY-IGO |
| census-2024 | Census 2024 population, sex, age group, ethnicity, religion | 9,125 rows | ADM0/2/3 (365 units) | DCS, licence unstated |
| (in admin_stats) | Census 2012 ethnicity + religion | 375 rows | ADM2 | DCS |
| elections | Presidential + parliamentary results | 820 results | electoral / polling division | Election Commission via nuuuwan, licence unstated |
| places | Named places | 553 | point | OSM, ODbL |
| postal-codes | Postal codes | 1,833 | point | GeoNames, CC BY 4.0 |
| pois | Hospitals/clinics 1,228, schools/colleges/universities 5,390, stations/airfields 324 | 6,942 | point -> ADM4 | OSM, ODbL |
| roads | Trunk/primary/secondary/motorway (604 motorway segments) | 11,252 | line | OSM, ODbL |
| railways | Lines + 294 stations/halts | 1,618 | line/point | OSM, ODbL |
| waterways | Rivers + water bodies | 3,919 | line/polygon | OSM, ODbL |
| protected-areas | National parks, reserves | 66 | polygon | OSM, ODbL |
| country | National outline | 1 | polygon | geoBoundaries |
| cells | WorldPop 2025 on the canonical 0.001 deg grid | 5,468,498 cells | grid | WorldPop, CC BY 4.0 |
| cell-lookup | Reverse-geocode index (GN, postal, nearest place) | 5,467,970 cells | grid -> ADM4 | COD-AB |

### v2 (local Postgres, published release)

- `catalog.*` holds the registry mirror (244 datasets before the additions in this change).
- **4 published dataset versions holding 9,963 observations:**
  - `wdi-lka`: 1,203 national observations, 1960-2025.
  - `dcs-census-2024-population`: 2,920 observations at ADM0/2/3, covering total, coarse age x sex and sex ratio.
  - `dcs-census-2024-ethnicity-religion`: 5,840 observations at ADM0/2/3.
  - `cod-ab-lka-boundaries`: geometry only. 14,417 rows in `geo.admin_units` and 364 rows in `geo.crosswalks`.
- `geo.features` and `geo.grid_layers` are still empty, so no point, line or grid layer has been loaded into v2 yet.

### Registry (planned)

The registry now holds **174 sources, 263 datasets and 512 indicators**, after adding 7 sources and 19 datasets in this change. `lanka registry validate` passes, and so does `cargo test -p lanka-ingest --lib registry`.

---

## (b) Status against dc, by theme

Counts come from `dc-parity.csv`. "Have" means data is already served from v1 or v2. Every "have" row still carries a residual cost, because none is at *exact* parity: v1 has no status attributes and v2 has no GN level yet.

| Status | Rows | Meaning |
|---|---|---|
| `have_v1` | 6 | parks, hospitals, schools, expressways (geometry only), roads tile, water tile |
| `have_v2` | 7 | Census 2024 age-sex, ethnicity, religion, population change x2, demographics (DS only), admin-unit counts |
| `planned_registry` | 134 | upstream, method and joins are registered; no connector built yet. 4 of these were added today (`climate`, `courthouse`, `silent_zone`, `noise_ordinance`); before today they were `missing` |
| `missing` | 7 | `lk-initiative-events` (excluded by design: built on copyrighted news text; replaced by IATI/WB/ERD/budget/Cabinet sources), plus 6 platform/runtime layers that are not data (`sun`, `stepper`, `prospects`, `analyses`, `data_views`, `regions`) |

**Real coverage today: 13 of 148 data rows (9%) are served, all at partial parity.** The other 135 data rows are registered but not built.

### Themes

**Visitor attractions / tourism.** dc has OSM-only `attractions` (1,111 points), `waterfalls` (141), `trails` (24), `parks` (61), plus Pocket Book `tourism-activity` and CBSL `tourism-earnings-monthly`. dc says itself "Tourist Board GIS not ingested".
- We have parks (v1 protected areas).
- Everything else is planned: `osm-tourism-attractions`, `osm-trails-cycling`, `cbsl-tourism-earnings`, `dcs-pocketbook-sector-services`.
- We go further with `derived-attractions-register`, which conflates OSM + Overture + Wikidata + UNESCO World Heritage (`world-heritage-sites`, new) + gazetted Archaeology monuments (`protected-monuments`, new). We also have SLTDA accommodation and arrivals, and the `derived-tourism-pressure-index`.

**Board of Investment.** dc has `boi-zones`: 15 hand-placed *rectangles* ("boundaries indicative"). Nothing on BOI projects or FDI.
- We plan `boi-zones` with OSM `landuse=industrial` polygons.
- New today:
  - `boi-fdi-inflows`: BOI monthly-performance page and annual reports, giving FDI, approvals and employment by sector and origin.
  - `boi-enterprises`: the enterprise directory plus the land bank at `/home/available-lands/`.
  - `derived-investment-density`.
- BOI publishes no GIS, and the licence is unstated.

**Cycling routes.** dc has `cycling`: 2 OSM `route=bicycle` relations. We plan `osm-trails-cycling` (0.5 d) and will add `highway=cycleway` ways and km per district.

**Expressways and road network status.** dc `expressways` has 628 features: OSM motorways classed open / under construction / proposed, plus curated proposed corridors (Kandy link, Ruwanpura). It also has `national-road-length` (Abstract PDF) and an explore `roads` tile.
- v1 already has 604 motorway segments and the full trunk-secondary network, without status.
- New `rda-expressway-programme` adds status, opening dates, interchanges and cost.
- New finding: the RDA project map serves **GeoJSON for 79 A-class and 1,166 B-class roads** (2021 vintage) at `rda.gov.lk/rda_project_new/uploads/uploaded_geojson_files/`. That gives `rda-national-roads` an official geometry instead of OSM ref-matching. The E-class file returns 404.

**Public developments / projects.** dc `developments` has 36 curated megaprojects from 9 publishers (UDA plan PDFs, CPCEC, SLPA, AASL, MoT, MoE, Mahaweli, BOI) plus press URLs. Its `lk-initiative-*` datasets are a news-extraction ledger.
- We plan `lk-major-projects` (class C).
- The structured replacements are `budget-capital-projects`, `iati-activities-lka`, `wb-projects-lka`, `erd-foreign-financing` and `cabinet-decisions-geotagged`.
- Exact parity with `lk-initiative-funding` would need dc's news corpus. That is costed (6 d) but not recommended.

**Population and administration.** This is our strongest area.
- v2 serves Census 2024 at DS level and the full ADM0-ADM4 spine.
- Gaps against dc:
  - GN-level Census 2024 + 2012 (`demographics`, 14,021 units). dc matches only 97.4% of GN ids because of recodes; we target 100% via `gnd-id-crosswalk` + `admin-region-history`.
  - Full age bands.
  - DCS mid-year estimates (`population-estimates-district`).
  - Vital statistics.
  - Census disability, literacy and language tables.
  - Local-authority counts.

**Transport (bus/rail/metro, ports, vehicles).** dc has 10 Colombo Metro Bus analyses, `metro`, `metro_catchment`, `metro_walksheds`, `osm_foot_graph`, NTC `public-transport-capacity`, `road-crashes-fatalities`, vehicle registrations, port tables and `sea-routes`.
- All are planned. `derived-transit-access` generalises dc's Colombo study to the whole island.
- The routing graph (2 d) and the walkshed engine (4 d) are on the critical path for 12 dc rows.

**Energy / electricity.** dc has `electricity-generation-daily` and `-quarter-hourly` (NSO API), `electricity-customers-sales` (CEB PDF), `electricity-supply-reliability` (PUCSL xlsx, 7 quarterly files) and `energy-supply-consumption` (Pocket Book).
- All are planned.
- New `power-plants` (Global Energy Monitor, CC BY 4.0) gives plant-level capacity and status, which dc lacks.

**Water.** dc has `water-operating-performance` and `water-supply-connections-coverage` (NWSDB, national), `irrigation-gauges` (40 stations, snapshot) and the explore `water` tile.
- We have the water tile (v1).
- NWSDB, irrigation gauges and reservoir levels are planned. Ours is a time series, where dc's is a snapshot.

**Health.** dc has `hospitals` (OSM), `government-health-services` (Pocket Book), `inpatient-morbidity-mortality` (IMMR xlsx), `child-nutrition-annual` (FHB PDFs) and the explore `clinic_moh` (not ingested by dc).
- We have hospitals (v1 OSM, 1,228 points).
- Planned: `moh-health-institutions` (with hospital tier), `moh-immr` and `fhb-maternal-child-health`.

**Education.** dc has `schools` (OSM, 4,627), `school-resources`, `school-examination-results` and `vocational-training` (all from the Pocket Book).
- We have schools (v1 OSM, 5,390).
- Planned: `giga-schools`, `moe-school-census` and `exam-results-district`.

**Economy / CBSL / Treasury / DCS / World Bank.** This is the largest block: 39 rows and **117 person-days**. It covers CBSL statistical tables (fiscal, debt, trade, remittances, tourism earnings, exchange rates, NEER/REER, bank branches), 17 Treasury PDF-derived tables (debt bulletin, guarantees, SOEs, procurement, province transfers), DCS national accounts, CCPI/NCPI and HIES, and World Bank SLDU PDFs.
- All are planned.
- WDI (v2) is an open national mirror, but it is not exact parity.

**Environment.** dc has `land-cover-grid`/`-district` (WorldCover 2021), `weather-station-observations` (one Pocket Book year), `natural-disaster-impacts`, plus the explore `terrain` (SRTM) and `climate` (WorldClim).
- All are planned. `worldclim-climate-normals` was added today.
- We exceed dc with Copernicus DEM, ERA5-Land, CHIRPS, JRC flood and DesInventar.

**Housing / land.** dc has census housing structure, condition and tenure, `building-construction-district`, `building-material-prices`, CBSL `colombo-land-valuation-indicator` and `colombo-asking-price-indices`.
- All are planned.
- Below the Colombo indicator there is no open land-price data at all.

**Other.**
- Labour and migration: 10 rows, 31 d. These are LFS quarterly/annual PDFs and SLBFE.
- Agriculture and food: 10 rows, 20 d. Paddy has a 48-year district series; the rest is Pocket Book.
- Local government and justice: 4 rows, 10 d. Covers Abstract LG finance and courthouses.

---

## (c) Total cost to exact parity

Exact parity means the same coverage, period and granularity as dc, with our provenance, validation and p-code joins included.

### Effort model

The model is in `dc-parity.csv`, and every override is visible in `cost_drivers`.

**Per row, effort =**
- access base: API 1.0, CSV/zip/git 0.75, raster 1.0, OSM 0.5, xlsx 1.5, HTML 1.5, PDF 2.5
- **plus** geo join: national/province 0.25, district 0.5, DS or local authority 1.0, GN 1.5, points 0.5
- **plus** history: PDF 0.04 per extra monthly file, 0.08 per quarterly, 0.15 per annual edition (cap 4); 0.5 for more than 10 years of sheets or HTML
- **plus** 0.5 per extra source (cap 2)
- **plus** 0.5 for validation and cell-level provenance

**Adjustments:**
- Rows that reuse a connector already counted are charged 50%.
- `have_v1` rows are charged 40% of the computed figure.
- `have_v2`, OSM, derived and curated rows are costed by hand.

### Totals

| Priority | Rows | Person-days |
|---|---|---|
| P0 (spine, census, population, poverty, paddy, land cover, routing graph) | 19 | 34.25 |
| P1 (official sub-national + operational: LFS, CBSL, Treasury debt, energy, transport, spotlight layers) | 63 | 170.75 |
| P2 (Pocket Book national tables, Treasury long tail, WB SLDU, metro analyses) | 58 | 118.5 |
| P3 (asking prices, MSME finance, sea routes, courts, noise, initiative funding) | 7 | 15.0 |
| **Total to exact parity (148 data rows)** | **147** | **338.5** |
| Excluded (`lk-initiative-events`; 6 platform layers at 0 d) | 7 | 15.0 (not recommended) |

**Shared enablers that are not inside any row add about 9 person-days:**
- finish the DS/LA name crosswalks: 3 d
- class-B PDF extraction harness and review queue: 5 d
- permission letters to DCS, CBSL, MoF and MoH: 1 d

**The grand total is about 348 person-days.** That is roughly 8.5 months for 2 people, or 5.5 months for 3.

### By theme

Person-days, excluding the out-of-scope rows.

| Theme | dc rows | have | planned | missing | P0 | P1 | P2 | P3 | Total |
|---|---|---|---|---|---|---|---|---|---|
| Economy: CBSL / Treasury / DCS / WB | 39 | 0 | 39 | 0 | 7 | 53.5 | 53 | 3.5 | 117 |
| Transport (bus/rail/metro/port/vehicles) | 21 | 0 | 21 | 0 | 2 | 19.5 | 14 | 0.75 | 36.25 |
| Housing & land | 11 | 0 | 11 | 0 | 5 | 15.25 | 9.5 | 2.5 | 32.25 |
| Labour & migration | 10 | 0 | 10 | 0 | 1 | 26.75 | 3.5 | 0 | 31.25 |
| Population & administration | 12 | 7 | 5 | 0 | 9 | 12 | 0 | 0 | 21 |
| Agriculture & food | 10 | 0 | 10 | 0 | 8.75 | 0 | 11.5 | 0 | 20.25 |
| Health | 5 | 1 | 4 | 0 | 0 | 5 | 8.25 | 0 | 13.25 |
| Energy & electricity | 5 | 0 | 5 | 0 | 0 | 10 | 3.25 | 0 | 13.25 |
| Public developments & projects | 3 | 0 | 2 | 1 | 0 | 4 | 0 | 6 | 10 |
| Local government & justice | 4 | 0 | 4 | 0 | 0 | 7 | 2 | 1 | 10 |
| Water | 5 | 1 | 4 | 0 | 0 | 7 | 1.75 | 0 | 8.75 |
| Environment & climate | 8 | 0 | 8 | 0 | 1.5 | 0 | 5 | 1.25 | 7.75 |
| Tourism & attractions | 6 | 1 | 5 | 0 | 0 | 4.25 | 2.25 | 0 | 6.5 |
| Education | 4 | 1 | 3 | 0 | 0 | 0.5 | 4.5 | 0 | 5 |
| Expressways & roads | 3 | 2 | 1 | 0 | 0 | 4.5 | 0 | 0 | 4.5 |
| Board of Investment | 1 | 0 | 1 | 0 | 0 | 1 | 0 | 0 | 1 |
| Cycling | 1 | 0 | 1 | 0 | 0 | 0.5 | 0 | 0 | 0.5 |
| Platform / runtime (out of scope) | 6 | 0 | 0 | 6 | - | - | - | - | 0 |

### By upstream family (where the money goes)

About half of the cost is DCS publications: roughly 165 d across Pocket Book 39.75, Census 2024 tables 31.5, LFS 23.5, Statistical Abstract 21.75, HIES 12.5, agriculture 11, prices 9.5, national accounts 8.25 and population/vital 7.25. Next come Treasury (about 45 d: debt bulletin 20.25, annual report 13.25, fiscal reports 7.25, procurement 4.25), CBSL (about 30 d), World Bank SLDU PDFs (10.75) and OSM (9.5 d for 12 rows). The ten spatial "headline" layers (attractions, BOI, cycling, expressways, developments, hospitals, schools, parks, trails, waterfalls) total only **10.75 d**.

### Critical path

1. **Spine and joins, about 7 d.**
   - GN id crosswalk with the 2012 to 2024 recodes (blocks `demographics`, `census-households-dsd`, all metro rows).
   - DS and local-authority name crosswalks (block about 90 rows).
   - Province roll-ups.
2. **Class-B PDF harness, 5 d.** 85 of the 147 rows, and 238 of the 338.5 person-days, are PDF tables (Pocket Book, Abstract, LFS, HIES, Treasury, census reports). Template-per-table extraction with cell locators and a review queue needs to exist before throughput rises.
3. **Routing graph (2 d), then the walkshed engine (4 d), then 9 metro views (0.75 d each).** This chain is strictly sequential.
4. **Permission letters.** Not on the build path, but on the *publication* path for 122 rows (see blockers).

### Automated vs one-off

| Class | Rows | Person-days | Refresh after build |
|---|---|---|---|
| A, auto-publish (APIs, OSM, rasters, xlsx with stable sheet ids) | 51 | 83.75 | scheduled, unattended |
| B, deterministic parse + review (PDF / HTML tables) | 94 | 248.75 | scheduled fetch, human review of each new release (about 0.1-0.25 d per release) |
| C, curated / LLM-assisted (developments, expressway status, BOI enterprises) | 2 primary + curated parts of `expressways`, `boi-zones` | 6 + about 7 inside overrides | quarterly manual curation |

One-off builds that do not recur:
- census 2024 tables (15 rows)
- historic back-catalogues: paddy since 1979, exchange rates since 1986, fiscal since 1990, LFS since 2019
- metro analyses (re-run only on input change)

A large share of the parity effort is building back-catalogue history once. The steady-state refresh load is an estimate of about 3-4 person-days a month, mostly reviewing class-B releases across the monthly and quarterly series.

### Top blockers

1. **Licence.** 122 of 148 data rows are `permission_unclear`: Sri Lankan government publications with no stated reuse licence (DCS, CBSL, Treasury, MoH, NWSDB, CEB, PUCSL, NTC, BOI, RDA, UDA). dc publishes them with attribution, and so will we (attribution plus a takedown contact, excluded from npm bundles), while written permission is requested. Only 24 rows are `attribution` and 2 `open`. The only `restricted` layer (`stepper`) is out of scope.
2. **PDF-only publication.** About 70% of the effort (238 d) is PDF table extraction, and several series exist only as yearly editions with layout drift (Abstract, Pocket Book, LFS quarterly, Treasury bulletins).
3. **Unreachable hosts.** The `.gov.lk` hosts listed below dropped connections from the verification host. Build their connectors from a Sri Lankan vantage point:
   - On dc-parity paths: `epid-unit`, `slr`, `meteo`, `harti`, `naosl`, `ugc`, `erd`.
   - New today: `archaeology-dept`, plus `jsc.gov.lk` (used by `moj-courts`).
   - `whc.unesco.org` returns 403 to non-browser clients.
4. **No official GIS** for BOI zones, MoH facilities, MoE schools, UDA plans or expressway status. Geometry is digitised from OSM, plans or press (class C) and must carry precision flags. The RDA A/B GeoJSON is the welcome exception.
5. **GN recodes** between 2012 and 2024 (Nuwara Eliya, Galle, Ratnapura) cap dc at 97.4% GN matching. We need the explicit recode table to reach 100%.

---

## (d) Recommended order to close the gap

Quick wins come first. Each wave is independent of later ones.

| Wave | Rows | Person-days | Contents | Why first |
|---|---|---|---|---|
| 1. Port v1 to v2 + OSM spotlight layers | 15 | 10.5 | `hospitals`, `schools`, `parks`, `roads`, `water`, `attractions`, `waterfalls`, `cycling`, `trails`, `expressways` (+ status), `boi-zones`, `land-cover-grid`/`-district`, `terrain`, `climate` | open/attribution licences; v1 pipelines exist; covers every spotlight layer except developments; first v2 point/line/grid loads |
| 2. Population spine to GN | 9 | 12.25 | GN crosswalk + `demographics` (GN 2012 + 2024), `census-age-sex-dsd` full bands, ethnicity/religion 2012 port, `population-estimates-district`, `population-change*`, `administrative-units-district` (LA counts), `vital-statistics` | denominators for every per-capita figure; ours becomes strictly better than dc (100% GN match) |
| 3. API / xlsx / HTML series | 22 | 48.5 | NSO generation (daily, quarter-hourly), CBSL tables (exchange rates, NEER/REER, fiscal, debt, revenue, trade, remittances, tourism earnings, bank branches), PUCSL reliability, IMMR, Treasury procurement, irrigation gauges, paddy 1979-2026 (4 tables) | class A or near-A; unattended refresh afterwards |
| 4. Census 2024 + HIES + LFS PDF tables | 21 | 65.0 | basic services, housing (3), households, disability, literacy (2), equipment, waste; poverty headcount/depth; household income/expenditure; labour force, unemployment, employment by industry/occupation, informal sector, LFS quarterly history (2) | needs the PDF harness (+5 d); highest analytical value of the PDF block |
| 5. Developments + transport analyses | 16 | 20.25 | `developments` (class C), routing graph, walksheds, 9 metro views, `metro`, `metro_catchment`, `sea-routes`, `workplaces` | sequential critical path; showcases island-wide transit access |
| 6. Remaining P0/P1 PDF series | 21 | 71.25 | CCPI/NCPI, GDP by industry, Treasury debt (3) + province transfers, Abstract transport (port, roads, vehicles) + LG finance (2), CEB sales, NWSDB (2), NTC capacity + road crashes, SLBFE, MoH clinics | large, mostly national; publish with attribution while permissions are pending |
| 7. P2/P3 long tail | 43 | 110.75 | 19 Pocket Book tables, Treasury guarantees/SOE/fiscal/provincial debt, World Bank SLDU (3), CBSL real estate (2), FHB nutrition, building construction, courts, noise zones, `lk-initiative-funding` replacement | lowest marginal value; many are covered better by WDI/FAOSTAT/ILOSTAT mirrors already planned |
| **Total** | **147** | **338.5** | plus about 9 d of shared enablers | |

Waves 1-2 (about 23 d) put us at or beyond parity on every spotlight dataset and on population. After waves 1-4 (about 136 d plus the 5 d PDF harness), 67 of 148 data rows are at exact parity, and those are the rows users actually query sub-nationally.
