//! `relate@1` — the headline method for two variables (11 §4): Pearson and
//! Spearman with intervals, the spatially adjusted p (effective n after
//! Dutilleul) as the headline significance on areal support, OLS (HC3) and
//! Huber fits on the chosen transforms (+ controls), residual Moran's I on
//! kNN weights, bivariate classes, a scale-sensitivity table. With controls
//! the headline correlation is the partial one (x and y residualised on the
//! controls), its adjusted p on the effective n of the residuals less one
//! degree of freedom per control; the zero-order r stays beside it.
//! Lee's L and contiguity weights arrive with WP-30.

use anyhow::{Result, bail};
use lanka_stats::classify::{self, Method as ClassMethod};
use lanka_stats::correlate::{p_adjusted, p_adjusted_k, partial, pearson, spearman};
use lanka_stats::describe::{hexbin, ranks};
use lanka_stats::effn::{DEFAULT_CLASSES, effective_n};
use serde_json::{Value, json};

use super::fit::{coef_json, diagnostics_json, fit, knn_k, moran_json, reading, robust_json, term};
use crate::guards::Raised;
use crate::method::{
    AnalysisMethod, Ctx, MethodInfo, MethodOutput, MinN, Output, RoleSpec, fmt_num, frame_data,
    num, nums, p_phrase, thousands, with_defaults,
};

pub struct Relate;

pub static INFO: MethodInfo = MethodInfo {
    id: "relate",
    version: "1.0.0",
    // r2: estimates.partial and the partial headline with controls
    revision: 2,
    tier: 1,
    summary: "Association between two variables on one support: correlations with intervals, spatially adjusted significance, OLS and robust fits, bivariate classes",
    citations: &[
        "Clifford, Richardson & Hémon 1989, Biometrics 45",
        "Dutilleul 1993, Biometrics 49",
        "MacKinnon & White 1985, J. Econometrics 29 (HC3)",
        "Huber 1964, Ann. Math. Statist. 35",
        "Fieller, Hartley & Pearson 1957, Biometrika 44",
        "Fisher 1924, Metron 3 (partial correlation)",
    ],
    roles: &[
        RoleSpec {
            name: "x",
            required: true,
            many: false,
        },
        RoleSpec {
            name: "y",
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
        "correlation": {"type": "array", "items": {"enum": ["pearson", "spearman"]}, "uniqueItems": true},
        "regression": {"enum": ["ols", "none"]},
        "robust": {"type": "boolean"},
        "spatial": {"oneOf": [{"type": "null"}, {"type": "object", "additionalProperties": false,
          "properties": {"weights": {"enum": ["knn4", "knn6", "knn8", "queen"]},
                         "local": {"enum": ["none", "lee", "moran"]},
                         "permutations": {"type": "integer", "minimum": 0, "maximum": 9999}}}]},
        "bivariate": {"type": "object", "additionalProperties": false,
          "properties": {"classes": {"enum": [3, 4]}, "method": {"enum": ["quantile", "equal", "jenks"]}}},
        "scale_sensitivity": {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 4}},
        "uncertainty": {"enum": ["analytic", "mc"]},
        "effective_n": {"type": "boolean"}
      }
    }"#,
    estimates: r#"{"type": "object", "properties": {
      "pearson": {"type": "object"}, "spearman": {"type": "object"}, "partial": {"type": "object"},
      "correlation_headline": {"enum": ["zero_order", "partial"]}, "ols": {"type": "object"},
      "robust": {"type": "object"}, "residual_moran": {"type": "object"}, "scale_sensitivity": {"type": "array"},
      "headline": {"enum": ["ols", "robust"]}, "significance": {"enum": ["adjusted", "naive", "hidden"]}}}"#,
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
        "MAUP_UNSTABLE",
        "ECOLOGICAL",
        "RESIDUAL_AUTOCORR",
        "SIZE_CONFOUND",
        "SHARED_LINEAGE",
        "ESTIMATED_INPUT",
        "MODEL_ON_MODEL",
        "INPUT_UNCERTAINTY_IGNORED",
        "PERIOD_MISMATCH",
        "LOW_COVERAGE",
        "PSEUDO_REPLICATION",
        "OUTLIER_LEVERAGE",
        "NO_CONTROLS",
        "MULTICOLLINEAR",
        "SUPPRESSED_CELLS",
        "SYNTHETIC_INPUT",
        "COMMUNITY_INPUT",
    ],
    page: "docs/methods/analysis-relate.md",
};

