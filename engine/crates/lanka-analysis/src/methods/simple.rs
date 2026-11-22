//! The descriptive tier-1 methods: `describe`, `classify`, `zonal`,
//! `exposure` (no relationship, so no `NOT_CAUSAL`).

use anyhow::{Result, bail};
use lanka_stats::classify::{self, Method as ClassMethod};
use lanka_stats::describe::{histogram, summary};
use serde_json::{Value, json};

use crate::method::{
    AnalysisMethod, Ctx, MethodInfo, MethodOutput, MinN, Output, RoleSpec, fmt_num, frame_data,
    num, nums, thousands, with_defaults,
};

// ───────────────────────────── describe ────────────────────────────────────

pub struct Describe;

pub static DESCRIBE: MethodInfo = MethodInfo {
    id: "describe",
    version: "1.0.0",
    revision: 1,
    tier: 1,
    summary: "Univariate summary of one variable on a support: n, missing, mean, sd, quantiles, MAD, skewness, kurtosis, histogram",
    citations: &[
        "Hyndman & Fan 1996, Am. Stat. 50 (quantile type 7)",
        "Joanes & Gill 1998, The Statistician 47 (G1, G2)",
    ],
    roles: &[RoleSpec {
        name: "x",
        required: true,
        many: false,
    }],
    supports: &["admin"],
    params: r#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object", "additionalProperties": false,
      "properties": {"bins": {"type": "integer", "minimum": 0, "maximum": 200}}}"#,
    estimates: r#"{"type": "object", "required": ["summary", "histogram"]}"#,
    min_n: Some(MinN { block: 1, warn: 30 }),
    relationship: false,
    guards: &[
        "MIN_N",
        "SMALL_N",
        "ESTIMATED_INPUT",
        "LOW_COVERAGE",
        "SUPPRESSED_CELLS",
        "SYNTHETIC_INPUT",
        "PERIOD_MISMATCH",
    ],
    page: "docs/methods/analysis-describe.md",
};

impl AnalysisMethod for Describe {
    fn info(&self) -> &'static MethodInfo {
        &DESCRIBE
    }
    fn defaults(&self, params: &Value) -> Value {
        with_defaults(params, json!({"bins": 0}))
    }
    fn cost_ms(&self, n: usize, _: &Value) -> u64 {
        1 + n as u64 / 50_000
    }
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let f = ctx.frame;
        let Some(x) = f.col("x") else {
            bail!("describe needs x")
        };
        let rows = f.complete(&["x"]);
        let t = x.transformed();
        let v: Vec<f64> = rows.iter().map(|&i| t[i]).collect();
        let s = summary(&v);
        let bins = usize::try_from(ctx.params["bins"].as_u64().unwrap_or(0)).unwrap_or(0);
        let h = histogram(&v, bins);
        let codes: Vec<String> = rows.iter().map(|&i| f.support.codes[i].clone()).collect();
        Ok(MethodOutput {
            estimates: json!({
                "summary": {"n": s.n, "mean": num(s.mean), "sd": num(s.sd), "min": num(s.min), "max": num(s.max),
                            "q": {"p0": num(s.q[0]), "p5": num(s.q[1]), "p25": num(s.q[2]), "p50": num(s.q[3]),
                                  "p75": num(s.q[4]), "p95": num(s.q[5]), "p100": num(s.q[6])},
                            "mad": num(s.mad), "skewness": num(s.skewness), "kurtosis": num(s.kurtosis),
                            "non_positive": s.non_positive},
                "histogram": {"edges": nums(&h.edges), "counts": h.counts},
                "transform": x.transform.as_str(),
            }),
            outputs: vec![
                Output {
                    id: "values",
                    kind: "frame",
                    columns: vec!["value".into()],
                    encoding: None,
                    data: frame_data(&codes, &[("value", nums(&v))]),
                    suggested_layer: Some(json!({"type": "choropleth", "name": x.input.label,
                        "style": {"fill": {"field": "value", "scale": {"type": "quantile", "classes": 5, "ramp": "ldl.seq.teal"}}}})),
                },
                Output {
                    id: "histogram",
                    kind: "chart",
                    columns: Vec::new(),
                    encoding: Some("histogram"),
                    data: json!({"encoding": "histogram", "edges": nums(&h.edges), "counts": h.counts}),
                    suggested_layer: None,
                },
            ],
            raised: Vec::new(),
            n_used: rows.len(),
            dropped: f.drop_counts(&["x"]),
            summary: format!(
                "{} across {} {}: median {}, middle half {} to {}, range {} to {}.",
                x.input.label,
                thousands(s.n),
                ctx.noun,
                fmt_num(s.q[3], 3),
                fmt_num(s.q[2], 3),
                fmt_num(s.q[4], 3),
                fmt_num(s.min, 3),
                fmt_num(s.max, 3)
            ),
        })
    }
}

