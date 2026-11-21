//! World Bank Indicators API v2 — national series for Sri Lanka (LKA).
//!
//! Registry: source `worldbank-wdi`, dataset `wdi-lka`. Series fetched =
//! every named indicator the dataset declares (`wdi.<code lowercased, . → _>`
//! or its `upstream_id`) plus the [`CORE_SERIES`] list. Series without a
//! named indicator are stored under the dataset's generic `wdi.series`
//! indicator with dims `series_code=<CODE>`.
//!
//! One request per series; each response is archived as its own artifact and
//! every value keeps the JSON path of its record and the number exactly as
//! serialised by the API.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result, bail};
use futures::FutureExt;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::json;
use serde_json::value::RawValue;

use crate::connector::{Candidate, Connector, Ctx, Fetched, ObsRow, Output, dims_key};

pub struct WorldBankWdi;

const SOURCE: &str = "worldbank-wdi";
const DATASET: &str = "wdi-lka";
const GENERIC: &str = "wdi.series";

/// Headline series pulled on every run (beyond the dataset's named indicators).
pub const CORE_SERIES: &[&str] = &[
    "SP.POP.TOTL",
    "SP.POP.GROW",
    "SP.URB.TOTL.IN.ZS",
    "SP.DYN.LE00.IN",
    "SP.DYN.TFRT.IN",
    "SH.DYN.MORT",
    "NY.GDP.MKTP.CD",
    "NY.GDP.MKTP.KD.ZG",
    "NY.GDP.PCAP.CD",
    "NY.GDP.PCAP.PP.CD",
    "FP.CPI.TOTL.ZG",
    "SI.POV.DDAY",
    "SI.POV.NAHC",
    "SI.POV.GINI",
    "SL.UEM.TOTL.ZS",
    "SL.TLF.CACT.ZS",
    "EG.ELC.ACCS.ZS",
    "IT.NET.USER.ZS",
    "SE.ADT.LITR.ZS",
    "SH.XPD.CHEX.GD.ZS",
    "NE.EXP.GNFS.ZS",
    "BX.TRF.PWKR.DT.GD.ZS",
    "DT.DOD.DECT.CD",
    "GC.DOD.TOTL.GD.ZS",
    "EN.GHG.CO2.PC.CE.AR5",
    "AG.LND.FRST.ZS",
];

#[derive(Deserialize)]
struct Page {
    page: u32,
    pages: u32,
    lastupdated: Option<String>,
}

#[derive(Deserialize)]
struct Record<'a> {
    date: String,
    #[serde(borrow)]
    value: Option<&'a RawValue>,
    #[serde(default)]
    obs_status: Option<String>,
    #[serde(default)]
    indicator: Option<Named>,
}

#[derive(Deserialize)]
struct Named {
    value: String,
}

/// Series code → conventional indicator id (`SP.POP.TOTL` → `wdi.sp_pop_totl`).
#[must_use]
pub fn indicator_id(code: &str) -> String {
    format!("wdi.{}", code.to_ascii_lowercase().replace('.', "_"))
}

fn url_for(code: &str) -> String {
    format!("https://api.worldbank.org/v2/country/LKA/indicator/{code}?format=json&per_page=500")
}

/// Where a series' rows go: a named indicator, or the generic one + dims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub indicator_id: String,
    pub dims: String,
}

impl Target {
    #[must_use]
    pub fn for_code(code: &str, named: &HashMap<String, String>) -> Self {
        named.get(code).map_or_else(
            || Self {
                indicator_id: GENERIC.to_owned(),
                dims: dims_key(&[("series_code", code)]),
            },
            |id| Self {
                indicator_id: id.clone(),
                dims: String::new(),
            },
        )
    }
}