/// Rows above which the scatter is served as hexbins.
const SCATTER_POINTS_MAX: usize = 2000;

/// The suggested bivariate layer's palette: a 3×3 `ldl.biv.*` ramp of
/// `spec/map/1/ramps.json` (the default of 10 §7).
pub const BIVARIATE_PALETTE: &str = "ldl.biv.blue-orange";

/// Class breaks cut on transformed values, on the scale of the raw values
/// the output carries. Every transform is increasing, so the classes are
/// the same (`v ≥ break` → upper class); pointwise transforms invert
/// exactly, `zscore`/`rank` take the smallest raw value at or above the break.
fn raw_breaks(
    breaks: &[f64],
    t: lanka_stats::transform::Transform,
    raw: &[f64],
    transformed: &[f64],
) -> Vec<f64> {
    use lanka_stats::transform::Transform as T;
    breaks
        .iter()
        .map(|&b| match t {
            T::Identity => b,
            T::Log => b.exp(),
            T::Log1p => b.exp_m1(),
            T::Sqrt => b.max(0.0) * b.max(0.0),
            T::ZScore | T::Rank => raw
                .iter()
                .zip(transformed)
                .filter(|(r, v)| r.is_finite() && v.is_finite() && **v >= b)
                .map(|(r, _)| *r)
                .reduce(f64::min)
                .unwrap_or_else(|| {
                    raw.iter()
                        .copied()
                        .filter(|r| r.is_finite())
                        .fold(b, f64::max)
                }),
        })
        .collect()
}

