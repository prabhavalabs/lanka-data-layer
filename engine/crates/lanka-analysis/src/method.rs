//! The method contract (11 §7): an `AnalysisMethod` is pure and
//! deterministic — aligned frame + parameters + seed → estimates, outputs
//! and raised guards. No database, no clock, no network.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{Value, json};

use crate::frame::AlignedFrame;
use crate::guards::Raised;

#[derive(Debug, Clone, Copy)]
pub struct RoleSpec {
    pub name: &'static str,
    pub required: bool,
    /// `controls`: a list.
    pub many: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct MinN {
    pub block: usize,
    pub warn: usize,
}

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub id: &'static str,
    pub version: &'static str,
    pub tier: u8,
    pub summary: &'static str,
    pub citations: &'static [&'static str],
    pub roles: &'static [RoleSpec],
    pub supports: &'static [&'static str],
    /// JSON Schema (draft 2020-12) of `params`.
    pub params: &'static str,
    /// JSON Schema of `estimates`.
    pub estimates: &'static str,
    pub min_n: Option<MinN>,
    /// Relationship methods carry `NOT_CAUSAL` and the areal guards.
    pub relationship: bool,
    pub guards: &'static [&'static str],
    pub page: &'static str,
}

impl MethodInfo {
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "version": self.version,
            "tier": self.tier,
            "summary": self.summary,
            "citations": self.citations,
            "roles": self.roles.iter().map(|r| json!({"name": r.name, "required": r.required, "many": r.many})).collect::<Vec<_>>(),
            "supports": self.supports,
            "params": serde_json::from_str::<Value>(self.params).unwrap_or(Value::Null),
            "estimates": serde_json::from_str::<Value>(self.estimates).unwrap_or(Value::Null),
            "min_n": self.min_n.map(|m| json!({"block": m.block, "warn": m.warn})),
            "relationship": self.relationship,
            "guards": self.guards,
            "page": self.page,
        })
    }
}

/// Exposure inputs (`exposure`): per unit of the support, from
/// `lanka_grid::exposure::exposure_overlay`.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ExposureFrame {
    pub hazard: String,
    pub exposed: String,
    pub threshold: f64,
    pub total: Vec<f64>,
    pub exposed_in_mask: Vec<f64>,
    pub area_km2: Vec<f64>,
    pub masked_area_km2: Vec<f64>,
    pub cells: Vec<u64>,
}

/// Everything a method sees.
pub struct Ctx<'a> {
    pub frame: &'a AlignedFrame,
    /// The same variables aligned at other levels (identity alignments),
    /// for the scale-sensitivity table.
    pub scale: &'a [AlignedFrame],
    pub params: &'a Value,
    pub seed: u64,
    /// Plural noun of the support's units ("DS divisions").
    pub noun: &'a str,
    pub noun_of: &'a dyn Fn(i16) -> String,
    pub exposure: Option<&'a ExposureFrame>,
}

/// A frame/chart output before it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub id: &'static str,
    /// frame | chart
    pub kind: &'static str,
    pub columns: Vec<String>,
    pub encoding: Option<&'static str>,
    /// frame: `{codes: [...], columns: {name: [...]}}`; chart: the chart data.
    pub data: Value,
    pub suggested_layer: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MethodOutput {
    pub estimates: Value,
    pub outputs: Vec<Output>,
    pub raised: Vec<Raised>,
    pub n_used: usize,
    pub dropped: BTreeMap<String, u64>,
    /// Interpretation summary (templated).
    pub summary: String,
}

pub trait AnalysisMethod: Send + Sync {
    fn info(&self) -> &'static MethodInfo;
    /// Parameters with every default made explicit (enters the resolved
    /// request, hence the fingerprint).
    fn defaults(&self, params: &Value) -> Value;
    /// Expected wall time (ms) from the support size: drives sync/async.
    fn cost_ms(&self, n: usize, params: &Value) -> u64;
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput>;
}

/// Merges `defaults` under `params` (one level deep; objects merged).
#[must_use]
pub fn with_defaults(params: &Value, defaults: Value) -> Value {
    let mut out = defaults;
    if let (Some(o), Some(p)) = (out.as_object_mut(), params.as_object()) {
        for (k, v) in p {
            match (o.get_mut(k), v) {
                (Some(Value::Object(d)), Value::Object(s)) => {
                    for (kk, vv) in s {
                        d.insert(kk.clone(), vv.clone());
                    }
                }
                _ => {
                    o.insert(k.clone(), v.clone());
                }
            }
        }
    }
    out
}

/// A frame output's data: codes + named columns (NaN → null).
#[must_use]
pub fn frame_data(codes: &[String], columns: &[(&str, Value)]) -> Value {
    let mut cols = serde_json::Map::new();
    for (k, v) in columns {
        cols.insert((*k).to_owned(), v.clone());
    }
    json!({"codes": codes, "columns": cols})
}

/// f64s as JSON with NaN/inf → null.
#[must_use]
pub fn nums(v: &[f64]) -> Value {
    Value::Array(v.iter().map(|x| num(*x)).collect())
}

#[must_use]
pub fn num(x: f64) -> Value {
    if x.is_finite() { json!(x) } else { Value::Null }
}

/// A number for a sentence: `sig` significant digits, unicode minus.
#[must_use]
pub fn fmt_num(x: f64, sig: usize) -> String {
    if !x.is_finite() {
        return "n/a".to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    let mag = x.abs().log10().floor();
    #[allow(clippy::cast_possible_truncation)]
    let decimals = (sig as i32 - 1 - mag as i32).max(0);
    #[allow(clippy::cast_sign_loss)]
    let s = format!("{:.*}", decimals as usize, x);
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    s.replace('-', "\u{2212}")
}

/// A p-value for a sentence.
#[must_use]
pub fn fmt_p(p: f64) -> String {
    if !p.is_finite() {
        "n/a".to_owned()
    } else if p < 0.001 {
        "< 0.001".to_owned()
    } else {
        fmt_num(p, 2)
    }
}

/// `= 0.004` or `< 0.001`, for "p {phrase}".
#[must_use]
pub fn p_phrase(p: f64) -> String {
    let f = fmt_p(p);
    if f.starts_with('<') {
        f
    } else {
        format!("= {f}")
    }
}

#[must_use]
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        assert_eq!(fmt_num(-0.3123, 2), "\u{2212}0.31");
        assert_eq!(fmt_num(1234.6, 3), "1235");
        assert_eq!(fmt_num(0.004_12, 2), "0.0041");
        assert_eq!(fmt_p(2e-35), "< 0.001");
        assert_eq!(thousands(1528), "1,528");
        assert_eq!(thousands(14043), "14,043");
    }

    #[test]
    fn defaults_merge() {
        let d = with_defaults(
            &json!({"robust": false, "bivariate": {"classes": 4}}),
            json!({"robust": true, "bivariate": {"classes": 3, "method": "quantile"}}),
        );
        assert_eq!(
            d,
            json!({"robust": false, "bivariate": {"classes": 4, "method": "quantile"}})
        );
    }
}
