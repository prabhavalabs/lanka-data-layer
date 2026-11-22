//! One-off WP-01 migration of `registry/datasets/*.yaml` to the standard
//! model (02-standard-model.md §3): free-text units → UCUM codes (+ power of
//! ten), and `measure_type` / `aggregation` / `class` filled in by rule.
//! Anything the rules cannot decide is written with a conservative value
//! (`aggregation: none`) and listed for human review.
//!
//! Edits are line-based so the diff of 263 files stays reviewable (no YAML
//! re-serialisation); the result is re-validated by `Registry::load`.
//! Idempotent: indicators that already carry `measure_type` are skipped.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use lanka_core::Profile;
use lanka_core::units::{Aggregation, MeasureType, UnitKind};

/// Legacy unit text → (UCUM code, unit_mult, base_period, label, kind).
type UnitMap = (
    &'static str,
    i16,
    Option<&'static str>,
    &'static str,
    UnitKind,
);

#[allow(clippy::too_many_lines)]
fn legacy_unit(text: &str) -> Option<UnitMap> {
    use UnitKind::{Count, Currency, Index, Physical, Ratio, Score};
    Some(match text {
        "count" => ("{count}", 0, None, "count", Count),
        "persons" => ("{person}", 0, None, "persons", Count),
        "percent" | "percent of time" | "percent of population" | "percent of GDP" => {
            ("%", 0, None, "percent", Ratio)
        }
        "percentage points" => ("{percentage_point}", 0, None, "percentage points", Ratio),
        "ratio" | "fraction" | "correlation" => ("1", 0, None, "ratio (unitless)", Ratio),
        "per 1,000" => ("/10*3", 0, None, "per 1,000", Ratio),
        "per 10k" => ("/10*4", 0, None, "per 10,000", Ratio),
        "per 100k" => ("/10*5", 0, None, "per 100,000", Ratio),
        "per 100k live births" => (
            "/10*5{live_birth}",
            0,
            None,
            "per 100,000 live births",
            Ratio,
        ),
        "per person" => ("/{person}", 0, None, "per person", Ratio),
        "per km2" => ("/km2", 0, None, "per km²", Ratio),
        "births per woman" => ("{birth}/{woman}", 0, None, "births per woman", Ratio),
        "LKR" => ("LKR", 0, None, "Sri Lanka rupees", Currency),
        "LKR thousand" => ("LKR", 3, None, "Sri Lanka rupees", Currency),
        "LKR million" => ("LKR", 6, None, "Sri Lanka rupees", Currency),
        "LKR billion" => ("LKR", 9, None, "Sri Lanka rupees", Currency),
        "USD" => ("USD", 0, None, "US dollars", Currency),
        "USD thousand" => ("USD", 3, None, "US dollars", Currency),
        "USD million" => ("USD", 6, None, "US dollars", Currency),
        "intl $" => (
            "{int_dollar}",
            0,
            None,
            "international dollars (PPP)",
            Currency,
        ),
        "LKR per unit" | "LKR/unit" => ("LKR/{unit}", 0, None, "rupees per unit", Currency),
        "LKR/kg" => ("LKR/kg", 0, None, "rupees per kilogram", Currency),
        "LKR/ha" => ("LKR/har", 0, None, "rupees per hectare", Currency),
        "LKR or USD" => (
            "{currency}",
            0,
            None,
            "currency (series-specific)",
            Currency,
        ),
        "index" | "index 0-100" | "index 0-1" | "index (50=neutral)" => {
            ("{index}", 0, None, "index", Index)
        }
        "index (2002=100)" => ("{index}", 0, Some("2002=100"), "index", Index),
        "score 0-5" | "0-100" => ("{score}", 0, None, "score", Score),
        "class" | "class 1-4" => ("{class}", 0, None, "class", Score),
        "category" => ("{category}", 0, None, "category", Score),
        "rank" | "ranks" => ("{rank}", 0, None, "rank", Score),
        "phase 1-5" => ("{phase}", 0, None, "phase", Score),
        "level 0-3" => ("{level}", 0, None, "level", Score),
        "votes" => ("{vote}", 0, None, "votes", Count),
        "seats" => ("{seat}", 0, None, "seats", Count),
        "species" => ("{species}", 0, None, "species", Count),
        "TEU" => ("{TEU}", 0, None, "twenty-foot equivalent units", Count),
        "person-days" => ("{person}.d", 0, None, "person-days", Count),
        "births/year" => ("{birth}/a", 0, None, "births per year", Count),
        "pregnancies/year" => ("{pregnancy}/a", 0, None, "pregnancies per year", Count),
        "trips/day" => ("{trip}/d", 0, None, "trips per day", Count),
        "trains/day" => ("{train}/d", 0, None, "trains per day", Count),
        "interruptions/customer" => (
            "{interruption}/{customer}",
            0,
            None,
            "interruptions per customer",
            Ratio,
        ),
        "minutes/customer" => ("min/{customer}", 0, None, "minutes per customer", Ratio),
        "million pkm" => ("{passenger}.km", 6, None, "passenger-kilometres", Physical),
        "ha" => ("har", 0, None, "hectares", Physical),
        "1000 ha" => ("har", 3, None, "hectares", Physical),
        "km" => ("km", 0, None, "kilometres", Physical),
        "km2" => ("km2", 0, None, "square kilometres", Physical),
        "m" => ("m", 0, None, "metres", Physical),
        "m2" => ("m2", 0, None, "square metres", Physical),
        "m3" => ("m3", 0, None, "cubic metres", Physical),
        "m3/s" => ("m3/s", 0, None, "cubic metres per second", Physical),
        "m3/m3" => ("m3/m3", 0, None, "volumetric fraction", Ratio),
        "m2/person" => ("m2/{person}", 0, None, "square metres per person", Ratio),
        "km/km2" => ("km/km2", 0, None, "kilometres per km²", Ratio),
        "mm" => ("mm", 0, None, "millimetres", Physical),
        "tonnes" => ("t", 0, None, "tonnes", Physical),
        "t CO2e" => ("t{CO2e}", 0, None, "tonnes CO₂-equivalent", Physical),
        "kg" => ("kg", 0, None, "kilograms", Physical),
        "kg/ha" => ("kg/har", 0, None, "kilograms per hectare", Ratio),
        "Mg/ha" => ("Mg/har", 0, None, "tonnes per hectare", Ratio),
        "g/kg" => ("g/kg", 0, None, "grams per kilogram", Ratio),
        "ug/m3" => ("ug/m3", 0, None, "micrograms per cubic metre", Ratio),
        "ktoe" => ("{ktoe}", 0, None, "kilotonnes of oil equivalent", Physical),
        "MW" => ("MW", 0, None, "megawatts", Physical),
        "MWh" => ("MW.h", 0, None, "megawatt-hours", Physical),
        "GWh" => ("GW.h", 0, None, "gigawatt-hours", Physical),
        "kWh/m2/day" => ("kW.h/m2/d", 0, None, "kWh per m² per day", Ratio),
        "kWh/kWp/day" => ("kW.h/{kWp}/d", 0, None, "kWh per kWp per day", Ratio),
        "W/m2" => ("W/m2", 0, None, "watts per square metre", Ratio),
        "nW/cm2/sr" => ("nW/cm2/sr", 0, None, "radiance (nW/cm²/sr)", Physical),
        "nW/cm2/sr x km2" => ("nW/cm2/sr.km2", 0, None, "radiance × area", Physical),
        "radiance per person" => ("nW/cm2/sr/{person}", 0, None, "radiance per person", Ratio),
        "Mbps" => ("Mbit/s", 0, None, "megabits per second", Physical),
        "kcal/day" => ("kcal/d", 0, None, "kilocalories per day", Physical),
        "deg C" | "degC" => ("Cel", 0, None, "degrees Celsius", Physical),
        "degrees" => ("deg", 0, None, "degrees", Physical),
        "knots" => ("[kn_i]", 0, None, "knots", Physical),
        "pH" => ("[pH]", 0, None, "pH", Physical),
        "dB(A)" => ("dB{A}", 0, None, "decibels (A-weighted)", Physical),
        "minutes" => ("min", 0, None, "minutes", Physical),
        "days" => ("d", 0, None, "days", Physical),
        "months" => ("mo", 0, None, "months", Physical),
        "years" => ("a", 0, None, "years", Physical),
        "m or degrees" => (
            "{m_or_deg}",
            0,
            None,
            "metres or degrees (series-specific)",
            Physical,
        ),
        "various" | "varies" => ("{various}", 0, None, "series-specific unit", Score),
        _ => return None,
    })
}

