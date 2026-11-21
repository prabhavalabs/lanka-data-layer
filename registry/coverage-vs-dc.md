# Coverage vs developing-country.org

dc catalogue snapshot: 140 datasets (2026-10/11). **139 mapped to our datasets, 1 excluded with reason.** Many dc tables are merged into fewer, richer datasets of ours (e.g. 4 paddy tables -> `dcs-paddy-statistics`; 10 metro analyses -> `derived-transit-access`). We reuse dc's knowledge of upstream URLs, never their data.

## Mapping

| dc slug | dc title | Our dataset(s) | Primary source | Licence status | Note |
|---|---|---|---|---|---|
| `administrative-units-district` | Administrative units by district | `cod-ab-lka-boundaries` | `hdx-cod-ab-lka` | attribution |  |
| `agricultural-production-index` | Agricultural production index | `dcs-agri-production-index` | `dcs-agriculture` | permission_unclear |  |
| `attractions` | Visitor attractions | `osm-tourism-attractions` | `osm-geofabrik` | attribution |  |
| `bank-branches-density` | Bank branches and banking density | `cbsl-bank-branches` | `cbsl-ess` | permission_unclear |  |
| `boi-zones` | Board of Investment zones | `boi-zones` | `boi` | permission_unclear |  |
| `building-construction-district` | Building construction by district | `dcs-building-construction` | `dcs-statistical-abstract` | permission_unclear |  |
| `building-material-prices` | Building material retail prices | `dcs-pocketbook-prices-wages` | `dcs-pocketbook` | permission_unclear |  |
| `ccpi-monthly` | Colombo Consumer Price Index | `dcs-ccpi` | `dcs-prices` | permission_unclear |  |
| `census-age-sex-dsd` | Population by age and sex | `dcs-census-2024-population` | `dcs-census-2024` | permission_unclear |  |
| `census-basic-services-district` | Household access to basic services | `dcs-census-2024-basic-services` | `dcs-census-2024` | permission_unclear |  |
| `census-ethnicity-district` | Population by ethnic group and district | `dcs-census-2024-ethnicity-religion` | `dcs-census-2024` | permission_unclear |  |
| `census-households-dsd` | Households and basic services | `dcs-census-2024-households` | `dcs-census-2024` | permission_unclear |  |
| `census-housing-structure-district` | Housing unit structure by district | `dcs-census-2024-housing` | `dcs-census-2024` | permission_unclear |  |
| `census-religion-district` | Population by religion and district | `dcs-census-2024-ethnicity-religion` | `dcs-census-2024` | permission_unclear |  |
| `central-government-debt-annual` | Annual central government debt | `cbsl-fiscal-operations` | `cbsl-statistical-tables` | permission_unclear |  |
| `child-nutrition-annual` | Child nutrition | `fhb-maternal-child-health` | `fhb` | permission_unclear |  |
| `colombo-asking-price-indices` | Colombo asking-price indices | `cbsl-condominium-prices` | `cbsl-real-estate` | permission_unclear |  |
| `colombo-land-valuation-indicator` | Colombo Land Valuation Indicator | `cbsl-land-valuation-indicator` | `cbsl-real-estate` | permission_unclear |  |
| `colombo-port-performance` | Colombo port performance | `dcs-abstract-transport` | `dcs-statistical-abstract` | permission_unclear |  |
| `computer-digital-literacy` | Computer and digital literacy | `dcs-census-2024-education-literacy` | `dcs-census-2024` | permission_unclear |  |
| `crop-area-production` | Crop area and production | `dcs-pocketbook-agriculture` | `dcs-pocketbook` | permission_unclear |  |
| `crop-production-costs` | Crop production costs | `dcs-pocketbook-agriculture` | `dcs-pocketbook` | permission_unclear |  |
| `cycling` | Cycling routes | `osm-trails-cycling` | `osm-geofabrik` | attribution |  |
| `demographics` | Population and administrative areas | `dcs-census-2024-gnd` | `nuuuwan-lk-census-2024` | permission_unclear | upgraded: full GN-level Census 2024 with p-code crosswalk |
| `developments` | Public development projects | `lk-major-projects` | `rda` | permission_unclear | curated class C register built from agency sources |
| `effective-exchange-rate-indices` | Nominal and real effective exchange-rate indices | `cbsl-neer-reer` | `cbsl-exchange-rates` | permission_unclear |  |
| `electricity-customers-sales` | Electricity customers and sales | `ceb-statistical-digest` | `ceb` | permission_unclear |  |
| `electricity-generation-daily` | Daily electricity generation | `nso-electricity-generation` | `nso-gensum` | permission_unclear |  |
| `electricity-generation-quarter-hourly` | Electricity generation and battery power | `nso-electricity-generation` | `nso-gensum` | permission_unclear |  |
| `electricity-supply-reliability` | Electricity supply reliability | `pucsl-supply-reliability` | `pucsl` | permission_unclear |  |
| `employment-by-industry` | Employment by industry | `dcs-lfs-annual-subnational` | `dcs-lfs` | permission_unclear |  |
| `employment-by-occupation` | Employment by occupation | `dcs-lfs-annual-subnational` | `dcs-lfs` | permission_unclear |  |
| `energy-supply-consumption` | Energy supply and consumption | `dcs-pocketbook-sector-services` | `dcs-pocketbook` | permission_unclear |  |
| `exchange-rates-monthly` | Monthly exchange rates | `cbsl-exchange-rates` | `cbsl-exchange-rates` | permission_unclear |  |
| `expressways` | Expressways | `osm-road-network` | `osm-geofabrik` | attribution |  |
| `external-trade-indices` | External trade indices | `dcs-pocketbook-trade-indices` | `dcs-pocketbook` | permission_unclear |  |
| `fisheries-production-capacity` | Fisheries production and capacity | `dcs-pocketbook-agriculture` | `dcs-pocketbook` | permission_unclear |  |
| `food-nutrient-availability` | Food and nutrient availability | `dcs-pocketbook-agriculture` | `dcs-pocketbook` | permission_unclear |  |
| `foreign-employment-departures` | Foreign employment departures | `slbfe-labour-migration` | `slbfe` | permission_unclear |  |
| `functional-difficulties-disability` | Functional difficulties and disability | `dcs-census-2024-disability` | `dcs-census-2024` | permission_unclear |  |
| `gdp-by-industry` | GDP by economic activity | `dcs-gdp-by-industry` | `dcs-national-accounts` | permission_unclear |  |
| `gdp-expenditure` | GDP by expenditure | `dcs-gdp-expenditure` | `dcs-national-accounts` | permission_unclear |  |
| `government-fiscal-operations-annual` | Annual government fiscal operations | `cbsl-fiscal-operations` | `cbsl-statistical-tables` | permission_unclear |  |
| `government-health-services` | Government health services | `dcs-pocketbook-social-services`, `moh-health-bulletin-rdhs` | `dcs-pocketbook` | permission_unclear |  |
| `government-revenue-annual` | Annual government revenue composition | `cbsl-fiscal-operations` | `cbsl-statistical-tables` | permission_unclear |  |
| `hospitals` | Hospitals and clinics on OpenStreetMap | `osm-pois-amenities`, `moh-health-institutions` | `osm-geofabrik` | attribution | OSM points plus MoH institution registry |
| `household-equipment-use` | Household equipment use | `dcs-census-2024-household-assets` | `dcs-census-2024` | permission_unclear |  |
| `household-expenditure` | Household expenditure | `dcs-hies-income-expenditure` | `dcs-hies` | permission_unclear |  |
| `household-income-inequality` | Household income and inequality | `dcs-hies-income-expenditure` | `dcs-hies` | permission_unclear |  |
| `household-waste-disposal` | Household waste disposal | `dcs-census-2024-basic-services` | `dcs-census-2024` | permission_unclear |  |
| `housing-condition-materials` | Housing condition and materials | `dcs-census-2024-housing` | `dcs-census-2024` | permission_unclear |  |
| `housing-occupancy-tenure` | Housing occupancy and tenure | `dcs-census-2024-housing` | `dcs-census-2024` | permission_unclear |  |
| `informal-sector-employment-district` | Informal-sector employment by district | `dcs-lfs-annual-subnational` | `dcs-lfs` | permission_unclear |  |
| `inpatient-morbidity-mortality` | Inpatient morbidity and mortality | `moh-immr` | `moh-health` | permission_unclear |  |
| `irrigation-gauges` | River gauges | `irrigation-river-water-levels` | `nuuuwan-lk-irrigation` | permission_unclear |  |
| `labour-employment-profile-history` | Employment by age, education and work status | `dcs-lfs-quarterly` | `dcs-lfs` | permission_unclear |  |
| `labour-force-sex-history` | Labour force by sex | `dcs-lfs-quarterly` | `dcs-lfs` | permission_unclear |  |
| `labour-force` | Labour-force participation and unemployment | `dcs-lfs-annual-subnational` | `dcs-lfs` | permission_unclear |  |
| `land-cover-district` | Land cover by district | `worldcover-land-cover`, `derived-land-cover-zonal` | `esa-worldcover` | attribution | plus our own zonal product at ADM1-ADM4 |
| `land-cover-grid` | Dominant land-cover grid | `worldcover-land-cover` | `esa-worldcover` | attribution |  |
| `language-literacy` | Language literacy | `dcs-census-2024-education-literacy` | `dcs-census-2024` | permission_unclear |  |
| `livestock-production` | Livestock, milk and eggs | `dcs-pocketbook-agriculture` | `dcs-pocketbook` | permission_unclear |  |
| `lk-initiative-events` | Initiative events | excluded: dc's news-extracted initiative ledger (14k articles, 41 reviewed events) - noisy and built on copyrighted news text. Public-money activity is covered by structured sources instead: iati-activities-lka, wb-projects-lka, erd-foreign-financing, budget-capital-projects, cabinet-decisions-geotagged. |  |  |  |
| `lk-initiative-funding` | Initiative funding by stage | `iati-activities-lka` | `iati-datastore` | attribution | replacement, not a copy: structured IATI aid activities instead of news extraction |
| `local-government-expenditure` | Local government expenditure | `dcs-local-government-finance` | `dcs-statistical-abstract` | permission_unclear |  |
| `local-government-revenue` | Local government revenue | `dcs-local-government-finance` | `dcs-statistical-abstract` | permission_unclear |  |
| `local-government-services` | Local government services | `dcs-pocketbook-social-services` | `dcs-pocketbook` | permission_unclear |  |
| `merchandise-trade-category-annual` | Annual merchandise trade by category | `cbsl-merchandise-trade` | `cbsl-statistical-tables` | permission_unclear |  |
| `merchandise-trade-monthly` | Monthly merchandise trade | `cbsl-merchandise-trade` | `cbsl-statistical-tables` | permission_unclear |  |
| `metro-coverage-deciles` | Metro Bus walking coverage by population density | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-coverage-facilities` | Schools, hospitals and offices near Metro Bus stops | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-coverage-gnd` | Metro Bus walking coverage by GN division | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-coverage-lines` | Metro Bus walking coverage by line | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-coverage-stops` | Metro Bus walking coverage by stop | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-gap-clusters` | Dense neighbourhoods beyond Metro Bus walking reach | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-route-candidates` | Illustrative Metro Bus route candidates | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-route-scenarios` | Metro Bus coverage with candidate routes | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro-stop-uniqueness` | Metro Bus stops by residents no other stop reaches | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `metro` | Colombo Metro Bus | `lk-metro-proposed-lines` | `nuuuwan-lk-metro` | attribution | input only; dc's metro analyses are generalised island-wide in derived-transit-access |
| `metro_catchment` | Walking access to Metro Bus | `lk-metro-proposed-lines` | `nuuuwan-lk-metro` | attribution |  |
| `metro_walksheds` | Walking reach of Metro Bus stops | `derived-transit-access` | `lanka-data-layer-derived` | permission_unclear |  |
| `minimum-wage-indices` | Minimum wage indices | `dcs-pocketbook-prices-wages` | `dcs-pocketbook` | permission_unclear |  |
| `national-road-length` | National road network length | `dcs-abstract-transport` | `dcs-statistical-abstract` | permission_unclear |  |
| `natural-disaster-impacts` | Natural disaster impacts | `dcs-pocketbook-social-services`, `dmc-situation-report-impacts` | `dcs-pocketbook` | permission_unclear |  |
| `ncpi-monthly` | National Consumer Price Index | `dcs-ncpi` | `dcs-prices` | permission_unclear |  |
| `new-vehicle-registrations-annual` | New vehicle registrations | `dcs-abstract-transport` | `dcs-statistical-abstract` | permission_unclear |  |
| `osm_foot_graph` | Colombo walking network | `osm-routing-graph` | `osm-geofabrik` | attribution |  |
| `paddy-harvested-area-history` | Paddy harvested area | `dcs-paddy-statistics` | `dcs-agriculture` | permission_unclear |  |
| `paddy-irrigation` | Paddy irrigation | `dcs-pocketbook-agriculture` | `dcs-pocketbook` | permission_unclear |  |
| `paddy-production-history` | Paddy production | `dcs-paddy-statistics` | `dcs-agriculture` | permission_unclear |  |
| `paddy-sown-area-history` | Paddy sown area | `dcs-paddy-statistics` | `dcs-agriculture` | permission_unclear |  |
| `paddy-yield-history` | Paddy yield | `dcs-paddy-statistics` | `dcs-agriculture` | permission_unclear |  |
| `parks` | Parks on OpenStreetMap | `osm-parks-greenspace` | `osm-geofabrik` | attribution |  |
| `population-change-district` | Population change 2012 to 2024 | `dcs-census-2024-population` | `dcs-census-2024` | permission_unclear |  |
| `population-change` | District population change 2012 to 2024 | `dcs-census-2024-population` | `dcs-census-2024` | permission_unclear |  |
| `population-estimates-district` | Mid-year population estimates | `dcs-midyear-population-district` | `dcs-population-vital` | permission_unclear |  |
| `port-traffic-cargo` | Port traffic and cargo | `dcs-abstract-transport` | `dcs-statistical-abstract` | permission_unclear |  |
| `poverty-depth-severity` | Poverty depth and severity | `dcs-hies-poverty` | `dcs-hies` | permission_unclear |  |
| `poverty-headcount-district` | Poverty headcount ratio | `dcs-hies-poverty` | `dcs-hies` | permission_unclear |  |
| `public-transport-capacity` | Public transport capacity | `ntc-bus-statistics` | `ntc` | permission_unclear |  |
| `registered-motor-vehicles-annual` | Registered motor vehicles | `dmt-vehicle-registrations` | `dmt` | permission_unclear |  |
| `road-crashes-fatalities` | Road crashes and fatalities | `ntc-bus-statistics`, `police-road-accidents` | `ntc` | permission_unclear |  |
| `school-examination-results` | School examination results | `dcs-pocketbook-social-services`, `exam-results-district` | `dcs-pocketbook` | permission_unclear |  |
| `school-resources` | School enrolment and resources | `dcs-pocketbook-social-services`, `moe-school-census` | `dcs-pocketbook` | permission_unclear |  |
| `schools` | Schools on OpenStreetMap | `osm-pois-amenities`, `giga-schools` | `osm-geofabrik` | attribution | OSM points plus Giga school geolocation |
| `sea-routes` | Computed sea routes | `searoute-shipping-lanes` | `eurostat-searoute` | attribution |  |
| `telecom-services` | Telecom subscriptions and telephone density | `dcs-pocketbook-sector-services`, `trcsl-telecom-indicators` | `dcs-pocketbook` | permission_unclear |  |
| `tourism-activity` | Tourism activity | `dcs-pocketbook-sector-services` | `dcs-pocketbook` | permission_unclear |  |
| `tourism-earnings-monthly` | Monthly earnings from tourism | `cbsl-tourism-earnings` | `cbsl-statistical-tables` | permission_unclear |  |
| `trails` | Walking and hiking trails | `osm-trails-cycling` | `osm-geofabrik` | attribution |  |
| `transport-passengers-freight` | Transport passengers and freight | `dcs-pocketbook-sector-services` | `dcs-pocketbook` | permission_unclear |  |
| `treasury-domestic-debt` | Domestic government debt by instrument | `treasury-public-debt` | `treasury-debt` | permission_unclear |  |
| `treasury-external-creditors` | External debt by creditor | `treasury-public-debt` | `treasury-debt` | permission_unclear |  |
| `treasury-financial-soundness` | Banking and finance-company soundness | `treasury-fiscal-performance` | `treasury-fiscal-reports` | permission_unclear |  |
| `treasury-fiscal-in-year` | In-year fiscal performance | `treasury-fiscal-performance` | `treasury-fiscal-reports` | permission_unclear |  |
| `treasury-fiscal-performance` | Fiscal budget versus actual performance | `treasury-fiscal-performance` | `treasury-fiscal-reports` | permission_unclear |  |
| `treasury-guarantees` | Treasury guarantees by institution and facility | `treasury-guarantees` | `treasury-debt` | permission_unclear |  |
| `treasury-letters-of-comfort` | Treasury letters of comfort by institution | `treasury-guarantees` | `treasury-debt` | permission_unclear |  |
| `treasury-procurement-bids` | Procurement bid competition | `treasury-procurement` | `treasury-procurement` | permission_unclear |  |
| `treasury-procurement-competitiveness` | Competitive procurement awards | `treasury-procurement` | `treasury-procurement` | permission_unclear |  |
| `treasury-procurement-status` | Procurement outcomes | `treasury-procurement` | `treasury-procurement` | permission_unclear |  |
| `treasury-province-debt` | Local-authority debt by province | `treasury-provincial-debt` | `treasury-debt` | permission_unclear |  |
| `treasury-province-transfers` | Government transfers to provinces | `treasury-province-transfers` | `treasury-annual-report` | permission_unclear |  |
| `treasury-public-debt` | Quarterly public debt composition | `treasury-public-debt` | `treasury-debt` | permission_unclear |  |
| `treasury-revenue-budget-outturn` | Government revenue budget and outturn | `treasury-revenue-outturn` | `treasury-annual-report` | permission_unclear |  |
| `treasury-soe-finances` | SOE annual financial performance | `treasury-soe-finances` | `treasury-annual-report` | permission_unclear |  |
| `treasury-soe-sector-outturn` | SOE sector financial outturn | `treasury-soe-finances` | `treasury-annual-report` | permission_unclear |  |
| `treasury-women-msme-finance` | Women-owned and led MSME finance | `treasury-women-msme-finance` | `treasury-publications` | permission_unclear |  |
| `unemployment-by-province` | Unemployment by province | `dcs-lfs-annual-subnational` | `dcs-lfs` | permission_unclear |  |
| `vital-statistics` | Vital statistics | `dcs-vital-statistics` | `dcs-population-vital` | permission_unclear |  |
| `vocational-training` | Vocational training | `dcs-pocketbook-social-services` | `dcs-pocketbook` | permission_unclear |  |
| `water-operating-performance` | Water utility operating performance | `nwsdb-water-supply` | `nwsdb` | permission_unclear |  |
| `water-supply-connections-coverage` | Water supply connections and coverage | `nwsdb-water-supply` | `nwsdb` | permission_unclear |  |
| `waterfalls` | Waterfalls on OpenStreetMap | `osm-tourism-attractions` | `osm-geofabrik` | attribution |  |
| `weather-station-observations` | Weather station observations | `ghcnd-stations`, `meteo-climate-normals` | `noaa-ghcnd` | open | station series from Met Dept, NOAA GHCN-D |
| `workers-remittances-monthly` | Monthly workers’ remittances | `cbsl-workers-remittances` | `cbsl-statistical-tables` | permission_unclear |  |
| `workplaces` | Offices in Colombo (Overture Maps) | `osm-pois-amenities`, `overture-places` | `osm-geofabrik` | attribution |  |
| `worldbank-economic-outlook` | Economic indicators and forecasts | `wb-sldu-outlook` | `worldbank-sldu` | attribution |  |
| `worldbank-poverty-estimates` | Poverty estimates and forecasts | `wb-sldu-outlook` | `worldbank-sldu` | attribution |  |
| `worldbank-poverty-province` | Provincial poverty estimates | `wb-sldu-outlook` | `worldbank-sldu` | attribution |  |

