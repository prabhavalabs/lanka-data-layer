"""Sri Lankan official / national classifications."""
from lib import c, T, cl

DCS = "Department of Census and Statistics (DCS), Sri Lanka"

# ---------------------------------------------------------------- residential sector
cl("sector", "Residential sector (urban / rural / estate)",
   [T("All sectors"), c("urban", "Urban", si="නාගරික"), c("rural", "Rural", si="ග්‍රාමීය"), c("estate", "Estate", si="වතු")],
   source=[f"{DCS}: sector definition used in the Census of Population and Housing, HIES and LFS (urban = MC/UC areas; estate = plantations of 20+ acres with 10+ resident labourers; rural = remainder)."],
   notes=["02-standard-model.md section 3 reserves `sector` for this country codelist; economic sectors use `industry` (and the source-specific *_sector lists).",
          "si labels are the standard DCS Sinhala terms (native sign-off requested); ta labels pending native-speaker review."])

# ---------------------------------------------------------------- seasons
cl("season", "Cultivation season (Maha / Yala)",
   [T("Both seasons (cultivation year)"), c("maha", "Maha (September/October - March)", si="මහ", ta="பெரும்போகம்"),
    c("yala", "Yala (April - August/September)", si="යල", ta="சிறுபோகம்")],
   source=[f"{DCS}, Paddy Statistics (Maha and Yala seasonal reports); Department of Agriculture cropping calendar."],
   notes=["Maha season is labelled by its starting/ending years (e.g. Maha 2024/25); the period of an observation carries that, not this code."],
   review="si/ta are the standard season names; native-speaker sign-off still requested.")

# ---------------------------------------------------------------- languages
cl("language", "Language (Census language ability)",
   [c("sinhala", "Sinhala", si="සිංහල", ta="சிங்களம்"), c("tamil", "Tamil", si="දෙමළ", ta="தமிழ்"),
    c("english", "English", si="ඉංග්‍රීසි", ta="ஆங்கிலம்")],
   source=[f"{DCS}, Census of Population and Housing 2024: ability to speak / read / write Sinhala, Tamil and English (persons aged 10+)."],
   notes=["A person may have ability in several languages, so there is no total."],
   review="si/ta are the standard language names; native-speaker sign-off still requested.")
cl("language_skill", "Language skill",
   [c("speak", "Speak"), c("read", "Read"), c("write", "Write"), c("read_write", "Read and write"), c("speak_read_write", "Speak, read and write")],
   source=[f"{DCS}, Census of Population and Housing 2024 language ability questions."],
   notes=["Skills overlap; no total. Keep only the combinations the source table reports."])

# ---------------------------------------------------------------- local authorities, schools, hospitals, courts
cl("la_type", "Local authority type",
   [T("All local authorities"), c("mc", "Municipal Council"), c("uc", "Urban Council"), c("ps", "Pradeshiya Sabha")],
   source=["Municipal Councils Ordinance (1947), Urban Councils Ordinance (1939), Pradeshiya Sabhas Act No. 15 of 1987; Ministry of Public Administration, Provincial Councils and Local Government - list of 341 local authorities."],
   notes=["Administrative terms: si/ta must come from the official gazette names, not translation."])
cl("school_type", "Government school type (Ministry of Education)",
   [T("All government schools"),
    c("1AB", "Type 1AB: grades 1/6-13 with GCE A/L science stream"),
    c("1C", "Type 1C: grades 1/6-13 with GCE A/L arts/commerce (no science)"),
    c("2", "Type 2: grades 1-11"), c("3", "Type 3: grades 1-5 or 1-8")],
   source=["Ministry of Education, Annual School Census of Sri Lanka - school type classification (1AB, 1C, Type 2, Type 3)."],
   notes=["Pirivenas and private/international schools are outside this list (add as separate codes if a dataset reports them)."])
cl("grade_band", "Grade span (general education)",
   [T("All grades"), c("primary", "Primary (grades 1-5)"), c("secondary", "Secondary (grades 6-13)"),
    c("junior_secondary", "Junior secondary (grades 6-9)", p="secondary"),
    c("senior_secondary_ol", "Senior secondary, O/L (grades 10-11)", p="secondary"),
    c("senior_secondary_al", "Senior secondary, A/L (grades 12-13)", p="secondary")],
   source=["Ministry of Education, School Census; National Education Commission key stages (primary 1-5, junior secondary 6-9, senior secondary 10-11 and 12-13)."])
cl("school_level", "Education level of a school (GIGA)",
   [T("All schools")] + [c(k, v) for k, v in [("pre_primary", "Pre-primary"), ("primary", "Primary"), ("secondary", "Secondary"),
                                              ("primary_secondary", "Primary and secondary"), ("post_secondary", "Post-secondary"),
                                              ("unknown", "Unknown")]],
   source=["UNICEF/ITU GIGA school location data, `education_level` attribute; levels aligned to ISCED 0-3."])
cl("exam", "National examination",
   [c("gce_ol", "GCE Ordinary Level"), c("gce_al", "GCE Advanced Level"), c("grade5_scholarship", "Grade 5 Scholarship Examination")],
   source=["Department of Examinations, Sri Lanka - GCE O/L, GCE A/L and Grade 5 scholarship examinations."],
   notes=["Pass rates per exam are non-additive: no total."])
cl("stream", "GCE A/L subject stream",
   [T("All streams")] + [c(k, v) for k, v in [
       ("physical_science", "Physical science (mathematics)"), ("biological_science", "Biological science"),
       ("commerce", "Commerce"), ("arts", "Arts"), ("engineering_technology", "Engineering technology"),
       ("biosystems_technology", "Biosystems technology"), ("other", "Other / common stream")]],
   source=["Department of Examinations, GCE A/L subject streams; University Grants Commission, University Admissions statistical handbook (admissions by stream)."])
cl("education", "Educational attainment (DCS Labour Force Survey groups)",
   [T("All levels"), c("grade_5_below", "Grade 5 and below (incl. no schooling)"), c("no_schooling", "No schooling", p="grade_5_below"),
    c("grade_6_10", "Grade 6-10"), c("gce_ol", "GCE O/L or equivalent"), c("gce_al_above", "GCE A/L and above"),
    c("gce_al", "GCE A/L", p="gce_al_above"), c("degree_above", "Degree and above", p="gce_al_above")],
   source=[f"{DCS}, Sri Lanka Labour Force Survey annual report - educational attainment groups."],
   notes=["ISCED 2011 mapping (indicative): grade_5_below = ISCED 0-1, grade_6_10 = ISCED 2, gce_ol = ISCED 3 (partial), gce_al = ISCED 3, degree_above = ISCED 6+.",
          "Some LFS tables report only the four headline groups; children are optional."])
cl("hospital_type", "Government hospital type (Ministry of Health)",
   [T("All government health institutions"), c("tertiary", "Tertiary care"), c("secondary", "Secondary care"), c("primary", "Primary care"),
    c("NH", "National Hospital", p="tertiary"), c("TH", "Teaching Hospital", p="tertiary"),
    c("PGH", "Provincial General Hospital", p="tertiary"), c("SH", "Specialised hospital", p="tertiary"),
    c("DGH", "District General Hospital", p="secondary"), c("BH-A", "Base Hospital - Type A", p="secondary"),
    c("BH-B", "Base Hospital - Type B", p="secondary"), c("DH-A", "Divisional Hospital - Type A", p="primary"),
    c("DH-B", "Divisional Hospital - Type B", p="primary"), c("DH-C", "Divisional Hospital - Type C", p="primary"),
    c("PMCU", "Primary Medical Care Unit", p="primary"), c("other", "Other institution (e.g. MOH office, maternity home)")],
   source=["Ministry of Health, Annual Health Bulletin and Health Institutions list - categorisation of government hospitals (Ministry of Health circular on reclassification of hospitals, 2014)."],
   notes=["Shared by moh-health-bulletin-rdhs, moh-health-institutions (institution_type), dcs-pocketbook-social-services (type) and derived travel-time tiers.",
          "Base hospitals are sometimes reported without A/B: use a parent-level `BH` only if a source does (add then)."])
cl("court_type", "Court type",
   [T("All courts")] + [c(k, v) for k, v in [
       ("supreme", "Supreme Court"), ("appeal", "Court of Appeal"), ("high", "High Court (incl. Provincial High Courts)"),
       ("commercial_high", "Commercial High Court"), ("district", "District Court"), ("magistrate", "Magistrate's Court"),
       ("primary", "Primary Court"), ("labour_tribunal", "Labour Tribunal"), ("other", "Other (e.g. Quazi courts, tribunals)")]],
   source=["Constitution of Sri Lanka (1978) Chapter XV; Judicature Act No. 2 of 1978; Ministry of Justice - courts by judicial zone."])
cl("road_class", "Road class (RDA national and provincial classification)",
   [T("All roads"), c("national", "National roads (RDA)"), c("provincial", "Provincial roads"), c("local", "Local authority roads"),
    c("E", "Class E - expressways", p="national"), c("A", "Class A - trunk roads", p="national"),
    c("B", "Class B - main roads", p="national"), c("C", "Class C - provincial roads", p="provincial"),
    c("D", "Class D - provincial roads", p="provincial"), c("other", "Other / unclassified")],
   source=["Road Development Authority (RDA), classification of national highways (A, B, E); Provincial Road Development Authorities (C, D); RDA Statistics / DCS Statistical Abstract road length by class."],
   notes=["`local` covers Pradeshiya Sabha / municipal roads (class E is not used for them)."])