/// Extensive physical quantities: children add up to the parent.
const EXTENSIVE: &[&str] = &[
    "har",
    "km2",
    "km",
    "m2",
    "m3",
    "t",
    "t{CO2e}",
    "kg",
    "{ktoe}",
    "MW",
    "MW.h",
    "GW.h",
    "{passenger}.km",
    "nW/cm2/sr.km2",
];

/// What the rules decided for one indicator, and whether a human must look.
struct Inferred {
    measure: MeasureType,
    aggregation: Aggregation,
    review: Option<&'static str>,
}

fn infer_measure(code: &str, kind: UnitKind, unit_text: &str) -> Inferred {
    use Aggregation as A;
    use MeasureType as M;
    let r = |measure, aggregation, review| Inferred {
        measure,
        aggregation,
        review,
    };
    match kind {
        UnitKind::Count if code.contains('/') => r(
            M::Rate,
            A::Sum,
            Some("rate of a count over time: check that children add up"),
        ),
        UnitKind::Count => r(M::Count, A::Sum, None),
        UnitKind::Currency if code.contains('/') || code == "{currency}" => r(
            M::Mean,
            A::None,
            Some("price/unit value: weighted_mean needs a weight indicator"),
        ),
        UnitKind::Currency => r(M::Sum, A::Sum, None),
        UnitKind::Physical if EXTENSIVE.contains(&code) => r(M::Sum, A::Sum, None),
        UnitKind::Physical => r(
            M::Mean,
            A::None,
            Some("intensive quantity: weighted_mean needs a weight indicator"),
        ),
        UnitKind::Ratio if code == "%" => r(
            M::Share,
            A::None,
            Some("share: ratio_of_sums needs numerator/denominator indicators"),
        ),
        UnitKind::Ratio if unit_text == "ratio" || code == "1" => r(
            M::Ratio,
            A::None,
            Some("ratio: ratio_of_sums needs numerator/denominator indicators"),
        ),
        UnitKind::Ratio => r(
            M::Rate,
            A::None,
            Some("rate: ratio_of_sums needs numerator/denominator indicators"),
        ),
        UnitKind::Index => r(M::Index, A::None, None),
        UnitKind::Score => match code {
            "{rank}" => r(M::Rank, A::None, None),
            "{category}" | "{class}" | "{phase}" | "{level}" => r(M::Category, A::None, None),
            "{various}" => r(
                M::Score,
                A::None,
                Some("unit varies by series: split or declare per series"),
            ),
            _ => r(M::Score, A::None, None),
        },
    }
}