/// Parses one API response into rows. `artifact` indexes the fetched slice.
pub fn parse_response(
    bytes: &[u8],
    code: &str,
    target: &Target,
    url: &str,
    artifact: usize,
) -> Result<(Vec<ObsRow>, Option<String>)> {
    // Shape: [ {page meta}, [records…] | null ]   or   [ {"message": [...]} ] on error.
    let doc: Vec<&RawValue> =
        serde_json::from_slice(bytes).context("World Bank response is not a JSON array")?;
    let meta = doc.first().context("empty World Bank response")?;
    let page: Page = serde_json::from_str(meta.get())
        .with_context(|| format!("World Bank error for {code}: {}", truncate(meta.get(), 300)))?;
    if page.pages > 1 || page.page != 1 {
        bail!(
            "{code}: response is paginated ({} pages); raise per_page",
            page.pages
        );
    }
    let Some(records) = doc.get(1) else {
        return Ok((Vec::new(), page.lastupdated));
    };
    let records: Option<Vec<Record<'_>>> = serde_json::from_str(records.get())?;
    let mut rows = Vec::new();
    for (i, r) in records.unwrap_or_default().into_iter().enumerate() {
        let Some(raw) = r.value else { continue };
        let token = raw.get();
        if token == "null" {
            continue;
        }
        let value: f64 = token
            .parse()
            .with_context(|| format!("{code} {}: non-numeric value {token}", r.date))?;
        if r.date.len() != 4 || !r.date.bytes().all(|b| b.is_ascii_digit()) {
            bail!("{code}: unexpected non-annual period {:?}", r.date);
        }
        let status = match r.obs_status.as_deref().unwrap_or("") {
            "E" | "e" => "estimate",
            "F" | "f" => "forecast",
            "P" | "p" => "provisional",
            _ => "reported",
        };
        rows.push(ObsRow {
            indicator_id: target.indicator_id.clone(),
            geo_code: "LK".to_owned(),
            period: r.date,
            dims: target.dims.clone(),
            value: Some(value),
            status,
            artifact,
            locator: json!({
                "url": url, "path": format!("$[1][{i}].value"), "series": code,
                "series_label": r.indicator.map(|n| n.value),
            }),
            verbatim: Some(token.to_owned()),
            method: "api",
            confidence: Some(1.0),
        });
    }
    Ok((rows, page.lastupdated))
}

