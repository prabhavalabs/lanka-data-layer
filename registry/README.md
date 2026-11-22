# Lanka Data Layer - source and dataset registry

Machine-readable catalogue of every upstream **source** (`sources/<id>.yaml`) and every **dataset** we publish (`datasets/<id>.yaml`), validated against `schema/source.schema.json` and `schema/dataset.schema.json`. The v2 ingestion engine reads this registry to schedule fetches, attach provenance, and build joins to p-codes (ADM0-ADM4) and the 0.001-degree grid. `index.csv` is a flat one-row-per-dataset view; `coverage-vs-dc.md` maps the reference platform's 140 datasets to ours.

## Overview

- **167 sources**, **244 datasets**, **475 indicators** (349 of them available below national level).
- 157 of 167 sources were reached on 2026-11-21 (`verified_at` set); 10 were not reachable from the verification host and carry a note (see *Blockers*).
- Publication class: A = 148, B = 87, C = 9.
- Every one of developing-country.org's 140 datasets is mapped (139 covered, 1 excluded with reason); 160 datasets are additions dc does not have.

### By theme (EU DCAT codes; a source/dataset can carry several)

| Theme | Sources | Datasets | Indicators |
|---|---|---|---|
| AGRI | 13 | 22 | 46 |
| ECON | 43 | 58 | 115 |
| EDUC | 7 | 11 | 30 |
| ENER | 9 | 11 | 27 |
| ENVI | 47 | 52 | 95 |
| GOVE | 21 | 28 | 56 |
| HEAL | 19 | 26 | 58 |
| INTR | 11 | 12 | 24 |
| JUST | 5 | 6 | 11 |
| REGI | 21 | 27 | 41 |
| SOCI | 43 | 53 | 121 |
| TECH | 4 | 7 | 14 |
| TRAN | 19 | 24 | 45 |

### By geographic level (datasets / indicators publishing at that level)

| Level | Datasets | Indicators |
|---|---|---|
| national | 123 | 270 |
| adm1 | 108 | 175 |
| adm2 | 144 | 264 |
| adm3 | 96 | 167 |
| adm4 | 59 | 105 |
| point | 39 | 52 |
| line | 10 | 11 |
| polygon | 28 | 37 |
| raster | 32 | 46 |

### By licence status

| Status | Sources | Datasets |
|---|---|---|
| open | 8 | 12 |
| attribution | 67 | 90 |
| permission_unclear | 86 | 136 |
| restricted | 6 | 6 |

Status meanings used in this registry: `open` = public domain / CC0, no conditions; `attribution` = open licence with attribution and/or share-alike (CC BY, CC BY-IGO, ODbL, CDLA-Permissive, CC BY-SA) - redistributable, share-alike outputs kept in separate files; `permission_unclear` = public government publication with no stated reuse licence (published with attribution and a takedown contact, excluded from the npm packages, permission requested in writing); `restricted` = non-commercial or no-redistribution terms (metadata/link only, internal QA). Dataset-level status in `index.csv` is the strictest status among its sources and, for derived products, among the sources of its inputs.

### By priority

| Priority | Sources | Datasets |
|---|---|---|
| P0 | 26 | 36 |
| P1 | 53 | 83 |
| P2 | 57 | 90 |
| P3 | 31 | 35 |

## Priority rationale

- **P0** - the join spine (boundaries, crosswalks, population) and geo-native layers with clean licences that light up every admin level at once, plus the existing core official datasets we already serve. Low effort, highest leverage: one grid layer becomes indicators for ~14,000 GN divisions.
- **P1** - sub-national official statistics (district/DS/RDHS/LA) and live operational lanes (rivers, dengue, generation, prices) that answer concrete planning questions; most are `permission_unclear` and are published with attribution while reuse permission is requested. Also the first derived Tier-2 products.
- **P2** - national-only series, international mirrors, secondary compilations, class C document extraction, and specialised layers.
- **P3** - historic, low-signal, or licence-restricted/blocked sources (kept for crosswalks, QA or future negotiation).

Publication classes: **A** auto-publish (clean API/open files, deterministic), **B** deterministic parse of documents with PR review, **C** LLM-assisted extraction with human sign-off.

## P0 sources

