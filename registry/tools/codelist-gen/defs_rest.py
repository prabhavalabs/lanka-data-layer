"""Remaining codelists: elections, entities (seeds), access, health, generated lists."""
import glob, os, yaml
from lib import c, T, cl

# ---------------------------------------------------------------- elections
_el = [c(f"pres-{y}", f"Presidential election {y}") for y in (1982, 1988, 1994, 1999, 2005, 2010, 2015, 2019, 2024)]
_el += [c(f"parl-{y}", f"Parliamentary election {y}") for y in (1989, 1994, 2000, 2001, 2004, 2010, 2015, 2020, 2024)]
_el += [c(f"pc-{y}", f"Provincial council elections {y} (round)") for y in (1988, 1993, 1999, 2004, 2008, 2012, 2013, 2014)]
_el += [c(f"lg-{y}", f"Local authorities elections {y}") for y in (2006, 2011, 2018, 2025)]
cl("election", "Election event", _el,
   source=["Election Commission of Sri Lanka (elections.gov.lk), past election results."],
   status="seed",
   notes=["Code = <type>-<year>. Provincial council and local authority elections were staggered across years: a round may need several codes (e.g. pc-2012 Eastern/Sabaragamuwa/North Central); confirm against EC results before ingest.",
          "Events are not additive: no total. The election is arguably the period+indicator, see review CSV."])
_party = [T("All parties and independent groups")] + [c(k, v) for k, v in [
    ("npp", "National People's Power (NPP)"), ("sjb", "Samagi Jana Balawegaya (SJB)"), ("slpp", "Sri Lanka Podujana Peramuna (SLPP)"),
    ("unp", "United National Party (UNP)"), ("slfp", "Sri Lanka Freedom Party (SLFP)"), ("upfa", "United People's Freedom Alliance (UPFA)"),
    ("unfgg", "United National Front for Good Governance (UNFGG)"), ("jvp", "Janatha Vimukthi Peramuna (JVP)"),
    ("itak", "Ilankai Tamil Arasu Kachchi (ITAK)"), ("tna", "Tamil National Alliance (TNA)"), ("actc", "All Ceylon Tamil Congress (ACTC)"),
    ("epdp", "Eelam People's Democratic Party (EPDP)"), ("slmc", "Sri Lanka Muslim Congress (SLMC)"),
    ("acmc", "All Ceylon Makkal Congress (ACMC)"), ("cwc", "Ceylon Workers' Congress (CWC)"),
    ("independent", "Independent groups"), ("other", "Other parties")]]
cl("party", "Political party or alliance (as contested)", _party,
   source=["Election Commission of Sri Lanka - registered political parties and results by party / independent group."],
   status="seed",
   notes=["Parties often contest under an alliance name and symbol (e.g. SLFP within UPFA, SLPP alliance): code the name under which votes were cast; party-alliance links belong in a crosswalk, not parent codes.",
          "Proper names: si/ta labels must be the EC registered names (native review), never translated."])
_cand = [T("All candidates")] + [c(k, v) for k, v in [
    ("anura_kumara_dissanayake", "Anura Kumara Dissanayake"), ("sajith_premadasa", "Sajith Premadasa"),
    ("ranil_wickremesinghe", "Ranil Wickremesinghe"), ("namal_rajapaksa", "Namal Rajapaksa"),
    ("gotabaya_rajapaksa", "Gotabaya Rajapaksa"), ("maithripala_sirisena", "Maithripala Sirisena"),
    ("mahinda_rajapaksa", "Mahinda Rajapaksa"), ("sarath_fonseka", "Sarath Fonseka"), ("other", "Other candidates")]]
cl("candidate", "Presidential candidate", _cand,
   source=["Election Commission of Sri Lanka - presidential election results by candidate."], status="seed",
   notes=["Code = person slug, stable across elections. Complete from the EC results of each election (2024 had 38 candidates).",
          "Proper names: si/ta from EC nomination lists only."])

# ---------------------------------------------------------------- SOEs and partners (seeds)
_soe = [T("All state-owned enterprises")] + [c(k, v) for k, v in [
    ("ceb", "Ceylon Electricity Board"), ("cpc", "Ceylon Petroleum Corporation"), ("srilankan_airlines", "SriLankan Airlines Limited"),
    ("nwsdb", "National Water Supply and Drainage Board"), ("slpa", "Sri Lanka Ports Authority"),
    ("aasl", "Airport and Aviation Services (Sri Lanka) (Private) Limited"), ("boc", "Bank of Ceylon"), ("peoples_bank", "People's Bank"),
    ("nsb", "National Savings Bank"), ("slic", "Sri Lanka Insurance Corporation"), ("sltb", "Sri Lanka Transport Board"),
    ("other", "Other SOEs")]]