## Our additions that dc lacks

160 datasets (of 244) and 111 sources (of 167) have no dc equivalent.

**AGRI** (12): `dcs-census-of-agriculture`, `dcs-other-field-crops`, `derived-agri-season-monitor`, `faostat-crops-livestock`, `faostat-food-security`, `faostat-land-inputs`, `fewsnet-food-insecurity`, `fisheries-statistics`, `harti-food-prices`, `soilgrids-properties`, `tea-production-exports`, `wfp-food-prices`

**ECON** (20): `adb-key-indicators-lka`, `cbsl-annual-review-tables`, `cbsl-ess-provincial-indicators`, `cbsl-pmi`, `cbsl-weekly-indicators`, `company-registrations`, `dcs-economic-census-establishments`, `dcs-ppi`, `dcs-provincial-gdp`, `derived-nightlights-growth`, `erd-foreign-financing`, `ilostat-lka`, `imf-weo-lka`, `sltda-accommodation`, `sltda-registered-establishments`, `sltda-tourist-arrivals`, `viirs-nightlights`, `wb-projects-lka`, `wdi-lka`, `wits-trade-partners`

**EDUC** (2): `dcs-abstract-education-health`, `ugc-admissions-district`

**ENER** (4): `black-marble-ntl-daily`, `slsea-energy-balance`, `solar-pv-potential`, `wind-power-density`

