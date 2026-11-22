//! WP-07 golden tests of the tabular drivers on real government files kept
//! as fixtures (`tests/fixtures/<source>/`), through `lanka connector test`'s
//! engine: extract → Parquet round trip → standardise → expectations.
//! No network, no database (the district lookup is built here).

use std::path::PathBuf;

use lanka_core::Profile;
use lanka_ingest::Registry;
use lanka_ingest::connector::{Lookup, UnitRef};
use lanka_ingest::connector_test::{TestReport, run};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn registry() -> (Registry, Profile) {
    let reg = Registry::load(root().join("registry")).expect("registry");
    let p = Profile::load(Profile::discover(&root().join("registry")).expect("profile"))
        .expect("profile loads");
    (reg, p)
}

/// COD-AB districts (code, name) under their provinces.
const DISTRICTS: &[(&str, &str)] = &[
    ("LK11", "Colombo District"),
    ("LK12", "Gampaha District"),
    ("LK13", "Kalutara District"),
    ("LK21", "Kandy District"),
    ("LK22", "Matale District"),
    ("LK23", "Nuwara Eliya District"),
    ("LK31", "Galle District"),
    ("LK32", "Matara District"),
    ("LK33", "Hambantota District"),
    ("LK41", "Jaffna District"),
    ("LK42", "Mannar District"),
    ("LK43", "Vavuniya District"),
    ("LK44", "Mullaitivu District"),
    ("LK45", "Kilinochchi District"),
    ("LK51", "Batticaloa District"),
    ("LK52", "Ampara District"),
    ("LK53", "Trincomalee District"),
    ("LK61", "Kurunegala District"),
    ("LK62", "Puttalam District"),
    ("LK71", "Anuradhapura District"),
    ("LK72", "Polonnaruwa District"),
    ("LK81", "Badulla District"),
    ("LK82", "Monaragala District"),
    ("LK91", "Ratnapura District"),
    ("LK92", "Kegalle District"),
];

fn lookup() -> Lookup {
    let mut units = vec![UnitRef {
        code: "LK".into(),
        level: 0,
        name_en: "Sri Lanka".into(),
        parent: None,
    }];
    for (c, n) in DISTRICTS {
        units.push(UnitRef {
            code: (*c).into(),
            level: 2,
            name_en: (*n).into(),
            parent: Some(c[..3].to_owned()),
        });
    }
    Lookup {
        units,
        coverage: None,
    }
}

async fn test(source: &str, reg: &Registry, p: &Profile) -> anyhow::Result<TestReport> {
    let dir = root()
        .join("engine/crates/lanka-ingest/tests/fixtures")
        .join(source);
    let work = std::env::temp_dir().join(format!("lanka-golden-{source}-{}", std::process::id()));
    run(reg, p, source, Some(&dir), &work, &lookup(), usize::MAX).await
}

fn find<'a>(rows: &'a [Value], f: &[(&str, &str)]) -> &'a Value {
    rows.iter()
        .find(|r| f.iter().all(|(k, v)| r[*k].as_str() == Some(*v)))
        .unwrap_or_else(|| panic!("no row {f:?}"))
}