/// Publisher → is it a public body of the country (the official-class rule)?
fn official_publisher(publisher: &str, country: &str) -> bool {
    let p = publisher;
    if p.contains("community-mapped") {
        return false;
    }
    let mirrored = p.contains("data from");
    let international = [
        "World Bank",
        "WorldPop",
        "United Nations",
        "UN ",
        "UNDP",
        "UNICEF",
        "UNESCO",
        "UNEP",
        "UNDRR",
        "UNITAR",
        "OCHA",
        "WHO",
        "World Health",
        "IMF",
        "International",
        "FAO",
        "NASA",
        "NOAA",
        "Copernicus",
        "European",
        "OpenStreetMap",
        "Meta",
        "Google",
        "Microsoft",
        "Oxford",
        "University",
        "Institute",
        "Foundation",
        "Wikimedia",
        "Ookla",
        "Kontur",
        "OpenAQ",
        "Open-Meteo",
        "Transparency",
        "Uppsala",
        "ICF",
        "IATI",
        "Malaria Atlas",
        "Overture",
        "OurAirports",
        "MobilityData",
        "Unwired",
        "Japan",
        "ISRIC",
        "WWF",
        "World Resources",
        "World Food",
        "Oak Ridge",
        "US Geological",
        "geoBoundaries",
        "Lanka Data Layer",
    ];
    let public_body = [
        "Department",
        "Ministry",
        "Central Bank",
        "Commission",
        "Authority",
        "Board",
        "Bureau",
        "Parliament",
        "Police",
        "Railways",
        "Ports",
        "National ",
        "Survey Department",
        "Council",
        "Cabinet",
        "Registrar",
        "Auditor",
        "Secretariat",
        country,
        // acronyms of national bodies seen in mirror attributions
        "DCS",
        "SLTDA",
        "DMC",
        "NDCU",
    ];
    let names_public = public_body.iter().any(|k| p.contains(k));
    let names_intl = international.iter().any(|k| p.contains(k));
    // "UN OCHA (data from Survey Department …)" and nuuuwan mirrors of
    // government tables carry official figures.
    (mirrored && names_public) || (names_public && !names_intl)
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub dataset: String,
    pub indicator: String,
    pub field: &'static str,
    pub value: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct MigrateReport {
    pub files_changed: usize,
    pub indicators: usize,
    pub review: Vec<ReviewItem>,
    /// UCUM code → (label, kind) of every unit used.
    pub units: BTreeMap<String, (String, UnitKind)>,
}

impl MigrateReport {
    #[must_use]
    pub fn review_csv(&self) -> String {
        let mut s = String::from("dataset,indicator,field,value,reason\n");
        let q = |x: &str| format!("\"{}\"", x.replace('"', "\"\""));
        for r in &self.review {
            let _ = writeln!(
                s,
                "{},{},{},{},{}",
                r.dataset,
                r.indicator,
                r.field,
                q(&r.value),
                q(&r.reason)
            );
        }
        s
    }
}

/// Plain scalar when YAML allows it, else single-quoted.
fn yaml_scalar(s: &str) -> String {
    let plain = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-'))
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && !matches!(
            s,
            "y" | "n" | "yes" | "no" | "on" | "off" | "true" | "false" | "null"
        );
    if plain { s.to_owned() } else { yaml_quote(s) }
}

fn yaml_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Migrates every dataset file under `registry/datasets/`. With `write`,
/// files are rewritten and `registry/units.yaml` is created if missing.
#[allow(clippy::too_many_lines)]
pub fn migrate(registry: &Path, profile: &Profile, write: bool) -> Result<MigrateReport> {
    let mut report = MigrateReport::default();
    // Source publishers, by id.
    let mut publishers: HashMap<String, String> = HashMap::new();
    for e in std::fs::read_dir(registry.join("sources"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "yaml") {
            let v: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&std::fs::read(&p)?)
                .with_context(|| p.display().to_string())?;
            if let (Some(id), Some(publ)) = (v["id"].as_str(), v["publisher"].as_str()) {
                publishers.insert(id.to_owned(), publ.to_owned());
            }
        }
    }
    let mut files: Vec<_> = std::fs::read_dir(registry.join("datasets"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    let country = profile.spec.name.en.as_str();
    for path in files {
        let text = std::fs::read_to_string(&path)?;
        let doc: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&text).with_context(|| path.display().to_string())?;
        let ds_id = doc["id"].as_str().unwrap_or_default().to_owned();
        let sources: Vec<String> = doc["source_ids"]
            .as_sequence()
            .map(|s| {
                s.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let inds = doc["indicators"].as_sequence().cloned().unwrap_or_default();
        let classes: BTreeSet<bool> = sources
            .iter()
            .filter_map(|s| publishers.get(s))
            .map(|p| official_publisher(p, country))
            .collect();
        let official = classes.contains(&true);
        let mixed = classes.len() > 1;

        let kind = if ds_id == profile.boundary_dataset() {
            "boundaries"
        } else {
            let levels: BTreeSet<String> = inds
                .iter()
                .flat_map(|i| {
                    i["geo_levels"]
                        .as_sequence()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|l| l.as_str().map(str::to_owned))
                })
                .collect();
            let geomish = ["point", "line", "polygon"];
            if !levels.is_empty() && levels.iter().all(|l| geomish.contains(&l.as_str())) {
                "features"
            } else if levels.contains("raster")
                && levels
                    .iter()
                    .all(|l| l == "raster" || geomish.contains(&l.as_str()))
            {
                "grid"
            } else {
                "indicators"
            }
        };
        let push = |r: &mut MigrateReport, ind: &str, field, value: String, reason: String| {
            r.review.push(ReviewItem {
                dataset: ds_id.clone(),
                indicator: ind.to_owned(),
                field,
                value,
                reason,
            });
        };

        let mut out: Vec<String> = Vec::new();
        let mut idx = 0usize;
        let mut changed = false;
        let has_kind = doc.get("kind").is_some();
        for line in text.lines() {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            if !has_kind && kind != "indicators" && line.starts_with("publication_class:") {
                out.push(format!("kind: {kind}"));
                push(
                    &mut report,
                    "",
                    "kind",
                    kind.to_owned(),
                    "inferred from geo_levels / profile boundary dataset".into(),
                );
                changed = true;
            }
            let Some(unit_text) = trimmed.strip_prefix("unit: ") else {
                out.push(line.to_owned());
                continue;
            };
            let Some(ind) = inds.get(idx) else {
                out.push(line.to_owned());
                continue;
            };
            idx += 1;
            if ind.get("measure_type").is_some() {
                out.push(line.to_owned());
                continue;
            }
            let ind_id = ind["id"].as_str().unwrap_or_default().to_owned();
            let unit_text = ind["unit"]
                .as_str()
                .map_or_else(|| unit_text.trim().to_owned(), str::to_owned);
            report.indicators += 1;
            changed = true;
            let (code, mult, base, label, ukind) = legacy_unit(&unit_text).unwrap_or((
                "{various}",
                0,
                None,
                "series-specific unit",
                UnitKind::Score,
            ));
            if legacy_unit(&unit_text).is_none() {
                push(
                    &mut report,
                    &ind_id,
                    "unit",
                    unit_text.clone(),
                    "no UCUM mapping; set to {various}".into(),
                );
            } else if code.starts_with('{')
                && matches!(code, "{various}" | "{currency}" | "{m_or_deg}")
            {
                push(
                    &mut report,
                    &ind_id,
                    "unit",
                    code.to_owned(),
                    format!("ambiguous source unit {unit_text:?}"),
                );
            }
            report
                .units
                .entry(code.to_owned())
                .or_insert_with(|| (label.to_owned(), ukind));
            let mut inf = infer_measure(code, ukind, &unit_text);
            let label = ind["label"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(inf.aggregation, Aggregation::Sum) {
                let intensive = [
                    "per capita",
                    "per person",
                    "average",
                    "mean ",
                    "median",
                    "per household",
                ];
                if let Some(k) = intensive.iter().find(|k| label.contains(*k)) {
                    inf = Inferred {
                        measure: if *k == "median" {
                            MeasureType::Median
                        } else {
                            MeasureType::Mean
                        },
                        aggregation: Aggregation::None,
                        review: Some(
                            "label says per-capita/average: not additive; weighted_mean needs a weight",
                        ),
                    };
                }
            }
            let class = if ds_id.starts_with("derived-") {
                if ind_id.starts_with("est.") {
                    "estimated"
                } else {
                    "derived"
                }
            } else if official {
                "official"
            } else {
                "third_party"
            };
            if mixed {
                push(
                    &mut report,
                    &ind_id,
                    "class",
                    class.to_owned(),
                    "sources mix public bodies and third parties".into(),
                );
            }
            if let Some(why) = inf.review {
                push(
                    &mut report,
                    &ind_id,
                    "aggregation",
                    inf.aggregation.as_str().to_owned(),
                    why.to_owned(),
                );
            }
            out.push(format!("{indent}unit: {}", yaml_scalar(code)));
            if mult != 0 {
                out.push(format!("{indent}unit_mult: {mult}"));
            }
            if let Some(b) = base {
                out.push(format!("{indent}base_period: {}", yaml_quote(b)));
            }
            out.push(format!("{indent}measure_type: {}", inf.measure));
            out.push(format!("{indent}aggregation: {}", inf.aggregation));
            out.push(format!("{indent}class: {class}"));
        }
        if changed {
            report.files_changed += 1;
            if write {
                let mut s = out.join("\n");
                if text.ends_with('\n') {
                    s.push('\n');
                }
                std::fs::write(&path, s)?;
            }
        }
    }
    let units_path = registry.join("units.yaml");
    if write && !units_path.exists() {
        let mut s = String::from(
            "# Units of measure (UCUM case-sensitive codes; ISO 4217 for money).\n\
             # Validated by schema/units.schema.json, synced to catalog.units_of_measure.\n\
             # `{annotation}` codes are UCUM unity annotations for countables.\nunits:\n",
        );
        for (code, (label, kind)) in &report.units {
            let _ = writeln!(
                s,
                "  - {{ code: {}, label: {}, kind: {} }}",
                yaml_quote(code),
                yaml_quote(label),
                kind.as_str()
            );
        }
        std::fs::write(&units_path, s)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_map_to_ucum() {
        assert_eq!(legacy_unit("persons").map(|u| u.0), Some("{person}"));
        assert_eq!(legacy_unit("percent").map(|u| u.0), Some("%"));
        assert_eq!(legacy_unit("ratio").map(|u| u.0), Some("1"));
        assert_eq!(
            legacy_unit("LKR million").map(|u| (u.0, u.1)),
            Some(("LKR", 6))
        );
        for u in [
            "count",
            "percent",
            "persons",
            "ha",
            "USD million",
            "deg C",
            "per 1,000",
            "km2",
        ] {
            let (code, ..) = legacy_unit(u).expect("mapped");
            assert!(lanka_core::units::check_unit_code(code).is_ok(), "{code}");
        }
    }

    #[test]
    fn measure_rules() {
        let m = |t: &str| {
            let (c, _, _, _, k) = legacy_unit(t).expect("mapped");
            let i = infer_measure(c, k, t);
            (i.measure, i.aggregation, i.review.is_some())
        };
        assert_eq!(m("persons"), (MeasureType::Count, Aggregation::Sum, false));
        assert_eq!(
            m("LKR million"),
            (MeasureType::Sum, Aggregation::Sum, false)
        );
        assert_eq!(m("percent"), (MeasureType::Share, Aggregation::None, true));
        assert!(m("mm").2);
        assert_eq!(m("index"), (MeasureType::Index, Aggregation::None, false));
    }

    #[test]
    fn publisher_rules() {
        let c = "Sri Lanka";
        assert!(official_publisher(
            "Department of Census and Statistics, Sri Lanka",
            c
        ));
        assert!(official_publisher("Central Bank of Sri Lanka", c));
        assert!(official_publisher(
            "Nuwan Senaratna (nuuuwan) - data from DCS / Survey Department",
            c
        ));
        assert!(!official_publisher("World Bank", c));
        assert!(!official_publisher(
            "Nuwan Senaratna (nuuuwan) - community-mapped",
            c
        ));
        assert!(!official_publisher(
            "WorldPop, University of Southampton",
            c
        ));
    }
}
