//! Every expectation type of 03 §5.2 with a passing and a failing case
//! (`sql` runs against PostGIS: `tests/pg_expect.rs`).

use std::path::PathBuf;

use super::*;
use crate::connector::{FeatureIn, UnitRef};

fn registry() -> (Registry, Profile) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
    let reg = Registry::load(&root).expect("registry");
    let p = Profile::load(Profile::discover(&root).expect("profile")).expect("profile loads");
    (reg, p)
}

fn lookup() -> Lookup {
    let u = |code: &str, level: i16, parent: Option<&str>| UnitRef {
        code: code.into(),
        level,
        name_en: code.into(),
        parent: parent.map(Into::into),
    };
    Lookup {
        units: vec![
            u("LK", 0, None),
            u("LK1", 1, Some("LK")),
            u("LK2", 1, Some("LK")),
        ],
        coverage: None,
    }
}

const GDP: &str = "wdi.ny_gdp_mktp_cd";

fn row(ind: &str, geo: &str, period: &str, dims: &str, v: f64) -> ObsRow {
    ObsRow::reported(ind, geo, period, dims.to_owned(), Some(v))
}

fn exp(yaml: &str) -> Expectation {
    let v: serde_json::Value = serde_yaml_ng::from_str(yaml).expect("yaml");
    Expectation::parse(&v).expect("parses")
}

/// Runs one expectation over rows: (passed, skipped).
fn run(e: &str, rows: &[ObsRow], prev: Option<i64>) -> CheckResult {
    let (reg, p) = registry();
    let l = lookup();
    let ctx = ObsCtx {
        registry: &reg,
        profile: &p,
        lookup: &l,
        class: "A",
        prev_rows: prev,
        today: NaiveDate::from_ymd_opt(2026, 11, 22).expect("date"),
    };
    let mut r = check_observations(&[exp(e)], rows, &ctx);
    assert_eq!(r.len(), 1, "{e}");
    r.remove(0)
}

fn pass(e: &str, rows: &[ObsRow]) {
    let r = run(e, rows, Some(rows.len() as i64));
    assert!(r.passed && !r.skipped, "{e} should pass: {}", r.detail);
}

fn fail(e: &str, rows: &[ObsRow]) {
    let r = run(e, rows, Some(rows.len() as i64));
    assert!(!r.passed, "{e} should fail");
    assert!(r.is_error(), "class A defaults to error");
}

fn gdp(vals: &[(&str, f64)]) -> Vec<ObsRow> {
    vals.iter()
        .map(|(p, v)| row(GDP, "LK", p, "", *v))
        .collect()
}

#[test]
fn parse_rejects_unknown_checks_and_fields() {
    let bad = |y: &str| {
        let v: serde_json::Value = serde_yaml_ng::from_str(y).expect("yaml");
        Expectation::parse(&v).is_err()
    };
    assert!(bad("{check: nope}"));
    assert!(bad("{check: range, minimum: 1}"));
    assert!(bad("{check: sql, query: 'DELETE FROM x'}"));
    assert!(bad("{check: range, severity: fatal}"));
    let e = exp("{check: range, max: 1, severity: warn, note: n}");
    assert_eq!(e.severity_for("A"), Severity::Warn);
    assert_eq!(exp("{check: no_future}").severity_for("B"), Severity::Warn);
}

#[test]
fn row_count() {
    let rows = gdp(&[("2020", 1.0), ("2021", 2.0)]);
    pass("{check: row_count, min: 2, max: 5}", &rows);
    fail("{check: row_count, min: 3}", &rows);
}

#[test]
fn row_count_change() {
    let rows = gdp(&[("2020", 1.0), ("2021", 2.0)]);
    let r = run("{check: row_count_change, pct: 10}", &rows, Some(2));
    assert!(r.passed && !r.skipped);
    let r = run("{check: row_count_change, pct: 10}", &rows, Some(4));
    assert!(!r.passed, "{}", r.detail);
    let r = run("{check: row_count_change, pct: 10}", &rows, None);
    assert!(r.skipped);
}

