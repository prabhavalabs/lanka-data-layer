"""Generate registry/codelists/*.yaml for the WP-01 missing codelists and annotate the review CSV.

usage: REGISTRY=<repo>/registry python3 gen.py [--write]
"""
import csv, glob, os, sys, collections, yaml
sys.path.insert(0, os.path.dirname(__file__))
from lib import DEFS, emit
import defs_standard, defs_lk, defs_rest  # noqa: F401  (register DEFS)

ROOT = os.environ["REGISTRY"]
WRITE = "--write" in sys.argv
EXISTING = {os.path.splitext(os.path.basename(p))[0] for p in glob.glob(f"{ROOT}/codelists/*.yaml")} - set(DEFS)

# (dim, dataset) -> target codelist  (dataset dims must become {id: dim, codelist: target})
R = {
    "age_band": {"dcs-census-2024-gnd": "age"},
    "beneficiary": {"treasury-guarantees": "soe"},
    "category": {"aswesuma-beneficiaries": "aswesuma_category", "cbsl-merchandise-trade": "trade_category",
                 "dcs-hies-income-expenditure": "coicop_group", "derived-attractions-register": "attraction_type",
                 "overture-places": "overture_category", "world-heritage-sites": "whs_category"},
    "class": {"wikidata-places-entities": "wikidata_class"},
    "component": {"cbsl-pmi": "pmi_component", "dcs-gdp-expenditure": "gdp_expenditure_component"},
    "destination": {"searoute-shipping-lanes": "world_port", "slbfe-labour-migration": "country"},
    "domain": {"dcs-census-2024-disability": "functional_domain", "derived-development-index": "dev_index_domain"},
    "element": {"dcs-census-2024-housing": "housing_element"},
    "event": {"dmc-situation-report-impacts": "disaster_event", "observed-flood-extents": "disaster_event"},
    "facility": {"heigit-accessibility": "facility_type"},
    "fuel": {"dcs-census-2024-basic-services": "cooking_fuel", "slsea-energy-balance": "energy_source"},
    "group": {"dcs-agri-production-index": "agri_index_group", "dcs-ncpi": "coicop_group", "hrsl-population-30m": "demographic_group"},
    "hazard_class": {"nbro-landslide-hazard-zones": "hazard_level"},
    "indicator": {"derived-dasymetric-disaggregation": "source_indicator", "derived-electoral-demographics": "source_indicator",
                  "derived-harmonised-boundary-series": "source_indicator"},
    "institution_type": {"moh-health-institutions": "hospital_type"},
    "intensity": {"derived-multi-hazard-exposure": "hazard_level"},
    "item": {"dcs-census-2024-household-assets": "household_asset"},
    "land_use": {"cbsl-land-valuation-indicator": "land_value_use", "dcs-census-of-agriculture": "holding_land_use"},
    "level": {"giga-schools": "school_level"},
    "line": {"slr-timetable": "rail_line"},
    "material": {"dcs-census-2024-housing": "housing_material", "dcs-pocketbook-prices-wages": "building_material"},
    "method": {"dcs-census-2024-basic-services": "waste_disposal"},
    "origin": {"bia-flight-arrivals": "airport", "searoute-shipping-lanes": "port"},
    "origin_country": {"boi-fdi-inflows": "country"},
    "partner": {"erd-foreign-financing": "development_partner", "wits-trade-partners": "country"},
    "period": {"cckp-climate-projections": "climate_period"},
    "product": {"dcs-pocketbook-agriculture": "livestock_product", "wits-trade-partners": "hs_section"},
    "region": {"sltda-accommodation": "resort_region"},
    "revenue_type": {"dcs-local-government-finance": "lg_revenue_type"},
    "route": {"rda-expressway-programme": "expressway"},
    "scenario": {"cckp-climate-projections": "ssp_scenario", "fewsnet-food-insecurity": "ipc_projection"},
    "sector": {"boi-enterprises": "boi_sector", "boi-fdi-inflows": "boi_sector", "cbsl-pmi": "industry",
               "climate-trace-emissions": "climate_trace_sector", "dcs-lfs-quarterly": "industry",
               "dcs-pocketbook-prices-wages": "industry", "dcs-ppi": "industry", "dcs-provincial-gdp": "industry",
               "erd-foreign-financing": "dac_sector", "iati-activities-lka": "dac_sector", "lk-major-projects": "project_sector",
               "slsea-energy-balance": "energy_end_use_sector", "treasury-soe-finances": "soe_sector", "wb-projects-lka": "wb_sector"},
    "series": {"imf-weo-lka": "weo_subject"},
    "service": {"dcs-pocketbook-sector-services": "telecom_service", "trcsl-telecom-indicators": "telecom_service"},
    "severity": {"dcs-abstract-transport": "accident_severity", "dcs-census-2024-disability": "difficulty_degree",
                 "ntc-bus-statistics": "accident_severity"},
    "skill": {"dcs-census-2024-education-literacy": "language_skill", "slbfe-labour-migration": "migrant_skill_level"},
    "source": {"ceb-statistical-digest": "energy_source", "dcs-pocketbook-sector-services": "energy_source",
               "derived-land-cover-zonal": "land_cover_source", "nso-electricity-generation": "energy_source"},
    "source_country": {"sltda-tourist-arrivals": "country"},
    "status": {"dcs-abstract-justice": "prisoner_status", "lk-major-projects": "project_status", "power-plants": "gem_plant_status",
               "prisons-daily-population": "prisoner_status", "rda-expressway-programme": "project_status",
               "treasury-procurement": "contract_status", "wb-projects-lka": "project_status", "world-heritage-sites": "whs_status"},
    "tax_type": {"treasury-revenue-outturn": "revenue_type"},
    "technology": {"power-plants": "energy_source"},
    "topic": {"cabinet-decisions-geotagged": "cofog_division"},
    "type": {"dcs-pocketbook-social-services": "hospital_type", "osm-tourism-attractions": "attraction_type",
             "osm-trails-cycling": "trail_type", "osm-waterways-waterbodies": "waterway_type", "ourairports-lka": "airport_type",
             "protected-monuments": "monument_type", "sltda-registered-establishments": "establishment_type"},
    "use": {"ghsl-built-up": "building_type"},
    "variant": {"un-wpp-lka": "wpp_variant"},
    "vintage": {"imf-weo-lka": "edition"},
    "walk_minutes": {"derived-transit-access": "threshold"},
    "water_source": {"dcs-census-2024-basic-services": "drinking_water_source", "dcs-pocketbook-agriculture": "irrigation_source"},
    "zone_type": {"noise-control-zones": "noise_zone", "uda-zoning-plans": "uda_zone"},
    "lc_class": {"dynamic-world-annual": "dynamic_world_class"},
}
WHY_GEO = "entity with a location: model as a geography (geo scheme `<scheme>:<id>`, geo_level point) or generate the codelist from the source extract; not hand-curated"
WHY_GEN = "source-defined code list with hundreds of members: generate registry/codelists/<id>.yaml from the source API/extract when the connector is built (as wdi-series)"
# (dim, dataset or '*') -> reason (no codelist written)
P = {
    ("station", "*"): WHY_GEO, ("reservoir", "*"): WHY_GEO, ("market", "*"): WHY_GEO, ("plant", "*"): WHY_GEO,
    ("project", "*"): WHY_GEO, ("zone", "*"): WHY_GEO + " (BOI zones are already a dataset: boi-zones)",
    ("feed", "*"): "GTFS feed ids are not yet known; generate from the feed registry (feed_info.txt / Mobility Database ids)",
    ("agency", "*"): "procuring entities (hundreds): generate from the procurement extract; consider reusing a shared government-entity list with `entity`",
    ("airline", "*"): "IATA airline designators: generate from the BIA schedule extract",
    ("head", "*"): "budget expenditure heads (3-digit, ~200): generate from the Budget Estimates of the year",
    ("segment", "*"): "CBSL condominium survey segments not confirmed from the source; write after reading the CBSL release",
    ("publisher", "*"): "IATI organisation identifiers: generate from the IATI registry publisher list",
    ("table", "*"): "CBSL Annual Report appendix table ids: generate from the appendix index",
    ("target", "*"): "SDG targets (169): generate from the UN SDG API (codes 1.1 ... 17.19)",
    ("storm", "*"): "cyclone ids: use IBTrACS SID, generate from the track extract",
    ("entity", "*"): "audited entities (ministries, SOEs, LAs): generate from the NAO report index; reuse `soe` / local-authority p-codes where possible",
    ("element", "faostat-crops-livestock"): "FAOSTAT element codes (5312 area harvested, 5510 production, ...): generate from the FAOSTAT API definitions",
    ("item", "faostat-crops-livestock"): "FAOSTAT item codes: generate from the FAOSTAT API definitions",
    ("item", "faostat-land-inputs"): "FAOSTAT land-use item codes: generate from the FAOSTAT API definitions",
    ("indicator", "adb-key-indicators-lka"): "ADB KIDB indicator codes: " + WHY_GEN,
    ("indicator", "cbsl-ess-provincial-indicators"): "CBSL ESS provincial table rows: generate from the table extract",
    ("indicator", "dcs-mpi"): "DCS national MPI indicator set (2019) not confirmed from the source report; write after reading it",
    ("indicator", "faostat-food-security"): "FAOSTAT food-security item codes: " + WHY_GEN,
    ("indicator", "ilostat-lka"): "ILOSTAT indicator ids: " + WHY_GEN,
    ("indicator", "unicef-lka"): "UNICEF SDMX indicator codes: " + WHY_GEN,
    ("indicator", "who-gho-lka"): "WHO GHO indicator codes: " + WHY_GEN,
    ("series", "cbsl-annual-review-tables"): "CBSL appendix series: " + WHY_GEN,
    ("series", "un-sdg-lka"): "SDG series codes: " + WHY_GEN,
    ("line", "lk-metro-proposed-lines"): "community-proposal line ids unknown; write from the proposal data",
    ("scenario", "derived-transit-access"): "scenario lines defined by the derived method; write when the method fixes them",
    ("service", "dcs-pocketbook-social-services"): "local-authority service list not confirmed from the Pocket Book; write after reading the table",
}
NOTES = [  # (dataset, dim, note) -> extra review rows
    ("dcs-census-2024-gnd", "age_band", "GN-level band edges not confirmed; every band must exist in `age` (A5 bands 0-14/15-59/60-64/65+ and 60+ are there)"),
    ("dcs-hies-income-expenditure", "category", "HIES expenditure groups are COICOP-like, not COICOP 2018 divisions: add a HIES->COICOP crosswalk or HIES-specific children before ingest"),
    ("dcs-gdp-by-industry", "industry", "DCS GVA lines are finer than ISIC sections (e.g. growing of tea): add ISIC division children to `industry`"),
    ("dcs-pocketbook-prices-wages", "sector", "minimum wage rate index sectors (agriculture; industry and commerce; services) only approximate ISIC broad sectors: confirm or create a wage-board list"),
    ("dcs-lfs-quarterly", "sector", "LFS 'sector' assumed to be agriculture/industry/services (industry aggregates); confirm it is not employment status or public/private sector"),
    ("erd-foreign-financing", "sector", "ERD sector names need a crosswalk to DAC 3-digit codes"),
    ("wb-projects-lka", "status", "WB statuses map pipeline->proposed, active->under_construction, closed->completed, dropped->cancelled"),
    ("derived-land-cover-zonal", "lc_class", "uses ESA WorldCover codes (40 cropland, 10 tree, 50 built-up, 80 water); 'protected area' is not a land-cover class - split into its own indicator"),
    ("dcs-census-historic", "census_year", "census year is the observation period (time_start), not a dimension: drop the dim and use period_type snapshot per census"),
    ("ec-presidential-results", "election", "election event duplicates period + indicator; keep only if one indicator mixes election types"),
    ("worldclim-climate-normals", "bio_var", "BIO variables have different units (degC x10/%, mm): split clim.wc_bio into temperature/precipitation indicators or declare per-variable units"),
    ("dcs-census-of-agriculture", "holding_size", "holding size bands not confirmed against the census table"),
    ("dcs-census-2012", "amenity", "amenity list must be taken from the 2012 GN table headers"),
    ("dcs-ncpi", "group", "food / non-food are the `food` and `nonfood` aggregates of coicop_group"),
]