// ───────────────────────────── classify ────────────────────────────────────

pub struct Classify;

pub static CLASSIFY: MethodInfo = MethodInfo {
    id: "classify",
    version: "1.0.0",
    revision: 1,
    tier: 1,
    summary: "Class breaks for one column (quantile, equal, jenks, stddev, headtail, pretty) or bivariate classes for two",
    citations: &[
        "Jenks 1967, Int. Yearbook of Cartography 7",
        "Jiang 2013, Prof. Geogr. 65 (head/tail)",
        "R Core, pretty()",
    ],
    roles: &[
        RoleSpec {
            name: "x",
            required: true,
            many: false,
        },
        RoleSpec {
            name: "y",
            required: false,
            many: false,
        },
    ],
    supports: &["admin"],
    params: r#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object", "additionalProperties": false,
      "properties": {"method": {"enum": ["quantile", "equal", "jenks", "stddev", "headtail", "pretty"]},
                     "classes": {"type": "integer", "minimum": 2, "maximum": 12}}}"#,
    estimates: r#"{"type": "object", "required": ["breaks"]}"#,
    min_n: Some(MinN { block: 2, warn: 2 }),
    relationship: false,
    guards: &["MIN_N", "LOW_COVERAGE", "SYNTHETIC_INPUT"],
    page: "docs/methods/analysis-classify.md",
};

impl AnalysisMethod for Classify {
    fn info(&self) -> &'static MethodInfo {
        &CLASSIFY
    }
    fn defaults(&self, params: &Value) -> Value {
        with_defaults(params, json!({"method": "quantile", "classes": 5}))
    }
    fn cost_ms(&self, n: usize, _: &Value) -> u64 {
        1 + n as u64 / 20_000
    }
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let f = ctx.frame;
        let Some(x) = f.col("x") else {
            bail!("classify needs x")
        };
        let m = ClassMethod::parse(ctx.params["method"].as_str().unwrap_or("quantile"))
            .unwrap_or(ClassMethod::Quantile);
        let k = usize::try_from(ctx.params["classes"].as_u64().unwrap_or(5)).unwrap_or(5);
        let roles: Vec<&str> = if f.col("y").is_some() {
            vec!["x", "y"]
        } else {
            vec!["x"]
        };
        let rows = f.complete(&roles);
        let codes: Vec<String> = rows.iter().map(|&i| f.support.codes[i].clone()).collect();
        let xt = x.transformed();
        let xs: Vec<f64> = rows.iter().map(|&i| xt[i]).collect();
        let (estimates, class) = if let Some(y) = f.col("y") {
            let yt = y.transformed();
            let ys: Vec<f64> = rows.iter().map(|&i| yt[i]).collect();
            let b = classify::bivariate(&xs, &ys, m, k);
            (
                json!({"method": m.as_str(), "classes": k, "x_breaks": nums(&b.x_breaks), "y_breaks": nums(&b.y_breaks),
                    "class_code": "class_y * classes + class_x"}),
                b.class,
            )
        } else {
            let b = classify::breaks(&xs, m, k);
            let c = classify::classes(&xs, &b);
            (
                json!({"method": m.as_str(), "classes": k, "breaks": nums(&b)}),
                c,
            )
        };
        let mut est = estimates;
        let bivariate = est.get("breaks").is_none();
        if bivariate {
            est["breaks"] = est["x_breaks"].clone();
        }
        let style = if bivariate {
            json!({"fill": {"field": "class", "scale": {"type": "categorical", "ramp": "ldl.cat.default"}}})
        } else {
            json!({"fill": {"field": "value", "scale": {"type": m.as_str(), "classes": k,
                "breaks": est["breaks"], "ramp": "ldl.seq.teal"}}})
        };
        Ok(MethodOutput {
            estimates: est,
            outputs: vec![Output {
                id: "classes",
                kind: "frame",
                columns: vec!["value".into(), "class".into()],
                encoding: None,
                data: frame_data(&codes, &[("value", nums(&xs)), ("class", json!(class))]),
                suggested_layer: Some(
                    json!({"type": "choropleth", "name": x.input.label, "style": style}),
                ),
            }],
            raised: Vec::new(),
            n_used: rows.len(),
            dropped: f.drop_counts(&roles),
            summary: format!(
                "{} classes by {} over {} {}.",
                k,
                m.as_str(),
                thousands(rows.len()),
                ctx.noun
            ),
        })
    }
}