| Source | Publisher | Licence | Effort | Feeds |
|---|---|---|---|---|
| dcs-agriculture | Department of Census and Statistics, Sri Lanka | lk-gov-unstated | M | dcs-paddy-statistics, dcs-agri-production-index, dcs-other-field-crops, dcs-census-of-agriculture |
| wikidata | Wikimedia Foundation / Wikidata community | cc0-1.0 | S | wikidata-admin-crosswalk, wikidata-places-entities |
| worldbank-wdi | World Bank | cc-by-4.0 | S | wdi-lka |
| worldpop | WorldPop, University of Southampton | cc-by-4.0 | S | worldpop-pop-grid, worldpop-age-sex, worldpop-births-pregnancies |
| esa-worldcover | European Space Agency (WorldCover consortium) | cc-by-4.0 | S | worldcover-land-cover |
| hdx-cod-ab-lka | UN OCHA (data from Survey Department of Sri Lanka) | cc-by-igo-3.0 | S | cod-ab-lka-boundaries, gnd-id-crosswalk, local-authority-boundaries |
| meta-hrsl | Meta Data for Good & CIESIN | cc-by-4.0 | S | hrsl-population-30m |
| geoboundaries-lka | William & Mary geoLab (geoBoundaries) | cc-by-igo-3.0 | S | cod-ab-lka-boundaries |
| jrc-flood-hazard | European Commission Joint Research Centre | cc-by-4.0 | S | jrc-river-flood-hazard |
| elections-commission | Election Commission of Sri Lanka | lk-gov-unstated | M | ec-presidential-results, ec-parliamentary-results, ec-local-authority-results, ec-provincial-council-results ... |
| ghsl | European Commission Joint Research Centre | cc-by-4.0 | M | ghsl-built-up, ghsl-settlement-model, ghsl-population-grid |
| osm-geofabrik | OpenStreetMap contributors (extract by Geofabrik GmbH) | odbl-1.0 | M | osm-road-network, osm-rail-network, osm-pois-amenities, osm-tourism-attractions ... |
| eog-viirs | Earth Observation Group, Payne Institute, Colorado School of Mines | cc-by-4.0 | S | viirs-nightlights |
| geonames | GeoNames | cc-by-4.0 | S | geonames-postal-codes, geonames-gazetteer |
| dcs-hies | Department of Census and Statistics, Sri Lanka | lk-gov-unstated | M | dcs-hies-poverty, dcs-hies-income-expenditure, dcs-mpi |
| overture-maps | Overture Maps Foundation | cdla-permissive-2.0+odbl | M | overture-places, overture-buildings, sltda-registered-establishments |
| giga-schools | Giga (UNICEF and ITU) | cc-by-4.0 | S | giga-schools |
| nuuuwan-lk-census-2024 | Nuwan Senaratna (nuuuwan) - data from DCS | mit-code+lk-gov-unstated | M | dcs-census-2024-population, dcs-census-2024-households, dcs-census-2024-ethnicity-religion, dcs-census-2024-gnd |
| chirps | Climate Hazards Center, UC Santa Barbara | cc0-1.0 | S | chirps-rainfall |
| wfp-vam | World Food Programme (VAM / HDX) | cc-by-igo-3.0 | S | wfp-food-prices, wfp-rainfall-ndvi-subnational |
| desinventar | UNDRR DesInventar Sendai (data by DMC) | cc-by-4.0 | S | desinventar-losses |
| copernicus-dem | European Space Agency / Airbus (Copernicus) | copernicus-dem-licence | S | copernicus-dem-terrain |
| nuuuwan-lk-elections | Nuwan Senaratna (nuuuwan) - data from Election Commission | mit-code+lk-gov-unstated | S | ec-presidential-results, ec-parliamentary-results, ec-provincial-council-results |
| nuuuwan-sl-topojson | Nuwan Senaratna (nuuuwan) - data from Survey Department / Election Commission | mit-code+lk-gov-unstated | S | electoral-boundaries |
| google-open-buildings | Google Research | cc-by-4.0+odbl | M | google-open-buildings |
| dcs-census-2024 | Department of Census and Statistics, Sri Lanka | lk-gov-unstated | L | dcs-census-2024-population, dcs-census-2024-households, dcs-census-2024-ethnicity-religion, dcs-census-2024-housing ... |