# ---------------------------------------------------------------- usage
use = collections.defaultdict(list)          # codelist -> [(dataset, indicator, dim)]
bare = collections.defaultdict(list)
for p in sorted(glob.glob(f"{ROOT}/datasets/*.yaml")):
    d = yaml.safe_load(open(p))
    for ind in d.get("indicators") or []:
        for dim in ind.get("dims") or []:
            dim_id = dim if isinstance(dim, str) else dim["id"]
            clid = dim if isinstance(dim, str) else dim["codelist"]
            bare[clid].append((d["id"], ind["id"], dim_id))

rows, problems = [], []
resolved = collections.Counter()
for clid, refs in sorted(bare.items()):
    for ds, ind, dim in refs:
        if clid in EXISTING:
            continue
        tgt = R.get(clid, {}).get(ds)
        if tgt:
            assert tgt in DEFS or tgt in EXISTING, (clid, ds, tgt)
            use[tgt].append(ds)
            rows.append([ds, ind, "dims", f"{{id: {dim}, codelist: {tgt}}}",
                         f"WP-01 codelists: `{clid}` is not a codelist id (ambiguous or shared concept); reference the shared codelist `{tgt}`"])
            resolved["rewrite"] += 1
        elif clid in DEFS:
            use[clid].append(ds)
            resolved["bare"] += 1
        else:
            why = P.get((clid, ds)) or P.get((clid, "*"))
            if not why:
                problems.append((clid, ds, ind))
                continue
            rows.append([ds, ind, "dims", clid, f"WP-01 codelists: unresolved - {why}"])
            resolved["pending"] += 1

