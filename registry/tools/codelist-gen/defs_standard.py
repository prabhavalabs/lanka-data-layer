"""International-standard codelists (SDMX / UN / ISO / ILO / WHO / ESA ...)."""
from lib import c, T, cl

# ---------------------------------------------------------------- age (master)
_age = [T("All ages")]
_age += [c("0", "Under 1 year", s="Y_LT1"), c("1-4", "1-4 years", s="Y1T4")]
for lo in range(0, 100, 5):
    _age.append(c(f"{lo}-{lo+4}", f"{lo}-{lo+4} years", s=f"Y{lo}T{lo+4}"))
_age += [
    c("80+", "80 years and over", s="Y_GE80"), c("85+", "85 years and over", s="Y_GE85"),
    c("90+", "90 years and over", s="Y_GE90"), c("100+", "100 years and over", s="Y_GE100"),
    c("0-14", "Under 15 years", s="Y_LT15"), c("0-17", "Under 18 years", s="Y_LT18"),
    c("15+", "15 years and over", s="Y_GE15"), c("18+", "18 years and over", s="Y_GE18"),
    c("15-24", "15-24 years", s="Y15T24"), c("15-49", "15-49 years", s="Y15T49"),
    c("15-59", "15-59 years", s="Y15T59"), c("15-64", "15-64 years", s="Y15T64"),
    c("25+", "25 years and over", s="Y_GE25"), c("25-34", "25-34 years", s="Y25T34"),
    c("25-54", "25-54 years", s="Y25T54"), c("30-39", "30-39 years", s="Y30T39"),
    c("35-44", "35-44 years", s="Y35T44"), c("40-49", "40-49 years", s="Y40T49"),
    c("45-54", "45-54 years", s="Y45T54"), c("50+", "50 years and over", s="Y_GE50"),
    c("55-64", "55-64 years", s="Y55T64"), c("60+", "60 years and over", s="Y_GE60"),
    c("65+", "65 years and over", s="Y_GE65"),
]
cl("age", "Age (shared master: single, 5-year and broad bands)", _age, total_parent=False,
   sdmx_ref="SDMX:CL_AGE(1.0)",
   source=["SDMX global codelist CL_AGE (SDMX Global Registry); band choice covers DCS census A5/GN tables, DCS LFS, ILOSTAT (ILO harmonised bands), UN WPP and WorldPop 5-year groups."],
   notes=["Bands overlap by design (5-year, broad and survey-specific bands live in one list, as in CL_AGE); "
          "no code has a parent because overlapping bands do not form one additive hierarchy - an indicator "
          "must use one non-overlapping partition per release.",
          "Census 2024 A5 tables keep using the existing `age_group` codelist (0-14, 15-59, 60-64, 65+); every one of those bands also exists here."])

# ---------------------------------------------------------------- country
_iso_fix = {"AG": "Antigua and Barbuda", "AS": "American Samoa", "BA": "Bosnia and Herzegovina", "BL": "Saint Barthelemy",
            "BN": "Brunei Darussalam", "BO": "Bolivia", "CD": "Congo, Democratic Republic of the", "CG": "Congo",
            "CV": "Cabo Verde", "CZ": "Czechia", "FM": "Micronesia (Federated States of)", "GB": "United Kingdom",
            "GS": "South Georgia and the South Sandwich Islands", "HM": "Heard Island and McDonald Islands",
            "IR": "Iran", "KN": "Saint Kitts and Nevis", "KP": "Korea, Democratic People's Republic of",
            "KR": "Korea, Republic of", "LA": "Lao People's Democratic Republic", "LC": "Saint Lucia",
            "MD": "Moldova", "MF": "Saint Martin (French part)", "MM": "Myanmar", "PM": "Saint Pierre and Miquelon",
            "PS": "Palestine, State of", "RU": "Russian Federation", "SH": "Saint Helena, Ascension and Tristan da Cunha",
            "SJ": "Svalbard and Jan Mayen", "ST": "Sao Tome and Principe", "SX": "Sint Maarten (Dutch part)",
            "SY": "Syrian Arab Republic", "SZ": "Eswatini", "TC": "Turks and Caicos Islands", "TR": "Turkiye",
            "TT": "Trinidad and Tobago", "TW": "Taiwan", "TZ": "Tanzania, United Republic of", "VA": "Holy See",
            "VC": "Saint Vincent and the Grenadines", "VE": "Venezuela", "VG": "Virgin Islands (British)",
            "VI": "Virgin Islands (U.S.)", "VN": "Viet Nam", "WF": "Wallis and Futuna", "WS": "Samoa",
            "US": "United States of America"}