#[tokio::test]
async fn dcs_agriculture_html_and_xlsx() {
    let (reg, p) = registry();
    let r = test("dcs-agriculture", &reg, &p).await.expect("runs");
    assert_eq!(r.driver, "html_table");
    let idx = r
        .outputs
        .iter()
        .find(|o| o.dataset == "dcs-agri-production-index")
        .expect("index");
    let paddy = r
        .outputs
        .iter()
        .find(|o| o.dataset == "dcs-paddy-statistics")
        .expect("paddy");
    // 11 groups × 12 years from the HTML table (commented-out cells are not read)
    assert_eq!(idx.rows, 132);
    let tea = find(&idx.first, &[("dims", "group=tea"), ("period", "2014")]);
    assert_eq!(tea["value"], 109.03);
    assert_eq!(tea["verbatim"], "109.03");
    assert_eq!(tea["locator"]["table"], 0);
    let overall = find(&idx.first, &[("dims", "group=total"), ("period", "2025")]);
    assert!(overall["value"].as_f64().is_some());
    // 8 workbooks, districts × seasons (Total columns only), reporting areas skipped
    // Maha production by district only for 1983/84-2007/08 (mislabelled elsewhere upstream)
    assert_eq!(paddy.rows, 9002);
    let is = |r: &Value, i: &str, g: &str, p: &str| {
        r["indicator"] == i && r["geo"] == g && r["period"] == p
    };
    assert!(
        !paddy
            .first
            .iter()
            .any(|r| is(r, "agri.paddy_production_t", "LK11", "1978-Maha"))
    );
    let national = find(
        &paddy.first,
        &[
            ("indicator", "agri.paddy_production_t"),
            ("geo", "LK"),
            ("period", "1978-Maha"),
        ],
    );
    assert_eq!(national["value"], 1395.6);
    assert_eq!(national["locator"]["sheet"], "Maha Season");
    assert_eq!(national["locator"]["cell"], "C32");
    find(
        &paddy.first,
        &[
            ("indicator", "agri.paddy_production_t"),
            ("geo", "LK11"),
            ("period", "1983-Maha"),
        ],
    );
    // the printed header "198" is the 1980 Yala season (spec rename)
    find(
        &paddy.first,
        &[
            ("indicator", "agri.paddy_production_t"),
            ("geo", "LK11"),
            ("period", "1980-Yala"),
        ],
    );
    // "2000/2021" in the extent and yield sheets is the 2020/21 Maha season
    find(
        &paddy.first,
        &[
            ("indicator", "agri.paddy_sown_ha"),
            ("geo", "LK"),
            ("period", "2020-Maha"),
        ],
    );
    // alias-matched district spellings (KILLINOCHCHI, MULLATIVU)
    find(
        &paddy.first,
        &[
            ("indicator", "agri.paddy_harvested_ha"),
            ("geo", "LK45"),
            ("period", "2010-Maha"),
        ],
    );
    assert!(r.warnings.iter().any(|w| w.contains("UDAWALAWE")));
    assert!(!r.failed(), "no error-severity expectation fails");
}

#[tokio::test]
async fn a_printed_header_typo_without_a_rename_is_a_duplicate_key() {
    let (mut reg, p) = registry();
    let spec = reg.connectors.get_mut("dcs-agriculture").expect("spec");
    for t in spec.json["standardise"]["tables"]
        .as_array_mut()
        .expect("tables")
    {
        if t["id"] == "sown_maha"
            && let Some(m) = t.as_object_mut()
        {
            m.remove("rename");
        }
    }
    let err = test("dcs-agriculture", &reg, &p)
        .await
        .expect_err("duplicate");
    let msg = format!("{err:#}");
    assert!(msg.contains("duplicate key agri.paddy_sown_ha"), "{msg}");
    assert!(msg.contains("2000-Maha"), "{msg}");
}

#[tokio::test]
async fn cbsl_exchange_rates_xlsx() {
    let (reg, p) = registry();
    let r = test("cbsl-exchange-rates", &reg, &p).await.expect("runs");
    let o = &r.outputs[0];
    assert_eq!((o.dataset.as_str(), o.rows), ("cbsl-exchange-rates", 9600));
    let usd = find(
        &o.first,
        &[
            ("indicator", "fx.rate_monthly_avg"),
            ("dims", "currency=USD"),
            ("period", "1986-01"),
        ],
    );
    assert_eq!(usd["value"], 27.5052);
    assert_eq!(usd["locator"]["cell"], "E9");
    // SDR is ISO 4217 XDR; the year is filled down; "Annual Averages" rows are not months
    find(
        &o.first,
        &[
            ("indicator", "fx.rate_end_month"),
            ("dims", "currency=XDR"),
            ("period", "1995-02"),
        ],
    );
    assert!(
        !o.first
            .iter()
            .any(|r| r["verbatim"].as_str().is_some_and(|v| v.contains("Annual")))
    );
    assert!(!r.failed());
    let names: Vec<&str> = o.expectations.iter().map(|e| e.check).collect();
    assert!(names.contains(&"period_continuity") && names.contains(&"max_step_change"));
}

