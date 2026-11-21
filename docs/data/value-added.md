# Value-added datasets (beyond developing-country.org)

These are products dc does not offer. Each combines official Sri Lankan statistics with international and open layers through the **enrichment module** described below. All are registered as `derived-*` datasets of source `lanka-data-layer-derived`, or as new open inputs. Inputs below are registry dataset ids. Effort is person-days *after* the inputs are ingested, and covers method, validation, uncertainty and the method page.

## The enrichment module

A derived product is a run of the engine over published input versions. It is never a hand-edited file. Every output observation carries:

- `method`: `derived`, `modelled` or `spatial`.
- `status`: `modelled`, `estimate` or `forecast` (from the v2 enum).
- `confidence`: a 0-1 score.
- `locator`: `{formula, inputs:[dataset_version…], params}`.

Each product has a method page with its limits.

The stages run in this order:

1. **Harmonise geography.** Bring every input to 2024 ADM1-ADM4 p-codes and the 0.001 degree grid, through `geo.crosswalks`: area and population overlap weights, GN recodes and `admin-region-history`.
2. **Harmonise time.** Match denominators to the same year using intercensal and mid-year estimates. Never use a census-year denominator for a 2019 numerator.
3. **Disaggregate and aggregate.**
   - Going down (district to DS to GN to grid): dasymetric allocation only, with source totals preserved (pycnophylactic constraint).
   - Going up: recompute rates from numerators and denominators; rates are never averaged.
4. **Model** where observation is impossible: small-area estimation, interpolation, accessibility. Every modelled value carries an interval or quality class.
5. **Normalise and composite.** Per-capita, per-km2 and per-household values, z/min-max scores, composite indices with published weights and sensitivity.
6. **QA gates.**
   - Sum-to-total checks against the parent unit (tolerance 0.5%).
   - Back-casting against held-out census values.
   - Cross-model agreement (WorldPop vs HRSL vs GHSL vs Census).
   - A run that fails a gate is not published.

**Interpolation policy.**
- Spatial interpolation (kriging or IDW) is used **only for continuous physical fields sampled at stations**: rainfall and temperature station gaps, groundwater and air quality.
- It is **never** used for counts or administrative statistics. Those use dasymetric or areal-weighted methods.
- Temporal interpolation of populations uses geometric growth or ratio-to-trend, constrained to official controls.

## Top 20, ranked by value to users ÷ effort, with dependency order respected