## Enrichment model

Each dataset's `enrichment` block states how it reaches the spine:

- **pcode** - native p-codes (COD-AB, WFP, CCKP after code mapping): direct join.
- **name_match** - government tables keyed by district/DS/RDHS/LA names: resolved through `registry/crosswalks/ (ds-name-aliases.csv and siblings)` (built from COD-AB, Wikidata labels, GeoNames alternates and `admin-region-history` for renames/splits). GN-level tables keyed by DCS ids use `gnd-id-crosswalk`.
- **grid** - rasters are resampled to the canonical 0.001-degree grid; cells map to ADM4 via the reverse-geocode lookup and roll up by p-code. Counts are summed, intensities area- or population-weighted.
- **spatial** - points/lines/polygons are assigned ADM4 p-codes by point-in-polygon or overlay; lines are split at boundaries.
- **non-admin geographies** (polling divisions, local authorities, MOH/RDHS areas, police divisions, education zones, resort regions) get explicit allocation tables to ADM units (population-weighted where they cross boundaries).
- Roll-ups always recompute ratios from numerators/denominators; per-capita versions use matched-year denominators (`derived-per-capita-indicators`).

## Top 25 cross-source analyses this registry unlocks

| # | Question a decision-maker can answer | Datasets combined |
|---|---|---|
| 1 | Which GN divisions have the most people more than 30 minutes from a hospital with a specialist/base tier, and how many of them are over 60? | derived-travel-time-facilities, moh-health-institutions, worldpop-age-sex, osm-routing-graph |
| 2 | How many people, schools and hospitals sit in the 1-in-100-year flood zone in each DS division, and which of those DSs have actually flooded in DesInventar records? | jrc-river-flood-hazard, worldpop-pop-grid, giga-schools, moh-health-institutions, desinventar-losses, derived-flood-exposure |
| 3 | Where did Cyclone Ditwah (Nov-Dec 2025) hit hardest relative to population, and did night-lights recover within 30 days? | observed-flood-extents, dmc-situation-report-impacts, black-marble-ntl-daily, derived-zonal-population |
| 4 | Is the Aswesuma welfare programme reaching the DS divisions with the highest modelled poverty? | aswesuma-beneficiaries, derived-small-area-poverty, dcs-hies-poverty, dcs-census-2024-households |
| 5 | Which districts' paddy failure (harvested/sown) is explained by rainfall deficit versus other causes (e.g. the 2021 fertiliser ban)? | dcs-paddy-statistics, chirps-rainfall, derived-rainfall-anomaly, faostat-land-inputs |
| 6 | Where is economic activity growing fastest between censuses, using night-lights and building growth as proxies calibrated to provincial GDP? | viirs-nightlights, google-open-buildings, dcs-provincial-gdp, derived-nightlights-growth |
| 7 | Which MOH areas show dengue surges 3-6 weeks after heavy rain, and how does incidence relate to building density? | dengue-weekly-cases, chirps-rainfall, derived-building-density, moh-area-boundaries, derived-dengue-risk |
| 8 | Are central transfers to provincial councils per capita aligned with provincial poverty and service gaps? | treasury-province-transfers, dcs-midyear-population-district, dcs-hies-poverty, derived-service-gap-index |
| 9 | Which local authorities collect the least own revenue per resident and also carry the largest audit findings? | dcs-local-government-finance, auditor-general-findings, local-authority-boundaries, derived-zonal-population |
| 10 | How did vote shares shift between 2019, 2024 presidential and 2025 local elections, and how do shifts correlate with age structure, poverty and urbanisation? | ec-presidential-results, ec-local-authority-results, derived-electoral-demographics, ghsl-settlement-model |
| 11 | Where does piped water coverage lag what the census says households use, and which DSs are both underserved and drought-prone? | nwsdb-water-supply, dcs-census-2024-basic-services, derived-rainfall-anomaly, aqueduct-water-risk |
| 12 | How many school-age children live more than 5 km (or 60 walking minutes) from a school, and which schools lack connectivity? | giga-schools, worldpop-age-sex, derived-facility-coverage, moe-school-census |
| 13 | How much aid (WB, IATI, ERD) has gone to each district per capita and per poor person, by sector, over 2010-2025? | wb-projects-lka, iati-activities-lka, erd-foreign-financing, dcs-hies-poverty |
| 14 | Which coastal DS divisions combine high cyclone frequency, mangrove loss and fishing-dependent populations? | cyclone-tracks, mangrove-extent, fisheries-statistics, worldpop-pop-grid |
| 15 | What share of each district's forest loss since 2001 happened inside or next to protected areas? | forest-cover-loss, osm-protected-areas, derived-forest-loss-zonal |
| 16 | Which districts send the most migrant workers per capita, and how does that track remittances and local poverty? | slbfe-labour-migration, cbsl-workers-remittances, dcs-hies-poverty, dcs-midyear-population-district |
| 17 | How many people would a proposed rail/metro alignment newly bring within a 10-minute walk, and which GN divisions remain transit deserts? | derived-transit-access, lk-metro-proposed-lines, osm-rail-network, slr-timetable, lk-gtfs-feeds |
| 18 | Where do landslide warnings recur most often, and how many people and buildings are on steep slopes in those DSs? | dmc-landslide-warnings, copernicus-dem-terrain, google-open-buildings, derived-landslide-exposure |
| 19 | How exposed is each DS to extreme heat days, weighted by elderly population, now and under CMIP6 projections? | era5-land-climate, worldpop-age-sex, cckp-climate-projections, derived-heat-exposure |
| 20 | Are reservoir storage and Maha/Yala rainfall predicting hydropower share and grid stress (and SAIDI interruptions)? | irrigation-reservoir-levels, chirps-rainfall, nso-electricity-generation, pucsl-supply-reliability |
| 21 | How fast are urban areas expanding relative to population growth (SDG 11.3.1), and do UDA zoning plans anticipate it? | derived-urban-growth, ghsl-built-up, ghsl-population-grid, uda-zoning-plans |
| 22 | Which food markets show the widest price gaps from Dambulla wholesale, signalling poor market access? | harti-food-prices, wfp-food-prices, derived-travel-time-facilities |
| 23 | How does tourism capacity (rooms) per district compare with attractions, arrivals by origin and airport connectivity? | sltda-accommodation, sltda-tourist-arrivals, osm-tourism-attractions, bia-flight-arrivals |
| 24 | Where are road crash fatalities per 100k highest after accounting for road length and vehicle fleet? | police-road-accidents, osm-road-network, dmt-vehicle-registrations, dcs-midyear-population-district |
| 25 | How well do the three population models (WorldPop, HRSL, GHSL) agree with Census 2024 at GN level, and where are planning numbers most uncertain? | derived-zonal-population, dcs-census-2024-gnd, worldpop-pop-grid, hrsl-population-30m, ghsl-population-grid |