cl("vehicle_class", "Vehicle class (Department of Motor Traffic)",
   [T("All vehicles")] + [c(k, v) for k, v in [
       ("motor_car", "Motor cars"), ("motor_tricycle", "Motor tricycles"), ("motor_cycle", "Motor cycles"), ("bus", "Buses"),
       ("dual_purpose", "Dual purpose vehicles"), ("motor_lorry", "Motor lorries"), ("motor_home", "Motor homes"),
       ("quadricycle", "Motor quadricycles"), ("special_purpose", "Special purpose vehicles"),
       ("land_vehicle_tractor", "Land vehicles - tractors"), ("land_vehicle_trailer", "Land vehicles - trailers"),
       ("other", "Other")]],
   source=["Department of Motor Traffic (DMT), new registration statistics by vehicle class; DCS Statistical Abstract table 'new registration of motor vehicles'."])
cl("accident_severity", "Road accident severity (Police)",
   [T("All accidents")] + [c(k, v) for k, v in [("fatal", "Fatal"), ("grievous", "Grievous injury"), ("non_grievous", "Non-grievous (minor) injury"),
                                                ("damage_only", "Damage only")]],
   source=["Sri Lanka Police, Traffic Headquarters road accident statistics (fatal, grievous, non-grievous, damage only); DCS Statistical Abstract."],
   notes=["Shared by dcs-abstract-transport and ntc-bus-statistics `severity`. Fatality counts (persons) are separate indicators from accident counts."])
cl("road_user", "Road user type (fatalities)",
   [T("All road users")] + [c(k, v) for k, v in [
       ("pedestrian", "Pedestrians"), ("pedal_cyclist", "Pedal cyclists"), ("motorcyclist", "Motor cycle riders"),
       ("pillion_rider", "Pillion riders"), ("three_wheeler", "Three-wheeler occupants"), ("driver", "Drivers (other vehicles)"),
       ("passenger", "Passengers (other vehicles)"), ("other", "Other / unknown")]],
   source=["Sri Lanka Police, Traffic Headquarters - persons killed by road user category."],
   notes=["Category boundaries (three-wheeler vs driver/passenger) vary by year; confirm against the police table before ingest."], status="seed")
cl("operator", "Bus operator",
   [T("All operators"), c("sltb", "Sri Lanka Transport Board (SLTB)"), c("private", "Private operators")],
   source=["National Transport Commission (NTC), Bus Transport Statistics - SLTB and private bus fleets."])
cl("route_type", "Bus route permit type",
   [T("All route permits"), c("inter_provincial", "Inter-provincial (NTC)"), c("intra_provincial", "Intra-provincial (Provincial Road Passenger Transport Authorities)")],
   source=["National Transport Commission Act No. 37 of 1991 (inter-provincial permits); Provincial Road Passenger Transport Authority statutes."],
   status="seed", notes=["NTC also classifies services (normal, semi-luxury, luxury, super-luxury); add as a separate dimension if a table needs it."])
cl("rail_line", "Railway line (Sri Lanka Railways)",
   [T("All lines")] + [c(k, v) for k, v in [
       ("main", "Main Line (Colombo Fort - Badulla)"), ("matale", "Matale Line"), ("coast", "Coastal Line (Colombo - Matara - Beliatta)"),
       ("puttalam", "Puttalam Line"), ("kelani_valley", "Kelani Valley Line"), ("northern", "Northern Line (Polgahawela - Kankesanthurai)"),
       ("mannar", "Mannar Line (Medawachchiya - Talaimannar)"), ("trincomalee", "Trincomalee Line"),
       ("batticaloa", "Batticaloa Line"), ("mihintale", "Mihintale Line")]],
   source=["Sri Lanka Railways, network map and timetable line names."],
   notes=["Proposed metro lines (lk-metro-proposed-lines) are not SLR lines and need their own list once the proposal's line ids are known."])
cl("expressway", "Expressway route (RDA E-class)",
   [T("All expressways")] + [c(k, v) for k, v in [
       ("E01", "E01 Southern Expressway"), ("E02", "E02 Outer Circular Highway"), ("E03", "E03 Colombo - Katunayake Expressway"),
       ("E04", "E04 Central Expressway"), ("E06", "E06 Ruwanpura Expressway")]],
   source=["Road Development Authority, Expressway Operation Maintenance and Management Division - expressway route numbers."],
   status="seed", notes=["Add new E-routes (and sections such as E01 Hambantota extension) from the RDA programme list."])
cl("airport", "Airport (IATA location identifier)",
   [T("All airports"), c("CMB", "Bandaranaike International Airport, Katunayake"), c("HRI", "Mattala Rajapaksa International Airport"),
    c("JAF", "Jaffna International Airport, Palaly"), c("RML", "Colombo International Airport, Ratmalana"),
    c("BTC", "Batticaloa Airport"), c("TRR", "China Bay Airport, Trincomalee"), c("other", "Other airports")],
   source=["IATA location identifiers (IATA Airline Coding Directory); Airport and Aviation Services (Sri Lanka) (Private) Limited."],
   status="seed",
   notes=["Shared by bia-flight-arrivals `airport` and `origin` (origin airport): add foreign origin airports by IATA code from the BIA schedule extract."])
cl("port", "Sri Lankan seaport",
   [T("All ports")] + [c(k, v) for k, v in [
       ("colombo", "Port of Colombo"), ("hambantota", "Hambantota International Port"), ("galle", "Port of Galle"),
       ("trincomalee", "Port of Trincomalee"), ("kankesanthurai", "Kankesanthurai Port"), ("oluvil", "Oluvil Port"),
       ("point_pedro", "Point Pedro Port")]],
   source=["Sri Lanka Ports Authority (SLPA) - ports administered; IMF PortWatch port list for Sri Lanka."],
   notes=["UN/LOCODE crosswalk (e.g. LKCMB for Colombo) to be added in registry/crosswalks.",
          "Shared by slpa-port-statistics, imf-portwatch-lka, dcs-abstract-transport and searoute-shipping-lanes (origin)."])
cl("cargo_type", "Port cargo type",
   [T("All cargo")] + [c(k, v) for k, v in [("containerised", "Containerised cargo"), ("break_bulk", "Break bulk / general cargo"),
                                            ("dry_bulk", "Dry bulk"), ("liquid_bulk", "Liquid bulk (incl. petroleum)")]],
   source=["Sri Lanka Ports Authority, Performance statistics - cargo handled by type; UNCTAD maritime cargo categories."])
cl("movement_type", "Container movement type (TEU)",
   [T("All TEU handled")] + [c(k, v) for k, v in [("domestic", "Domestic (import and export)"), ("transshipment", "Transshipment"), ("restow", "Re-stowing")]],
   source=["Sri Lanka Ports Authority, container throughput statistics (domestic, transhipment, restows)."])
cl("vessel_type", "Vessel type (IMF PortWatch)",
   [T("All vessels")] + [c(k, v) for k, v in [("container", "Container"), ("dry_bulk", "Dry bulk"), ("general_cargo", "General cargo"),
                                              ("roro", "Roll-on/roll-off"), ("tanker", "Tanker")]],
   source=["IMF PortWatch daily port activity data (portcalls_container, _dry_bulk, _general_cargo, _roro, _tanker)."])
cl("direction", "Traffic direction",
   [T("Arrivals and departures"), c("arrival", "Arrivals"), c("departure", "Departures")],
   source=["Airport and Aviation Services (Sri Lanka) passenger statistics; generic direction concept."])
cl("flow", "Trade flow",
   [T("Total trade"), c("export", "Exports", s="X"), c("import", "Imports", s="M"), c("re_export", "Re-exports", p="export")],
   source=["UN IMTS 2010 trade flows; IMF DOTS/SDMX indicator suffixes X (exports) and M (imports)."],
   notes=["Shared by dcs-pocketbook-trade-indices, imf-portwatch-lka and wits-trade-partners. Volume/unit-value indices by flow are not additive (total unused there)."])

# ---------------------------------------------------------------- agriculture / fisheries
_crop = [T("All crops"), c("cereals", "Cereals"), c("pulses", "Pulses"), c("oilseeds", "Oilseeds"), c("condiments", "Condiments / vegetables (OFC)"),
         c("roots_tubers", "Roots and tubers"), c("plantation", "Plantation crops"), c("export_agri", "Export agricultural crops"),
         c("vegetables", "Vegetables"), c("fruits", "Fruits"),
         c("paddy", "Paddy", p="cereals"), c("maize", "Maize", p="cereals"), c("finger_millet", "Finger millet (kurakkan)", p="cereals"),
         c("sorghum", "Sorghum", p="cereals"), c("green_gram", "Green gram (mung bean)", p="pulses"), c("cowpea", "Cowpea", p="pulses"),
         c("black_gram", "Black gram", p="pulses"), c("soya_bean", "Soya bean", p="pulses"), c("groundnut", "Groundnut", p="oilseeds"),
         c("gingelly", "Gingelly (sesame)", p="oilseeds"), c("chilli", "Chilli", p="condiments"), c("big_onion", "Big onion", p="condiments"),
         c("red_onion", "Red onion", p="condiments"), c("potato", "Potato", p="roots_tubers"), c("sweet_potato", "Sweet potato", p="roots_tubers"),
         c("cassava", "Cassava (manioc)", p="roots_tubers"), c("tea", "Tea", p="plantation"), c("rubber", "Rubber", p="plantation"),
         c("coconut", "Coconut", p="plantation"), c("sugarcane", "Sugarcane", p="plantation"), c("cinnamon", "Cinnamon", p="export_agri"),
         c("pepper", "Pepper", p="export_agri"), c("cloves", "Cloves", p="export_agri"), c("cardamom", "Cardamom", p="export_agri"),
         c("nutmeg_mace", "Nutmeg and mace", p="export_agri"), c("coffee", "Coffee", p="export_agri"), c("cocoa", "Cocoa", p="export_agri"),
         c("arecanut", "Arecanut", p="export_agri"), c("other", "Other crops")]