for ds, dim, note in NOTES:
    rows.append([ds, "*", "codelist", dim, f"WP-01 codelists: {note}"])
for clid in sorted(DEFS):
    d = DEFS[clid]
    if d["status"] == "seed":
        rows.append([f"codelists/{clid}", "*", "codelist", "seed", "WP-01 codelists: seed list - complete the members from the source extract before ingest"])
    rows.append([f"codelists/{clid}", "*", "labels", "si/ta",
                 "WP-01 codelists: " + ("si/ta given for some codes; native-speaker sign-off needed" if any(x["si"] or x["ta"] for x in d["codes"])
                                       else "si/ta empty - native-speaker translation needed (no machine translation)")])

if problems:
    print("UNHANDLED:", problems)
    sys.exit(1)
unused = [k for k in DEFS if not use.get(k)]
print("codelists defined:", len(DEFS), "unused:", unused)
print("references:", dict(resolved))
shared = {k: sorted(set(v)) for k, v in use.items() if len(set(v)) > 1}
print("shared (>1 dataset):", len(shared))
print("seed:", sorted(k for k, d in DEFS.items() if d["status"] == "seed"))

if WRITE:
    for k, d in DEFS.items():
        with open(f"{ROOT}/codelists/{k}.yaml", "w", encoding="utf-8") as f:
            f.write(emit(d, sorted(set(use.get(k, [])))))
    csvp = f"{ROOT}/review-wp01-standard-model.csv"
    existing = open(csvp, encoding="utf-8").read()
    marker = "WP-01 codelists:"
    kept = [l for l in existing.splitlines() if marker not in l]
    with open(csvp, "w", encoding="utf-8", newline="") as f:
        f.write("\n".join(kept) + "\n")
        w = csv.writer(f, lineterminator="\n")
        for r in rows:
            w.writerow(r)
    print("wrote", len(DEFS), "files and", len(rows), "review rows")