impl AnalysisMethod for Relate {
    fn info(&self) -> &'static MethodInfo {
        &INFO
    }

    fn defaults(&self, params: &Value) -> Value {
        with_defaults(
            params,
            json!({
                "correlation": ["pearson", "spearman"],
                "regression": "ols",
                "robust": true,
                "spatial": {"weights": "knn6", "local": "none", "permutations": 999},
                "bivariate": {"classes": 3, "method": "quantile"},
                "uncertainty": "analytic",
                "effective_n": true,
            }),
        )
    }

    fn cost_ms(&self, n: usize, params: &Value) -> u64 {
        let perms = params["spatial"]["permutations"].as_u64().unwrap_or(999);
        let n = n as u64;
        // effective n is O(n²); permutations O(n·k·R)
        5 + n * n / 40_000 + n * perms / 150_000
    }

    #[allow(clippy::too_many_lines)]
    fn run(&self, ctx: &Ctx<'_>) -> Result<MethodOutput> {
        let p = ctx.params;
        if p["uncertainty"] == "mc" {
            bail!(
                "uncertainty \"mc\" (Monte Carlo over input intervals) arrives with WP-30; use \"analytic\""
            );
        }
        if !matches!(p["spatial"]["local"].as_str(), None | Some("none")) {
            bail!(
                "local statistics (Lee's L, local Moran) arrive with WP-30; set spatial.local to \"none\""
            );
        }
        let f = ctx.frame;
        let (Some(x), Some(y)) = (f.col("x"), f.col("y")) else {
            bail!("relate needs x and y");
        };
        let controls = f.controls();
        let mut roles = vec!["x", "y"];
        let control_roles: Vec<String> = controls.iter().map(|c| c.role.clone()).collect();
        roles.extend(control_roles.iter().map(String::as_str));
        let rows = f.complete(&roles);
        let n = rows.len();
        let mut out = MethodOutput {
            n_used: n,
            dropped: f.drop_counts(&roles),
            ..MethodOutput::default()
        };
        let mut raised = Vec::new();
        let warn = INFO.min_n.map_or(30, |m| m.warn);
        let show_p = n >= warn;
        let xt = x.transformed();
        let yt = y.transformed();
        let xs: Vec<f64> = rows.iter().map(|&i| xt[i]).collect();
        let ys: Vec<f64> = rows.iter().map(|&i| yt[i]).collect();
        let lon: Vec<f64> = rows.iter().map(|&i| f.support.lon[i]).collect();
        let lat: Vec<f64> = rows.iter().map(|&i| f.support.lat[i]).collect();
        let corr: Vec<&str> = p["correlation"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let use_effn = p["effective_n"].as_bool().unwrap_or(true) && n <= 20_000;
        let mut est = json!({});

        let pr = pearson(&xs, &ys, 0.95);
        let mut significance = "hidden";
        if corr.contains(&"pearson") {
            let mut e = json!({"r": num(pr.r), "ci": [num(pr.ci[0]), num(pr.ci[1])], "ci_level": 0.95, "n": n});
            if show_p {
                e["p"] = num(pr.p);
                significance = "naive";
            }
            if use_effn && let Some(m) = effective_n(&xs, &ys, &lon, &lat, DEFAULT_CLASSES) {
                e["n_effective"] = num(m.m);
                if show_p {
                    e["p_adjusted"] = num(p_adjusted(pr.r, m.m));
                    significance = "adjusted";
                }
            }
            est["pearson"] = e;
        }
        if corr.contains(&"spearman") {
            let s = spearman(&xs, &ys, 0.95);
            let mut e =
                json!({"rho": num(s.r), "ci": [num(s.ci[0]), num(s.ci[1])], "ci_level": 0.95});
            if show_p {
                e["p"] = num(s.p);
                if use_effn
                    && let Some(m) =
                        effective_n(&ranks(&xs), &ranks(&ys), &lon, &lat, DEFAULT_CLASSES)
                {
                    e["p_adjusted"] = num(p_adjusted(s.r, m.m));
                    e["n_effective"] = num(m.m);
                }
            }
            est["spearman"] = e;
        }
        // partial correlations given the controls (the headline when there are any)
        let mut partial_r: Option<f64> = None;
        if !controls.is_empty() {
            let ctl: Vec<Vec<f64>> = controls
                .iter()
                .map(|c| {
                    let t = c.transformed();
                    rows.iter().map(|&i| t[i]).collect()
                })
                .collect();
            let refs: Vec<&[f64]> = ctl.iter().map(Vec::as_slice).collect();
            let k = refs.len();
            let mut pe = json!({
                "controls": controls.iter().map(|c| json!(c.input.label)).collect::<Vec<_>>(),
                "k": k,
            });
            for (name, rank) in [("pearson", false), ("spearman", true)] {
                if !corr.contains(&name) {
                    continue;
                }
                let Some(pc) = partial(&xs, &ys, &refs, 0.95, rank) else {
                    continue;
                };
                let key = if rank { "rho" } else { "r" };
                let mut e = json!({key: num(pc.c.r), "ci": [num(pc.c.ci[0]), num(pc.c.ci[1])], "ci_level": 0.95, "n": n, "df": n.saturating_sub(2 + k)});
                if show_p {
                    e["p"] = num(pc.c.p);
                    if !rank {
                        significance = "naive";
                    }
                }
                if use_effn
                    && let Some(m) = effective_n(&pc.ex, &pc.ey, &lon, &lat, DEFAULT_CLASSES)
                {
                    e["n_effective"] = num(m.m);
                    if show_p {
                        e["p_adjusted"] = num(p_adjusted_k(pc.c.r, m.m, k));
                        if !rank {
                            significance = "adjusted";
                        }
                    }
                }
                if !rank {
                    partial_r = Some(pc.c.r);
                }
                pe[name] = e;
            }
            est["partial"] = pe;
            // present only with controls, so results without them are unchanged
            est["correlation_headline"] = json!(if partial_r.is_some() {
                "partial"
            } else {
                "zero_order"
            });
        }

        let spatial = match &p["spatial"] {
            Value::Null => None,
            s => {
                let k = knn_k(s["weights"].as_str().unwrap_or("knn6"))?;
                let perms = u32::try_from(s["permutations"].as_u64().unwrap_or(999)).unwrap_or(999);
                Some((k, perms))
            }
        };
        let mut fitted: Option<super::fit::Fit> = None;
        if p["regression"] != "none" {
            let mut preds = vec![x];
            preds.extend(controls.iter().copied());
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
            let held = !controls.is_empty();
            let b = if fr.headline == "robust" {
                fr.robust.as_ref().map_or(fr.ols.coef[1], |h| h.coef[1])
            } else {
                fr.ols.coef[1]
            };
            let formula = format!("{} ~ {}", term("y", y.transform), fr.terms[1..].join(" + "));
            est["ols"] = json!({
                "formula": formula,
                "coef": coef_json(&fr, show_p),
                "r2": num(fr.ols.r2),
                "adj_r2": num(fr.ols.adj_r2),
                "n": n,
                "reading": reading(b, x, y, held),
                "diagnostics": diagnostics_json(&fr, show_p),
            });
            if let Some(r) = robust_json(&fr) {
                est["robust"] = r;
            }
            if let Some(m) = moran_json(&fr) {
                est["residual_moran"] = m;
            }
            est["headline"] = json!(fr.headline);
            if controls.is_empty() {
                raised.push(
                    Raised::new("NO_CONTROLS")
                        .var("candidates", "any indicator published at this level"),
                );
            }
            fitted = Some(fr);
        }
        est["significance"] = json!(significance);

        // scale sensitivity: the same pair at adjacent levels
        let mut table = vec![json!({"level": f.support.level, "n": n, "pearson_r": num(pr.r)})];
        for sf in ctx.scale {
            let (Some(sx), Some(sy)) = (sf.col("x"), sf.col("y")) else {
                continue;
            };
            let rr = sf.complete(&["x", "y"]);
            if rr.len() < 3 {
                continue;
            }
            let (a, b): (Vec<f64>, Vec<f64>) = {
                let (tx, ty) = (sx.transformed(), sy.transformed());
                rr.iter().map(|&i| (tx[i], ty[i])).unzip()
            };
            let c = pearson(&a, &b, 0.95);
            table.push(json!({"level": sf.support.level, "n": rr.len(), "pearson_r": num(c.r)}));
            let unstable = (c.r.signum() != pr.r.signum() && c.r.abs() > 0.05 && pr.r.abs() > 0.05)
                || (c.r - pr.r).abs() > 0.2;
            if unstable {
                raised.push(
                    Raised::new("MAUP_UNSTABLE")
                        .var("r_other", fmt_num(c.r, 2))
                        .var("level_other", (ctx.noun_of)(sf.support.level))
                        .var("r_here", fmt_num(pr.r, 2))
                        .var("level_here", ctx.noun.to_owned())
                        .detail(json!({"level": sf.support.level, "pearson_r": num(c.r)})),
                );
            }
        }
        table.sort_by_key(|t| t["level"].as_i64().unwrap_or(0));
        let sens = if table.len() > 1 {
            format!(
                "; r is {}",
                table
                    .iter()
                    .map(|t| {
                        let l = i16::try_from(t["level"].as_i64().unwrap_or(0)).unwrap_or(0);
                        format!(
                            "{} at {}",
                            fmt_num(t["pearson_r"].as_f64().unwrap_or(f64::NAN), 2),
                            (ctx.noun_of)(l)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            String::new()
        };
        raised.push(
            Raised::new("MAUP")
                .var("units", ctx.noun)
                .var("sensitivity", sens),
        );
        est["scale_sensitivity"] = Value::Array(table);

        // outputs
        let bp = &p["bivariate"];
        let k = usize::try_from(bp["classes"].as_u64().unwrap_or(3)).unwrap_or(3);
        let m = ClassMethod::parse(bp["method"].as_str().unwrap_or("quantile"))
            .unwrap_or(ClassMethod::Quantile);
        let biv = classify::bivariate(&xs, &ys, m, k);
        let codes: Vec<String> = rows.iter().map(|&i| f.support.codes[i].clone()).collect();
        let raw_x: Vec<f64> = rows.iter().map(|&i| x.values[i]).collect();
        let raw_y: Vec<f64> = rows.iter().map(|&i| y.values[i]).collect();
        let basis: Vec<Value> = rows
            .iter()
            .map(|&i| json!(crate::frame::BASIS[usize::from(x.basis[i].max(y.basis[i]).min(3))]))
            .collect();
        out.outputs.push(Output {
            id: "bivariate",
            kind: "frame",
            columns: vec!["x".into(), "y".into(), "class".into(), "basis".into()],
            encoding: None,
            data: {
                let mut d = frame_data(&codes, &[
                    ("x", nums(&raw_x)),
                    ("y", nums(&raw_y)),
                    ("class", json!(biv.class)),
                    ("basis", Value::Array(basis)),
                ]);
                d["x_breaks"] = nums(&biv.x_breaks);
                d["y_breaks"] = nums(&biv.y_breaks);
                d["classes"] = json!(k);
                d["breaks_on"] = json!("transformed");
                d
            },
            suggested_layer: Some(json!({
                "type": "bivariate", "name": format!("{} × {}", x.input.label, y.input.label),
                "style": {"fill": {"bivariate": {
                    // breaks on the scale of the `x`/`y` fields (the classes were cut on the transformed values)
                    "x": {"field": "x", "scale": {"type": m.as_str(), "classes": k, "breaks": nums(&raw_breaks(&biv.x_breaks, x.transform, &raw_x, &xs))}},
                    "y": {"field": "y", "scale": {"type": m.as_str(), "classes": k, "breaks": nums(&raw_breaks(&biv.y_breaks, y.transform, &raw_y, &ys))}},
                    "palette": BIVARIATE_PALETTE}}}
            })),
        });
        if let Some(fr) = &fitted {
            out.outputs.push(Output {
                id: "residuals",
                kind: "frame",
                columns: vec!["fitted".into(), "residual".into(), "cooks".into(), "leverage".into()],
                encoding: None,
                data: frame_data(&codes, &[
                    ("fitted", nums(&fr.ols.fitted)),
                    ("residual", nums(&fr.ols.resid)),
                    ("cooks", nums(&fr.ols.cooks)),
                    ("leverage", nums(&fr.ols.leverage)),
                ]),
                suggested_layer: Some(json!({
                    "type": "choropleth", "name": "Residuals",
                    "style": {"fill": {"field": "residual", "scale": {"type": "diverging", "classes": 7, "mid": 0, "ramp": "ldl.div.blue-orange"}}}
                })),
            });
        }
        let fit_line = fitted.as_ref().map(|fr| {
            let (kind, c) = match (&fr.robust, fr.headline) {
                (Some(h), "robust") => ("robust", h.coef.clone()),
                _ => ("ols", fr.ols.coef.clone()),
            };
            // with controls the line is the partial slope at the other terms' means
            json!({"kind": kind, "intercept": num(c[0]), "slope": num(c[1]), "partial": !controls.is_empty()})
        });
        let scatter = if n <= SCATTER_POINTS_MAX {
            json!({"encoding": "points", "x": nums(&xs), "y": nums(&ys), "codes": codes, "fit": fit_line})
        } else {
            let (lo, hi) = xs
                .iter()
                .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            #[allow(clippy::cast_precision_loss)]
            let radius = ((hi - lo) / 40.0).max(1e-9);
            let h = hexbin(&xs, &ys, radius);
            json!({"encoding": "hexbin", "cx": nums(&h.cx), "cy": nums(&h.cy), "count": h.count, "radius": radius, "fit": fit_line})
        };
        out.outputs.push(Output {
            id: "scatter",
            kind: "chart",
            columns: Vec::new(),
            encoding: Some(if n <= SCATTER_POINTS_MAX {
                "points"
            } else {
                "hexbin"
            }),
            data: scatter,
            suggested_layer: None,
        });

        // summary (templated)
        let hr = partial_r.unwrap_or(pr.r);
        let head = if partial_r.is_some() {
            &est["partial"]["pearson"]
        } else {
            &est["pearson"]
        };
        let held = if partial_r.is_some() {
            format!(
                " holding {} constant",
                controls
                    .iter()
                    .map(|c| c.input.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ")
            )
        } else {
            String::new()
        };
        let rname = if partial_r.is_some() {
            "partial r"
        } else {
            "r"
        };
        let strength = match hr.abs() {
            a if a < 0.1 => "no clear",
            a if a < 0.3 => "a weak",
            a if a < 0.5 => "a moderate",
            _ => "a strong",
        };
        let dir = if hr >= 0.0 { "higher" } else { "lower" };
        let within = f
            .support
            .within
            .as_deref()
            .map(|w| format!(" of {w}"))
            .unwrap_or_default();
        let (ci_lo, ci_hi) = (
            head["ci"][0].as_f64().unwrap_or(f64::NAN),
            head["ci"][1].as_f64().unwrap_or(f64::NAN),
        );
        let sig = match (head.get("p_adjusted"), head.get("p")) {
            (Some(pa), _) => format!(", adjusted p {}", p_phrase(pa.as_f64().unwrap_or(f64::NAN))),
            (None, Some(pp)) => format!(", p {}", p_phrase(pp.as_f64().unwrap_or(f64::NAN))),
            _ => format!(
                ", 95 % interval {} to {}",
                fmt_num(ci_lo, 2),
                fmt_num(ci_hi, 2)
            ),
        };
        out.summary = if strength == "no clear" {
            format!(
                "Across {} {}{within}, {} and {} show no clear association{held} ({rname} = {}{sig}).",
                thousands(n),
                ctx.noun,
                x.input.label,
                y.input.label,
                fmt_num(hr, 2)
            )
        } else {
            format!(
                "Across {} {}{within}, a higher {} is associated with {dir} {}{held} ({strength} association, {rname} = {}{sig}).",
                thousands(n),
                ctx.noun,
                x.input.label,
                y.input.label,
                fmt_num(hr, 2)
            )
        };
        out.estimates = est;
        out.raised = raised;
        Ok(out)
    }
}