cl("crop", "Crop (DCS / Department of Agriculture crop list)", _crop,
   source=[f"{DCS}, Other Field Crops (OFC) extent and production; Pocket Book of Agricultural Statistics; Department of Export Agriculture crop list."],
   notes=["Shared across dcs-other-field-crops and dcs-pocketbook-agriculture; FAOSTAT items use FAO item codes (separate list, crosswalk later)."])
cl("irrigation_source", "Paddy water source",
   [T("All sown extent"), c("major_irrigation", "Major irrigation schemes"), c("minor_irrigation", "Minor irrigation schemes"), c("rainfed", "Rainfed")],
   source=[f"{DCS}, Paddy Statistics - extent sown under major irrigation, minor irrigation and rainfed."])
cl("elevation", "Tea elevation category",
   [T("All elevations"), c("high_grown", "High grown (above 1,200 m)"), c("medium_grown", "Medium grown (600-1,200 m)"), c("low_grown", "Low grown (below 600 m)")],
   source=["Sri Lanka Tea Board, monthly tea production statistics by elevation."])
cl("fishery_type", "Fishery sub-sector",
   [T("All fish production"), c("marine", "Marine fisheries"), c("inland", "Inland fisheries and aquaculture"),
    c("coastal", "Coastal fisheries", p="marine"), c("offshore", "Offshore / deep-sea fisheries", p="marine"),
    c("inland_capture", "Inland capture fisheries", p="inland"), c("aquaculture", "Aquaculture", p="inland")],
   source=["Ministry of Fisheries / NARA Fisheries Statistics (marine: coastal and offshore; inland capture and aquaculture)."])
cl("boat_type", "Fishing craft type (Ministry of Fisheries codes)",
   [T("All fishing craft"), c("motorised", "Motorised craft"), c("non_motorised", "Non-motorised craft"),
    c("IMUL", "IMUL - inboard multi-day boats", p="motorised"), c("IDAY", "IDAY - inboard single-day boats", p="motorised"),
    c("OFRP", "OFRP - outboard FRP boats", p="motorised"), c("MTRB", "MTRB - motorised traditional boats", p="motorised"),
    c("NTRB", "NTRB - non-motorised traditional boats", p="non_motorised"), c("NBSB", "NBSB - beach seine craft", p="non_motorised")],
   source=["Ministry of Fisheries and Aquatic Resources, Fisheries Statistics - fishing fleet by craft type (IMUL, IDAY, OFRP, MTRB, NTRB, NBSB)."])
cl("livestock_product", "Livestock product",
   [T("All livestock products")] + [c(k, v) for k, v in [("milk", "Milk"), ("eggs", "Eggs"), ("chicken_meat", "Chicken meat"),
                                                       ("beef", "Beef"), ("mutton", "Mutton / goat meat"), ("pork", "Pork")]],
   source=[f"{DCS}, Pocket Book of Agricultural Statistics - livestock production; Department of Animal Production and Health."],
   status="seed")
cl("nutrient", "Fertiliser nutrient",
   [T("All nutrients"), c("n", "Nitrogen (N)"), c("p2o5", "Phosphate (P2O5)"), c("k2o", "Potash (K2O)")],
   source=["FAOSTAT Fertilizers by Nutrient domain (items 3102 N, 3103 P2O5, 3104 K2O)."])
cl("holding_size", "Agricultural holding size class",
   [T("All holdings")] + [c(k, v) for k, v in [("lt0.4", "Under 0.4 ha (under 1 acre)"), ("0.4-0.8", "0.4-0.8 ha (1-2 acres)"),
                                               ("0.8-2", "0.8-2 ha (2-5 acres)"), ("2-4", "2-4 ha (5-10 acres)"),
                                               ("4-8", "4-8 ha (10-20 acres)"), ("8+", "8 ha and over (20+ acres)"), ("landless", "Without land")]],
   source=[f"{DCS}, Census of Agriculture 2013/14 - holdings by size class (acre-based bands)."],
   status="seed", notes=["Band edges are the usual DCS acre bands converted to ha; confirm against the census table (see review CSV)."])
cl("land_value_use", "Land use (CBSL Land Valuation Indicator)",
   [c("composite", "Composite (all uses)"), c("residential", "Residential"), c("commercial", "Commercial"), c("industrial", "Industrial")],
   source=["Central Bank of Sri Lanka, Land Valuation Indicator (LVI) for Colombo District - residential, commercial and industrial sub-indicators."],
   notes=["Index sub-indicators are not additive: `composite` is not a total member."])
cl("holding_land_use", "Land use of agricultural holdings",
   [T("All land in holdings")] + [c(k, v) for k, v in [("paddy", "Paddy land"), ("highland", "Highland (other field crops, plantations)"),
                                                      ("home_garden", "Home garden"), ("other", "Other land")]],
   source=[f"{DCS}, Census of Agriculture 2013/14 - extent of holdings by land use."], status="seed")

# ---------------------------------------------------------------- census housing / households
cl("structure_type", "Housing unit type (Census of Population and Housing)",
   [T("All housing units")] + [c(k, v) for k, v in [
       ("single_house_one_floor", "Single house - one floor"), ("single_house_multi_floor", "Single house - two or more floors"),
       ("attached_house_annex", "Attached house / annex"), ("flat", "Flat"), ("condominium", "Condominium / apartment"),
       ("line_room", "Line room / row house"), ("hut_shanty", "Hut / shanty"), ("other", "Other")]],
   source=[f"{DCS}, Census of Population and Housing 2012 and 2024 - type of housing unit."], status="seed")
cl("housing_element", "Housing structural element",
   [c("wall", "Wall"), c("roof", "Roof"), c("floor", "Floor")],
   source=[f"{DCS}, Census of Population and Housing - principal material of wall, roof and floor."],
   notes=["Each element is a separate breakdown; no total."])
cl("housing_material", "Principal construction material (census)",
   [T("All housing units")] + [c(k, v) for k, v in [
       ("brick", "Brick"), ("cement_block", "Cement block / stone"), ("cabook", "Cabook"), ("mud", "Soil bricks / mud"),
       ("cadjan_palmyrah", "Cadjan / palmyrah / straw"), ("plank_metal_sheet", "Plank / metal sheet"),
       ("tile", "Tile (roof)"), ("asbestos", "Asbestos (roof)"), ("concrete", "Concrete"), ("metal_sheet", "Metal sheet (roof)"),
       ("cement", "Cement (floor)"), ("terrazzo_tile", "Terrazzo / tile / granite (floor)"), ("wood", "Wood"),
       ("other", "Other")]],
   source=[f"{DCS}, Census of Population and Housing 2012 and 2024 - material of wall, roof and floor."],
   status="seed", notes=["Used together with `housing_element`; valid material x element pairs follow the census table layout."])
cl("tenure", "Tenure of housing unit",
   [T("All households")] + [c(k, v) for k, v in [
       ("owned", "Owned by a household member"), ("rented_government", "Rented / leased - government"),
       ("rented_private", "Rented / leased - private"), ("rent_free", "Rent free"), ("encroached", "Encroached"), ("other", "Other")]],
   source=[f"{DCS}, Census of Population and Housing - tenure of occupied housing units."], status="seed")
cl("drinking_water_source", "Main source of drinking water",
   [T("All households"), c("piped", "Pipe-borne water"), c("well", "Well"),
    c("tap_within_unit", "Tap within the unit (main line)", p="piped"), c("tap_within_premises", "Tap within premises but outside the unit", p="piped"),
    c("tap_outside_premises", "Tap outside the premises", p="piped"), c("rural_water_project", "Rural water supply project", p="piped"),
    c("protected_well_within", "Protected well within premises", p="well"), c("protected_well_outside", "Protected well outside premises", p="well"),
    c("unprotected_well", "Unprotected well", p="well"), c("tube_well", "Tube well"), c("bottled_water", "Bottled water"),
    c("bowser", "Bowser"), c("surface_water", "River / tank / stream / spring"), c("other", "Other")],
   source=[f"{DCS}, Census of Population and Housing 2012 and 2024 - principal source of drinking water."], status="seed")
cl("toilet_type", "Toilet facility",
   [T("All households")] + [c(k, v) for k, v in [
       ("water_seal_sewer", "Water seal, connected to piped sewer"), ("water_seal_septic", "Water seal, connected to septic tank / pit"),
       ("pour_flush", "Pour flush (not water seal)"), ("direct_pit", "Direct pit"), ("other", "Other"), ("none", "Not using a toilet")]],
   source=[f"{DCS}, Census of Population and Housing - type of toilet facility used by the household."], status="seed",
   notes=["Census also cross-classifies exclusive vs shared use; add a separate dimension if released."])
cl("lighting", "Principal source of lighting",
   [T("All households"), c("electricity", "Electricity"), c("national_grid", "Electricity - national grid", p="electricity"),
    c("rural_hydro", "Electricity - rural hydro project", p="electricity"), c("solar", "Solar power"), c("kerosene", "Kerosene"),
    c("biogas", "Biogas"), c("other", "Other")],
   source=[f"{DCS}, Census of Population and Housing - principal type of lighting."], status="seed")