**ENVI** (42): `aboveground-biomass`, `aqueduct-water-risk`, `aw3d30-dsm`, `cckp-climate-projections`, `chirps-rainfall`, `climate-trace-emissions`, `copernicus-dem-terrain`, `cyclone-tracks`, `derived-flood-exposure`, `derived-forest-loss-zonal`, `derived-heat-exposure`, `derived-landslide-exposure`, `derived-rainfall-anomaly`, `derived-terrain-zonal`, `desinventar-losses`, `dmc-landslide-warnings`, `dynamic-world-annual`, `earthquakes-near-lka`, `emdat-lka`, `era5-land-climate`, `firms-active-fires`, `forest-cover-loss`, `gbif-biodiversity`, `gdacs-alerts`, `gpm-imerg-precipitation`, `hydrosheds-rivers-basins`, `irrigation-reservoir-levels`, `jrc-river-flood-hazard`, `jrc-surface-water`, `mangrove-extent`, `meteo-daily-station-weather`, `modis-ndvi`, `nasadem`, `nbro-landslide-hazard-zones`, `observed-flood-extents`, `open-meteo-forecast`, `openaq-air-quality`, `osm-protected-areas`, `osm-waterways-waterbodies`, `thinkhazard-levels`, `wdpa-protected-areas`, `wfp-rainfall-ndvi-subnational`

