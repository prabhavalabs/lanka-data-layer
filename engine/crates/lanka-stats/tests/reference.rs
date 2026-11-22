//! Reference fixtures (11 §13): agreement with numpy/scipy/statsmodels to
//! 1e-8 (relative, + 1e-12 absolute for values near zero), and exact
//! agreement for seeded resampling. Generator: `fixtures/gen_reference.py`.

use lanka_stats::classify::{Method, breaks};
use lanka_stats::compare::{Stat, bootstrap, cohens_d, hodges_lehmann, mann_whitney, welch};
use lanka_stats::correlate::{pearson, spearman};
use lanka_stats::describe::summary;
use lanka_stats::effn::{DEFAULT_CLASSES, effective_n};
use lanka_stats::regress::{Design, breusch_pagan, huber, ols, vif};
use lanka_stats::spatial::{knn, moran_test};
use lanka_stats::special;
use serde_json::Value;

const TOL: f64 = 1e-8;

fn fixtures() -> Value {
    let raw = include_str!("fixtures/reference.json");
    serde_json::from_str(raw).expect("reference.json")
}

fn f(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn fv(v: &Value) -> Vec<f64> {
    v.as_array().expect("array").iter().map(f).collect()
}

#[track_caller]
fn near(what: &str, got: f64, want: f64) {
    let ok = (got - want).abs() <= TOL * want.abs() + 1e-12;
    assert!(
        ok,
        "{what}: got {got:e}, want {want:e} (diff {:e})",
        got - want
    );
}

#[track_caller]
fn near_all(what: &str, got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        near(&format!("{what}[{i}]"), *g, *w);
    }
}

#[test]
fn special_functions() {
    let fx = fixtures();
    let mut n = 0;
    for c in fx["special"].as_array().expect("special") {
        let a = fv(&c["a"]);
        let want = f(&c["v"]);
        let got = match c["f"].as_str().expect("f") {
            "t_ppf" => special::t_ppf(a[0], a[1]),
            "t_p2" => special::t_p2(a[0], a[1]),
            "norm_ppf" => special::norm_ppf(a[0]),
            "beta_inc" => special::beta_inc(a[0], a[1], a[2]),
            "chi2_sf" => special::chi2_sf(a[0], a[1]),
            "f_sf" => special::f_sf(a[0], a[1], a[2]),
            other => panic!("unknown {other}"),
        };
        near(&format!("{} {:?}", c["f"], a), got, want);
        n += 1;
    }
    assert!(n > 100);
}

#[test]
fn describe_matches_numpy_scipy() {
    let fx = fixtures();
    let d = &fx["describe"];
    let s = summary(&fv(&d["values"]));
    near("mean", s.mean, f(&d["mean"]));
    near("sd", s.sd, f(&d["sd"]));
    near_all("q", &s.q, &fv(&d["q"]));
    near("mad", s.mad, f(&d["mad"]));
    near("skewness", s.skewness, f(&d["skewness"]));
    near("kurtosis", s.kurtosis, f(&d["kurtosis"]));
}

#[test]
fn correlations_match_scipy() {
    let fx = fixtures();
    for c in fx["correlate"].as_array().expect("correlate") {
        let name = c["name"].as_str().expect("name");
        let (x, y) = (fv(&c["x"]), fv(&c["y"]));
        let p = pearson(&x, &y, 0.95);
        near(&format!("{name} r"), p.r, f(&c["pearson_r"]));
        near(&format!("{name} p"), p.p, f(&c["pearson_p"]));
        near_all(&format!("{name} ci"), &p.ci, &fv(&c["pearson_ci"]));
        let s = spearman(&x, &y, 0.95);
        near(&format!("{name} rho"), s.r, f(&c["spearman_rho"]));
        near(&format!("{name} rho p"), s.p, f(&c["spearman_p"]));
        near_all(&format!("{name} rho ci"), &s.ci, &fv(&c["spearman_ci"]));
    }
}