cl("cooking_fuel", "Principal type of cooking fuel",
   [T("All households")] + [c(k, v) for k, v in [("firewood", "Firewood"), ("lp_gas", "LP gas"), ("kerosene", "Kerosene"),
                                                 ("electricity", "Electricity"), ("sawdust_paddy_husk", "Sawdust / paddy husk"),
                                                 ("biogas", "Biogas"), ("none", "Not cooking"), ("other", "Other")]],
   source=[f"{DCS}, Census of Population and Housing - principal type of cooking fuel."], status="seed")
cl("waste_disposal", "Solid waste disposal method",
   [T("All households")] + [c(k, v) for k, v in [
       ("collected_by_la", "Collected by local authority"), ("burnt", "Occupants burn"), ("buried", "Occupants bury"),
       ("composted", "Occupants compost"), ("dumped_within", "Dumped within the premises"), ("dumped_outside", "Dumped outside the premises"),
       ("other", "Other")]],
   source=[f"{DCS}, Census of Population and Housing - method of disposal of solid waste."], status="seed")
cl("household_asset", "Household durable / item",
   [T("All households")] + [c(k, v) for k, v in [
       ("radio", "Radio"), ("television", "Television"), ("fixed_telephone", "Fixed telephone"), ("mobile_phone", "Mobile phone"),
       ("computer", "Desktop / laptop computer"), ("internet", "Internet facility"), ("refrigerator", "Refrigerator"),
       ("washing_machine", "Washing machine"), ("gas_cooker", "Gas cooker"), ("electric_fan", "Electric fan"),
       ("bicycle", "Bicycle"), ("motorcycle", "Motor cycle / scooter"), ("three_wheeler", "Three-wheeler"),
       ("car_van", "Motor car / van"), ("lorry", "Lorry / tractor"), ("boat", "Boat")]],
   source=[f"{DCS}, Census of Population and Housing 2024 - availability of household items."],
   status="seed", notes=["Households may own several items: `total` means all households (the denominator), not a sum."], total_parent=False)
cl("amenity", "Housing amenity (Census 2012)",
   [T("All households")] + [c(k, v) for k, v in [("electricity", "Electricity for lighting"), ("safe_drinking_water", "Safe drinking water"),
                                                 ("toilet", "Toilet facility"), ("lp_gas", "LP gas for cooking")]],
   source=[f"{DCS}, Census of Population and Housing 2012 - housing amenities at GN level."], total_parent=False,
   status="seed", notes=["Exact amenity list must be taken from the 2012 GN-level table headers; households have several amenities, so `total` is the denominator only."])
cl("building_type", "Building use (broad)",
   [T("All buildings"), c("residential", "Residential"), c("non_residential", "Non-residential"),
    c("commercial", "Commercial / retail / office", p="non_residential"), c("industrial", "Industrial / warehouse", p="non_residential"),
    c("institutional", "Institutional (school, hospital, government)", p="non_residential"),
    c("religious", "Religious", p="non_residential"), c("other", "Other / mixed", p="non_residential"), c("unspecified", "Unspecified (building=yes)")],
   source=[f"{DCS}, Building construction statistics (building plans approved by type); OpenStreetMap Key:building values mapped to broad use; GHSL GHS-BUILT-S residential / non-residential split."],
   notes=["Shared by dcs-building-construction, osm-buildings and ghsl-built-up (`use`: residential vs non_residential)."])

# ---------------------------------------------------------------- energy
_es = [T("All sources"), c("renewable", "Renewable"), c("fossil", "Fossil"),
       c("hydro", "Hydro", p="renewable"), c("major_hydro", "Major hydro (CEB)", p="hydro"), c("mini_hydro", "Mini / small hydro", p="hydro"),
       c("wind", "Wind", p="renewable"), c("solar", "Solar", p="renewable"), c("biomass", "Biomass (incl. fuelwood, bagasse)", p="renewable"),
       c("other_renewable", "Other renewable (e.g. waste-to-energy, dendro)", p="renewable"),
       c("coal", "Coal", p="fossil"), c("petroleum", "Petroleum (crude oil and products)", p="fossil"),
       c("fuel_oil", "Fuel oil / diesel (thermal oil plants)", p="petroleum"), c("lng", "Natural gas / LNG", p="fossil"),
       c("other", "Other")]
cl("energy_source", "Energy source / fuel", _es,
   source=["Sri Lanka Sustainable Energy Authority (SLSEA), Sri Lanka Energy Balance (primary supply by source); Ceylon Electricity Board Statistical Digest (generation by source); IRENA/IEA source groupings."],
   notes=["Shared by slsea-energy-balance (fuel), ceb-statistical-digest (source), nso-electricity-generation (source), dcs-pocketbook-sector-services (source) and power-plants (technology)."])
cl("energy_end_use_sector", "Energy end-use sector (SLSEA energy balance)",
   [T("All final consumption")] + [c(k, v) for k, v in [("household_commercial_other", "Household, commercial and other"), ("industry", "Industry"),
                                                       ("transport", "Transport"), ("non_energy", "Non-energy use")]],
   source=["Sri Lanka Sustainable Energy Authority, Sri Lanka Energy Balance - final energy consumption by sector."])
cl("tariff_category", "Electricity tariff category (CEB / PUCSL)",
   [T("All customers")] + [c(k, v) for k, v in [
       ("domestic", "Domestic (D-1)"), ("religious", "Religious and charitable institutions (R-1)"), ("general_purpose", "General purpose (GP)"),
       ("industrial", "Industrial (I)"), ("hotel", "Hotel (H)"), ("government", "Government (GV)"), ("agriculture", "Agriculture (AGR)"),
       ("street_lighting", "Street lighting")]],
   source=["Public Utilities Commission of Sri Lanka (PUCSL), approved electricity tariffs; CEB Statistical Digest customer categories."],
   status="seed", notes=["Sub-categories (GP-1/2/3, I-1/2/3, H-1/2/3) can be added as children."])
cl("licensee", "Electricity distribution licensee",
   [T("All licensees")] + [c(k, v) for k, v in [("DL1", "CEB Distribution Division 1"), ("DL2", "CEB Distribution Division 2"),
                                                ("DL3", "CEB Distribution Division 3"), ("DL4", "CEB Distribution Division 4"),
                                                ("DL5", "Lanka Electricity Company (LECO)")]],
   source=["Public Utilities Commission of Sri Lanka, distribution licences (CEB DL1-DL4, LECO DL5); PUCSL supply reliability reports."],
   status="seed", notes=["Confirm licence numbering against the PUCSL report before ingest."])
cl("gem_plant_status", "Power plant status (Global Energy Monitor)",
   [T("All units")] + [c(k, v) for k, v in [
       ("announced", "Announced"), ("pre_construction", "Pre-construction"), ("construction", "Construction"), ("operating", "Operating"),
       ("mothballed", "Mothballed"), ("retired", "Retired"), ("cancelled", "Cancelled"), ("shelved", "Shelved")]],
   source=["Global Energy Monitor, Global Integrated Power Tracker - unit status definitions."])

# ---------------------------------------------------------------- tourism
cl("resort_region", "Tourism resort region (SLTDA)",
   [T("All regions")] + [c(k, v) for k, v in [("colombo_city", "Colombo City"), ("greater_colombo", "Greater Colombo"),
                                              ("south_coast", "South Coast"), ("east_coast", "East Coast"), ("hill_country", "Hill Country"),
                                              ("ancient_cities", "Ancient Cities"), ("northern", "Northern Region")]],
   source=["Sri Lanka Tourism Development Authority (SLTDA), Annual Statistical Report - resort regions."])
cl("establishment_type", "Tourist establishment type (SLTDA registration)",
   [T("All registered establishments"), c("accommodation", "Accommodation"), c("other_services", "Other tourist services"),
    c("tourist_hotel", "Classified tourist hotel (star class)", p="accommodation"), c("boutique", "Boutique hotel / villa", p="accommodation"),
    c("guest_house", "Guest house", p="accommodation"), c("home_stay", "Home stay unit", p="accommodation"),
    c("bungalow", "Bungalow", p="accommodation"), c("rented_home", "Rented home / apartment", p="accommodation"),
    c("travel_agent", "Travel agent / tour operator", p="other_services"), c("restaurant", "Restaurant", p="other_services"),
    c("other", "Other", p="other_services")],
   source=["Sri Lanka Tourism Development Authority, Tourism Act No. 38 of 2005 registration categories; SLTDA Annual Statistical Report."],
   status="seed", notes=["Shared by sltda-accommodation and sltda-registered-establishments (`type`). Star class (1-5) is a separate breakdown."])
cl("attraction_type", "Tourist attraction type",
   [T("All attractions")] + [c(k, v) for k, v in [
       ("heritage", "Heritage / archaeological site"), ("religious", "Religious site"), ("museum", "Museum"), ("nature", "Nature / wildlife"),
       ("beach", "Beach"), ("viewpoint", "Viewpoint"), ("waterfall", "Waterfall"), ("park", "Park / garden"),
       ("theme_park", "Theme park / zoo / aquarium"), ("other", "Other attraction")]],
   source=["OpenStreetMap Key:tourism / historic / natural values (attraction, viewpoint, museum, beach, waterfall...); SLTDA destination categories."],
   notes=["Shared by derived-attractions-register (`category`) and osm-tourism-attractions (`type`)."])