// ───────────────────────────── zonal ───────────────────────────────────────

pub struct Zonal;

pub static ZONAL: MethodInfo = MethodInfo {
    id: "zonal",
    version: "1.0.0",
    revision: 1,
    tier: 1,
    summary: "A grid or feature layer summarised to units (the alignment step exposed as a result)",
    citations: &["05 §8 grid_zonal; 04 §3 features_zonal"],
    roles: &[RoleSpec {
        name: "x",
        required: true,
        many: false,
    }],
    supports: &["admin"],
    params: r#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object", "additionalProperties": false, "properties": {}}"#,
    estimates: r#"{"type": "object", "required": ["n", "total"]}"#,
    min_n: None,
    relationship: false,
    guards: &["ESTIMATED_INPUT", "LOW_COVERAGE", "SYNTHETIC_INPUT"],
    page: "docs/methods/analysis-zonal.md",
};

impl AnalysisMethod for Zonal {
    fn info(&self) -> &'static MethodInfo {
        &ZONAL
    }
    fn defaults(&self, params: &Value) -> Value {
        with_defaults(params, json!({}))
    }
    fn cost_ms(&self, _: usize, _: &Value) -> u64 {
        60
    }
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let f = ctx.frame;
        let Some(x) = f.col("x") else {
            bail!("zonal needs x")
        };
        let rows = f.complete(&["x"]);
        let v: Vec<f64> = f
            .support
            .codes
            .iter()
            .enumerate()
            .map(|(i, _)| x.values[i])
            .collect();
        let total: f64 = rows.iter().map(|&i| x.values[i]).sum();
        Ok(MethodOutput {
            estimates: json!({"n": rows.len(), "total": if x.input.extensive() { num(total) } else { Value::Null },
                              "method": x.alignment.method, "params": x.alignment.params}),
            outputs: vec![Output {
                id: "zonal",
                kind: "frame",
                columns: vec!["value".into()],
                encoding: None,
                data: frame_data(&f.support.codes, &[("value", nums(&v))]),
                suggested_layer: Some(json!({"type": "choropleth", "name": x.input.label,
                    "style": {"fill": {"field": "value", "scale": {"type": "quantile", "classes": 5, "ramp": "ldl.seq.teal"}}}})),
            }],
            raised: Vec::new(),
            n_used: rows.len(),
            dropped: f.drop_counts(&["x"]),
            summary: format!(
                "{} summarised to {} {} by {}.",
                x.input.label,
                thousands(rows.len()),
                ctx.noun,
                x.alignment.method
            ),
        })
    }
}

// ───────────────────────────── exposure ────────────────────────────────────

pub struct Exposure;