cl("soe", "State-owned enterprise", _soe,
   source=["Ministry of Finance, Annual Report - performance of the 52 strategic state-owned business enterprises; Department of Public Enterprises."],
   status="seed", notes=["Shared by treasury-soe-finances (soe) and treasury-guarantees (beneficiary). Complete from the Treasury SOE list."])
cl("soe_sector", "SOE sector grouping (Treasury)",
   [T("All SOEs")] + [c(k, v) for k, v in [("energy", "Energy"), ("transport_logistics", "Transport and logistics (ports, aviation, bus)"),
                                           ("water", "Water supply"), ("banking_finance", "Banking, finance and insurance"),
                                           ("agriculture_plantation", "Agriculture and plantations"), ("trade_services", "Trade and services"),
                                           ("other", "Other")]],
   source=["Ministry of Finance, Annual Report - SOE sector groupings."], status="seed")
cl("development_partner", "Development partner (lender / donor)",
   [T("All development partners"), c("multilateral", "Multilateral"), c("bilateral", "Bilateral"),
    c("adb", "Asian Development Bank", p="multilateral"), c("ida", "World Bank - IDA", p="multilateral"),
    c("ibrd", "World Bank - IBRD", p="multilateral"), c("aiib", "Asian Infrastructure Investment Bank", p="multilateral"),
    c("eu", "European Union", p="multilateral"), c("un", "United Nations agencies", p="multilateral"),
    c("ifad", "International Fund for Agricultural Development", p="multilateral"), c("opec_fund", "OPEC Fund", p="multilateral"),
    c("JP", "Japan", p="bilateral"), c("CN", "China", p="bilateral"), c("IN", "India", p="bilateral"), c("KR", "Korea, Republic of", p="bilateral"),
    c("DE", "Germany", p="bilateral"), c("FR", "France", p="bilateral"), c("US", "United States of America", p="bilateral"),
    c("SA", "Saudi Arabia (Saudi Fund)", p="bilateral"), c("KW", "Kuwait (Kuwait Fund)", p="bilateral"), c("other", "Other")],
   source=["Department of External Resources (ERD), Ministry of Finance - foreign financing by development partner."],
   status="seed", notes=["Bilateral partners use ISO 3166-1 alpha-2 (as in `country`); agencies of one country (JICA, KEXIM) roll up to the country."])
cl("project_sector", "Major project sector",
   [T("All projects")] + [c(k, v) for k, v in [("transport", "Transport (roads, expressways, rail)"), ("ports_aviation", "Ports and aviation"),
                                               ("urban_development", "Urban development"), ("energy", "Energy"), ("irrigation_water", "Irrigation and water"),
                                               ("industrial_zone", "Industrial / investment zones"), ("other", "Other")]],
   source=["Lanka Data Layer curated major projects register (lk-major-projects)."], status="seed")

# ---------------------------------------------------------------- generated / source-defined lists (seeds)
cl("weo_subject", "IMF WEO subject code",
   [c(k, v) for k, v in [("NGDP_RPCH", "Gross domestic product, constant prices (percent change)"),
                         ("NGDPD", "Gross domestic product, current prices (U.S. dollars)"),
                         ("NGDPDPC", "Gross domestic product per capita, current prices (U.S. dollars)"),
                         ("PCPIPCH", "Inflation, average consumer prices (percent change)"),
                         ("PCPIEPCH", "Inflation, end of period consumer prices (percent change)"),
                         ("LUR", "Unemployment rate"), ("LP", "Population"),
                         ("GGR_NGDP", "General government revenue (percent of GDP)"),
                         ("GGX_NGDP", "General government total expenditure (percent of GDP)"),
                         ("GGXCNL_NGDP", "General government net lending/borrowing (percent of GDP)"),
                         ("GGXWDG_NGDP", "General government gross debt (percent of GDP)"),
                         ("BCA_NGDPD", "Current account balance (percent of GDP)")]],
   source=["IMF World Economic Outlook database, WEO subject codes."], status="seed",
   notes=["Series are not additive: no total. Complete from the WEO download for the vintage."])
_ed = [c(f"{y}-{m:02d}", f"{y}-{m:02d} edition") for y in range(2000, 2028) for m in range(1, 13)]
cl("edition", "Publication edition (year-month)", _ed,
   source=["Release month of a periodical publication (IMF WEO April/October vintages; World Bank Sri Lanka Development Update editions)."],
   notes=["Shared by wb-sldu-outlook (edition) and imf-weo-lka (vintage). Editions are alternative vintages: no total.",
          "Year-months 2000-01..2027-12 are pre-listed so new editions need no registry change until 2028."])