cl("designation", "Heritage / protection designation of a site",
   [T("All sites")] + [c(k, v) for k, v in [("world_heritage", "UNESCO World Heritage property"), ("world_heritage_tentative", "UNESCO tentative list"),
                                            ("protected_monument", "Protected monument (Antiquities Ordinance)"),
                                            ("archaeological_reserve", "Archaeological reserve (Antiquities Ordinance)"),
                                            ("protected_area", "Protected area (DWC / Forest Department)"), ("none", "No designation")]],
   source=["UNESCO World Heritage Centre lists; Antiquities Ordinance No. 9 of 1940 (Department of Archaeology); Fauna and Flora Protection Ordinance (DWC) and Forest Conservation Ordinance (Forest Department)."],
   notes=["A site can hold several designations; counts by designation are not additive."], total_parent=False)
cl("whs_category", "World Heritage property category",
   [T("All properties"), c("cultural", "Cultural"), c("natural", "Natural"), c("mixed", "Mixed")],
   source=["UNESCO Operational Guidelines for the Implementation of the World Heritage Convention - cultural, natural, mixed properties."])
cl("whs_status", "World Heritage listing status",
   [T("All properties"), c("inscribed", "Inscribed"), c("tentative", "Tentative list")],
   source=["UNESCO World Heritage Centre - World Heritage List and Tentative Lists."])

# ---------------------------------------------------------------- government finance
cl("revenue_type", "Government revenue type",
   [T("Total revenue and grants"), c("tax", "Tax revenue"), c("non_tax", "Non-tax revenue"), c("grants", "Grants"),
    c("income_tax", "Income taxes", p="tax"), c("vat", "Value added tax", p="tax"), c("excise", "Excise duties", p="tax"),
    c("excise_liquor", "Excise - liquor", p="excise"), c("excise_tobacco", "Excise - cigarettes / tobacco", p="excise"),
    c("excise_petroleum", "Excise - petroleum", p="excise"), c("excise_motor_vehicles", "Excise - motor vehicles", p="excise"),
    c("excise_other", "Excise - other", p="excise"), c("import_duties", "Import duties (customs)", p="tax"),
    c("cess", "Cess", p="tax"), c("special_commodity_levy", "Special commodity levy", p="tax"),
    c("ssc_levy", "Social security contribution levy", p="tax"), c("pal", "Ports and airports development levy", p="tax"),
    c("other_taxes", "Other taxes (stamp duty, licences, ...)", p="tax")],
   source=["Central Bank of Sri Lanka, Annual Report - Fiscal Sector statistical tables (economic classification of revenue); Ministry of Finance, Annual Report - revenue by tax type; IMF GFSM 2014 revenue classification (11 taxes, 13 grants, 14 other revenue)."],
   notes=["Shared by cbsl-fiscal-operations (revenue_type) and treasury-revenue-outturn (tax_type). Taxes that are introduced or abolished keep their codes for history."])
cl("expenditure_type", "Government expenditure type (economic classification)",
   [T("Total expenditure and net lending"), c("recurrent", "Recurrent expenditure"), c("capital", "Capital expenditure and net lending"),
    c("salaries_wages", "Salaries and wages", p="recurrent"), c("other_goods_services", "Other goods and services", p="recurrent"),
    c("interest", "Interest payments", p="recurrent"), c("transfers_subsidies", "Subsidies and transfers", p="recurrent"),
    c("public_investment", "Public investment / acquisition of capital assets", p="capital"),
    c("capital_transfers", "Capital transfers", p="capital"), c("net_lending", "Net lending", p="capital")],
   source=["Central Bank of Sri Lanka, Annual Report - economic classification of government expenditure; DCS local government finance (recurrent / capital)."],
   notes=["Shared by cbsl-fiscal-operations and dcs-local-government-finance (which reports only recurrent / capital)."])
cl("lg_revenue_type", "Local authority revenue type",
   [T("Total revenue"), c("rates_taxes", "Rates and taxes"), c("rents", "Rents"), c("licences", "Licences"), c("fees_charges", "Fees and charges for services"),
    c("warrants_fines", "Warrant costs and fines"), c("grants", "Grants (central / provincial)"), c("other", "Other revenue")],
   source=[f"{DCS}, Local Government Finance statistics; Local Authorities Financial Regulations - revenue heads."], status="seed")
cl("debt_type", "Central government debt by residence of creditor",
   [T("Total central government debt"), c("domestic", "Domestic debt"), c("foreign", "Foreign debt")],
   source=["Central Bank of Sri Lanka, Annual Report - outstanding central government debt (domestic / foreign)."])
cl("instrument", "Government debt instrument",
   [T("Total central government debt"), c("domestic", "Domestic debt"), c("foreign", "Foreign debt"),
    c("treasury_bills", "Treasury bills", p="domestic"), c("treasury_bonds", "Treasury bonds", p="domestic"),
    c("rupee_loans", "Rupee loans", p="domestic"), c("sldb", "Sri Lanka Development Bonds", p="domestic"),
    c("cbsl_provisional_advances", "CBSL provisional advances", p="domestic"), c("other_domestic", "Other domestic (overdraft, other)", p="domestic"),
    c("isb", "International sovereign bonds", p="foreign"), c("project_loans", "Project and programme loans", p="foreign"),
    c("foreign_t_bills_bonds", "Non-resident holdings of T-bills / T-bonds", p="foreign"), c("other_foreign", "Other foreign", p="foreign")],
   source=["Ministry of Finance, Public Debt Management Office, Annual Debt Report; CBSL Annual Report - composition of government debt."], status="seed")
cl("creditor", "External creditor category",
   [T("All external creditors")] + [c(k, v) for k, v in [("multilateral", "Multilateral"), ("bilateral", "Bilateral"),
                                                        ("commercial", "Commercial (ISB holders, commercial banks)"), ("other", "Other")]],
   source=["Ministry of Finance, Public Debt Management Office - external debt by creditor category; World Bank IDS creditor types."])
cl("tenor", "Treasury bill tenor",
   [c("91d", "91-day"), c("182d", "182-day"), c("364d", "364-day")],
   source=["Central Bank of Sri Lanka, Public Debt Department - Treasury bill auction tenors (weekly economic indicators)."])
cl("funding_source", "Capital project funding source (budget)",
   [T("All sources"), c("domestic", "Domestic funds"), c("foreign", "Foreign funds"),
    c("foreign_loan", "Foreign loans", p="foreign"), c("foreign_grant", "Foreign grants", p="foreign"),
    c("reimbursable", "Reimbursable foreign aid", p="foreign")],
   source=["Ministry of Finance, Budget Estimates - financing source columns for capital expenditure."], status="seed")
cl("grant_type", "Central government transfer to provincial councils",
   [T("All transfers")] + [c(k, v) for k, v in [("block_grant", "Block grant"), ("criteria_based_grant", "Criteria based grant"),
                                                ("psdg", "Province specific development grant"), ("matching_grant", "Matching grant"),
                                                ("foreign_funded", "Foreign-funded projects"), ("other", "Other")]],
   source=["Finance Commission of Sri Lanka, annual reports - grants to provincial councils (Article 154R of the Constitution)."], status="seed")
cl("programme", "Budget programme",
   [T("All programmes"), c("01", "Programme 01 - Operational activities"), c("02", "Programme 02 - Development activities"),
    c("03", "Programme 03 (head-specific)")],
   source=["Ministry of Finance, Budget Estimates (Volumes I-III) - programme structure of each expenditure head."],
   status="seed", notes=["Programme numbers beyond 02 are head-specific; the label then comes from the head, so a (head, programme) pair is the real key."])
cl("object_code", "Budget object code (economic classification)",
   [T("All objects"), c("recurrent", "Recurrent expenditure"), c("capital", "Capital expenditure")] + [
       c(k, v, p="recurrent") for k, v in [("10", "10 Personal emoluments"), ("11", "11 Travelling expenses"), ("12", "12 Supplies"),
                                            ("13", "13 Maintenance expenditure"), ("14", "14 Services"), ("15", "15 Transfers")]] + [
       c(k, v, p="capital") for k, v in [("20", "20 Rehabilitation and improvement of capital assets"), ("21", "21 Acquisition of capital assets"),
                                          ("22", "22 Capital transfers"), ("23", "23 Acquisition of financial assets"),
                                          ("24", "24 Capacity building"), ("25", "25 Other capital expenditure")]],
   source=["Ministry of Finance, Budget Estimates and Financial Regulations - object code classification (4-digit objects grouped under 2-digit categories)."],
   status="seed", notes=["4-digit object codes (e.g. 1001 salaries and wages) go in as children of the 2-digit groups from the budget extract."])
cl("line_item", "Fiscal outturn line item",
   [c(k, v) for k, v in [("total_revenue", "Total revenue and grants"), ("tax_revenue", "Tax revenue"), ("non_tax_revenue", "Non-tax revenue"),
                         ("grants", "Grants"), ("total_expenditure", "Total expenditure and net lending"),
                         ("recurrent_expenditure", "Recurrent expenditure"), ("capital_expenditure", "Capital expenditure and net lending"),
                         ("interest", "Interest payments"), ("primary_balance", "Primary balance"), ("overall_balance", "Overall (budget) balance")]],
   source=["Ministry of Finance, Fiscal Management Report / mid-year fiscal position report - summary of fiscal operations."],
   status="seed", notes=["Balances are not additive with flows: no total."])