fn truncate(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// WB code → named indicator id, from the registry dataset.
fn named_indicators(ctx: &Ctx<'_>) -> Result<HashMap<String, String>> {
    let ds = ctx
        .registry
        .dataset(DATASET)
        .with_context(|| format!("registry has no dataset {DATASET}"))?;
    anyhow::ensure!(
        ds.indicators.iter().any(|i| i.id == GENERIC),
        "{DATASET} must declare the generic indicator {GENERIC}"
    );
    let mut named = HashMap::new();
    for ind in ds.indicators.iter().filter(|i| i.id != GENERIC) {
        let code = match &ind.upstream_id {
            Some(c) => c.clone(),
            None => ind
                .id
                .strip_prefix("wdi.")
                .unwrap_or(&ind.id)
                .replace('_', ".")
                .to_ascii_uppercase(),
        };
        named.insert(code, ind.id.clone());
    }
    Ok(named)
}

impl Connector for WorldBankWdi {
    fn source_id(&self) -> &'static str {
        SOURCE
    }

    fn discover<'a>(&'a self, ctx: &'a Ctx<'a>) -> BoxFuture<'a, Result<Vec<Candidate>>> {
        async move {
            let named = named_indicators(ctx)?;
            let mut codes: BTreeMap<String, ()> =
                CORE_SERIES.iter().map(|c| ((*c).to_owned(), ())).collect();
            codes.extend(named.keys().map(|c| (c.clone(), ())));
            Ok(codes
                .into_keys()
                .map(|code| Candidate {
                    url: url_for(&code),
                    key: code,
                    local_path: None,
                    media_type: Some("application/json".to_owned()),
                    edition: None,
                })
                .collect())
        }
        .boxed()
    }

    fn parse<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        fetched: &'a [Fetched],
    ) -> BoxFuture<'a, Result<Vec<Output>>> {
        async move {
            let named = named_indicators(ctx)?;
            let mut rows = Vec::new();
            let mut updated: Option<String> = None;
            for (i, f) in fetched.iter().enumerate() {
                let target = Target::for_code(&f.key, &named);
                let (mut r, lu) = parse_response(&f.bytes, &f.key, &target, &f.url, i)?;
                if r.is_empty() {
                    ctx.run
                        .warn(
                            crate::run::Stage::Parse,
                            &format!("{}: no values for Sri Lanka", f.key),
                            None,
                        )
                        .await;
                }
                rows.append(&mut r);
                updated = updated.max(lu);
            }
            Ok(vec![Output::Observations {
                dataset_id: DATASET.to_owned(),
                vintage: updated.map(|u| format!("WDI lastupdated {u}")),
                rows,
            }])
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{"page":1,"pages":1,"per_page":500,"total":3,"sourceid":"2","lastupdated":"2026-11-11"},
      [{"indicator":{"id":"SP.POP.TOTL","value":"Population, total"},"country":{"id":"LK","value":"Sri Lanka"},"countryiso3code":"LKA","date":"2025","value":21756000,"unit":"","obs_status":"","decimal":0},
       {"indicator":{"id":"SP.POP.TOTL","value":"Population, total"},"country":{"id":"LK","value":"Sri Lanka"},"countryiso3code":"LKA","date":"2024","value":null,"unit":"","obs_status":"","decimal":0},
       {"indicator":{"id":"SP.POP.TOTL","value":"Population, total"},"country":{"id":"LK","value":"Sri Lanka"},"countryiso3code":"LKA","date":"2023","value":2.5e7,"unit":"","obs_status":"E","decimal":0}]]"#;

    #[test]
    fn parses_values_with_provenance() {
        let named: HashMap<String, String> = HashMap::new();
        let t = Target::for_code("SP.POP.TOTL", &named);
        assert_eq!(
            t,
            Target {
                indicator_id: "wdi.series".into(),
                dims: "series_code=SP.POP.TOTL".into()
            }
        );
        let (rows, lu) =
            parse_response(SAMPLE.as_bytes(), "SP.POP.TOTL", &t, "https://x", 3).expect("parses");
        assert_eq!(lu.as_deref(), Some("2026-11-11"));
        assert_eq!(rows.len(), 2, "null values are skipped");
        assert_eq!(rows[0].value, Some(21_756_000.0));
        assert_eq!(rows[0].verbatim.as_deref(), Some("21756000"));
        assert_eq!(rows[0].locator["path"], "$[1][0].value");
        assert_eq!(rows[0].locator["series_label"], "Population, total");
        assert_eq!(rows[1].locator["path"], "$[1][2].value");
        assert_eq!(rows[1].verbatim.as_deref(), Some("2.5e7"));
        assert_eq!(rows[1].status, "estimate");
        assert_eq!(rows[1].artifact, 3);
    }

    #[test]
    fn named_indicators_win() {
        let named: HashMap<String, String> =
            [("NY.GDP.MKTP.CD".to_owned(), "wdi.ny_gdp_mktp_cd".to_owned())].into();
        let t = Target::for_code("NY.GDP.MKTP.CD", &named);
        assert_eq!(
            (t.indicator_id.as_str(), t.dims.as_str()),
            ("wdi.ny_gdp_mktp_cd", "")
        );
    }

    #[test]
    fn surfaces_api_errors() {
        let t = Target::for_code("X.Y", &HashMap::new());
        let err = parse_response(
            br#"[{"message":[{"id":"120","key":"Invalid value"}]}]"#,
            "X.Y",
            &t,
            "u",
            0,
        )
        .expect_err("must fail");
        assert!(format!("{err:#}").contains("Invalid value"));
    }

    #[test]
    fn id_mapping() {
        assert_eq!(
            indicator_id("EN.GHG.CO2.PC.CE.AR5"),
            "wdi.en_ghg_co2_pc_ce_ar5"
        );
    }
}