_cty = [T("All countries / world")]
for line in open("/usr/share/zoneinfo/iso3166.tab", encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        continue
    a2, name = line.rstrip("\n").split("\t")[:2]
    _cty.append(c(a2, _iso_fix.get(a2, name)))
_cty.append(c("XK", "Kosovo (user-assigned code)"))
_cty.append(c("other", "Other / not specified", s="_O"))
cl("country", "Country or area (ISO 3166-1 alpha-2)", _cty, sdmx_ref="SDMX:CL_AREA",
   source=["ISO 3166-1 alpha-2 (ISO 3166 Maintenance Agency); code list taken from the IANA tz iso3166.tab copy of ISO 3166-1, names normalised to ISO short names."],
   notes=["Shared by every country-valued dimension (origin_country, source_country, destination country, trade partner): reference it as {id: <dim>, codelist: country}.",
          "Codes are alpha-2 as in SDMX CL_AREA; alpha-3 / M49 crosswalk is a registry/crosswalks job.",
          "Source-specific regional aggregates (e.g. SLTDA 'Western Europe', SLBFE 'Middle East') are not countries: add them as codes here only once a dataset needs them, with parent total."])

# ---------------------------------------------------------------- currency
_cur = [("USD", "US dollar"), ("EUR", "Euro"), ("GBP", "Pound sterling"), ("JPY", "Yen"), ("CNY", "Yuan renminbi"),
        ("INR", "Indian rupee"), ("AUD", "Australian dollar"), ("CAD", "Canadian dollar"), ("CHF", "Swiss franc"),
        ("SGD", "Singapore dollar"), ("HKD", "Hong Kong dollar"), ("NZD", "New Zealand dollar"), ("SEK", "Swedish krona"),
        ("DKK", "Danish krone"), ("NOK", "Norwegian krone"), ("MYR", "Malaysian ringgit"), ("THB", "Baht"),
        ("KRW", "Won"), ("RUB", "Russian ruble"), ("ZAR", "Rand"), ("AED", "UAE dirham"), ("SAR", "Saudi riyal"),
        ("KWD", "Kuwaiti dinar"), ("QAR", "Qatari riyal"), ("BHD", "Bahraini dinar"), ("OMR", "Rial Omani"),
        ("JOD", "Jordanian dinar"), ("PKR", "Pakistan rupee"), ("BDT", "Taka"), ("MVR", "Rufiyaa"), ("NPR", "Nepalese rupee"),
        ("IDR", "Rupiah"), ("PHP", "Philippine peso"), ("LKR", "Sri Lanka rupee"), ("XDR", "SDR (Special Drawing Right)")]
cl("currency", "Currency (ISO 4217)", [c(a, b) for a, b in _cur], sdmx_ref="SDMX:CL_CURRENCY",
   source=["ISO 4217 currency codes (SIX, ISO 4217 Maintenance Agency); member set = currencies quoted in CBSL indicative/exchange-rate releases plus SDR."],
   notes=["Quote currency of an exchange-rate observation (LKR per unit of <code>). Extend from the CBSL release when a new currency appears."],
   status="seed")

# ---------------------------------------------------------------- industry (ISIC Rev.4 / SLSIC Rev.4 sections)
_isic = [T("All economic activities"),
         c("agriculture", "Agriculture (section A)"), c("industry", "Industry (sections B-F)"), c("services", "Services (sections G-U)"),
         c("A", "Agriculture, forestry and fishing", p="agriculture"),
         c("B", "Mining and quarrying", p="industry"), c("C", "Manufacturing", p="industry"),
         c("D", "Electricity, gas, steam and air conditioning supply", p="industry"),
         c("E", "Water supply; sewerage, waste management and remediation activities", p="industry"),
         c("F", "Construction", p="industry"),
         c("G", "Wholesale and retail trade; repair of motor vehicles and motorcycles", p="services"),
         c("H", "Transportation and storage", p="services"), c("I", "Accommodation and food service activities", p="services"),
         c("J", "Information and communication", p="services"), c("K", "Financial and insurance activities", p="services"),
         c("L", "Real estate activities", p="services"), c("M", "Professional, scientific and technical activities", p="services"),
         c("N", "Administrative and support service activities", p="services"),
         c("O", "Public administration and defence; compulsory social security", p="services"),
         c("P", "Education", p="services"), c("Q", "Human health and social work activities", p="services"),
         c("R", "Arts, entertainment and recreation", p="services"), c("S", "Other service activities", p="services"),
         c("T", "Activities of households as employers; undifferentiated goods- and services-producing activities of households for own use", p="services"),
         c("U", "Activities of extraterritorial organizations and bodies", p="services"),
         c("taxes_less_subsidies", "Taxes less subsidies on products (GDP reconciliation item)")]
cl("industry", "Economic activity (ISIC Rev.4 / SLSIC Rev.4 sections, with DCS broad sectors)", _isic,
   sdmx_ref="SDMX:CL_ACTIVITY_ISIC4",
   source=["UN ISIC Rev.4 (UNSD, 2008) sections A-U; DCS Sri Lanka Standard Industrial Classification (SLSIC) Rev.4 is identical at section level.",
           "DCS national accounts broad sectors: Agriculture = A, Industries = B-F, Services = G-T (DCS GDP releases)."],
   notes=["Broad sectors agriculture/industry/services are aggregates with the sections as children, so provincial GDP, LFS 'sector' and PMI (C / services) reuse this list.",
          "DCS GDP by industry publishes ~20+ activity lines (e.g. growing of tea, rice) finer than sections; those need ISIC division codes added here as children of the sections."])

# ---------------------------------------------------------------- occupation (ISCO-08 / SLSCO-08 major groups)
_isco = [T("All occupations")] + [c(k, v, s=f"ISCO08_{k}") for k, v in [
    ("1", "Managers"), ("2", "Professionals"), ("3", "Technicians and associate professionals"),
    ("4", "Clerical support workers"), ("5", "Service and sales workers"),
    ("6", "Skilled agricultural, forestry and fishery workers"), ("7", "Craft and related trades workers"),
    ("8", "Plant and machine operators, and assemblers"), ("9", "Elementary occupations"), ("0", "Armed forces occupations")]]
_isco.append(c("X", "Not elsewhere classified", s="ISCO08_X"))
cl("occupation", "Occupation (ISCO-08 / SLSCO-08 major groups)", _isco, sdmx_ref="ILO:CL_OCU_ISCO08",
   source=["ILO International Standard Classification of Occupations ISCO-08 major groups; DCS Sri Lanka Standard Classification of Occupations (SLSCO-08) uses the same major groups in the LFS."],
   notes=["`sdmx` carries the ILOSTAT classif1 suffix (OCU_ISCO08_<code>)."])

# ---------------------------------------------------------------- COICOP 2018
_coicop = [T("All items (headline)"), c("food", "Food and non-alcoholic beverages (food group)"), c("nonfood", "Non-food items")]
for k, v in [("01", "Food and non-alcoholic beverages"), ("02", "Alcoholic beverages, tobacco and narcotics"),
             ("03", "Clothing and footwear"), ("04", "Housing, water, electricity, gas and other fuels"),
             ("05", "Furnishings, household equipment and routine household maintenance"), ("06", "Health"),
             ("07", "Transport"), ("08", "Information and communication"), ("09", "Recreation, sport and culture"),
             ("10", "Education services"), ("11", "Restaurants and accommodation services"),
             ("12", "Insurance and financial services"), ("13", "Personal care, social protection and miscellaneous goods and services")]:
    _coicop.append(c(k, v, p="food" if k == "01" else "nonfood"))
cl("coicop_group", "Consumption purpose (COICOP 2018 divisions, with food / non-food)", _coicop, sdmx_ref="UN:COICOP_2018",
   source=["UN Classification of Individual Consumption According to Purpose (COICOP 2018), divisions 01-13 (UNSD).",
           "DCS CCPI (2021=100) and NCPI (2021=100) publish group indices by COICOP division and a food / non-food split."],
   notes=["DCS NCPI 'group' (food / non-food) reuses this list via the `food` and `nonfood` aggregates.",
          "HIES expenditure categories are COICOP-like but not identical (see review CSV); map them here before ingest."])

# ---------------------------------------------------------------- ICD-10 chapters
_icd = [("I", "A00-B99", "Certain infectious and parasitic diseases"), ("II", "C00-D48", "Neoplasms"),
        ("III", "D50-D89", "Diseases of the blood and blood-forming organs and certain disorders involving the immune mechanism"),
        ("IV", "E00-E90", "Endocrine, nutritional and metabolic diseases"), ("V", "F00-F99", "Mental and behavioural disorders"),
        ("VI", "G00-G99", "Diseases of the nervous system"), ("VII", "H00-H59", "Diseases of the eye and adnexa"),
        ("VIII", "H60-H95", "Diseases of the ear and mastoid process"), ("IX", "I00-I99", "Diseases of the circulatory system"),
        ("X", "J00-J99", "Diseases of the respiratory system"), ("XI", "K00-K93", "Diseases of the digestive system"),
        ("XII", "L00-L99", "Diseases of the skin and subcutaneous tissue"),
        ("XIII", "M00-M99", "Diseases of the musculoskeletal system and connective tissue"),
        ("XIV", "N00-N99", "Diseases of the genitourinary system"), ("XV", "O00-O99", "Pregnancy, childbirth and the puerperium"),
        ("XVI", "P00-P96", "Certain conditions originating in the perinatal period"),
        ("XVII", "Q00-Q99", "Congenital malformations, deformations and chromosomal abnormalities"),
        ("XVIII", "R00-R99", "Symptoms, signs and abnormal clinical and laboratory findings, not elsewhere classified"),
        ("XIX", "S00-T98", "Injury, poisoning and certain other consequences of external causes"),
        ("XX", "V01-Y98", "External causes of morbidity and mortality"),
        ("XXI", "Z00-Z99", "Factors influencing health status and contact with health services"),
        ("XXII", "U00-U85", "Codes for special purposes")]
cl("icd_group", "Disease group (ICD-10 chapters)", [T("All causes")] + [c(k, f"{v} ({r})") for k, r, v in _icd],
   source=["WHO International Statistical Classification of Diseases and Related Health Problems, 10th revision (ICD-10, 2019 edition), chapters I-XXII.",
           "Ministry of Health Indoor Morbidity and Mortality Return (IMMR) reports discharges/deaths by ICD-10 disease group."],
   notes=["IMMR publishes finer ICD-10 groupings (block level); add them as children of the chapter codes when ingested."])

# ---------------------------------------------------------------- SDG goals
_sdg = [("1", "No poverty"), ("2", "Zero hunger"), ("3", "Good health and well-being"), ("4", "Quality education"),
        ("5", "Gender equality"), ("6", "Clean water and sanitation"), ("7", "Affordable and clean energy"),
        ("8", "Decent work and economic growth"), ("9", "Industry, innovation and infrastructure"),
        ("10", "Reduced inequalities"), ("11", "Sustainable cities and communities"),
        ("12", "Responsible consumption and production"), ("13", "Climate action"), ("14", "Life below water"),
        ("15", "Life on land"), ("16", "Peace, justice and strong institutions"), ("17", "Partnerships for the goals")]
cl("goal", "Sustainable Development Goal", [c(k, f"Goal {k}: {v}") for k, v in _sdg],
   source=["UN General Assembly resolution A/RES/70/1 (2015); UN SDG Global Database (unstats.un.org/sdgs) goal codes."],
   notes=["Goals are a classification of series, not additive members, so there is no total."])

# ---------------------------------------------------------------- WorldCover / Dynamic World / GHSL SMOD
_wc = [("10", "Tree cover"), ("20", "Shrubland"), ("30", "Grassland"), ("40", "Cropland"), ("50", "Built-up"),
       ("60", "Bare / sparse vegetation"), ("70", "Snow and ice"), ("80", "Permanent water bodies"),
       ("90", "Herbaceous wetland"), ("95", "Mangroves"), ("100", "Moss and lichen")]
cl("lc_class", "Land cover class (ESA WorldCover 10 m, official class codes)", [T("All land cover")] + [c(k, v) for k, v in _wc],
   source=["ESA WorldCover 10 m 2020 v100 / 2021 v200 Product User Manual, Table 1 (class values 10-100)."],
   notes=["The platform reference land-cover scheme. derived-land-cover-zonal shares (cropland, tree cover, built-up, water) map to 40/10/50/80; "
          "'protected area' is not a land-cover class and belongs in its own indicator.",
          "Dynamic World uses a different scheme: see dynamic_world_class."])
_dw = [("0", "Water"), ("1", "Trees"), ("2", "Grass"), ("3", "Flooded vegetation"), ("4", "Crops"),
       ("5", "Shrub and scrub"), ("6", "Built"), ("7", "Bare"), ("8", "Snow and ice")]
cl("dynamic_world_class", "Land cover class (Google/WRI Dynamic World V1 label band)", [T("All land cover")] + [c(k, v) for k, v in _dw],
   source=["Brown et al. (2022) Dynamic World, Scientific Data 9:251; Earth Engine catalogue GOOGLE/DYNAMICWORLD/V1 'label' band values 0-8."])
_smod = [T("All settlement classes"), c("3", "Urban centre (L1)"), c("2", "Urban cluster (L1)"), c("1", "Rural grid cells (L1)"),
         c("30", "Urban centre grid cell", p="3"), c("23", "Dense urban cluster grid cell", p="2"),
         c("22", "Semi-dense urban cluster grid cell", p="2"), c("21", "Suburban or peri-urban grid cell", p="2"),
         c("13", "Rural cluster grid cell", p="1"), c("12", "Low density rural grid cell", p="1"),
         c("11", "Very low density rural grid cell", p="1"), c("10", "Water grid cell")]
cl("smod_class", "Settlement typology (GHSL SMOD, Degree of Urbanisation stage I)", _smod,
   source=["European Commission JRC, GHSL Data Package 2023, GHS-SMOD R2023A; Degree of Urbanisation methodology (UN Statistical Commission 2020) level 1 and level 2 classes."])

# ---------------------------------------------------------------- IUCN / WDPA
_iucn = [T("All protected areas"), c("Ia", "Ia Strict nature reserve"), c("Ib", "Ib Wilderness area"), c("II", "II National park"),
         c("III", "III Natural monument or feature"), c("IV", "IV Habitat/species management area"),
         c("V", "V Protected landscape/seascape"), c("VI", "VI Protected area with sustainable use of natural resources"),
         c("not_reported", "Not reported"), c("not_applicable", "Not applicable"), c("not_assigned", "Not assigned")]
cl("iucn_cat", "IUCN protected area management category (WDPA IUCN_CAT)", _iucn,
   source=["Dudley (ed.) 2008, Guidelines for Applying Protected Area Management Categories, IUCN; UNEP-WCMC WDPA User Manual 1.6, field IUCN_CAT (values incl. 'Not Reported', 'Not Applicable', 'Not Assigned')."],
   notes=["National designations (DWC national park, sanctuary, strict natural reserve; Forest Department forest reserve, conservation forest) are WDPA DESIG values, not IUCN categories."])

# ---------------------------------------------------------------- OSM-derived
_amen = [T("All amenities"), c("health", "Health"), c("education", "Education"), c("finance", "Finance"),
         c("public_service", "Public services"), c("transport", "Transport"), c("community", "Community and worship"),
         c("hospital", "Hospital", p="health"), c("clinic", "Clinic", p="health"), c("doctors", "Doctors", p="health"),
         c("dentist", "Dentist", p="health"), c("pharmacy", "Pharmacy", p="health"),
         c("kindergarten", "Kindergarten / preschool", p="education"), c("school", "School", p="education"),
         c("college", "College", p="education"), c("university", "University", p="education"),
         c("library", "Library", p="education"), c("bank", "Bank", p="finance"), c("atm", "ATM", p="finance"),
         c("bureau_de_change", "Bureau de change", p="finance"), c("police", "Police", p="public_service"),
         c("fire_station", "Fire station", p="public_service"), c("post_office", "Post office", p="public_service"),
         c("townhall", "Town hall", p="public_service"), c("courthouse", "Courthouse", p="public_service"),
         c("government_office", "Government office (office=government)", p="public_service"),
         c("fuel", "Fuel station", p="transport"), c("bus_station", "Bus station", p="transport"),
         c("marketplace", "Marketplace", p="community"), c("place_of_worship", "Place of worship", p="community"),
         c("community_centre", "Community centre", p="community"), c("other", "Other amenity")]
cl("amenity_type", "Amenity type (OpenStreetMap amenity=* values, grouped)", _amen,
   source=["OpenStreetMap wiki, Key:amenity (codes are the tag values); `government_office` is office=government."],
   notes=["Places of worship are not split by religion here; use the existing `religion` codelist as a second dimension if needed (OSM religion=*)."])
_hw = [T("All roads and paths")] + [c(k, v) for k, v in [
    ("motorway", "Motorway (expressway)"), ("trunk", "Trunk"), ("primary", "Primary"), ("secondary", "Secondary"),
    ("tertiary", "Tertiary"), ("unclassified", "Unclassified (minor public road)"), ("residential", "Residential"),
    ("living_street", "Living street"), ("service", "Service"), ("track", "Track"), ("pedestrian", "Pedestrian"),
    ("footway", "Footway"), ("path", "Path"), ("cycleway", "Cycleway"), ("steps", "Steps"), ("road", "Road (unknown class)")]]
_hw += [c(f"{k}_link", f"{k.capitalize()} link", p=k) for k in ("motorway", "trunk", "primary", "secondary", "tertiary")]
cl("highway_class", "Road class (OpenStreetMap highway=* values)", _hw,
   source=["OpenStreetMap wiki, Key:highway; Sri Lanka tagging: A roads usually trunk/primary, B roads secondary/tertiary (OSM Sri Lanka road tagging guidelines)."],
   notes=["Official RDA/provincial classes are in `road_class`; OSM classes are not a 1:1 match."])
_pl = [T("All places")] + [c(k, v) for k, v in [("city", "City"), ("town", "Town"), ("village", "Village"),
                                                 ("hamlet", "Hamlet"), ("suburb", "Suburb"), ("neighbourhood", "Neighbourhood"),
                                                 ("quarter", "Quarter"), ("locality", "Locality (unpopulated named place)"),
                                                 ("isolated_dwelling", "Isolated dwelling"), ("island", "Island"), ("islet", "Islet")]]
cl("place_class", "Place class (OpenStreetMap place=* values)", _pl, source=["OpenStreetMap wiki, Key:place."])
_pc = [T("All protected areas")] + [c(k, v) for k, v in [
    ("1a", "1a Strict nature reserve (IUCN Ia)"), ("1b", "1b Wilderness area (IUCN Ib)"), ("2", "2 National park (IUCN II)"),
    ("3", "3 Natural monument (IUCN III)"), ("4", "4 Habitat/species management (IUCN IV)"),
    ("5", "5 Protected landscape/seascape (IUCN V)"), ("6", "6 Resource-use protected area (IUCN VI)"),
    ("7", "7 Nature protection outside IUCN categories"), ("resource", "11-19 Resource protection (water, soil, forest...)"),
    ("social", "21-29 Social protection (cultural, religious, heritage...)"), ("unknown", "No protect_class tagged")]]
cl("protect_class", "Protection class (OpenStreetMap protect_class=*)", _pc,
   source=["OpenStreetMap wiki, Key:protect_class (1a-6 follow IUCN categories; 11-19 resource protection; 21-29 social protection)."],
   notes=["11-19 and 21-29 are collapsed to `resource` / `social` because the individual sub-values are sparsely used in Sri Lanka; split if needed."])
_trail = [T("All trails and cycleways")] + [c(k, v) for k, v in [
    ("footway", "Footway (highway=footway)"), ("path", "Path (highway=path)"), ("bridleway", "Bridleway (highway=bridleway)"),
    ("track", "Track (highway=track)"), ("cycleway", "Cycleway (highway=cycleway)"), ("steps", "Steps (highway=steps)"),
    ("hiking_route", "Hiking route relation (route=hiking)"), ("bicycle_route", "Cycle route relation (route=bicycle)")]]
cl("trail_type", "Trail and cycleway type (OpenStreetMap)", _trail, source=["OpenStreetMap wiki, Key:highway and Tag:route=hiking / route=bicycle."])
_ww = [T("All waterways and water bodies"), c("linear", "Waterways (lines)"), c("area", "Water bodies (areas)"),
       c("river", "River", p="linear"), c("stream", "Stream", p="linear"), c("canal", "Canal (incl. irrigation channels)", p="linear"),
       c("drain", "Drain", p="linear"), c("ditch", "Ditch", p="linear"),
       c("lake", "Lake (water=lake)", p="area"), c("reservoir", "Reservoir / tank (water=reservoir)", p="area"),
       c("pond", "Pond", p="area"), c("lagoon", "Lagoon", p="area"), c("river_area", "River area (water=river)", p="area"),
       c("wetland", "Wetland (natural=wetland)", p="area")]
cl("waterway_type", "Waterway / water body type (OpenStreetMap waterway=* and water=*)", _ww,
   source=["OpenStreetMap wiki, Key:waterway and Key:water."],
   notes=["Ancient irrigation tanks are tagged water=reservoir (or landuse=reservoir) in Sri Lanka."])
_apt = [T("All airports")] + [c(k, v) for k, v in [
    ("large_airport", "Large airport"), ("medium_airport", "Medium airport"), ("small_airport", "Small airport / airstrip"),
    ("heliport", "Heliport"), ("seaplane_base", "Seaplane base"), ("balloonport", "Balloon port"), ("closed", "Closed")]]
cl("airport_type", "Airport type (OurAirports `type` field)", _apt, source=["OurAirports data dictionary, airports.csv field `type` (ourairports.com/help/data-dictionary.html)."])
_fc = [T("All feature classes")] + [c(k, v) for k, v in [
    ("A", "A: country, state, region (administrative)"), ("H", "H: stream, lake (hydrographic)"), ("L", "L: parks, area"),
    ("P", "P: city, village (populated place)"), ("R", "R: road, railroad"), ("S", "S: spot, building, farm"),
    ("T", "T: mountain, hill, rock (hypsographic)"), ("U", "U: undersea"), ("V", "V: forest, heath (vegetation)")]]
cl("feature_class", "GeoNames feature class", _fc, source=["GeoNames feature codes (geonames.org/export/codes.html), feature classes A, H, L, P, R, S, T, U, V."])

# ---------------------------------------------------------------- hazards
_hz = [T("All hazards"),
       c("geophysical", "Geophysical"), c("hydrological", "Hydrological"), c("meteorological", "Meteorological"),
       c("climatological", "Climatological"), c("biological", "Biological"), c("other", "Other / unspecified"),
       c("earthquake", "Earthquake (GLIDE EQ)", p="geophysical"), c("tsunami", "Tsunami (GLIDE TS)", p="geophysical"),
       c("volcano", "Volcanic activity (GLIDE VO)", p="geophysical"),
       c("flood", "Flood (GLIDE FL)", p="hydrological"), c("river_flood", "River flood", p="flood"),
       c("flash_flood", "Flash flood (GLIDE FF)", p="flood"), c("urban_flood", "Urban flood", p="flood"),
       c("coastal_flood", "Coastal flood / storm surge (GLIDE SS)", p="flood"),
       c("landslide", "Landslide / mass movement (GLIDE LS)", p="hydrological"),
       c("coastal_erosion", "Coastal erosion", p="hydrological"),
       c("cyclone", "Tropical cyclone (GLIDE TC)", p="meteorological"), c("storm", "Severe local storm / high wind (GLIDE ST)", p="meteorological"),
       c("lightning", "Lightning", p="meteorological"), c("extreme_heat", "Extreme heat / heat wave (GLIDE HT)", p="meteorological"),
       c("drought", "Drought / water scarcity (GLIDE DR)", p="climatological"), c("wildfire", "Wildfire (GLIDE WF)", p="climatological"),
       c("epidemic", "Epidemic (GLIDE EP)", p="biological"), c("animal_attack", "Wild animal attack (e.g. elephant)", p="biological")]
cl("hazard", "Natural hazard type (EM-DAT families, GLIDE codes)", _hz,
   source=["CRED EM-DAT classification of natural hazards (geophysical, hydrological, meteorological, climatological, biological); GLIDE hazard codes (glidenumber.net).",
           "Covers ThinkHazard! (11 hazards), DesInventar Sri Lanka and DMC situation-report event types."],
   notes=["ThinkHazard 'water scarcity' maps to drought; 'river flood', 'urban flood', 'coastal flood' map to the flood children.",
          "EM-DAT places landslide (wet mass movement) under hydrological; dry mass movement would be geophysical."])
_hl = [T("All levels")] + [c(k, v) for k, v in [
    ("very_low", "Very low"), ("low", "Low (NBRO: landslides not likely to occur)"),
    ("medium", "Medium / moderate (NBRO: modest level of landslide hazard)"),
    ("high", "High (NBRO: landslides are to be expected)"), ("very_high", "Very high (NBRO: landslides most likely to occur)"),
    ("no_data", "No data")]]
cl("hazard_level", "Hazard level (ordinal)", _hl,
   source=["GFDRR ThinkHazard! hazard levels (very low, low, medium, high); NBRO Landslide Hazard Zonation Mapping Project 1:10,000 zone legend (four classes)."],
   notes=["Shared by thinkhazard-levels, nbro-landslide-hazard-zones (hazard_class) and derived-multi-hazard-exposure (intensity).",
          "NBRO uses four classes with no 'very low'; ThinkHazard has no 'very high'."])
_ga = [T("All alerts")] + [c(k, v) for k, v in [("green", "Green"), ("orange", "Orange"), ("red", "Red")]]
cl("alert_level", "GDACS alert level", _ga, source=["Global Disaster Alert and Coordination System (GDACS, EC-JRC / UN OCHA) alert levels green / orange / red."])
_rp = [c(f"rp{n}", f"1-in-{n}-year (return period {n} years)") for n in (2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000)]
cl("return_period", "Return period", _rp, source=["EC-JRC Global River Flood Hazard Maps (return periods 10, 20, 50, 100, 200, 500 years); other periods for other hazard models."],
   notes=["Return periods are alternative scenarios, never additive: no total."])

# ---------------------------------------------------------------- climate / environment
_bio = [("BIO1", "Annual mean temperature"), ("BIO2", "Mean diurnal range (mean of monthly (max temp - min temp))"),
        ("BIO3", "Isothermality (BIO2/BIO7 x 100)"), ("BIO4", "Temperature seasonality (standard deviation x 100)"),
        ("BIO5", "Max temperature of warmest month"), ("BIO6", "Min temperature of coldest month"),
        ("BIO7", "Temperature annual range (BIO5-BIO6)"), ("BIO8", "Mean temperature of wettest quarter"),
        ("BIO9", "Mean temperature of driest quarter"), ("BIO10", "Mean temperature of warmest quarter"),
        ("BIO11", "Mean temperature of coldest quarter"), ("BIO12", "Annual precipitation"),
        ("BIO13", "Precipitation of wettest month"), ("BIO14", "Precipitation of driest month"),
        ("BIO15", "Precipitation seasonality (coefficient of variation)"), ("BIO16", "Precipitation of wettest quarter"),
        ("BIO17", "Precipitation of driest quarter"), ("BIO18", "Precipitation of warmest quarter"),
        ("BIO19", "Precipitation of coldest quarter")]
cl("bio_var", "Bioclimatic variable (WorldClim BIO1-BIO19)", [c(k, v) for k, v in _bio],
   source=["WorldClim 2.1 bioclimatic variables (worldclim.org/data/bioclim.html); Fick & Hijmans (2017)."],
   notes=["Units differ by variable (degC, %, mm) - clim.wc_bio needs per-variable units (see review CSV)."])
_ssp = [c(k, v) for k, v in [("historical", "Historical reference (1995-2014)"), ("ssp119", "SSP1-1.9"), ("ssp126", "SSP1-2.6"),
                             ("ssp245", "SSP2-4.5"), ("ssp370", "SSP3-7.0"), ("ssp585", "SSP5-8.5")]]
cl("ssp_scenario", "Climate scenario (CMIP6 Shared Socioeconomic Pathways)", _ssp,
   source=["IPCC AR6 WGI / CMIP6 ScenarioMIP (O'Neill et al. 2016); World Bank Climate Change Knowledge Portal CMIP6 collection."])
_cp = [c(k, v) for k, v in [("1995-2014", "Reference period 1995-2014"), ("2020-2039", "2020-2039 (near term)"),
                            ("2040-2059", "2040-2059 (mid century)"), ("2060-2079", "2060-2079"), ("2080-2099", "2080-2099 (end of century)")]]
cl("climate_period", "Climate projection period (20-year window)", _cp,
   source=["World Bank Climate Change Knowledge Portal CMIP6 projection windows; reference period 1995-2014 per IPCC AR6."])
_gas = [c(k, v) for k, v in [("co2e_100yr", "CO2-equivalent, 100-year GWP"), ("co2e_20yr", "CO2-equivalent, 20-year GWP"),
                             ("co2", "Carbon dioxide (CO2)"), ("ch4", "Methane (CH4)"), ("n2o", "Nitrous oxide (N2O)"),
                             ("fgas", "Fluorinated gases"), ("pm2_5", "Fine particulate matter (PM2.5)"),
                             ("nox", "Nitrogen oxides (NOx)"), ("so2", "Sulphur dioxide (SO2)"), ("co", "Carbon monoxide (CO)")]]
cl("gas", "Greenhouse gas / pollutant", _gas,
   source=["Climate TRACE data release gas fields (co2, ch4, n2o, co2e_100yr, co2e_20yr) and pollutant fields; GWP per IPCC AR6."],
   notes=["co2e is itself the aggregate, so there is no separate total; units differ (t CO2e vs t pollutant)."])
_vt = [T("All organised violence"), c("1", "State-based conflict"), c("2", "Non-state conflict"), c("3", "One-sided violence")]
cl("violence_type", "UCDP type of violence", _vt, source=["UCDP Georeferenced Event Dataset (GED) codebook v24.1, variable type_of_violence (1, 2, 3)."])
_acled = [T("All events")] + [c(k, v) for k, v in [
    ("battles", "Battles"), ("explosions_remote_violence", "Explosions/Remote violence"),
    ("violence_against_civilians", "Violence against civilians"), ("protests", "Protests"), ("riots", "Riots"),
    ("strategic_developments", "Strategic developments")]]
cl("event_type", "Conflict event type (ACLED)", _acled, source=["ACLED Codebook (2023), six event types."])
_cause = [T("All causes"), c("conflict", "Conflict and violence"), c("disaster", "Disasters")]
cl("cause", "Displacement cause (IDMC)", _cause, source=["IDMC Global Internal Displacement Database: displacement associated with conflict and violence vs disasters."])

# ---------------------------------------------------------------- Washington Group disability
_wg = [T("Any domain")] + [c(k, v) for k, v in [("seeing", "Seeing"), ("hearing", "Hearing"), ("walking", "Walking or climbing steps"),
                                                ("cognition", "Remembering or concentrating"), ("self_care", "Self-care (washing, dressing)"),
                                                ("communication", "Communicating")]]
cl("functional_domain", "Functional domain (Washington Group Short Set)", _wg, total_parent=False,
   source=["Washington Group on Disability Statistics, Short Set on Functioning (WG-SS); used in Census of Population and Housing 2024 (DCS)."],
   notes=["A person can have difficulty in several domains: 'Any domain' is not the sum of the domains (no parent links)."])
_wgs = [T("All levels of difficulty"), c("no_difficulty", "No difficulty"), c("some_difficulty", "Some difficulty"),
        c("a_lot_of_difficulty", "A lot of difficulty"), c("cannot_do_at_all", "Cannot do at all")]
cl("difficulty_degree", "Degree of functional difficulty (Washington Group response categories)", _wgs,
   source=["Washington Group Short Set response categories (no difficulty, some difficulty, a lot of difficulty, cannot do at all)."])

# ---------------------------------------------------------------- HS sections
_hs = [("I", "Live animals; animal products"), ("II", "Vegetable products"),
       ("III", "Animal, vegetable or microbial fats and oils"), ("IV", "Prepared foodstuffs; beverages, spirits and vinegar; tobacco"),
       ("V", "Mineral products"), ("VI", "Products of the chemical or allied industries"),
       ("VII", "Plastics and rubber and articles thereof"), ("VIII", "Raw hides and skins, leather, furskins; travel goods, handbags"),
       ("IX", "Wood and articles of wood; cork; manufactures of straw"), ("X", "Pulp of wood; paper and paperboard"),
       ("XI", "Textiles and textile articles"), ("XII", "Footwear, headgear, umbrellas"),
       ("XIII", "Articles of stone, plaster, cement, asbestos; ceramic products; glass"),
       ("XIV", "Natural or cultured pearls, precious stones and metals; imitation jewellery; coin"),
       ("XV", "Base metals and articles of base metal"), ("XVI", "Machinery and mechanical appliances; electrical equipment"),
       ("XVII", "Vehicles, aircraft, vessels and associated transport equipment"),
       ("XVIII", "Optical, photographic, medical instruments; clocks and watches; musical instruments"),
       ("XIX", "Arms and ammunition"), ("XX", "Miscellaneous manufactured articles"), ("XXI", "Works of art, collectors' pieces and antiques")]
cl("hs_section", "Product (Harmonized System sections)", [T("All products")] + [c(k, v) for k, v in _hs],
   source=["WCO Harmonized Commodity Description and Coding System (HS 2022) sections I-XXI; WITS product groups by HS section."])

# ---------------------------------------------------------------- COFOG
_cofog = [T("All functions")] + [c(k, v) for k, v in [
    ("01", "General public services"), ("02", "Defence"), ("03", "Public order and safety"), ("04", "Economic affairs"),
    ("05", "Environmental protection"), ("06", "Housing and community amenities"), ("07", "Health"),
    ("08", "Recreation, culture and religion"), ("09", "Education"), ("10", "Social protection")]]
cl("cofog_division", "Function of government (COFOG divisions)", _cofog,
   source=["UN Classification of the Functions of Government (COFOG, UNSD 1999 / IMF GFSM 2014), divisions 01-10."],
   notes=["Used as the topic scheme for cabinet decisions (cabinet-decisions-geotagged `topic`); one decision may carry several topics, so counts by topic are not additive."])

# ---------------------------------------------------------------- DAC / World Bank sectors
_dac = [T("All sectors"), c("g100", "Social infrastructure and services"), c("g200", "Economic infrastructure and services"),
        c("g300", "Production sectors"), c("g400", "Multi-sector / cross-cutting"), c("g500", "Commodity aid and general programme assistance"),
        c("g600", "Action relating to debt"), c("g700", "Humanitarian aid"), c("g900", "Other / unallocated"),
        c("110", "Education", p="g100"), c("120", "Health", p="g100"), c("130", "Population policies/programmes and reproductive health", p="g100"),
        c("140", "Water supply and sanitation", p="g100"), c("150", "Government and civil society", p="g100"),
        c("160", "Other social infrastructure and services", p="g100"),
        c("210", "Transport and storage", p="g200"), c("220", "Communications", p="g200"), c("230", "Energy", p="g200"),
        c("240", "Banking and financial services", p="g200"), c("250", "Business and other services", p="g200"),
        c("310", "Agriculture, forestry, fishing", p="g300"), c("320", "Industry, mining, construction", p="g300"),
        c("331", "Trade policies and regulations", p="g300"), c("332", "Tourism", p="g300"),
        c("410", "General environment protection", p="g400"), c("430", "Other multisector", p="g400"),
        c("510", "General budget support", p="g500"), c("520", "Development food assistance", p="g500"),
        c("530", "Other commodity assistance", p="g500"), c("600", "Action relating to debt", p="g600"),
        c("720", "Emergency response", p="g700"), c("730", "Reconstruction relief and rehabilitation", p="g700"),
        c("740", "Disaster prevention and preparedness", p="g700"), c("910", "Administrative costs of donors", p="g900"),
        c("930", "Refugees in donor countries", p="g900"), c("998", "Unallocated / unspecified", p="g900")]
cl("dac_sector", "Aid sector (OECD DAC sector codes, 3-digit, with DAC groups)", _dac,
   source=["OECD DAC and CRS code lists, sector (purpose) codes - 3-digit DAC5 sectors; IATI Sector vocabulary 2 (DAC 3-digit)."],
   notes=["IATI activities carry 5-digit CRS purpose codes: roll them up to the 3-digit sector (first three digits).",
          "Group codes g100-g900 are local roll-ups of the DAC sector groups; sector codes are the official 3-digit DAC codes.",
          "ERD foreign-financing sectors are ERD-specific; map them to DAC 3-digit codes in a crosswalk before ingest."])
_wbs = [T("All sectors")] + [c(k, v) for k, v in [
    ("agriculture_fishing_forestry", "Agriculture, Fishing and Forestry"), ("education", "Education"),
    ("energy_extractives", "Energy and Extractives"), ("financial", "Financial Sector"), ("health", "Health"),
    ("industry_trade_services", "Industry, Trade and Services"), ("ict", "Information and Communications Technologies"),
    ("public_administration", "Public Administration"), ("social_protection", "Social Protection"),
    ("transportation", "Transportation"), ("water_sanitation_waste", "Water, Sanitation and Waste Management")]]
cl("wb_sector", "World Bank project sector (major sector)", _wbs,
   source=["World Bank Projects & Operations API, major sector taxonomy (2016 revision; codes as published in the `sector` / `mjsector` fields)."],
   notes=["Local slugs for the eleven major sectors; add the API two-letter major-sector codes as a crosswalk once verified against the live response."],
   status="seed")