cl("bank_type", "Bank type (CBSL licensing)",
   [T("All banks"), c("lcb", "Licensed commercial bank"), c("lsb", "Licensed specialised bank")],
   source=["Central Bank of Sri Lanka, Banking Act No. 30 of 1988 - licensed commercial banks and licensed specialised banks; CBSL banking sector branch statistics."])
cl("lender_type", "Lending institution type",
   [T("All lenders")] + [c(k, v) for k, v in [("lcb", "Licensed commercial bank"), ("lsb", "Licensed specialised bank"),
                                              ("lfc", "Licensed finance company"), ("mfi", "Licensed microfinance company / NGO-MFI"),
                                              ("cooperative", "Co-operative rural bank / thrift society"), ("other", "Other")]],
   source=["Central Bank of Sri Lanka - licensed financial institution types (Banking Act; Finance Business Act No. 42 of 2011; Microfinance Act No. 6 of 2016)."], status="seed")
cl("company_type", "Company type (Companies Act No. 7 of 2007)",
   [T("All companies")] + [c(k, v) for k, v in [("private_limited", "Private limited company"), ("public_limited", "Public limited company"),
                                                ("guarantee", "Company limited by guarantee"), ("unlimited", "Unlimited company"),
                                                ("offshore", "Offshore company"), ("overseas", "Overseas company (foreign branch)")]],
   source=["Companies Act No. 7 of 2007, Part I and Part XVIII; Department of the Registrar of Companies."])
cl("trade_category", "Merchandise trade category (CBSL)",
   [T("Total merchandise trade"), c("exp_agricultural", "Agricultural exports"), c("exp_industrial", "Industrial exports"),
    c("exp_mineral", "Mineral exports"), c("exp_unclassified", "Unclassified exports"),
    c("tea", "Tea", p="exp_agricultural"), c("rubber", "Rubber", p="exp_agricultural"), c("coconut", "Coconut products", p="exp_agricultural"),
    c("spices", "Spices", p="exp_agricultural"), c("seafood", "Seafood", p="exp_agricultural"),
    c("textiles_garments", "Textiles and garments", p="exp_industrial"), c("rubber_products", "Rubber products", p="exp_industrial"),
    c("petroleum_products", "Petroleum products", p="exp_industrial"), c("food_beverages_tobacco", "Food, beverages and tobacco", p="exp_industrial"),
    c("gems_jewellery", "Gems, diamonds and jewellery", p="exp_industrial"), c("machinery_equipment", "Machinery and mechanical appliances", p="exp_industrial"),
    c("imp_consumer", "Consumer goods imports"), c("imp_intermediate", "Intermediate goods imports"), c("imp_investment", "Investment goods imports"),
    c("imp_unclassified", "Unclassified imports"),
    c("food_beverages", "Food and beverages", p="imp_consumer"), c("non_food_consumer", "Non-food consumer goods", p="imp_consumer"),
    c("fuel", "Fuel", p="imp_intermediate"), c("textiles_inputs", "Textiles and textile articles", p="imp_intermediate"),
    c("fertiliser", "Fertiliser", p="imp_intermediate"), c("other_intermediate", "Other intermediate goods", p="imp_intermediate"),
    c("machinery", "Machinery and equipment", p="imp_investment"), c("building_materials", "Building materials", p="imp_investment"),
    c("transport_equipment", "Transport equipment", p="imp_investment")],
   source=["Central Bank of Sri Lanka, External Sector Performance (monthly) and Annual Report - exports by major category and imports by end-use (BEC-based)."],
   status="seed", notes=["Export and import categories share one list; the trade flow is the indicator (exports_usd / imports_usd)."])
cl("pmi_component", "PMI sub-index (CBSL)",
   [c(k, v) for k, v in [("headline", "Headline PMI / Business Activity index"), ("new_orders", "New orders / new businesses"),
                         ("production", "Production"), ("employment", "Employment"), ("suppliers_delivery", "Suppliers' delivery time"),
                         ("stock_of_purchases", "Stock of purchases"), ("business_activity", "Business activity"),
                         ("backlogs", "Backlogs of work"), ("expectations", "Expectations for activity")]],
   source=["Central Bank of Sri Lanka, Purchasing Managers' Index survey - manufacturing and services sub-indices."],
   notes=["Diffusion indices: no total. Manufacturing vs services is the `sector` dim (industry codes C / services)."])
cl("gdp_expenditure_component", "GDP by expenditure component (SNA 2008)",
   [T("Gross domestic product"), c("P3", "Final consumption expenditure", s="P3"),
    c("P31_S14", "Household final consumption (incl. NPISH)", p="P3"), c("P3_S13", "Government final consumption", p="P3"),
    c("P5", "Gross capital formation", s="P5"), c("P51G", "Gross fixed capital formation", p="P5", s="P51G"),
    c("P52", "Changes in inventories", p="P5", s="P52"), c("P6", "Exports of goods and services", s="P6"),
    c("P7", "Imports of goods and services (subtracted)", s="P7")],
   source=["UN System of National Accounts 2008 transaction codes (SDMX STO); DCS national accounts, GDP by expenditure approach."],
   sdmx_ref="SDMX:CL_TRANSACTION (STO)",
   notes=["GDP = P3 + P5 + P6 - P7: children of total are not a plain sum (imports carry a minus sign)."], total_parent=False)
cl("price_basis", "National accounts price basis",
   [c("current", "Current prices"), c("constant_2015", "Constant (2015) prices")],
   source=[f"{DCS}, National Accounts (base year 2015)."], notes=["Alternative valuations: no total."])
cl("pricetype", "Price type", [c("retail", "Retail"), c("wholesale", "Wholesale")],
   source=["HARTI daily food commodity bulletin; WFP VAM market price data (pricetype)."], notes=["No total."])
cl("basket", "Effective exchange rate currency basket",
   [c("cur5", "5-currency basket"), c("cur24", "24-currency basket")],
   source=["Central Bank of Sri Lanka - NEER and REER indices (5- and 24-currency baskets)."], notes=["No total."])
cl("poverty_line", "International poverty line (World Bank PIP)",
   [c(k, v) for k, v in [("usd3_00_ppp2021", "$3.00 a day (2021 PPP) - extreme poverty"),
                         ("usd4_20_ppp2021", "$4.20 a day (2021 PPP) - lower-middle-income line"),
                         ("usd8_30_ppp2021", "$8.30 a day (2021 PPP) - upper-middle-income line"),
                         ("usd2_15_ppp2017", "$2.15 a day (2017 PPP)"), ("usd3_65_ppp2017", "$3.65 a day (2017 PPP)"),
                         ("usd6_85_ppp2017", "$6.85 a day (2017 PPP)")]],
   source=["World Bank Poverty and Inequality Platform (PIP) - global poverty lines (June 2025 update to 2021 PPPs; 2017 PPP lines for earlier vintages)."],
   notes=["Lines are alternative thresholds: no total."])
cl("aswesuma_category", "Aswesuma beneficiary category",
   [T("All beneficiary families")] + [c(k, v) for k, v in [("transitional", "Transitional"), ("vulnerable", "Vulnerable"),
                                                           ("poor", "Poor"), ("extreme_poor", "Extreme poor")]],
   source=["Welfare Benefits Board, Aswesuma welfare benefit scheme (Welfare Benefits Act No. 24 of 2002; regulations 2023) - four beneficiary categories."])

# ---------------------------------------------------------------- justice, investment, misc national
cl("prisoner_status", "Prisoner status",
   [T("All prisoners / admissions")] + [c(k, v) for k, v in [("convicted", "Convicted"), ("remand", "Unconvicted / remand"),
                                                            ("detainee", "Detainee (PTA / other)"), ("other", "Other")]],
   source=["Department of Prisons, Prison Statistics of Sri Lanka - convicted and unconvicted (remand) prisoners; daily prison population returns."], status="seed")
cl("crime_type", "Grave crime type (Police)",
   [T("All grave crimes")] + [c(k, v) for k, v in [
       ("homicide", "Homicide"), ("attempted_homicide", "Attempted homicide"), ("hurt_by_weapon", "Hurt by knives / weapons"),
       ("robbery", "Robbery"), ("house_breaking_theft", "House breaking and theft"), ("theft", "Theft of property"),
       ("cheating_breach_of_trust", "Cheating / criminal breach of trust"), ("rape", "Rape / grave sexual abuse"),
       ("child_abuse", "Cruelty to / abuse of children"), ("abduction", "Abduction / kidnapping"), ("arson", "Arson"),
       ("mischief", "Mischief"), ("other", "Other grave crimes")]],
   source=["Sri Lanka Police, Crime statistics - grave crime abstract by category; DCS Statistical Abstract (justice)."],
   status="seed", notes=["Police grave-crime headings change across years; align codes to the exact police abstract headings before ingest."])
cl("boi_sector", "BOI enterprise sector",
   [T("All sectors")] + [c(k, v) for k, v in [("manufacturing", "Manufacturing"), ("services", "Services"), ("agriculture", "Agriculture"),
                                              ("infrastructure", "Infrastructure"), ("other", "Other")]],
   source=["Board of Investment of Sri Lanka, monthly/annual performance reports - FDI and projects by sector."],
   status="seed", notes=["BOI also publishes finer manufacturing sub-sectors (textiles and apparel, food, chemicals...); add as children."])
cl("stage", "Investment project stage (BOI)",
   [c("approved", "Approved"), c("contracted", "Agreements signed (contracted)"), c("commercial_operation", "In commercial operation")],
   source=["Board of Investment of Sri Lanka - project pipeline stages (approved, agreements signed, commercial operation)."],
   notes=["Stages are cumulative pipeline counts: no total."])