cl("world_port", "World seaport (UN/LOCODE)",
   [c(k, v) for k, v in [("SGSIN", "Singapore"), ("NLRTM", "Rotterdam"), ("CNSHA", "Shanghai"), ("AEJEA", "Jebel Ali"),
                         ("DEHAM", "Hamburg"), ("INMAA", "Chennai"), ("INBOM", "Mumbai"), ("MYPKG", "Port Klang"), ("HKHKG", "Hong Kong")]],
   source=["UNECE UN/LOCODE (code list for trade and transport locations)."], status="seed",
   notes=["Destination ports of generated sea routes: complete from the searoute destination list. Routes are alternatives: no total."])
cl("wikidata_class", "Wikidata item class (instance of, grouped)",
   [T("All items")] + [c(k, v) for k, v in [("city", "City"), ("town", "Town"), ("village", "Village"), ("buddhist_temple", "Buddhist temple"),
                                            ("hindu_temple", "Hindu temple"), ("mosque", "Mosque"), ("church", "Church"), ("school", "School"),
                                            ("hospital", "Hospital"), ("university", "University"), ("heritage_site", "Heritage site"),
                                            ("museum", "Museum"), ("mountain", "Mountain / hill"), ("lake_reservoir", "Lake / reservoir"),
                                            ("other", "Other")]],
   source=["Wikidata (P31 instance of) - classes grouped for Sri Lanka items."], status="seed",
   notes=["Crosswalk group -> Wikidata class QIDs (incl. subclasses via P279) to be stored with the connector."])
cl("overture_category", "Overture Places category (top level)",
   [T("All places")] + [c(k, v) for k, v in [
       ("eat_and_drink", "Eat and drink"), ("shopping", "Shopping"), ("accommodation", "Accommodation"),
       ("arts_and_entertainment", "Arts and entertainment"), ("active_life", "Active life"), ("beauty_and_spa", "Beauty and spa"),
       ("education", "Education"), ("health_and_medical", "Health and medical"), ("financial_service", "Financial service"),
       ("public_service_and_government", "Public service and government"), ("religious_organization", "Religious organization"),
       ("automotive", "Automotive"), ("travel", "Travel"), ("professional_services", "Professional services"),
       ("home_service", "Home service"), ("real_estate", "Real estate"), ("mass_media", "Mass media"), ("pets", "Pets"),
       ("business_to_business", "Business to business"), ("attractions_and_activities", "Attractions and activities"),
       ("structure_and_geography", "Structure and geography"), ("other", "Other")]],
   source=["Overture Maps Foundation, Places theme category taxonomy (top-level of categories.primary)."], status="seed",
   notes=["Verify top-level names against the Overture release in use (taxonomy is versioned)."])
cl("monument_type", "Protected monument type",
   [T("All protected monuments")] + [c(k, v) for k, v in [("stupa", "Stupa / dagoba"), ("temple_shrine", "Temple / image house / shrine"),
                                                         ("cave_inscription", "Cave / rock inscription"), ("fort", "Fort / fortification"),
                                                         ("colonial_building", "Colonial-period building"), ("ancient_tank", "Ancient tank / irrigation work"),
                                                         ("ruins", "Ruins / archaeological site"), ("other", "Other")]],
   source=["Department of Archaeology, gazetted protected monuments (Antiquities Ordinance No. 9 of 1940)."], status="seed")
cl("uda_zone", "UDA development plan zone",
   [T("All zones")] + [c(k, v) for k, v in [("residential", "Residential"), ("commercial", "Commercial"), ("mixed_development", "Mixed development"),
                                            ("industrial", "Industrial"), ("institutional", "Institutional"), ("tourism", "Tourism"),
                                            ("green_conservation", "Green / conservation / wetland"), ("agriculture", "Agriculture"),
                                            ("other", "Other")]],
   source=["Urban Development Authority (UDA), development plans for declared urban areas (zoning plans and regulations)."], status="seed",
   notes=["Each plan names zones differently (e.g. 'Primary Residential Zone'); map plan-specific names here in a crosswalk."])
cl("agri_index_group", "Agricultural production index group",
   [T("All agriculture")] + [c(k, v) for k, v in [("paddy", "Paddy"), ("tea", "Tea"), ("rubber", "Rubber"), ("coconut", "Coconut"),
                                                  ("other_field_crops", "Other field crops"), ("vegetables", "Vegetables"), ("fruits", "Fruits"),
                                                  ("minor_export_crops", "Minor export crops"), ("livestock", "Livestock"), ("fisheries", "Fisheries")]],
   source=[f"Department of Census and Statistics, Agricultural production volume index (2002=100) - ten groups."], status="seed",
   notes=["Index groups are not additive: total is the all-agriculture index."])