| # | Dataset (registry id) | Inputs (registry ids) | Method (citable) | Output levels | Uncertainty / quality flags we must publish | Effort (d) | Value to users |
|---|---|---|---|---|---|---|---|
| 1 | `derived-harmonised-boundary-series` (new) | `admin-region-history`, `gnd-id-crosswalk`, `cod-ab-lka-boundaries`, `google-open-buildings`, `ghsl-population-grid` | Areal + population-weighted interpolation between boundary vintages (Logan, Xu & Stults 2014, LTDB); counts reallocated, rates recomputed | ADM2-ADM4, 1981-2026 | `status=modelled` on any re-based value; `allocation_share` per source/target unit pair; flag where share < 0.95 or the GN was split or merged | 8 | Makes 2001/2012/2024 census, HIES and elections comparable on one map; prerequisite for every trend below |
| 2 | `derived-intercensal-population` (new) | `dcs-census-2012`, `dcs-census-2024-gnd`, `dcs-midyear-population-district`, `gnd-id-crosswalk`, `admin-region-history`, `un-wpp-lka` | Geometric (exponential) growth per GN on 2024 boundaries; raked by IPF to DCS district mid-year totals; 2025-2030 by ratio-to-trend shares under DCS/UN WPP controls (UN Manual X; Swanson & Tayman 2012) | ADM1-ADM4 + grid, annual 2012-2030, by sex | `status=estimate` (2013-2023) / `forecast` (2025-2030); interval half-width from back-cast error at 2024; flag GNs with recodes; never labelled "census" | 6 | Correct denominators for every per-capita rate in any year. dc uses census-year totals only |
| 3 | `derived-zonal-population` | `worldpop-pop-grid`, `hrsl-population-30m`, `ghsl-population-grid`, `dcs-census-2024-gnd`, `kontur-population-h3` | Zonal sums on the canonical grid; model-agreement statistics against Census 2024 | ADM1-ADM4, grid | Per-GN coefficient of variation across models; `status=modelled` where a GN has < 10 cells | 4 | One honest population number per unit, with a disagreement band |
| 4 | `derived-per-capita-indicators` | all count indicators + #2 | Ratio with matched-year denominators; density from `admin.area_km2` | every level the numerator has | Inherits numerator status; flags denominator vintage | 3 | Comparability across districts and over time, at no extra cost per dataset |
| 5 | `derived-dasymetric-disaggregation` (new) | `worldpop-pop-grid`, `google-open-buildings`, `worldcover-land-cover`, `dcs-lfs-annual-subnational`, `dcs-hies-income-expenditure` | Intelligent dasymetric mapping (Mennis & Hultgren 2006), with buildings and residential land cover as ancillary; pycnophylactic total preservation (Tobler 1979) | ADM4, grid | `status=modelled`; quality class A/B/C by ancillary-density fit; source totals reproduced exactly (gate) | 6 | District/DS statistics (employment, households, vehicles) become usable for GN-level planning |
| 6 | `derived-travel-time-facilities` | `osm-routing-graph`, `osm-road-network`, `moh-health-institutions`, `giga-schools`, `osm-pois-amenities` | Network travel time (OSRM/Valhalla) plus a friction-surface cost-distance fallback (Weiss et al. 2018, Nature; 2020, Nat. Med.) | grid, ADM3-ADM4 | Mode and speed assumptions in the locator; `confidence` lower where road completeness is low (OSM vs `rda-national-roads` km ratio) | 8 | "Who is far from a hospital or school?" Our largest single gap-closer over dc's Colombo-only walksheds |
| 7 | `derived-facility-coverage` | #6, #2, `worldpop-age-sex` | Population within 15/30/60 min, by age group | ADM2-ADM4 | Inherits #6; age split from WorldPop flagged `modelled` | 3 | SDG 3.8 / 4.a style access indicators by GN |
| 8 | `derived-multi-hazard-exposure` (new) | `jrc-river-flood-hazard`, `derived-landslide-exposure`, `cyclone-tracks`, `derived-rainfall-anomaly`, `copernicus-dem-terrain`, `google-open-buildings`, `worldpop-pop-grid`, `giga-schools`, `moh-health-institutions` | Hazard x exposure overlay on the grid; INFORM Risk aggregation (JRC INFORM methodology) | ADM2-ADM4 | Hazard return period and model version per row; NBRO zonation not open, so a susceptibility proxy is used and flagged; counts overlaid, never interpolated | 6 | DRR planning, insurance, post-Ditwah recovery targeting |
| 9 | `derived-flood-exposure` | `jrc-river-flood-hazard`, `worldpop-pop-grid`, `google-open-buildings`, `giga-schools`, `moh-health-institutions`, `desinventar-losses` | Overlay at RP10/RP100; validation against DesInventar loss records | ADM2-ADM4 | Return period; validation hit-rate vs DesInventar per district | 4 | Ships early because inputs are open; feeds #8 |
| 10 | `derived-small-area-poverty` | `dcs-hies-poverty`, `dcs-census-2024-gnd`, `viirs-nightlights`, #6, `google-open-buildings`, `aswesuma-beneficiaries`; `meta-rwi-wealth-index` for internal validation only | Fay-Herriot area-level model / EBP (Elbers, Lanjouw & Lanjouw 2003; Molina & Rao 2010), benchmarked to HIES district estimates | ADM3 (publish), ADM4 (publish only where CV < 30%) | 95% interval and CV per unit; suppress (`status=suppressed`) where CV > 30%; "not for individual targeting" in limits | 15 | Poverty below district level. HIES stops at district, and dc stops at district/province |
| 11 | `derived-market-access` (new) | `osm-routing-graph`, `osm-road-network`, `osm-places`, `ghsl-settlement-model`, `worldpop-pop-grid` | Cost-distance to towns of 20k/50k+ (Weiss et al. 2018 accessibility to cities; GHSL Degree of Urbanisation for town definition) | grid, ADM3-ADM4 | Town threshold and year in the locator; no congestion (limit) | 3 | Rural remoteness, an input to poverty, agriculture and price-gap analyses |
| 12 | `derived-nightlights-growth` | `viirs-nightlights`, `black-marble-ntl-daily`, `dcs-provincial-gdp` | Annual VNL composites; sum of lights, calibrated to provincial GDP (Henderson, Storeygard & Weil 2012) | ADM2-ADM4 | Gas-flare/fishing-boat masking flags; elasticity confidence interval; `status=modelled` for any GDP-equivalent | 4 | Sub-national economic activity proxy between provincial GDP releases |
| 13 | `derived-service-gap-index` | `dcs-census-2024-basic-services`, #7, `nwsdb-water-supply`, `giga-schools`, `ookla-speed-tiles` (internal only) | Normalised gap scores per service; equal-weight plus PCA-weight variants | ADM3-ADM4 | Both weightings published; component scores always shown next to the index | 5 | Where water, health, school and connectivity gaps coincide |
| 14 | `derived-development-index` (new) | `dcs-census-2024-gnd`, `dcs-census-2024-basic-services`, #7, #10, `viirs-nightlights`, #11 | OECD/JRC Handbook on Constructing Composite Indicators (2008): normalise, weight (PCA-informed), geometric aggregation, Monte Carlo uncertainty and sensitivity analysis | ADM3-ADM4 | Rank 90% interval; domain sub-indices; weight-sensitivity table; no single-number league table without intervals | 6 | One comparable local-development measure for allocation formulas (e.g. province transfers) |
| 15 | `derived-transit-access` | `lk-gtfs-feeds`, `lk-metro-proposed-lines`, `colombo-bus-rail-network`, `osm-rail-network`, `slr-timetable`, #6 | Network walksheds (400/800 m, 10 min); GN population allocation by dasymetric weights rather than dc's uniform spread | ADM4, stop, line | Frequency unknown for most SLTB/NTC routes (flag); uniform-spread vs dasymetric comparison published | 8 | dc's 10 Colombo metro analyses generalised island-wide to any operator and any scenario |
| 16 | `derived-attractions-register` (new) | `osm-tourism-attractions`, `overture-places`, `wikidata-places-entities`, `world-heritage-sites`, `protected-monuments`, `osm-protected-areas` | Entity resolution (name similarity + 250 m + Wikidata QID; Christen 2012, *Data Matching*) with per-attribute provenance | point, ADM1-ADM4 | Match confidence per merged entity; designation source (UNESCO / gazette / OSM) | 5 | Authoritative attractions layer; dc's is raw OSM (1,111 points) |
| 17 | `derived-tourism-pressure-index` (new) | `sltda-accommodation`, `sltda-registered-establishments`, #16, `sltda-tourist-arrivals`, #2 | Defert tourist function index (beds/residents x 100) plus attraction density; min-max composite (UNWTO carrying-capacity indicators) | ADM2-ADM3 | Registered beds only (informal stock missing); weights published | 3 | Destination management, local-authority revenue planning |
| 18 | `derived-investment-density` (new) | `boi-enterprises`, `boi-zones`, `boi-fdi-inflows`, `iati-activities-lka`, `wb-projects-lka`, `budget-capital-projects`, #2 | Point-in-polygon sums per unit, per km2 and per capita; kernel density for display only | ADM2-ADM3 | Location precision class (site / address / district) per project; multi-district split rule | 4 | Where public, aid and BOI money lands relative to people and poverty |
| 19 | `derived-rainfall-anomaly` + `derived-agri-season-monitor` | `chirps-rainfall`, `era5-land-climate`, `modis-ndvi`, `dcs-paddy-statistics`, `meteo-daily-station-weather` | SPI-3/SPI-6 (McKee et al. 1993) from CHIRPS; station gap-filling by ordinary kriging with elevation drift (justified: continuous field) | grid, ADM2-ADM4, Maha/Yala season | CHIRPS station density flag; SPI baseline period; kriging variance where used | 4 | Drought early warning; explains paddy failures district by district |
| 20 | `derived-heat-exposure` | `era5-land-climate`, `worldpop-age-sex`, `cckp-climate-projections` | Days above WBGT/Tmax thresholds x population 65+ (Lancet Countdown heat-exposure indicator) | ADM2-ADM4, CMIP6 SSPs | Reanalysis bias note; projection ensemble spread | 3 | Health and labour planning under climate change |