cl("project_status", "Project / asset status",
   [T("All projects")] + [c(k, v) for k, v in [
       ("proposed", "Proposed / pipeline"), ("approved", "Approved"), ("under_construction", "Under construction / implementation (active)"),
       ("completed", "Completed / open / operational"), ("suspended", "Suspended / on hold"), ("cancelled", "Cancelled / dropped")]],
   source=["Generic project life-cycle (World Bank project status: pipeline, active, closed, dropped; RDA expressway programme: proposed, under construction, open)."],
   notes=["Shared by lk-major-projects, rda-expressway-programme and wb-projects-lka (pipeline -> proposed, active -> under_construction, closed -> completed, dropped -> cancelled)."])
cl("contract_status", "Procurement contract status",
   [T("All contracts")] + [c(k, v) for k, v in [("tendered", "Tendered"), ("awarded", "Awarded"), ("ongoing", "Ongoing"),
                                                ("completed", "Completed"), ("cancelled", "Cancelled")]],
   source=["National Procurement Commission / Ministry of Finance Public Finance Department, Procurement Guidelines 2006 - procurement stages."], status="seed")
cl("damage_level", "House damage level (DMC)",
   [T("All houses damaged"), c("fully_damaged", "Fully damaged"), c("partially_damaged", "Partially damaged")],
   source=["Disaster Management Centre, situation reports - houses fully / partially damaged."])
cl("change_type", "Administrative unit change type",
   [T("All change events")] + [c(k, v) for k, v in [("create", "Creation"), ("split", "Split"), ("merge", "Merge"), ("rename", "Rename"),
                                                    ("boundary_change", "Boundary change"), ("code_change", "Code (p-code) change"),
                                                    ("abolish", "Abolition")]],
   source=["UN OCHA COD-AB change log conventions; Ministry of Public Administration gazette notifications of DS / GN changes."])
cl("survey_series", "DCS survey series",
   [T("All surveys")] + [c(k, v) for k, v in [
       ("census_population_housing", "Census of Population and Housing"), ("census_agriculture", "Census of Agriculture"),
       ("economic_census", "Economic Census"), ("lfs", "Labour Force Survey"), ("hies", "Household Income and Expenditure Survey"),
       ("dhs", "Demographic and Health Survey"), ("asi", "Annual Survey of Industries"), ("trade_services", "Survey of Trade and Services"),
       ("construction", "Survey of Construction Industries"), ("child_activity", "Child Activity Survey"),
       ("computer_literacy", "Computer Literacy Survey"), ("time_use", "Time Use Survey"),
       ("crop_estimation", "Agricultural crop estimation surveys"), ("prices", "Price surveys (CCPI / NCPI / PPI)"),
       ("school_census", "School Census (Ministry of Education)"), ("other", "Other")]],
   source=[f"{DCS}, National Data Archive (NADA) survey catalogue."], status="seed")
cl("census_year", "Census of Population year (Sri Lanka)",
   [c(str(y), f"Census {y}") for y in (1871, 1881, 1891, 1901, 1911, 1921, 1931, 1946, 1953, 1963, 1971, 1981, 2001, 2012, 2024)],
   source=[f"{DCS}, history of the census in Sri Lanka (censuses 1871-2024; 1991 not held; 2001 partial - 18 districts)."],
   notes=["The census year is normally the observation period; a dimension is only justified where one release mixes census rounds (see review CSV)."])
cl("source_boundary_vintage", "Source boundary vintage (harmonised series)",
   [c(k, v) for k, v in [("1981", "1981 census boundaries"), ("2001", "2001 census boundaries"), ("2012", "2012 census boundaries"),
                         ("2024", "2024 reference boundaries (cod-ab-lka)")]],
   source=[f"{DCS} census GN/DS boundary vintages; reference boundary set cod-ab-lka (country/lk/profile.yaml)."], status="seed",
   notes=["Non-additive selector: no total."])
cl("channel", "Investment channel",
   [c("boi", "BOI enterprise investment / employment"), c("aid", "Development partner commitments"), c("budget_capital", "Budget capital projects")],
   source=["Lanka Data Layer derived-investment-density method (inputs boi-enterprises, erd-foreign-financing / iati, budget-capital-projects)."],
   notes=["Channels overlap (aid-funded budget projects): no total."])
cl("town_size", "Destination type for market access",
   [c(k, v) for k, v in [("town_20k", "Nearest town of 20,000+ people"), ("town_50k", "Nearest town of 50,000+ people"),
                         ("economic_centre", "Nearest dedicated economic centre"), ("district_capital", "Nearest district capital")]],
   source=["Lanka Data Layer derived-market-access method; town populations from Census 2024 / GHSL urban centres; HARTI dedicated economic centres."],
   notes=["Alternative destinations: no total."])
cl("threshold", "Travel-time threshold",
   [c(f"min{n}", f"Within {n} minutes") for n in (5, 10, 15, 20, 30, 45, 60, 90, 120)],
   source=["Lanka Data Layer accessibility products; HeiGIT accessibility thresholds."],
   notes=["Cumulative thresholds: no total. Shared by derived-facility-coverage, heigit-accessibility and derived-transit-access (walk_minutes)."])
cl("time_of_day", "Noise regulation period",
   [c("day", "Day time (06:00-18:00)"), c("night", "Night time (18:00-06:00)")],
   source=["National Environmental (Noise Control) Regulations No. 1 of 1996 (Gazette 924/12), Central Environmental Authority."], notes=["No total."])
cl("noise_zone", "Noise control zone type",
   [T("All zones")] + [c(k, v) for k, v in [("low_noise", "Low noise (residential) area"), ("medium_noise", "Medium noise (mixed / commercial) area"),
                                            ("high_noise", "High noise (industrial) area"), ("silent", "Silent zone (100 m from courts, hospitals, schools...)")]],
   source=["National Environmental (Noise Control) Regulations No. 1 of 1996, Schedule I (zones and permissible levels)."])
cl("network_type", "Network type (Ookla)", [c("fixed", "Fixed broadband"), c("mobile", "Mobile")],
   source=["Ookla Open Data speedtest performance tiles (fixed and mobile)."], notes=["Separate datasets: no total."])
cl("radio", "Radio access technology",
   [T("All cell sites")] + [c(k, v) for k, v in [("GSM", "GSM (2G)"), ("CDMA", "CDMA"), ("UMTS", "UMTS (3G)"), ("LTE", "LTE (4G)"), ("NR", "NR (5G)")]],
   source=["OpenCelliD cell tower database `radio` field (GSM, CDMA, UMTS, LTE, NR); 3GPP generations."])
cl("telecom_service", "Telecommunication service",
   [T("All subscriptions"), c("fixed_telephone", "Fixed telephone"), c("mobile_cellular", "Mobile cellular"), c("internet", "Internet (broadband)"),
    c("fixed_wireline", "Fixed wireline", p="fixed_telephone"), c("fixed_wireless", "Fixed wireless", p="fixed_telephone"),
    c("fixed_broadband", "Fixed broadband", p="internet"), c("mobile_broadband", "Mobile broadband", p="internet")],
   source=["Telecommunications Regulatory Commission of Sri Lanka (TRCSL), Statistical overview - subscriptions by service; ITU indicator definitions."],
   notes=["Mobile broadband subscribers are also mobile cellular subscribers: `total` is only meaningful within one parent branch."])
cl("sensor", "Active-fire sensor",
   [T("All detections")] + [c(k, v) for k, v in [("modis", "MODIS (Terra/Aqua)"), ("viirs_snpp", "VIIRS (Suomi NPP)"),
                                                 ("viirs_noaa20", "VIIRS (NOAA-20)"), ("viirs_noaa21", "VIIRS (NOAA-21)")]],
   source=["NASA FIRMS active fire products (MODIS C6.1, VIIRS 375 m S-NPP / NOAA-20 / NOAA-21)."],
   notes=["The same fire may be detected by several sensors: total is a sum of detections, not of fires."])
cl("basin_level", "HydroBASINS level",
   [c(f"lev{n:02d}", f"Pfafstetter level {n}") for n in range(1, 13)],
   source=["Lehner & Grill (2013), HydroSHEDS HydroBASINS v1c, nested levels 1-12."], notes=["Nested levels: no total."])
cl("statistic", "Zonal statistic",
   [c(k, v) for k, v in [("mean", "Mean"), ("min", "Minimum"), ("max", "Maximum"), ("median", "Median"), ("std", "Standard deviation"),
                         ("range", "Range"), ("tri", "Terrain ruggedness index (Riley)")]],
   source=["Lanka Data Layer derived-terrain-zonal method (zonal statistics over Copernicus DEM GLO-30)."], notes=["No total."])
cl("taxon_class", "Taxonomic group (GBIF backbone)",
   [T("All taxa")] + [c(k, v) for k, v in [("aves", "Birds (class Aves)"), ("mammalia", "Mammals (class Mammalia)"),
                                           ("reptilia", "Reptiles (class Reptilia)"), ("amphibia", "Amphibians (class Amphibia)"),
                                           ("actinopterygii", "Ray-finned fishes (class Actinopterygii)"), ("insecta", "Insects (class Insecta)"),
                                           ("plantae", "Plants (kingdom Plantae)"), ("fungi", "Fungi (kingdom Fungi)"), ("other", "Other taxa")]],
   source=["GBIF Backbone Taxonomy (classes/kingdoms); add GBIF taxonKey crosswalk from the occurrence download."],
   notes=["Mixed ranks (class and kingdom) as used in the dataset description; species richness is not additive across groups."])