#[test]
fn regressions_match_statsmodels() {
    let fx = fixtures();
    for c in fx["regress"].as_array().expect("regress") {
        let name = c["name"].as_str().expect("name");
        let y = fv(&c["y"]);
        let cols: Vec<Vec<f64>> = c["x"].as_array().expect("x").iter().map(fv).collect();
        let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
        let d = Design::with_intercept(&refs);
        let o = ols(&d, &y, 0.95).expect("ols");
        // Longley is the classic ill-conditioned design (condition number
        // ~5e9): agreement there is limited by the problem, not the code.
        let tol_scale = if name == "longley" { 1e3 } else { 1.0 };
        let near_s = |what: &str, g: f64, w: f64| {
            let ok = (g - w).abs() <= TOL * tol_scale * w.abs() + 1e-12;
            assert!(ok, "{name} {what}: got {g:e}, want {w:e}");
        };
        let all = |what: &str, g: &[f64], w: &[f64]| {
            assert_eq!(g.len(), w.len());
            for (a, b) in g.iter().zip(w) {
                near_s(what, *a, *b);
            }
        };
        all("coef", &o.coef, &fv(&c["coef"]));
        all("se", &o.se, &fv(&c["se"]));
        all("se_hc3", &o.se_hc3, &fv(&c["se_hc3"]));
        all("p_hc3", &o.p_hc3, &fv(&c["p_hc3"]));
        let ci: Vec<f64> = o.ci_hc3.iter().flatten().copied().collect();
        let want_ci: Vec<f64> = c["ci_hc3"]
            .as_array()
            .expect("ci")
            .iter()
            .flat_map(fv)
            .collect();
        all("ci_hc3", &ci, &want_ci);
        near_s("sigma", o.sigma, f(&c["sigma"]));
        near_s("r2", o.r2, f(&c["r2"]));
        near_s("adj_r2", o.adj_r2, f(&c["adj_r2"]));
        all("leverage", &o.leverage, &fv(&c["leverage"]));
        all("cooks", &o.cooks, &fv(&c["cooks"]));
        if d.p > 2 {
            all("vif", &vif(&d).expect("vif"), &fv(&c["vif"]));
        }
        let (lm, bp) = breusch_pagan(&d, &o.resid).expect("bp");
        near_s("bp_lm", lm, f(&c["bp_lm"]));
        near_s("bp_p", bp, f(&c["bp_p"]));
        let h = huber(&d, &y).expect("huber");
        assert!(h.converged, "{name}: huber did not converge");
        // IRLS fixed point: both sides iterate to ~1e-13 in the coefficients
        let near_h = |what: &str, g: f64, w: f64| {
            let ok = (g - w).abs() <= 1e-8 * tol_scale * w.abs().max(1.0);
            assert!(ok, "{name} {what}: got {g:e}, want {w:e}");
        };
        for (g, w) in h.coef.iter().zip(fv(&c["huber_coef"])) {
            near_h("huber coef", *g, w);
        }
        near_h("huber scale", h.scale, f(&c["huber_scale"]));
    }
}

#[test]
fn comparisons_match_scipy() {
    let fx = fixtures();
    let c = &fx["compare"];
    let (a, b) = (fv(&c["a"]), fv(&c["b"]));
    let w = welch(&a, &b, 0.95);
    near("welch t", w.t, f(&c["welch_t"]));
    near("welch df", w.df, f(&c["welch_df"]));
    near("welch p", w.p, f(&c["welch_p"]));
    near_all("welch ci", &w.ci, &fv(&c["welch_ci"]));
    let m = mann_whitney(&a, &b);
    near("mw u", m.u, f(&c["mw_u"]));
    near("mw p", m.p, f(&c["mw_p"]));
    near("hl", hodges_lehmann(&a, &b, 0).0, f(&c["hodges_lehmann"]));
    near("cohen d", cohens_d(&a, &b), f(&c["cohens_d"]));
}