## Phased ingestion order

**Phase A - spine and geo-native open layers (P0, ~4-5 weeks).** Everything else joins to these, all are clean-licence (CC BY / CC0 / ODbL / CC BY-IGO) and mostly `effort: S`:
1. `hdx-cod-ab-lka` + `geoboundaries-lka` (p-code spine), `nuuuwan-gig-data` -> `gnd-id-crosswalk`, `wikidata` names crosswalk, `geonames`.
2. Population grids: `worldpop` (counts + age/sex), `meta-hrsl`, `ghsl` -> `derived-zonal-population`, `derived-per-capita-indicators`.
3. Geo-native environment: `esa-worldcover`, `copernicus-dem`, `chirps`, `eog-viirs`, `jrc-flood-hazard`.
4. Vector: `osm-geofabrik` (roads, rail, POIs, routing graph, places), `overture-maps` places, `google-open-buildings`, `giga-schools`.
5. `desinventar` losses, `wfp-vam` prices/rainfall, `worldbank-wdi` (open mirror of national macro series).
6. Existing active sources re-pointed to live connectors: `dcs-census-2024` + `nuuuwan-lk-census-2024` (GN), `nuuuwan-lk-elections` + `elections-commission`, `dcs-hies`, `dcs-agriculture` (paddy).

