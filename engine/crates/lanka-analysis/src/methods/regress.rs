//! `regress@1` — multiple OLS / Huber regression with controls and optional
//! fixed effects by parent unit (11 §4): HC3 intervals, VIF, Cook's
//! distance, Breusch–Pagan, residual Moran's I.

use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::fit::{coef_json, diagnostics_json, fit, knn_k, moran_json, reading, robust_json, term};
use crate::frame::{Column, InputRef};
use crate::guards::Raised;
use crate::method::{
    AnalysisMethod, Ctx, MethodInfo, MethodOutput, MinN, Output, RoleSpec, fmt_num, frame_data,
    num, nums, thousands, with_defaults,
};

pub struct Regress;

pub static INFO: MethodInfo = MethodInfo {
    id: "regress",
    version: "1.0.0",
    tier: 1,
    summary: "Multiple regression of y on x and controls (OLS with HC3 intervals, Huber robust fit), optional fixed effects by parent unit, with diagnostics",
    citations: &[
        "MacKinnon & White 1985, J. Econometrics 29 (HC3)",
        "Huber 1964, Ann. Math. Statist. 35",
        "Koenker 1981, J. Econometrics 17 (studentised Breusch–Pagan)",
        "Cook 1977, Technometrics 19",
    ],
    roles: &[
        RoleSpec {
            name: "y",
            required: true,
            many: false,
        },
        RoleSpec {
            name: "x",
            required: true,
            many: false,
        },
        RoleSpec {
            name: "controls",
            required: false,
            many: true,
        },
    ],
    supports: &["admin"],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false,
      "properties": {
        "robust": {"type": "boolean"},
        "fixed_effects": {"enum": ["none", "parent"]},
        "spatial": {"oneOf": [{"type": "null"}, {"type": "object", "additionalProperties": false,
          "properties": {"weights": {"enum": ["knn4", "knn6", "knn8", "queen"]},
                         "permutations": {"type": "integer", "minimum": 0, "maximum": 9999}}}]}
      }
    }"#,
    estimates: r#"{"type": "object", "required": ["ols"], "properties": {"ols": {"type": "object"}, "robust": {"type": "object"}, "residual_moran": {"type": "object"}}}"#,
    min_n: Some(MinN {
        block: 10,
        warn: 30,
    }),
    relationship: true,
    guards: &[
        "NOT_CAUSAL",
        "MIN_N",
        "SMALL_N",
        "MAUP",
        "ECOLOGICAL",
        "RESIDUAL_AUTOCORR",
        "SIZE_CONFOUND",
        "SHARED_LINEAGE",
        "ESTIMATED_INPUT",
        "MODEL_ON_MODEL",
        "PERIOD_MISMATCH",
        "LOW_COVERAGE",
        "OUTLIER_LEVERAGE",
        "NO_CONTROLS",
        "MULTICOLLINEAR",
        "SUPPRESSED_CELLS",
        "SYNTHETIC_INPUT",
    ],
    page: "docs/methods/analysis-regress.md",
};