#[test]
fn completeness() {
    let both = vec![
        row(GDP, "LK1", "2020", "", 1.0),
        row(GDP, "LK2", "2020", "", 1.0),
    ];
    pass("{check: completeness, level: 1, min_units: 2}", &both);
    fail("{check: completeness, level: 1, min_units: 2}", &both[..1]);
}

#[test]
fn range() {
    let rows = gdp(&[("2020", 5.0), ("2021", 7.0)]);
    pass(
        &format!("{{check: range, indicator: {GDP}, min: 0, max: 10}}"),
        &rows,
    );
    fail("{check: range, max: 6}", &rows);
}

#[test]
fn sum_to_parent() {
    let mut rows = vec![
        row(GDP, "LK", "2020", "", 10.0),
        row(GDP, "LK1", "2020", "", 4.0),
        row(GDP, "LK2", "2020", "", 6.0),
    ];
    pass("{check: sum_to_parent, tol_pct: 0.5}", &rows);
    rows[2].value = Some(7.0);
    fail("{check: sum_to_parent, tol_pct: 0.5}", &rows);
}

#[test]
fn sum_dims() {
    let ind = "census24.pop_total";
    let mut rows = vec![
        row(ind, "LK", "2024", "sex=total", 10.0),
        row(ind, "LK", "2024", "sex=male", 4.0),
        row(ind, "LK", "2024", "sex=female", 6.0),
    ];
    pass("{check: sum_dims, dim: sex, tol: 0}", &rows);
    rows[0].value = Some(11.0);
    fail("{check: sum_dims, dim: sex, tol: 0.5}", &rows);
}

#[test]
fn ratio_consistency() {
    let r = "census24.sex_ratio";
    let num = "census24.pop_total";
    let mut rows = vec![
        row(num, "LK", "2024", "sex=male", 95.0),
        row("census24.pop_age_sex", "LK", "2024", "", 100.0),
        row(r, "LK", "2024", "", 95.0),
    ];
    let e = format!(
        "{{check: ratio_consistency, indicator: {r}, numerator: {num}, denominator: census24.pop_age_sex, scale: 100}}"
    );
    // numerator rows carry dims; ratio rows do not: align on an undimensioned numerator
    rows[0].dims = String::new();
    pass(&e, &rows);
    rows[2].value = Some(90.0);
    fail(&e, &rows);
}

#[test]
fn monotonic() {
    pass(
        "{check: monotonic, direction: increasing}",
        &gdp(&[("2020", 1.0), ("2021", 1.0), ("2022", 3.0)]),
    );
    fail(
        "{check: monotonic, direction: increasing}",
        &gdp(&[("2020", 1.0), ("2021", 0.5)]),
    );
}

#[test]
fn max_step_change() {
    pass(
        "{check: max_step_change, pct: 40}",
        &gdp(&[("2020", 100.0), ("2021", 130.0)]),
    );
    fail(
        "{check: max_step_change, pct: 40}",
        &gdp(&[("2020", 100.0), ("2021", 150.0)]),
    );
}

#[test]
fn revision_limit() {
    let e = exp("{check: revision_limit, pct_changed: 5, pct_removed: 1}");
    let ok = check_revision(&e, "A", 100, 5, 1).expect("result");
    assert!(ok.passed, "{}", ok.detail);
    let bad = check_revision(&e, "A", 100, 6, 0).expect("result");
    assert!(!bad.passed && bad.is_error());
    assert!(check_revision(&e, "A", 0, 6, 0).expect("r").skipped);
    assert_eq!(revision_limits(&[e], (0.01, 0.01)), (0.05, 0.01));
}

#[test]
fn no_future() {
    pass("{check: no_future}", &gdp(&[("2025", 1.0)]));
    fail("{check: no_future}", &gdp(&[("2027", 1.0)]));
    let mut f = gdp(&[("2027", 1.0)]);
    f[0].basis = Basis::Forecast;
    pass("{check: no_future}", &f);
}