**GOVE** (14): `auditor-general-findings`, `budget-capital-projects`, `budget-estimates-heads`, `cabinet-decisions-geotagged`, `dcs-survey-catalogue`, `derived-electoral-demographics`, `ec-local-authority-results`, `ec-parliamentary-results`, `ec-presidential-results`, `ec-provincial-council-results`, `electoral-boundaries`, `local-authority-boundaries`, `parliament-members`, `ti-cpi-lka`

**HEAL** (12): `dcs-dhs-2016-district`, `dengue-weekly-cases`, `derived-dengue-risk`, `derived-facility-coverage`, `epid-notifiable-diseases`, `healthsites-facilities`, `heigit-accessibility`, `map-travel-time-surfaces`, `moh-area-boundaries`, `unicef-lka`, `who-gho-lka`, `worldpop-births-pregnancies`

**JUST** (4): `acled-events-lka`, `dcs-abstract-justice`, `prisons-daily-population`, `ucdp-ged-lka`

**REGI** (18): `admin-region-history`, `derived-building-density`, `derived-urban-growth`, `gadm-gid-crosswalk`, `geonames-gazetteer`, `geonames-postal-codes`, `ghsl-built-up`, `ghsl-settlement-model`, `gnd-id-crosswalk`, `google-open-buildings`, `ms-building-footprints`, `osm-buildings`, `osm-places`, `overture-buildings`, `survey-topographic-layers`, `uda-zoning-plans`, `wikidata-admin-crosswalk`, `wikidata-places-entities`