**Total for the top 20 is about 108 person-days.** Items 1-4 (21 d) are the enabling core. Every later item consumes them.

## Also registered

These are lower rank and are not part of the top-20 total:

- `derived-landslide-exposure` (4 d): blocked on NBRO zonation; susceptibility proxy meanwhile.
- `derived-urban-growth` (3 d): SDG 11.3.1 from GHSL.
- `derived-dengue-risk` (5 d): needs NDCU weekly series from an LK vantage point.
- `derived-electoral-demographics` (3 d).
- `derived-forest-loss-zonal` (2 d).
- `derived-building-density` (2 d).
- `derived-land-cover-zonal` (1 d).
- `derived-terrain-zonal` (1 d).

New open inputs that feed the products above: `power-plants` (GEM, CC BY 4.0), `world-heritage-sites` (UNESCO), `protected-monuments` (Archaeology Dept), `worldclim-climate-normals`, plus `meta-rwi-wealth-index` (CC BY-NC: internal validation of #10 only, never redistributed).

## Publication rules for derived products

- The licence is the strictest licence among the inputs. Most products are `permission_unclear` because DCS denominators are; the exceptions are #6-#9, #11 and #20, which run on open inputs only and ship first.
- Modelled values never overwrite reported values. They are separate indicators with `status` set to modelled, estimate or forecast.
- Each product's method page states the following before the first release:
  - what the product cannot tell you
  - the validation score, against held-out census values or DesInventar
  - the interval method