/// basis of the observation for `year` in the first output.
fn basis_of(r: &TestReport, year: &str) -> String {
    r.outputs[0]
        .first
        .iter()
        .find(|x| x["period"].as_str() == Some(year))
        .and_then(|x| x["basis"].as_str())
        .unwrap_or_else(|| panic!("no row for {year}"))
        .to_owned()
}

#[tokio::test]
async fn imf_projections_follow_the_edition_year() {
    let (reg, p) = registry();
    // Real DataMapper responses, trimmed: NGDP_RPCH for LKA 2020-2031, WEO October 2026.
    let r = test("imf-data", &reg, &p).await.expect("runs");
    assert_eq!(
        (r.outputs[0].dataset.as_str(), r.outputs[0].rows),
        ("imf-weo-lka", 12)
    );
    assert_eq!(basis_of(&r, "2025"), "reported");
    assert_eq!(basis_of(&r, "2026"), "forecast");
    assert_eq!(basis_of(&r, "2031"), "forecast");
    // The next edition moves the boundary without a registry edit.
    let dir = std::env::temp_dir().join(format!("lanka-golden-imf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let src = root().join("engine/crates/lanka-ingest/tests/fixtures/imf-data");
    std::fs::copy(src.join("NGDP_RPCH.json"), dir.join("NGDP_RPCH.json")).expect("copy");
    let cat = std::fs::read_to_string(src.join("indicators.json"))
        .expect("catalogue")
        .replace("(October 2026)", "(April 2027)");
    std::fs::write(dir.join("indicators.json"), cat).expect("write");
    let work = dir.join("work");
    let r = run(
        &reg,
        &p,
        "imf-data",
        Some(&dir),
        &work,
        &lookup(),
        usize::MAX,
    )
    .await
    .expect("runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(basis_of(&r, "2026"), "reported");
    assert_eq!(basis_of(&r, "2027"), "forecast");
}

/// PUCSL SAIDI/SAIFI: two stacked blocks per sheet (`header.occurrence`),
/// unlabelled month columns (`header.min_width`), months by position.
#[tokio::test]
async fn pucsl_stacked_blocks_xlsx() {
    let (reg, p) = registry();
    let r = test("pucsl", &reg, &p).await.expect("runs");
    let o = &r.outputs[0];
    assert_eq!(
        (o.dataset.as_str(), o.rows),
        ("pucsl-supply-reliability", 5314)
    );
    // 2024-Q4 CEB sheets split each month by cause: Jaffna, October, planned
    let jaffna = find(
        &o.first,
        &[
            ("indicator", "ener.saidi"),
            (
                "dims",
                "cause=planned;distribution_area=dl1_jaffna;licensee=DL1",
            ),
            ("period", "2024-10"),
        ],
    );
    assert_eq!(jaffna["value"], 84.1);
    // the SAIFI block is the anchor's second occurrence
    find(
        &o.first,
        &[
            ("indicator", "ener.saifi"),
            (
                "dims",
                "cause=total;distribution_area=dl1_jaffna_east;licensee=DL1",
            ),
            ("period", "2024-07"),
        ],
    );
    // LECO 2025-Q1 prints SAIDI in hours: × 60, verbatim kept
    let kotte = find(
        &o.first,
        &[
            ("indicator", "ener.saidi"),
            (
                "dims",
                "cause=total;distribution_area=dl5_kotte;licensee=DL5",
            ),
            ("period", "2025-01"),
        ],
    );
    assert!((kotte["value"].as_f64().expect("number") - 57.6).abs() < 1e-9);
    assert_eq!(kotte["verbatim"], "0.96");
    assert!(!r.failed());
}

#[test]
fn every_connector_spec_builds_its_driver() {
    let (reg, _) = registry();
    for (id, spec) in &reg.connectors {
        lanka_ingest::drivers::driver_for(spec).unwrap_or_else(|e| panic!("{id}: {e:#}"));
    }
}