#[test]
fn period_continuity() {
    pass(
        "{check: period_continuity}",
        &gdp(&[("2019", 1.0), ("2020", 1.0), ("2021", 1.0)]),
    );
    fail(
        "{check: period_continuity}",
        &gdp(&[("2019", 1.0), ("2021", 1.0)]),
    );
    // seasons alternate Maha (Sep) → Yala (Apr next year) → Maha
    let cal = Calendars {
        fiscal_start_month: 1,
        seasons: vec![("Maha".into(), 9, 1), ("Yala".into(), 4, 1)],
    };
    let maha = NaiveDate::from_ymd_opt(2020, 9, 1).expect("d");
    assert_eq!(
        next_start(Freq::S, maha, &cal),
        NaiveDate::from_ymd_opt(2021, 4, 1)
    );
}

fn feature(key: &str, geojson: &str) -> FeatureIn {
    FeatureIn {
        key: key.into(),
        kind: "x".into(),
        name_en: None,
        name_si: None,
        name_ta: None,
        geometry: geojson.into(),
        rep: [80.0, 7.0],
        attrs: serde_json::Map::new(),
        flags: lanka_core::Flags::EMPTY,
        entity_id: None,
        artifact: 0,
        prov: None,
    }
}

#[test]
fn geometry() {
    let (_, p) = registry();
    let check = |e: &str, rows: &[FeatureIn]| {
        let r = check_geometry(&[exp(e)], &feature_items(rows), &p, "A");
        r[0].passed
    };
    let colombo = feature("a", r#"{"type":"Point","coordinates":[79.85,6.93]}"#);
    let london = feature("b", r#"{"type":"Point","coordinates":[-0.12,51.5]}"#);
    let open_ring = feature(
        "c",
        r#"{"type":"Polygon","coordinates":[[[80,7],[80.1,7],[80.1,7.1]]]}"#,
    );
    assert!(check(
        "{check: geometry, valid: true, within_bbox: true}",
        std::slice::from_ref(&colombo)
    ));
    assert!(!check(
        "{check: geometry, within_bbox: true}",
        &[colombo.clone(), london]
    ));
    assert!(!check("{check: geometry, valid: true}", &[open_ring]));
    // row_count is evaluated on features too
    assert!(check(
        "{check: row_count, min: 1, max: 1}",
        std::slice::from_ref(&colombo)
    ));
    assert!(!check(
        "{check: row_count, min: 2}",
        std::slice::from_ref(&colombo)
    ));
    // nesting and overlap over admin-unit-like items
    let sq = |x0: f64, y0: f64, d: f64| {
        format!(
            r#"{{"type":"Polygon","coordinates":[[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}],[{x0},{y0}]]]}}"#,
            x1 = x0 + d,
            y1 = y0 + d
        )
    };
    let (parent, inside, outside, overlap) = (
        sq(80.0, 7.0, 1.0),
        sq(80.1, 7.1, 0.2),
        sq(81.5, 7.1, 0.2),
        sq(80.15, 7.15, 0.2),
    );
    fn items<'a>(parent: &'a str, inside: &'a str, child: &'a str) -> Vec<GeomItem<'a>> {
        vec![
            GeomItem {
                key: "P",
                level: Some(1),
                parent: None,
                geojson: parent,
            },
            GeomItem {
                key: "C1",
                level: Some(2),
                parent: Some("P"),
                geojson: inside,
            },
            GeomItem {
                key: "C2",
                level: Some(2),
                parent: Some("P"),
                geojson: child,
            },
        ]
    }
    let g = |e: &str, child: &str| {
        check_geometry(&[exp(e)], &items(&parent, &inside, child), &p, "A")[0].passed
    };
    let far = sq(80.6, 7.6, 0.2);
    assert!(g("{check: geometry, nesting_tol: 0}", &far));
    assert!(!g("{check: geometry, nesting_tol: 0}", &outside));
    assert!(g("{check: geometry, overlap_tol: 0}", &far));
    assert!(!g("{check: geometry, overlap_tol: 0}", &overlap));
}

#[test]
fn connector_and_dataset_expectations_merge() {
    let (reg, _) = registry();
    // every connector's and dataset's expectations parse (registry load checks)
    for c in reg.connectors.keys() {
        let _ = c;
    }
    let all: usize = reg
        .datasets
        .keys()
        .map(|d| for_dataset(&reg, d).expect("parses").len())
        .sum();
    assert!(all > 0, "the registry declares expectations");
}