cl("demographic_group", "Population group (HRSL / Data for Good)",
   [T("Total population")] + [c(k, v) for k, v in [("women", "Women"), ("men", "Men"), ("children_under_five", "Children under five"),
                                                   ("youth_15_24", "Youth 15-24"), ("elderly_60_plus", "Elderly 60+"),
                                                   ("women_reproductive_age", "Women of reproductive age (15-49)")]],
   source=["Meta Data for Good / CIESIN High Resolution Settlement Layer demographic estimates (population_<group> rasters)."],
   total_parent=False, notes=["Groups overlap (women vs women of reproductive age): no parent links; prefer sex x age where the source permits."])
cl("dev_index_domain", "Development index domain",
   [c(k, v) for k, v in [("education", "Education"), ("health_access", "Health access"), ("basic_services", "Basic services"),
                         ("housing", "Housing quality"), ("connectivity", "Connectivity"), ("nightlights", "Night-time lights"),
                         ("poverty", "Poverty")]],
   source=["Lanka Data Layer derived-development-index method."], notes=["Sub-indices: no total."])
cl("land_cover_source", "Land cover input source",
   [c(k, v) for k, v in [("esa_worldcover", "ESA WorldCover"), ("hansen_gfc", "Hansen Global Forest Change"),
                         ("jrc_gsw", "JRC Global Surface Water"), ("osm", "OpenStreetMap"), ("wdpa", "WDPA protected areas")]],
   source=["Lanka Data Layer derived-land-cover-zonal method inputs."], notes=["Alternative sources: no total."])
cl("wpp_variant", "UN World Population Prospects variant",
   [c(k, v) for k, v in [("estimates", "Estimates (past)"), ("medium", "Medium"), ("high", "High"), ("low", "Low"),
                         ("constant_fertility", "Constant fertility"), ("instant_replacement", "Instant replacement fertility"),
                         ("zero_migration", "Zero migration"), ("constant_mortality", "Constant mortality"), ("no_change", "No change"),
                         ("momentum", "Momentum"), ("lower80", "Lower 80 PI"), ("upper80", "Upper 80 PI"),
                         ("lower95", "Lower 95 PI"), ("upper95", "Upper 95 PI")]],
   source=["UN DESA Population Division, World Population Prospects 2024 - projection variants and probabilistic intervals."],
   notes=["Alternative projections: no total."])
cl("ipc_projection", "IPC analysis period",
   [c("current", "Current situation (CS)"), c("ml1", "Most likely, near-term projection (ML1)"), c("ml2", "Most likely, medium-term projection (ML2)")],
   source=["FEWS NET IPC-compatible food security outlook - current, near-term and medium-term projections."], notes=["No total."])
cl("migrant_skill_level", "Migrant worker skill category (SLBFE)",
   [T("All departures")] + [c(k, v) for k, v in [("professional", "Professional"), ("middle_level", "Middle level"),
                                                ("clerical", "Clerical and related"), ("skilled", "Skilled"), ("semi_skilled", "Semi-skilled"),
                                                ("unskilled", "Unskilled"), ("domestic_housekeeping", "Domestic housekeeping assistant (housemaid)")]],
   source=["Sri Lanka Bureau of Foreign Employment, Annual Statistical Report of Foreign Employment - departures by manpower level."])
cl("provider", "Vocational training provider",
   [T("All providers")] + [c(k, v) for k, v in [("vta", "Vocational Training Authority (VTA)"), ("naita", "National Apprentice and Industrial Training Authority (NAITA)"),
                                                ("dtet", "Department of Technical Education and Training (DTET)"),
                                                ("nysc", "National Youth Services Council (NYSC)"), ("other", "Other (incl. private, registered with TVEC)")]],
   source=["Tertiary and Vocational Education Commission (TVEC), Labour Market Information Bulletin - training providers."], status="seed")
cl("finding_type", "Audit finding type",
   [T("All quantified findings")] + [c(k, v) for k, v in [("unsettled_advances", "Unsettled advances"), ("idle_assets", "Idle / underutilised assets"),
                                                         ("losses_damages", "Losses and damages"), ("irregular_payments", "Irregular / unauthorised payments"),
                                                         ("unrecovered_dues", "Unrecovered dues"), ("other", "Other")]],
   source=["National Audit Office (Auditor General's Department), annual detailed audit reports - categories of quantified findings."], status="seed")
cl("asset_type", "Emissions asset type (Climate TRACE)",
   [T("All assets")] + [c(k, v) for k, v in [("electricity_generation", "Power plant"), ("cement", "Cement plant"), ("steel", "Iron and steel plant"),
                                             ("oil_and_gas_refining", "Oil refinery"), ("port", "Port"), ("road_transportation", "Road segment"),
                                             ("solid_waste_disposal", "Solid waste site"), ("other", "Other asset")]],
   source=["Climate TRACE asset-level emissions data (subsector names)."], status="seed")
cl("climate_trace_sector", "Emissions sector (Climate TRACE)",
   [T("All sectors")] + [c(k, v) for k, v in [("power", "Power"), ("manufacturing", "Manufacturing"), ("transportation", "Transportation"),
                                              ("buildings", "Buildings"), ("fossil_fuel_operations", "Fossil fuel operations"),
                                              ("agriculture", "Agriculture"), ("waste", "Waste"), ("forestry_and_land_use", "Forestry and land use"),
                                              ("mineral_extraction", "Mineral extraction"), ("fluorinated_gases", "Fluorinated gases")]],
   source=["Climate TRACE sector taxonomy (climatetrace.org data downloads)."])
cl("commodity", "Food / market commodity",
   [T("All commodities"), c("rice", "Rice"), c("vegetables", "Vegetables"), c("fruits", "Fruits"), c("fish", "Fish"), c("pulses", "Pulses"),
    c("other_food", "Other food"), c("fuel", "Fuel"),
    c("rice_samba", "Samba rice", p="rice"), c("rice_nadu", "Nadu rice", p="rice"), c("rice_kekulu_white", "Kekulu rice (white)", p="rice"),
    c("rice_kekulu_red", "Kekulu rice (red)", p="rice"), c("beans", "Beans", p="vegetables"), c("carrot", "Carrot", p="vegetables"),
    c("cabbage", "Cabbage", p="vegetables"), c("tomato", "Tomato", p="vegetables"), c("brinjal", "Brinjal", p="vegetables"),
    c("pumpkin", "Pumpkin", p="vegetables"), c("potato", "Potato", p="vegetables"), c("big_onion", "Big onion", p="vegetables"),
    c("red_onion", "Red onion", p="vegetables"), c("dried_chillies", "Dried chillies", p="vegetables"),
    c("banana", "Banana", p="fruits"), c("papaya", "Papaya", p="fruits"), c("lentils", "Lentils (red dhal)", p="pulses"),
    c("wheat_flour", "Wheat flour", p="other_food"), c("sugar", "Sugar", p="other_food"), c("coconut", "Coconut", p="other_food"),
    c("eggs", "Eggs", p="other_food"), c("kerosene", "Kerosene", p="fuel"), c("diesel", "Diesel", p="fuel"), c("petrol", "Petrol", p="fuel")],
   source=["Hector Kobbekaduwa Agrarian Research and Training Institute (HARTI) daily food commodities bulletin; WFP VAM food prices (Sri Lanka)."],
   status="seed", notes=["Varieties (e.g. fish species, beans types) must be completed from the HARTI bulletin; WFP commodity names map here in a crosswalk.",
                          "Prices are not additive: parents are a grouping only."])
cl("building_material", "Building material (price statistics)",
   [c(k, v) for k, v in [("cement", "Cement (50 kg bag)"), ("steel_bars", "Steel reinforcement bars"), ("bricks", "Bricks"),
                         ("sand", "Sand"), ("metal_aggregate", "Metal (aggregate)"), ("timber", "Timber"), ("roofing_sheets", "Roofing sheets"),
                         ("paint", "Paint")]],
   source=[f"{DCS}, Pocket Book - average retail prices of building materials; ICTAD/CIDA building material price indices."],
   status="seed", notes=["Prices: no total."])
cl("disaster_event", "Named disaster event",
   [c(k, v) for k, v in [("2016-05-cyclone-roanu-floods", "May 2016 floods and landslides (Cyclone Roanu)"),
                         ("2017-05-sw-monsoon-floods", "May 2017 south-west monsoon floods and landslides"),
                         ("2024-06-sw-monsoon-floods", "June 2024 south-west monsoon floods"),
                         ("2025-11-cyclone-ditwah", "Cyclone Ditwah floods and landslides (Nov-Dec 2025)")]],
   source=["Disaster Management Centre situation reports; UNOSAT / Copernicus EMS activations for Sri Lanka."],
   status="seed", notes=["Add GLIDE numbers as the event code crosswalk; events are not additive (no total)."])
cl("size_class", "Establishment size class (persons engaged)",
   [T("All establishments")] + [c(k, v) for k, v in [("micro", "Micro (fewer than 10 persons engaged)"), ("small", "Small (10-49 persons engaged)"),
                                                     ("medium", "Medium (50-199 persons engaged)"), ("large", "Large (200+ persons engaged)")]],
   source=["Ministry of Industry and Commerce, National Policy Framework for SME Development (2016) employment thresholds; DCS Economic Census 2013/14 reports by persons-engaged size."],
   status="seed", notes=["DCS Economic Census tables use persons-engaged bands that may differ from the SME policy thresholds (which also differ for manufacturing vs services); confirm against the census table."])
