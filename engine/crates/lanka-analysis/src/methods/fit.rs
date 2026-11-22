//! Shared regression machinery of `relate` and `regress`: OLS (HC3), Huber,
//! diagnostics, residual Moran's I, the templated `reading` and the guards
//! the fit raises.

use anyhow::{Result, bail};
use lanka_stats::regress::{Design, RegressError, breusch_pagan, huber, ols, vif};
use lanka_stats::spatial::{knn, moran_test};
use lanka_stats::transform::Transform;
use serde_json::{Value, json};

use crate::frame::Column;
use crate::guards::Raised;
use crate::method::{fmt_num, fmt_p, num};

/// Parse `"knn4" | "knn6" | "knn8"` (queen contiguity arrives with WP-30).
pub fn knn_k(weights: &str) -> Result<usize> {
    Ok(match weights {
        "knn4" => 4,
        "knn6" => 6,
        "knn8" => 8,
        "queen" => bail!("queen contiguity weights arrive with WP-30; use knn4, knn6 or knn8"),
        w => bail!("unknown weights {w:?}"),
    })
}

pub struct Fit {
    pub terms: Vec<String>,
    pub ols: lanka_stats::regress::Ols,
    pub robust: Option<lanka_stats::regress::Huber>,
    pub vif: Vec<f64>,
    pub bp: Option<(f64, f64)>,
    pub moran: Option<lanka_stats::spatial::MoranTest>,
    pub moran_k: usize,
    /// `"ols"` or `"robust"` (OUTLIER_LEVERAGE makes the robust fit the headline).
    pub headline: &'static str,
    pub max_cooks: f64,
}

/// Formula term of a column: `log(y)`, `x`, `rank(c1)`.
#[must_use]
pub fn term(name: &str, t: Transform) -> String {
    match t {
        Transform::Identity => name.to_owned(),
        _ => format!("{}({name})", t.as_str()),
    }
}

/// Fits `y` on the predictor columns over `rows` (complete cases).
#[allow(clippy::too_many_arguments)]
pub fn fit(
    y: &Column,
    preds: &[&Column],
    rows: &[usize],
    lon: &[f64],
    lat: &[f64],
    robust: bool,
    spatial: Option<(usize, u32)>,
    seed: u64,
    raised: &mut Vec<Raised>,
    noun: &str,
) -> Result<Fit> {
    let yt = y.transformed();
    let yv: Vec<f64> = rows.iter().map(|&i| yt[i]).collect();
    let cols: Vec<Vec<f64>> = preds
        .iter()
        .map(|c| {
            let t = c.transformed();
            rows.iter().map(|&i| t[i]).collect()
        })
        .collect();
    let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
    let d = Design::with_intercept(&refs);
    let o = match ols(&d, &yv, 0.95) {
        Ok(o) => o,
        Err(RegressError::Singular { column }) => {
            let which = if column == 0 {
                "the intercept".to_owned()
            } else {
                preds
                    .get(column - 1)
                    .map_or_else(|| "a predictor".to_owned(), |c| c.input.label.clone())
            };
            bail!(
                "the regression design is singular at {which} (a constant or duplicated variable)"
            )
        }
        Err(e) => bail!("regression: {e}"),
    };
    let mut terms = vec!["(intercept)".to_owned()];
    terms.extend(preds.iter().map(|c| term(&c.term, c.transform)));
    let vif = if preds.len() > 1 {
        vif(&d).unwrap_or_default()
    } else {
        Vec::new()
    };
    let bp = breusch_pagan(&d, &o.resid).ok();
    let rob = if robust { huber(&d, &yv).ok() } else { None };
    let max_cooks = o
        .cooks
        .iter()
        .copied()
        .filter(|c| c.is_finite())
        .fold(0.0, f64::max);
    let mut headline = "ols";
    if let Some(h) = &rob {
        let (b_ols, b_rob) = (o.coef[1], h.coef[1]);
        let rel = (b_rob - b_ols).abs() / b_ols.abs().max(1e-300);
        if rel > 0.25 || max_cooks > 1.0 {
            headline = "robust";
            raised.push(
                Raised::new("OUTLIER_LEVERAGE")
                    .var("units", noun)
                    .var("cooks", fmt_num(max_cooks, 2))
                    .var("robust", fmt_num(b_rob, 3))
                    .var("ols", fmt_num(b_ols, 3))
                    .detail(json!({"max_cooks": num(max_cooks), "slope_ols": num(b_ols), "slope_robust": num(b_rob)})),
            );
        }
    } else if max_cooks > 1.0 {
        raised.push(
            Raised::new("OUTLIER_LEVERAGE")
                .var("units", noun)
                .var("cooks", fmt_num(max_cooks, 2))
                .var("robust", "n/a")
                .var("ols", fmt_num(o.coef[1], 3))
                .detail(json!({"max_cooks": num(max_cooks)})),
        );
    }
    if let Some((i, v)) = vif
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .max_by(|a, b| a.1.total_cmp(b.1))
        && *v > 10.0
    {
        let names: Vec<String> = terms[1..].to_vec();
        raised.push(
            Raised::new("MULTICOLLINEAR")
                .var("terms", names.join(", "))
                .var("vif", fmt_num(*v, 3))
                .detail(json!({"vif": vif.iter().map(|v| num(*v)).collect::<Vec<_>>(), "worst": names.get(i)})),
        );
    }
    let mut moran = None;
    let mut moran_k = 0;
    if let Some((k, perms)) = spatial
        && rows.len() > k + 1
    {
        let lo: Vec<f64> = rows.iter().map(|&i| lon[i]).collect();
        let la: Vec<f64> = rows.iter().map(|&i| lat[i]).collect();
        let w = knn(&lo, &la, k);
        let t = moran_test(&o.resid, &w, perms, seed, 0x6d6f72);
        if t.p_perm < 0.05 {
            raised.push(
                Raised::new("RESIDUAL_AUTOCORR")
                    .var("moran", fmt_num(t.i, 2))
                    .var("p", fmt_p(t.p_perm))
                    .detail(json!({"I": num(t.i), "p_perm": t.p_perm, "weights": format!("knn{k}")}))
                    .action(json!({"method": "spatial_regress", "label": "fit a spatial model (WP-30)"})),
            );
        }
        moran = Some(t);
        moran_k = k;
    }
    Ok(Fit {
        terms,
        ols: o,
        robust: rob,
        vif,
        bp,
        moran,
        moran_k,
        headline,
        max_cooks,
    })
}