# ---------------------------------------------------------------- access, facilities, modes, disease
cl("facility_type", "Service facility type",
   [T("All facilities"), c("health", "Health facilities"), c("education", "Education facilities"), c("finance", "Financial services"),
    c("transport", "Transport nodes"), c("public_service", "Public services"), c("settlement", "Settlement destinations"),
    c("hospital", "Hospital", p="health"), c("primary_health_care", "Primary health care (PMCU / clinic / MOH)", p="health"),
    c("clinic", "Clinic", p="health"), c("doctors", "Doctors / GP practice", p="health"), c("dentist", "Dentist", p="health"),
    c("pharmacy", "Pharmacy", p="health"), c("school", "School", p="education"), c("university", "University", p="education"),
    c("bank", "Bank branch", p="finance"), c("atm", "ATM", p="finance"), c("railway_station", "Railway station", p="transport"),
    c("bus_stop", "Bus stop / GTFS stop", p="transport"), c("police_station", "Police station", p="public_service"),
    c("district_town", "District capital / town", p="settlement")],
   source=["Lanka Data Layer accessibility products; OpenStreetMap / healthsites.io facility types; HeiGIT accessibility facility categories."],
   notes=["Shared by derived-facility-coverage, derived-flood-exposure, derived-travel-time-facilities, healthsites-facilities and heigit-accessibility (facility). Hospital tiers use `hospital_type` as a second dimension."])
cl("mode", "Travel / transport mode",
   [T("All modes"), c("walk", "Walking"), c("cycle", "Cycling"), c("motorised", "Motorised (road)"),
    c("drive", "Car / drive", p="motorised"), c("two_wheeler", "Motorcycle / two-wheeler", p="motorised"),
    c("bus", "Bus", p="motorised"), c("rail", "Rail"), c("sea", "Sea"), c("air", "Air")],
   source=["Lanka Data Layer routing profiles (OSM routing graph: walk, drive, two-wheeler); Malaria Atlas Project travel-time surfaces (motorised, walking-only); DCS Pocket Book passenger-km by rail and bus."],
   notes=["Travel-time indicators use one mode per observation (total unused there); passenger-km may use total."])
cl("disease", "Notifiable disease (Epidemiology Unit)",
   [T("All notifiable diseases")] + [c(k, v) for k, v in [
       ("dengue", "Dengue fever / DHF (A90-A91)"), ("dysentery", "Dysentery (A03, A06)"), ("encephalitis", "Encephalitis (A83-A86)"),
       ("enteric_fever", "Enteric fever (A01)"), ("food_poisoning", "Food poisoning (A05)"), ("leptospirosis", "Leptospirosis (A27)"),
       ("typhus", "Typhus fever (A75)"), ("viral_hepatitis", "Viral hepatitis (B15-B19)"), ("human_rabies", "Human rabies (A82)"),
       ("chickenpox", "Chickenpox (B01)"), ("meningitis", "Meningitis (G00-G03)"), ("mumps", "Mumps (B26)"),
       ("leishmaniasis", "Leishmaniasis (B55)"), ("measles", "Measles (B05)"), ("rubella", "Rubella (B06)"),
       ("whooping_cough", "Whooping cough (A37)"), ("tetanus", "Tetanus (A33-A35)"), ("tuberculosis", "Tuberculosis (A15-A19)"),
       ("malaria", "Malaria (B50-B54)"), ("other", "Other notifiable diseases")]],
   source=["Epidemiology Unit, Ministry of Health - Weekly Epidemiological Report (WER) notifiable disease table; Notifiable Diseases list (Quarantine and Prevention of Diseases Ordinance). ICD-10 codes indicative."],
   status="seed", notes=["Confirm the WER column set for each year (diseases are added to the notification list over time)."])

# ---------------------------------------------------------------- source_indicator (generated from the registry)
def _indicators():
    rows = []
    root = os.environ["REGISTRY"]
    for p in sorted(glob.glob(f"{root}/datasets/*.yaml")):
        d = yaml.safe_load(open(p))
        for ind in d.get("indicators") or []:
            rows.append((ind["id"], ind.get("label") or ind["id"]))
    return rows
cl("source_indicator", "Registry indicator (input of a derived indicator)",
   [c(k, v) for k, v in _indicators()],
   source=["Lanka Data Layer registry: every indicator id declared in registry/datasets/*.yaml (generated)."],
   notes=["Generated from the registry - regenerate whenever indicators are added or renamed (codes are indicator ids).",
          "Shared by derived-per-capita-indicators, derived-dasymetric-disaggregation, derived-electoral-demographics and derived-harmonised-boundary-series (`indicator`). No total."])
