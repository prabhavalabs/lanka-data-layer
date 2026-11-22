//! `group_compare@1` — inside vs outside a zone (11 §4). WP-29 runs it on
//! areal support: the zone is a variable (`variables.group`) and a unit is
//! inside when that variable is at or above `zone.threshold` (e.g. "flood
//! area share ≥ 10 %"). Point support with distance rings arrives with the
//! point layers (WP-27/WP-33). Welch t, Mann–Whitney U, Hodges–Lehmann
//! shift, Cohen's d, seeded bootstrap intervals.

use anyhow::{Result, bail};
use lanka_stats::compare::{Stat, bootstrap, cohens_d, hodges_lehmann, mann_whitney, welch};
use lanka_stats::describe::{mean, median_sorted, sorted_finite, variance};
use lanka_stats::special::t_ppf;
use serde_json::{Value, json};

use crate::guards::Raised;
use crate::method::{
    AnalysisMethod, Ctx, MethodInfo, MethodOutput, MinN, Output, RoleSpec, fmt_num, frame_data,
    num, nums, thousands, with_defaults,
};

pub struct GroupCompare;

pub static INFO: MethodInfo = MethodInfo {
    id: "group_compare",
    version: "1.0.0",
    tier: 1,
    summary: "Difference of a measured variable between units inside and outside a zone: Welch t, Mann–Whitney, Hodges–Lehmann shift, bootstrap interval, Cohen's d",
    citations: &[
        "Welch 1947, Biometrika 34",
        "Mann & Whitney 1947, Ann. Math. Statist. 18",
        "Hodges & Lehmann 1963, Ann. Math. Statist. 34",
        "Efron & Tibshirani 1993, An Introduction to the Bootstrap",
    ],
    roles: &[
        RoleSpec {
            name: "y",
            required: true,
            many: false,
        },
        RoleSpec {
            name: "group",
            required: true,
            many: false,
        },
    ],
    supports: &["admin"],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false,
      "properties": {
        "zone": {"type": "object", "additionalProperties": false,
          "properties": {"threshold": {"type": "number"}, "label": {"type": "string"},
                         "source": {"type": "string"}, "grid": {"type": "string"}, "dataset": {"type": "string"}, "where": {}}},
        "compare": {"enum": ["inside_vs_outside", "inside_vs_ring"]},
        "rings_m": {"type": "array", "items": {"type": "number", "exclusiveMinimum": 0}},
        "within_groups": {"oneOf": [{"type": "null"}, {"type": "object"}]},
        "tests": {"type": "array", "items": {"enum": ["welch", "mann_whitney"]}, "uniqueItems": true},
        "bootstrap": {"type": "integer", "minimum": 0, "maximum": 10000}
      }
    }"#,
    estimates: r#"{"type": "object", "required": ["groups", "contrasts"], "properties": {"groups": {"type": "array"}, "contrasts": {"type": "array"}}}"#,
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
        "ESTIMATED_INPUT",
        "MODEL_ON_MODEL",
        "PERIOD_MISMATCH",
        "LOW_COVERAGE",
        "SUPPRESSED_CELLS",
        "SYNTHETIC_INPUT",
        "COMMUNITY_INPUT",
    ],
    page: "docs/methods/analysis-group-compare.md",
};