#[test]
fn seeded_bootstrap_is_bit_identical_to_the_ported_stream() {
    let fx = fixtures();
    let bs = &fx["compare"]["bootstrap"];
    let (a, b) = (fv(&fx["compare"]["a"]), fv(&fx["compare"]["b"]));
    let seed = bs["seed"].as_u64().expect("seed");
    let stream = bs["stream"].as_u64().expect("stream");
    let reps = usize::try_from(bs["reps"].as_u64().expect("reps")).expect("reps");
    let hex = |v: &Value| -> Vec<String> {
        v.as_array()
            .expect("bits")
            .iter()
            .map(|s| s.as_str().expect("hex").to_owned())
            .collect()
    };
    let bits = |v: [f64; 2]| -> Vec<String> {
        v.iter().map(|x| format!("{:016x}", x.to_bits())).collect()
    };
    let got = bootstrap(&a, Some(&b), Stat::Median, reps, 0.95, seed, stream);
    assert_eq!(bits(got), hex(&bs["median_diff"]), "{got:?}");
    let got = bootstrap(&a, None, Stat::Mean, reps, 0.9, seed, stream + 1);
    assert_eq!(bits(got), hex(&bs["mean_one"]), "{got:?}");
}

#[test]
fn class_breaks_match_the_reference() {
    let fx = fixtures();
    let c = &fx["classify"];
    let v = fv(&c["values"]);
    let k = usize::try_from(c["k"].as_u64().expect("k")).expect("k");
    for (m, key) in [
        (Method::Quantile, "quantile"),
        (Method::Equal, "equal"),
        (Method::Jenks, "jenks"),
        (Method::HeadTail, "headtail"),
        (Method::StdDev, "stddev"),
    ] {
        near_all(key, &breaks(&v, m, k), &fv(&c[key]));
    }
}

#[test]
fn effective_n_matches_the_matrix_formula() {
    let fx = fixtures();
    let e = &fx["effn"];
    let got = effective_n(
        &fv(&e["x"]),
        &fv(&e["y"]),
        &fv(&e["lon"]),
        &fv(&e["lat"]),
        DEFAULT_CLASSES,
    )
    .expect("m");
    near("M", got.m, f(&e["m"]));
}

#[test]
fn knn_moran_and_permutations_match_the_port() {
    let fx = fixtures();
    let m = &fx["moran"];
    let (lon, lat) = (fv(&m["lon"]), fv(&m["lat"]));
    let k = usize::try_from(m["k"].as_u64().expect("k")).expect("k");
    let w = knn(&lon, &lat, k);
    let want: Vec<u32> = m["neighbours"]
        .as_array()
        .expect("nb")
        .iter()
        .flat_map(|r| {
            r.as_array()
                .expect("row")
                .iter()
                .map(|j| u32::try_from(j.as_u64().expect("j")).expect("j"))
        })
        .collect();
    assert_eq!(w.neighbours, want);
    let t = moran_test(
        &fv(&m["values"]),
        &w,
        u32::try_from(m["permutations"].as_u64().expect("perms")).expect("perms"),
        m["seed"].as_u64().expect("seed"),
        m["stream"].as_u64().expect("stream"),
    );
    assert_eq!(
        format!("{:016x}", t.i.to_bits()),
        m["i_bits"].as_str().expect("bits")
    );
    assert_eq!(t.p_perm, f(&m["p_perm"]));
}

#[test]
fn partial_correlation_matches_numpy_scipy() {
    use lanka_stats::correlate::{p_adjusted_k, partial};
    let fx: Value =
        serde_json::from_str(include_str!("fixtures/partial.json")).expect("partial.json");
    let (x, y) = (fv(&fx["x"]), fv(&fx["y"]));
    let controls: Vec<Vec<f64>> = fx["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .map(fv)
        .collect();
    for c in fx["cases"].as_array().expect("cases") {
        let k = usize::try_from(c["k"].as_u64().expect("k")).expect("k");
        let refs: Vec<&[f64]> = controls[..k].iter().map(Vec::as_slice).collect();
        for (name, rank) in [("pearson", false), ("spearman", true)] {
            let got = partial(&x, &y, &refs, 0.95, rank).expect("partial");
            let want = &c[name];
            near(&format!("k={k} {name} r"), got.c.r, f(&want["r"]));
            near(&format!("k={k} {name} p"), got.c.p, f(&want["p"]));
            near_all(&format!("k={k} {name} ci"), &got.c.ci, &fv(&want["ci"]));
        }
    }
    for c in fx["p_adjusted_k"].as_array().expect("p_adjusted_k") {
        let k = usize::try_from(c["k"].as_u64().expect("k")).expect("k");
        near(
            "p_adjusted_k",
            p_adjusted_k(f(&c["r"]), f(&c["m"]), k),
            f(&c["p"]),
        );
    }
}