**SOCI** (19): `aswesuma-beneficiaries`, `dcs-census-2012`, `dcs-census-historic`, `dcs-mpi`, `derived-per-capita-indicators`, `derived-service-gap-index`, `derived-small-area-poverty`, `derived-zonal-population`, `ghsl-population-grid`, `hrsl-population-30m`, `idmc-displacements`, `kontur-population-h3`, `landscan-ambient-population`, `un-sdg-lka`, `un-wpp-lka`, `undp-hdi-lka`, `wb-pip-poverty`, `worldpop-age-sex`, `worldpop-pop-grid`

**TECH** (2): `ookla-speed-tiles`, `opencellid-towers`

**TRAN** (11): `bia-flight-arrivals`, `colombo-bus-rail-network`, `derived-travel-time-facilities`, `heigit-road-surface`, `imf-portwatch-lka`, `lk-gtfs-feeds`, `osm-rail-network`, `ourairports-lka`, `rda-national-roads`, `slpa-port-statistics`, `slr-timetable`

**Sources dc does not use:** `acled`, `adb-kidb`, `caasl`, `cbsl-annual-reports`, `cbsl-economic-indicators`, `chirps`, `climate-trace`, `copernicus-cds`, `copernicus-dem`, `copernicus-ems`, `ctrees`, `dcs-dhs`, `dcs-economic-census`, `dcs-nada`, `desinventar`, `dhs-program`, `dynamic-world`, `elections-commission`, `emdat`, `eog-viirs`, `eos-rs`, `epid-unit`, `erd`, `faostat`, `fewsnet`, `fisheries-ministry`, `gadm-lka`, `gbif`, `gdacs`, `geonames`, `ghsl`, `global-mangrove-watch`, `global-solar-atlas`, `global-wind-atlas`, `google-open-buildings`, `hansen-gfc`, `harti`, `heigit`, `hydrosheds`, `ibtracs`, `idmc`, `ilostat`, `imf-data`, `imf-portwatch`, `jaxa-aw3d30`, `jrc-flood-hazard`, `jrc-gsw`, `kontur-population`, `landscan`, `map-accessibility`, `meta-hrsl`, `mobilitydb-lk`, `ms-buildings`, `naosl`, `nasa-earthdata`, `nasa-firms`, `nbro`, `ndcu-dengue`, `nuuuwan-cmb-transit`, `nuuuwan-gig-data`, `nuuuwan-lg-election-2025`, `nuuuwan-lk-admin-regions`, `nuuuwan-lk-cabinet-decisions`, `nuuuwan-lk-census-1971`, `nuuuwan-lk-census-2001`, `nuuuwan-lk-census-2012`, `nuuuwan-lk-dengue`, `nuuuwan-lk-elections`, `nuuuwan-lk-eroc`, `nuuuwan-lk-moh-regions`, `nuuuwan-lk-tourism`, `nuuuwan-sl-topojson`, `nuuuwan-weather-lk`, `ookla-open-data`, `open-meteo`, `openaq`, `opencellid`, `ophi-mpi`, `ourairports`, `parliament-lk`, `prisons`, `slr`, `slsea`, `sltda`, `soilgrids`, `survey-dept-lk`, `tea-board`, `thinkhazard`, `transitland-lmt`, `transparency-intl`, `treasury-budget-estimates`, `ucdp`, `ugc`, `un-sdg`, `un-wpp`, `undp-hdr`, `unicef-data`, `unosat`, `usgs-earthquakes`, `wb-cckp`, `wbb-aswesuma`, `wdpa`, `wfp-vam`, `who-gho`, `wikidata`, `wits-comtrade`, `worldbank-pip`, `worldbank-projects`, `worldbank-wdi`, `worldpop`, `wri-aqueduct`