impl AnalysisMethod for Regress {
    fn info(&self) -> &'static MethodInfo {
        &INFO
    }

    fn defaults(&self, params: &Value) -> Value {
        with_defaults(
            params,
            json!({"robust": true, "fixed_effects": "none", "spatial": {"weights": "knn6", "permutations": 999}}),
        )
    }

    fn cost_ms(&self, n: usize, params: &Value) -> u64 {
        let perms = params["spatial"]["permutations"].as_u64().unwrap_or(999);
        5 + n as u64 * perms / 150_000
    }

    #[allow(clippy::too_many_lines)]
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let f = ctx.frame;
        let p = ctx.params;
        let (Some(x), Some(y)) = (f.col("x"), f.col("y")) else {
            bail!("regress needs y and x");
        };
        let controls = f.controls();
        let mut roles = vec!["x", "y"];
        let cr: Vec<String> = controls.iter().map(|c| c.role.clone()).collect();
        roles.extend(cr.iter().map(String::as_str));
        let rows = f.complete(&roles);
        let n = rows.len();
        // block threshold grows with the predictors: 10 + 10 per predictor
        let k_pred = 1 + controls.len();
        let block = 10 + 10 * k_pred.saturating_sub(1);
        let mut out = MethodOutput {
            n_used: n,
            dropped: f.drop_counts(&roles),
            ..MethodOutput::default()
        };
        if n < block {
            out.raised.push(
                Raised::new("MIN_N")
                    .var("n", n.to_string())
                    .var("units", ctx.noun)
                    .var("method", "regress with this many predictors")
                    .var("min", block.to_string())
                    .detail(json!({"n": n, "min": block, "predictors": k_pred})),
            );
            return Ok(out);
        }
        let show_p = n >= 30;
        let mut preds: Vec<&Column> = vec![x];
        preds.extend(controls.iter().copied());
        // fixed effects: one dummy per parent unit but the first
        let mut dummies: Vec<Column> = Vec::new();
        if p["fixed_effects"] == "parent" {
            let mut groups: Vec<String> = rows
                .iter()
                .filter_map(|&i| f.support.parents[i].clone())
                .collect();
            groups.sort();
            groups.dedup();
            for g in groups.iter().skip(1) {
                let values: Vec<f64> = f
                    .support
                    .parents
                    .iter()
                    .map(|q| {
                        if q.as_deref() == Some(g.as_str()) {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .collect();
                dummies.push(Column {
                    role: format!("fe:{g}"),
                    term: format!("fe[{g}]"),
                    input: InputRef {
                        label: format!("fixed effect {g}"),
                        ..InputRef::default()
                    },
                    transform: lanka_stats::transform::Transform::Identity,
                    basis: vec![1; values.len()],
                    suppressed: vec![false; values.len()],
                    values,
                    lo: None,
                    hi: None,
                    alignment: crate::frame::AlignmentRecord::default(),
                });
            }
        }
        preds.extend(dummies.iter());
        let spatial = match &p["spatial"] {
            Value::Null => None,
            s => Some((
                knn_k(s["weights"].as_str().unwrap_or("knn6"))?,
                u32::try_from(s["permutations"].as_u64().unwrap_or(999)).unwrap_or(999),
            )),
        };
        let mut raised = Vec::new();
        let fr = fit(
            y,
            &preds,
            &rows,
            &f.support.lon,
            &f.support.lat,
            p["robust"].as_bool().unwrap_or(true),
            spatial,
            ctx.seed,
            &mut raised,
            ctx.noun,
        )?;
        if controls.is_empty() && dummies.is_empty() {
            raised.push(
                Raised::new("NO_CONTROLS")
                    .var("candidates", "any indicator published at this level"),
            );
        }
        let b = if fr.headline == "robust" {
            fr.robust.as_ref().map_or(fr.ols.coef[1], |h| h.coef[1])
        } else {
            fr.ols.coef[1]
        };
        let shown: Vec<String> = fr.terms[1..=controls.len() + 1].to_vec();
        let formula = format!(
            "{} ~ {}{}",
            term("y", y.transform),
            shown.join(" + "),
            if dummies.is_empty() {
                String::new()
            } else {
                " + fixed effects (parent unit)".to_owned()
            }
        );
        let mut coef = coef_json(&fr, show_p);
        coef.truncate(controls.len() + 2);
        let mut est = json!({
            "ols": {
                "formula": formula, "coef": coef, "r2": num(fr.ols.r2), "adj_r2": num(fr.ols.adj_r2), "n": n,
                "fixed_effects": if dummies.is_empty() { Value::Null } else { json!({"by": "parent", "groups": dummies.len() + 1}) },
                "reading": reading(b, x, y, true),
                "diagnostics": diagnostics_json(&fr, show_p),
            },
            "headline": fr.headline,
        });
        if let Some(mut r) = robust_json(&fr) {
            if let Some(a) = r["coef"].as_array_mut() {
                a.truncate(controls.len() + 2);
            }
            est["robust"] = r;
        }
        if let Some(m) = moran_json(&fr) {
            est["residual_moran"] = m;
        }
        raised.push(
            Raised::new("MAUP")
                .var("units", ctx.noun)
                .var("sensitivity", ""),
        );
        let codes: Vec<String> = rows.iter().map(|&i| f.support.codes[i].clone()).collect();
        out.outputs.push(Output {
            id: "residuals",
            kind: "frame",
            columns: vec![
                "fitted".into(),
                "residual".into(),
                "cooks".into(),
                "leverage".into(),
            ],
            encoding: None,
            data: frame_data(
                &codes,
                &[
                    ("fitted", nums(&fr.ols.fitted)),
                    ("residual", nums(&fr.ols.resid)),
                    ("cooks", nums(&fr.ols.cooks)),
                    ("leverage", nums(&fr.ols.leverage)),
                ],
            ),
            suggested_layer: Some(json!({
                "type": "choropleth", "name": "Residuals",
                "style": {"fill": {"field": "residual", "scale": {"type": "diverging", "classes": 7, "mid": 0, "ramp": "ldl.div.blue-orange"}}}
            })),
        });
        let b_ols = fr.ols.coef[1];
        out.summary = format!(
            "Across {} {}, the model {} explains {} % of the variation (R²); the coefficient of {} is {} (95 % interval {} to {}).",
            thousands(n),
            ctx.noun,
            est["ols"]["formula"].as_str().unwrap_or_default(),
            fmt_num(fr.ols.r2 * 100.0, 2),
            x.input.label,
            fmt_num(b_ols, 3),
            fmt_num(fr.ols.ci_hc3[1][0], 3),
            fmt_num(fr.ols.ci_hc3[1][1], 3),
        );
        out.estimates = est;
        out.raised.extend(raised);
        Ok(out)
    }
}