/// The `reading` of the first slope, by the transforms of x and y; never
/// causal wording ("is associated with").
#[must_use]
pub fn reading(b: f64, x: &Column, y: &Column, held: bool) -> String {
    let xl = &x.input.label;
    let yl = &y.input.label;
    let xu = x
        .input
        .unit
        .as_deref()
        .map(display_unit)
        .unwrap_or_default();
    let tail = if held {
        ", other terms held constant"
    } else {
        ""
    };
    let dir = |v: f64| if v >= 0.0 { "higher" } else { "lower" };
    match (x.transform, y.transform) {
        (Transform::Identity, Transform::Log) => {
            let pct = (b.exp() - 1.0) * 100.0;
            format!(
                "one {} more of {xl} is associated with {} % {} {yl}{tail}",
                if xu.is_empty() { "unit".to_owned() } else { xu },
                fmt_num(pct.abs(), 2),
                dir(pct)
            )
        }
        (Transform::Log, Transform::Log) => format!(
            "1 % higher {xl} is associated with {} % {} {yl}{tail}",
            fmt_num(b.abs(), 2),
            dir(b)
        ),
        (Transform::Log, Transform::Identity) => format!(
            "1 % higher {xl} is associated with {} {} {yl}{tail}",
            fmt_num((b / 100.0).abs(), 2),
            dir(b)
        ),
        (Transform::Identity, Transform::Identity) => format!(
            "one {} more of {xl} is associated with {} {} {yl}{tail}",
            if xu.is_empty() { "unit".to_owned() } else { xu },
            fmt_num(b.abs(), 3),
            dir(b)
        ),
        (tx, ty) => format!(
            "one unit more of {} is associated with {} units {} {}{tail}",
            term(xl, tx),
            fmt_num(b.abs(), 3),
            dir(b),
            term(yl, ty)
        ),
    }
}

/// UCUM-ish unit for a sentence.
#[must_use]
pub fn display_unit(u: &str) -> String {
    match u {
        "%" => "percentage point".to_owned(),
        "/km2" => "per km²".to_owned(),
        "{person}" => "person".to_owned(),
        u => u.trim_matches(|c| c == '{' || c == '}').to_owned(),
    }
}

/// Coefficient table of an OLS fit (p only when allowed).
#[must_use]
pub fn coef_json(f: &Fit, show_p: bool) -> Vec<Value> {
    (0..f.terms.len())
        .map(|j| {
            let mut c = json!({
                "term": f.terms[j],
                "b": num(f.ols.coef[j]),
                "se": num(f.ols.se[j]),
                "se_hc3": num(f.ols.se_hc3[j]),
                "ci": [num(f.ols.ci_hc3[j][0]), num(f.ols.ci_hc3[j][1])],
            });
            if show_p {
                c["p"] = num(f.ols.p_hc3[j]);
            }
            if j > 0
                && let Some(v) = f.vif.get(j - 1)
            {
                c["vif"] = num(*v);
            }
            c
        })
        .collect()
}

#[must_use]
pub fn robust_json(f: &Fit) -> Option<Value> {
    f.robust.as_ref().map(|h| {
        json!({
            "method": "huber",
            "tuning": lanka_stats::regress::HUBER_T,
            "scale": num(h.scale),
            "iterations": h.iterations,
            "converged": h.converged,
            "coef": (0..f.terms.len()).map(|j| json!({"term": f.terms[j], "b": num(h.coef[j])})).collect::<Vec<_>>(),
        })
    })
}

#[must_use]
pub fn diagnostics_json(f: &Fit, show_p: bool) -> Value {
    let mut d = json!({
        "max_cooks": num(f.max_cooks),
        "sigma": num(f.ols.sigma),
        "df_resid": num(f.ols.df_resid),
    });
    if let Some((lm, p)) = f.bp {
        d["breusch_pagan"] = if show_p {
            json!({"lm": num(lm), "p": num(p)})
        } else {
            json!({"lm": num(lm)})
        };
    }
    d
}

#[must_use]
pub fn moran_json(f: &Fit) -> Option<Value> {
    f.moran.map(|t| {
        json!({"I": num(t.i), "expected": num(t.expected), "p_perm": t.p_perm,
               "permutations": t.permutations, "weights": format!("knn{}", f.moran_k)})
    })
}