pub static EXPOSURE: MethodInfo = MethodInfo {
    id: "exposure",
    version: "1.0.0",
    revision: 1,
    tier: 1,
    summary: "Hazard mask × exposed quantity per unit (exposure_overlay): exposed counts and shares where the hazard grid is at or above a threshold",
    citations: &["04 §3 exposure_overlay; 11 §3.2 rule 4 (overlays on the canonical grid)"],
    roles: &[
        RoleSpec {
            name: "x",
            required: true,
            many: false,
        },
        RoleSpec {
            name: "weight",
            required: true,
            many: false,
        },
    ],
    supports: &["admin"],
    params: r#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object", "additionalProperties": false,
      "required": ["threshold"],
      "properties": {"threshold": {"type": "number"}, "return_period": {"type": "string"}}}"#,
    estimates: r#"{"type": "object", "required": ["total", "exposed", "share"]}"#,
    min_n: None,
    relationship: false,
    guards: &["ESTIMATED_INPUT", "SYNTHETIC_INPUT"],
    page: "docs/methods/analysis-exposure.md",
};

impl AnalysisMethod for Exposure {
    fn info(&self) -> &'static MethodInfo {
        &EXPOSURE
    }
    fn defaults(&self, params: &Value) -> Value {
        with_defaults(params, json!({}))
    }
    fn cost_ms(&self, _: usize, _: &Value) -> u64 {
        80
    }
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let Some(e) = ctx.exposure else {
            bail!("exposure needs a hazard grid (x) and an exposed grid (weight)")
        };
        let codes = &ctx.frame.support.codes;
        let share: Vec<f64> = e
            .total
            .iter()
            .zip(&e.exposed_in_mask)
            .map(|(t, x)| if *t > 0.0 { x / t } else { f64::NAN })
            .collect();
        let area_share: Vec<f64> = e
            .area_km2
            .iter()
            .zip(&e.masked_area_km2)
            .map(|(a, m)| if *a > 0.0 { m / a } else { f64::NAN })
            .collect();
        let total: f64 = e.total.iter().sum();
        let exposed: f64 = e.exposed_in_mask.iter().sum();
        let used = e.cells.iter().filter(|c| **c > 0).count();
        let mut top: Vec<(f64, &String)> = e.exposed_in_mask.iter().copied().zip(codes).collect();
        top.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
        let top: Vec<Value> = top
            .iter()
            .take(10)
            .map(|(v, c)| json!({"code": c, "exposed": num(*v)}))
            .collect();
        let pct = if total > 0.0 {
            exposed / total * 100.0
        } else {
            f64::NAN
        };
        Ok(MethodOutput {
            estimates: json!({
                "hazard": e.hazard, "exposed_layer": e.exposed, "threshold": e.threshold,
                "total": num(total), "exposed": num(exposed), "share": num(exposed / total),
                "top_units": top,
            }),
            outputs: vec![Output {
                id: "exposure",
                kind: "frame",
                columns: vec![
                    "exposed".into(),
                    "total".into(),
                    "share".into(),
                    "area_share".into(),
                ],
                encoding: None,
                data: frame_data(
                    codes,
                    &[
                        ("exposed", nums(&e.exposed_in_mask)),
                        ("total", nums(&e.total)),
                        ("share", nums(&share)),
                        ("area_share", nums(&area_share)),
                    ],
                ),
                suggested_layer: Some(
                    json!({"type": "choropleth", "name": format!("Exposed share ({} ≥ {})", e.hazard, e.threshold),
                    "style": {"fill": {"field": "share", "scale": {"type": "quantile", "classes": 5, "ramp": "ldl.seq.teal"}}}}),
                ),
            }],
            raised: Vec::new(),
            n_used: used,
            dropped: std::collections::BTreeMap::new(),
            summary: format!(
                "{} of {} ({} %) are in cells where {} is at least {}, across {} {}.",
                fmt_num(exposed, 3),
                fmt_num(total, 3),
                fmt_num(pct, 2),
                e.hazard,
                fmt_num(e.threshold, 3),
                thousands(used),
                ctx.noun
            ),
        })
    }
}