**Phase B - high-value official statistics with row-level provenance (P1, ~6 weeks).** Mostly `permission_unclear` government publications; send the written reuse requests to DCS, CBSL, MoF, MoH in week 1 of this phase and publish with attribution + takedown contact meanwhile. Order: DCS (population estimates, LFS, national accounts incl. provincial GDP, prices), CBSL statistical tables and exchange rates, Treasury debt/transfers/budget estimates, MoH/Epid/NDCU health, MoE school census, NTC/SLR/DMT transport, CEB/PUCSL/NSO/NWSDB utilities, SLTDA, SLBFE, DMC + Irrigation live lanes, Met Dept. Plus first derived products: travel time, facility coverage, flood exposure, nightlights growth, rainfall anomaly, urban growth.

**Phase C - breadth and document extraction (P2, ~6 weeks).** International mirrors (PIP, ILOSTAT, SDG, UNICEF, FAOSTAT, IMF, WITS), climate/hazard extras (ERA5-Land, IMERG, MODIS, GSW, IBTrACS, GDACS, observed floods), class B/C document lanes (Abstract tables, Pocket Book, Auditor General, budget projects, DMC sitreps), remaining derived products (small-area poverty, service-gap index, dengue risk, heat exposure, transit access scenarios).

**Phase D - long tail and licence-gated (P3).** Historic censuses, link-only restricted sources (GADM crosswalk, WHO GHO, WDPA, ACLED, EM-DAT, Ookla) kept as metadata/internal QA only, and blocked sources (NBRO hazard zonation, Survey Department topo) pursued through data-sharing agreements.

## Licence risk

- `restricted` sources (6): `acled` (acled-terms), `gadm-lka` (gadm-noncommercial), `wdpa` (wdpa-terms), `ookla-open-data` (cc-by-nc-sa-4.0), `emdat` (emdat-terms), `who-gho` (cc-by-nc-sa-3.0-igo) - link/metadata or internal QA only, never in open bundles.
- `permission_unclear` is the majority of Sri Lankan government sources (DCS, CBSL, MoF, MoH, MoE, utilities, DMC, Met Dept, Election Commission). nuuuwan's repositories carry MIT-licensed *code* but the scraped *data* keeps the agency's unstated terms, so they are classified the same. Written reuse requests to DCS, CBSL, MoF and MoH are a Phase B prerequisite; where an open international mirror exists (WDI, FAOSTAT, ILOSTAT, PIP, WFP) it is ingested as the default series.
- Share-alike: OSM/Overture buildings/Microsoft/HeiGIT (ODbL), UNOSAT and OpenCelliD (CC BY-SA) outputs are shipped in separate files.

## Blockers

Not reachable on 2026-11-21 (no `verified_at`; re-verify from a Sri Lankan network before building connectors):

| Source | Homepage |
|---|---|
| harti | https://www.harti.gov.lk/ |
| erd | https://www.erd.gov.lk/ |
| naosl | https://www.naosl.gov.lk/ |
| transitland-lmt | https://www.transit.land/feeds/f-lanka~metro~transit |
| ndcu-dengue | https://www.dengue.health.gov.lk/ |
| ugc | https://www.ugc.ac.lk/ |
| imf-data | https://www.imf.org/en/Countries/LKA |
| epid-unit | https://www.epid.gov.lk/ |
| slr | https://railway.gov.lk/ |
| meteo | https://www.meteo.gov.lk/ |

Blocked pending licence/data-sharing agreement:

| Source | Note |
|---|---|
| survey-dept-lk | 1:50k/1:10k topographic layers are sold, not openly licensed. Ask for an open-data release; until then rely on OSM, COD-AB and nuuuwan/survey_gov_lk metadata. |
| nbro | 1:10,000/1:50,000 landslide hazard zonation for 13 districts exists but is not openly downloadable; request data-sharing agreement. Interim: susceptibility mode |

Other gaps: no GTFS exists for SLTB/NTC buses or Sri Lanka Railways (only Lanka Metro Transit and one express route); no open land-price data below the CBSL Colombo indicator; MoE publishes no school-by-school list (Giga + OSM used instead); DCS microdata (NADA) cannot be redistributed.

## Files

- `sources/*.yaml`, `datasets/*.yaml` - registry entries (file stem = `id`).
- `index.csv` - id, title, sources, theme, geo levels, cadence, licence status, priority, effort, publication class.
- `coverage-vs-dc.md` - mapping of developing-country.org's catalogue.
- `schema/` - JSON Schemas (owned by the engine).