impl AnalysisMethod for GroupCompare {
    fn info(&self) -> &'static MethodInfo {
        &INFO
    }

    fn defaults(&self, params: &Value) -> Value {
        with_defaults(
            params,
            json!({"zone": {"threshold": 0.0}, "compare": "inside_vs_outside", "rings_m": [], "within_groups": null,
                   "tests": ["welch", "mann_whitney"], "bootstrap": 999}),
        )
    }

    fn cost_ms(&self, n: usize, params: &Value) -> u64 {
        let b = params["bootstrap"].as_u64().unwrap_or(999);
        2 + n as u64 * b / 200_000
    }

    #[allow(clippy::too_many_lines)]
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let f = ctx.frame;
        let p = ctx.params;
        if p["rings_m"].as_array().is_some_and(|a| !a.is_empty())
            || p["compare"] == "inside_vs_ring"
        {
            bail!(
                "distance rings need point support (WP-27/WP-33); on areal support compare inside_vs_outside"
            );
        }
        if !p["within_groups"].is_null() {
            bail!("within_groups (fixed effects by parent) arrives with point support (WP-33)");
        }
        let (Some(y), Some(g)) = (f.col("y"), f.col("group")) else {
            bail!("group_compare needs y and group (the zone variable)");
        };
        let rows = f.complete(&["y", "group"]);
        let th = p["zone"]["threshold"].as_f64().unwrap_or(0.0);
        let strict = th == 0.0; // "any share" means > 0
        let yt = y.transformed();
        let gt = g.transformed();
        let inside_of = |v: f64| if strict { v > th } else { v >= th };
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut label = Vec::with_capacity(rows.len());
        for &i in &rows {
            if inside_of(gt[i]) {
                a.push(yt[i]);
                label.push("inside");
            } else {
                b.push(yt[i]);
                label.push("outside");
            }
        }
        let mut out = MethodOutput {
            n_used: rows.len(),
            dropped: f.drop_counts(&["y", "group"]),
            ..MethodOutput::default()
        };
        let min = a.len().min(b.len());
        if min < 10 {
            out.raised.push(
                Raised::new("MIN_N")
                    .var("n", min.to_string())
                    .var("units", format!("{} in the smaller group", ctx.noun))
                    .var("method", "group_compare")
                    .var("min", "10 per group")
                    .detail(json!({"inside": a.len(), "outside": b.len(), "min_per_group": 10})),
            );
            return Ok(out);
        }
        let show_p = min >= 30;
        if !show_p {
            out.raised.push(
                Raised::new("SMALL_N")
                    .var("n", min.to_string())
                    .var("units", format!("{} in the smaller group", ctx.noun))
                    .var("warn", "30"),
            );
        }
        let reps = usize::try_from(p["bootstrap"].as_u64().unwrap_or(999)).unwrap_or(999);
        let zone_label = p["zone"]["label"].as_str().map_or_else(
            || {
                format!(
                    "{} {} {}",
                    g.input.label,
                    if strict { ">" } else { "≥" },
                    fmt_num(th, 3)
                )
            },
            str::to_owned,
        );
        let group_json = |id: &str, v: &[f64], stream: u64| {
            let s = sorted_finite(v);
            #[allow(clippy::cast_precision_loss)]
            let nf = v.len() as f64;
            let m = mean(v);
            let q = t_ppf(0.975, nf - 1.0);
            let se = (variance(v) / nf).sqrt();
            json!({
                "id": id,
                "label": if id == "inside" { zone_label.clone() } else { format!("not {zone_label}") },
                "n": v.len(), "median": num(median_sorted(&s)), "mean": num(m),
                "ci": [num(m - q * se), num(m + q * se)],
                "median_ci": nums(&bootstrap(v, None, Stat::Median, reps, 0.95, ctx.seed, stream)),
            })
        };
        let groups = vec![group_json("inside", &a, 1), group_json("outside", &b, 2)];
        let w = welch(&a, &b, 0.95);
        let mw = mann_whitney(&a, &b);
        let (hl, sampled) = hodges_lehmann(&a, &b, ctx.seed);
        let d = cohens_d(&a, &b);
        let mdci = bootstrap(&a, Some(&b), Stat::Median, reps, 0.95, ctx.seed, 3);
        let tests: Vec<&str> = p["tests"]
            .as_array()
            .map(|t| t.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let ylab = &y.input.label;
        let scale_note = match y.transform {
            lanka_stats::transform::Transform::Identity => String::new(),
            t => format!(" (on the {} scale)", t.as_str()),
        };
        let reading = format!(
            "{ylab} is on average {} {} inside the zone than outside{scale_note} ({} inside, {} outside); the typical difference (Hodges–Lehmann) is {}",
            fmt_num(w.diff.abs(), 3),
            if w.diff >= 0.0 { "higher" } else { "lower" },
            thousands(a.len()),
            thousands(b.len()),
            fmt_num(hl, 3),
        );
        let mut c = json!({
            "a": "inside", "b": "outside", "diff": num(w.diff), "ci": [num(w.ci[0]), num(w.ci[1])],
            "median_diff_ci": nums(&mdci), "hodges_lehmann": num(hl), "hodges_lehmann_sampled": sampled,
            "cohens_d": num(d), "welch_df": num(w.df), "reading": reading,
        });
        if show_p {
            if tests.contains(&"welch") {
                c["p_welch"] = num(w.p);
            }
            if tests.contains(&"mann_whitney") {
                c["p_mann_whitney"] = num(mw.p);
                c["u"] = num(mw.u);
            }
        }
        out.estimates = json!({"groups": groups, "contrasts": [c], "transform": y.transform.as_str(), "bootstrap": reps});
        let codes: Vec<String> = rows.iter().map(|&i| f.support.codes[i].clone()).collect();
        let yv: Vec<f64> = rows.iter().map(|&i| y.values[i]).collect();
        let gv: Vec<f64> = rows.iter().map(|&i| g.values[i]).collect();
        out.outputs.push(Output {
            id: "units",
            kind: "frame",
            columns: vec!["group".into(), "y".into(), "zone".into()],
            encoding: None,
            data: frame_data(&codes, &[("group", json!(label)), ("y", nums(&yv)), ("zone", nums(&gv))]),
            suggested_layer: Some(json!({
                "type": "choropleth", "name": format!("Inside / outside: {zone_label}"),
                "style": {"fill": {"field": "group", "scale": {"type": "categorical", "domain": ["inside", "outside"]}}}
            })),
        });
        out.outputs.push(Output {
            id: "groups",
            kind: "chart",
            columns: Vec::new(),
            encoding: Some("groups"),
            data: json!({"encoding": "groups", "groups": out.estimates["groups"].clone()}),
            suggested_layer: None,
        });
        out.raised.push(
            Raised::new("MAUP")
                .var("units", ctx.noun)
                .var("sensitivity", ""),
        );
        out.summary = format!(
            "Across {} {}, {ylab} is {} on average in the {} units where {zone_label} than in the other {} (difference{scale_note} {}, 95 % interval {} to {}).",
            thousands(rows.len()),
            ctx.noun,
            if w.diff >= 0.0 { "higher" } else { "lower" },
            thousands(a.len()),
            thousands(b.len()),
            fmt_num(w.diff, 3),
            fmt_num(w.ci[0], 3),
            fmt_num(w.ci[1], 3),
        );
        Ok(out)
    }
}
