//! End-to-end tests against a real PostGIS server, on throwaway databases.
//!
//! Gated: run only when `LANKA_TEST_DATABASE_URL` points at a server where
//! the user may CREATE DATABASE (e.g. the local infra/v2 compose stack):
//!
//!   LANKA_TEST_DATABASE_URL=postgres://lanka:lanka-local-dev@127.0.0.1:55432/postgres cargo test
//!
//! `full_cycle_against_postgis` uses a synthetic country profile (`zz`, root
//! code `ZZ`) — the engine must not depend on any real country — and covers:
//! migrations, profile + registry sync, run tracking, artifact archive,
//! admin units (geo.units/unit_geoms), class-A publish + release,
//! no-change detection, range-versioned revisions (gate → review → approve
//! closes the old rows), incremental appends (only new rows written),
//! reject, the immutability trigger, validation (geo, codelists, basis ×
//! class) and the query core (area, series, map, reverse, provenance).
//!
//! `range_versions_property` (proptest) stages random version sequences and
//! checks the range-version invariants after every release: the current rows
//! equal the delivered content, every released seq is reconstructible
//! (`vfrom <= k <= vto`), the incremental dsv equals a full recomputation,
//! changed values are `revised`, the same content always has the same dsv,
//! and returning to earlier content (A → B → A) publishes a new version.
//!
//! `staging_scales_and_rehashes_only_the_delta` stages ~1 M synthetic rows
//! (< 10 s in release builds), appends 1,000 and checks only those 1,000 are
//! hashed and written.
//!
//! `migration_0002_preserves_0001_rows` loads a 0001-shaped database and
//! asserts every `(indicator, geo, period, dims, value)` survives 0002.

use std::path::PathBuf;

use bytes::Bytes;
use lanka_core::query::{self, MapQuery, SeriesQuery};
use lanka_core::{Basis, Catalog, Flags, Method, Profile, Status};
use lanka_ingest::Registry;
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, ObsRow};
use lanka_ingest::geo_load;
use lanka_ingest::publish::{
    self, PublishResult, StageInput, multiset_at, publish_observations, reject,
};
use lanka_ingest::run::{RunStatus, RunTracker};
use lanka_ingest::train::run_train;
use lanka_store::{BlobStore, FsStore, Ns};
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lanka-it-{name}-{}", std::process::id()))
}

const PROFILE: &str = r#"
id: zz
name: { en: Testland }
iso3166: { alpha2: ZZ, alpha3: ZZZ }
site: { base_url: https://example.org }
languages: [en]
timezone: UTC
currency: XXX
grid: { lat_max: 10.0, lon_min: 79.4, step_deg: 0.001, rows: 4200, cols: 2700 }
admin:
  scheme: admin
  pcode_pattern: "^ZZ[0-9]*$"
  root_code: ZZ
  levels:
    - { level: 0, digits: 0, name: { en: Country } }
    - { level: 1, digits: 1, name: { en: Region } }
    - { level: 2, digits: 2, name: { en: District } }
    - { level: 3, digits: 4, name: { en: Division } }
  reference_boundary_set: it-set
  boundary_dataset: it-geo
metric_crs: EPSG:4326
calendars: { fiscal: { start_month: 1 }, agri: { start_month: 1 } }
schemes: [{ id: ed, title: Electoral district }]
"#;

fn fixture_registry() -> (PathBuf, Profile) {
    // One directory per call: the tests run in parallel.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = tmp(&format!(
        "registry-{}",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["schema", "sources", "datasets", "codelists"] {
        std::fs::create_dir_all(dir.join(d)).expect("mkdir");
    }
    for f in [
        "source.schema.json",
        "dataset.schema.json",
        "codelist.schema.json",
        "units.schema.json",
        "profile.schema.json",
    ] {
        std::fs::copy(
            repo_root().join("registry/schema").join(f),
            dir.join("schema").join(f),
        )
        .expect("copy");
    }
    std::fs::write(
        dir.join("units.yaml"),
        "units:\n  - { code: '{thing}', label: things, kind: count }\n",
    )
    .expect("write");
    std::fs::copy(
        repo_root().join("registry/codelists/sex.yaml"),
        dir.join("codelists/sex.yaml"),
    )
    .expect("copy codelist");
    std::fs::write(
        dir.join("sources/it-src.yaml"),
        "id: it-src\npublisher: Test\ntitle: Integration source\nhomepage: https://example.org\n\
         access: {method: api, endpoint: https://example.org, auth: none}\n\
         licence: {id: cc-by-4.0, status: open}\ncadence: {frequency: annual}\n\
         geo: {levels: [national, adm2], join: pcode}\nthemes: [ECON]\ndatasets: [it-a, it-geo, it-big]\n\
         priority: P3\neffort: S\nstatus: active\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-a.yaml"),
        "id: it-a\ntitle: Integration dataset\ndescription: Fixture dataset for tests.\nsource_ids: [it-src]\n\
         indicators:\n  - {id: it.count, label: Count, unit: '{thing}', measure_type: count, aggregation: sum,\n\
         \x20     class: official, dims: [sex], geo_levels: [national, adm2], period_type: year}\n\
         time_coverage: {from: 2020}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-big.yaml"),
        "id: it-big\ntitle: Large fixture\ndescription: Daily fixture series for scale tests.\nsource_ids: [it-src]\n\
         indicators:\n  - {id: it.daily, label: Daily, unit: '{thing}', measure_type: count, aggregation: sum,\n\
         \x20     class: official, dims: [sex], geo_levels: [adm3], period_type: day}\n\
         time_coverage: {from: 2020}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-geo.yaml"),
        "id: it-geo\ntitle: Fixture boundaries\ndescription: Fixture admin units.\nsource_ids: [it-src]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [Three squares.]\npublication_class: A\n\
         kind: boundaries\n",
    )
    .expect("write");
    let p = dir.join("profile.yaml");
    std::fs::write(&p, PROFILE).expect("write profile");
    let profile = Profile::load(&p).expect("fixture profile");
    Registry::validate_profile(&dir, &profile).expect("fixture profile matches schema");
    (dir, profile)
}

fn square(x0: f64, y0: f64, d: f64) -> Box<serde_json::value::RawValue> {
    let g = json!({"type": "Polygon", "coordinates": [[[x0, y0], [x0 + d, y0], [x0 + d, y0 + d], [x0, y0 + d], [x0, y0]]]});
    serde_json::value::to_raw_value(&g).expect("raw geometry")
}

fn unit(
    pcode: &str,
    level: i16,
    parent: Option<&str>,
    geom: Box<serde_json::value::RawValue>,
) -> AdminUnitIn {
    AdminUnitIn {
        pcode: pcode.into(),
        level,
        name_en: format!("Unit {pcode}"),
        name_si: None,
        name_ta: None,
        parent_pcode: parent.map(Into::into),
        geometry: geom,
        artifact: 0,
    }
}

fn obs(geo: &str, period: &str, sex: &str, v: f64) -> ObsRow {
    ObsRow {
        indicator_id: "it.count".into(),
        geo_code: geo.into(),
        period: period.into(),
        dims: format!("sex={sex}"),
        value: Some(v),
        lo: None,
        hi: None,
        basis: Basis::Reported,
        status: Status::Normal,
        flags: Flags::EMPTY,
        artifact: 0,
        prov: None,
        locator: json!({"sheet": "T1", "cell": format!("C{}", v as i64)}),
        verbatim: Some(format!("{v}")),
        method: Method::SheetCell,
        confidence: Some(1.0),
    }
}

/// Archives `body` and records it as an artifact of `run_id`; returns
/// (artifact id, sha256).
async fn fetched(
    pool: &PgPool,
    store: &FsStore,
    run_id: i64,
    body: &'static [u8],
) -> (i64, [u8; 32]) {
    let (blob, _) = store
        .put_cas(Ns::Raw, Bytes::from_static(body))
        .await
        .expect("archive");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, archive_path)
         VALUES ($1, 'it-src', 'https://example.org/t1', now(), $2, $3)
         ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256 RETURNING id",
    )
    .bind(blob.sha256.as_slice())
    .bind(blob.bytes as i64)
    .bind(blob.rel_path())
    .fetch_one(pool)
    .await
    .expect("artifact");
    sqlx::query("INSERT INTO ops.run_artifacts VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(run_id)
        .bind(id)
        .execute(pool)
        .await
        .expect("link");
    (id, blob.sha256)
}

/// Runs a release train when `r` was approved, so the assertions below see
/// the version released (staging itself never releases, ADR 0012).
async fn release_if_approved(pool: &PgPool, mut r: PublishResult) -> anyhow::Result<PublishResult> {
    if r.outcome == "approved" {
        let t = run_train(pool, "test", None, None).await?;
        anyhow::ensure!(t.outcome == "released", "train released nothing");
        r.outcome = "published";
        r.data_version = t.data_version;
    }
    Ok(r)
}

/// `publish_observations` with the fixture's single artifact, then the train.
#[allow(clippy::too_many_arguments)]
async fn stage(
    pool: &PgPool,
    reg: &Registry,
    profile: &Profile,
    run_id: i64,
    dataset_id: &str,
    vintage: Option<&str>,
    rows: &[ObsRow],
    artifact: i64,
) -> anyhow::Result<PublishResult> {
    let r = stage_only(
        pool, reg, profile, run_id, dataset_id, vintage, rows, artifact,
    )
    .await?;
    release_if_approved(pool, r).await
}

/// `publish_observations` without the train.
#[allow(clippy::too_many_arguments)]
async fn stage_only(
    pool: &PgPool,
    reg: &Registry,
    profile: &Profile,
    run_id: i64,
    dataset_id: &str,
    vintage: Option<&str>,
    rows: &[ObsRow],
    artifact: i64,
) -> anyhow::Result<PublishResult> {
    publish_observations(
        pool,
        reg,
        profile,
        StageInput {
            run_id,
            dataset_id,
            vintage,
            rows,
            artifact_ids: &[artifact],
            extract_ids: &[],
        },
    )
    .await
}

/// `geo_load::publish_admin_units`, then the train.
#[allow(clippy::too_many_arguments)]
async fn publish_admin_units(
    pool: &PgPool,
    profile: &Profile,
    run_id: i64,
    dataset_id: &str,
    set: &BoundarySet,
    units: &[AdminUnitIn],
    artifact_ids: &[i64],
    extract_ids: &[i64],
) -> anyhow::Result<PublishResult> {
    let r = geo_load::publish_admin_units(
        pool,
        profile,
        run_id,
        dataset_id,
        set,
        units,
        artifact_ids,
        extract_ids,
    )
    .await?;
    release_if_approved(pool, r).await
}

/// `publish::approve`, then the train.
async fn approve(
    pool: &PgPool,
    dsv: &str,
    by: &str,
    note: Option<&str>,
) -> anyhow::Result<PublishResult> {
    let r = publish::approve(pool, dsv, by, note).await?;
    anyhow::ensure!(r.outcome == "approved", "approve → {}", r.outcome);
    release_if_approved(pool, r).await
}

/// Creates a throwaway database, runs `f` against its URL, drops it.
async fn with_database<F, Fut>(tag: &str, f: F)
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_{tag}_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url)
        .await
        .expect("connect admin");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {db} WITH (FORCE)"
        ))))
        .await
        .expect("drop");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE {db}"
        ))))
        .await
        .expect("create");
    let base = admin_url
        .rsplit_once('/')
        .map_or(admin_url.as_str(), |(b, _)| b);
    let url = format!("{base}/{db}");
    let result = tokio::spawn(f(url)).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {db} WITH (FORCE)"
        ))))
        .await
        .expect("drop test database");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn full_cycle_against_postgis() {
    with_database("cycle", run_cycle).await;
}

#[allow(clippy::too_many_lines)]
async fn run_cycle(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture_registry();
    let reg = Registry::load(&dir).expect("fixture registry");
    let sync = reg.sync(&pool, &profile).await.expect("sync");
    assert_eq!((sync.sources, sync.datasets, sync.indicators), (1, 3, 2));
    assert_eq!((sync.units, sync.codelists), (1, 1));
    let store = FsStore::new(tmp("store"));

    // ── run 1: boundaries + first data version (class A → approved → train) ──
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    assert!(
        RunTracker::start(&pool, "it-src", "test").await.is_err(),
        "second concurrent run must be refused"
    );
    let f = fetched(&pool, &store, run.run_id, b"fixture table v1").await;
    let units = vec![
        unit("ZZ", 0, None, square(80.0, 7.0, 1.0)),
        unit("ZZ1", 1, Some("ZZ"), square(80.0, 7.0, 0.5)),
        unit("ZZ11", 2, Some("ZZ1"), square(80.0, 7.0, 0.25)),
    ];
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    let geo = publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[f.0],
        &[],
    )
    .await
    .expect("admin units");
    assert_eq!(geo.outcome, "published");
    let again = publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[f.0],
        &[],
    )
    .await
    .expect("admin units again");
    assert_eq!(
        (again.outcome, again.dsv.as_str()),
        ("unchanged", geo.dsv.as_str())
    );
    let lk_units = vec![unit("LK", 0, None, square(80.0, 7.0, 1.0))];
    assert!(
        publish_admin_units(
            &pool,
            &profile,
            run.run_id,
            "it-geo",
            &set,
            &lk_units,
            &[f.0],
            &[]
        )
        .await
        .is_err(),
        "codes outside the profile's pattern are refused"
    );
    let view: i64 = sqlx::query_scalar("SELECT count(*) FROM geo.admin_units")
        .fetch_one(&pool)
        .await
        .expect("view");
    assert_eq!(
        view, 3,
        "geo.admin_units view over geo.units + reference geometry"
    );

    let mut rows = Vec::new();
    for y in 2020..2030 {
        for (g, base) in [("ZZ", 1000.0), ("ZZ11", 100.0)] {
            rows.push(obs(g, &y.to_string(), "female", base + f64::from(y - 2020)));
            rows.push(obs(
                g,
                &y.to_string(),
                "male",
                base + 50.0 + f64::from(y - 2020),
            ));
        }
    }
    let r1 = stage(
        &pool,
        &reg,
        &profile,
        run.run_id,
        "it-a",
        Some("v1"),
        &rows,
        f.0,
    )
    .await
    .expect("publish");
    assert_eq!((r1.outcome, r1.rows_written), ("published", 40));
    assert!(r1.data_version.is_some());
    // identical content → no new version, nothing written
    let again = stage(
        &pool,
        &reg,
        &profile,
        run.run_id,
        "it-a",
        Some("v1"),
        &rows,
        f.0,
    )
    .await
    .expect("republish");
    assert_eq!(
        (again.outcome, again.dsv.as_str(), again.rows_written),
        ("unchanged", r1.dsv.as_str(), 0)
    );
    run.finish(RunStatus::Succeeded, "ok")
        .await
        .expect("finish");

    // validation: unknown geo, unknown code, estimate in an official indicator
    for (bad_row, why) in [
        (obs("ZZ99", "2020", "female", 1.0), "geo.units"),
        (obs("ZZ", "2031", "x", 1.0), "codelist"),
        (
            ObsRow {
                basis: Basis::Estimated,
                ..obs("ZZ", "2031", "female", 1.0)
            },
            "not allowed",
        ),
        (obs("ZZ", "2031-01", "female", 1.0), "period"),
    ] {
        let mut bad = rows.clone();
        bad.push(bad_row);
        let err = stage(&pool, &reg, &profile, 0, "it-a", None, &bad, f.0)
            .await
            .expect_err("must fail");
        assert!(format!("{err:#}").contains(why), "{why}: {err:#}");
    }

    // ── query core over the published release ──
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(cat.data_version, r1.data_version);
    assert_eq!(cat.boundary_dataset(), Some("it-geo"));
    let zz11 = query::parse_pcode(&cat, "zz11").expect("pcode");
    let (area, touched) = query::area_profile(&pool, &cat, &zz11).await.expect("area");
    assert_eq!(
        area.ancestors
            .iter()
            .map(|a| a.pcode.as_str())
            .collect::<Vec<_>>(),
        ["ZZ", "ZZ1"]
    );
    assert_eq!(area.indicators.len(), 1);
    assert_eq!(area.indicators[0].period, "2029");
    assert_eq!(area.indicators[0].values.len(), 2);
    assert_eq!(area.indicators[0].values[0].basis, "reported");
    assert_eq!(touched, ["it-a"]);
    assert!(area.area_km2.is_some_and(|a| a > 0.0));

    let s = query::series(
        &pool,
        &cat,
        "it.count",
        &SeriesQuery {
            geos: vec!["ZZ".into(), "ZZ11".into()],
            from: Some("2025".into()),
            dims: Some("sex:male".into()),
            ..Default::default()
        },
    )
    .await
    .expect("series");
    assert_eq!(s.series.len(), 2);
    assert_eq!(s.series[0].points.len(), 5);
    assert_eq!(s.series[1].points[0].value, Some(155.0));

    let m = query::map(
        &pool,
        &cat,
        "it.count",
        &MapQuery {
            level: 2,
            ..Default::default()
        },
    )
    .await
    .expect("map");
    assert_eq!(
        (m.pcodes.as_slice(), m.period.as_str(), m.dims.as_str()),
        (["ZZ11".to_owned()].as_slice(), "2029", "sex=female")
    );
    let m1 = query::map(
        &pool,
        &cat,
        "it.count",
        &MapQuery {
            level: 2,
            parent: Some("ZZ1".into()),
            dims: Some("sex=male".into()),
            period: Some("2020".into()),
        },
    )
    .await
    .expect("map with parent");
    assert_eq!(m1.values, vec![Some(150.0)]);

    let rev = query::reverse(&pool, &cat, 7.1, 80.1)
        .await
        .expect("reverse");
    assert_eq!(rev.unit.pcode, "ZZ11");
    assert!(matches!(
        query::reverse(&pool, &cat, 4.0, 80.0).await,
        Err(lanka_core::CoreError::NotInCoverage)
    ));

    // ── bulk units and unit geometry ──
    use lanka_core::geo_units::{self, Simplify, UnitsQuery};
    let l2 = geo_units::units(
        &pool,
        &cat,
        &UnitsQuery {
            level: 2,
            ..Default::default()
        },
    )
    .await
    .expect("units level 2");
    assert_eq!(
        (l2.total, l2.units.len(), l2.next_cursor.as_deref()),
        (1, 1, None)
    );
    let u = &l2.units[0];
    assert_eq!(
        (u.pcode.as_str(), u.parent.as_deref()),
        ("ZZ11", Some("ZZ1"))
    );
    assert!(u.area_km2.is_some_and(|a| a > 0.0));
    let c = u.centroid.expect("centroid");
    assert!(
        (80.0..80.25).contains(&c[0]) && (7.0..7.25).contains(&c[1]),
        "[lon, lat]: {c:?}"
    );
    let below = geo_units::units(
        &pool,
        &cat,
        &UnitsQuery {
            level: 2,
            parent: Some("ZZ".into()),
            limit: Some(1),
            ..Default::default()
        },
    )
    .await
    .expect("units below the root");
    assert_eq!(below.units.len(), 1);
    let after = geo_units::units(
        &pool,
        &cat,
        &UnitsQuery {
            level: 2,
            cursor: Some("ZZ11".into()),
            ..Default::default()
        },
    )
    .await
    .expect("past the last page");
    assert!(after.units.is_empty());
    assert!(matches!(
        geo_units::units(
            &pool,
            &cat,
            &UnitsQuery {
                level: 2,
                parent: Some("ZZ2".into()),
                ..Default::default()
            }
        )
        .await,
        Err(lanka_core::CoreError::NotFound(_))
    ));
    let full = geo_units::unit_geometry(&pool, &cat, "ZZ11", Simplify::Full)
        .await
        .expect("geometry");
    assert_eq!((full.kind.as_str(), full.id.as_str()), ("Feature", "ZZ11"));
    assert_eq!(full.geometry["type"], "MultiPolygon");
    let coarse = geo_units::unit_geometry(&pool, &cat, "ZZ11", Simplify::Tolerance(0.01))
        .await
        .expect("simplified geometry");
    assert!((coarse.properties.simplify_tolerance - 0.01).abs() < 1e-12);
    for x in full.geometry["coordinates"][0][0].as_array().expect("ring") {
        for v in x.as_array().expect("pair") {
            let t = v.to_string();
            assert!(t.split('.').nth(1).is_none_or(|d| d.len() <= 6), "{t}");
        }
    }

    let p = query::provenance(&pool, &cat, s.series[0].points[0].obs_id)
        .await
        .expect("provenance");
    assert_eq!(p.method.as_deref(), Some("sheet_cell"));
    assert_eq!(p.review.as_deref(), Some("auto_accepted"));
    assert_eq!(p.dsv, r1.dsv);
    assert_eq!(p.artifact_sha256.as_deref(), Some(hex_of(&f.1).as_str()));

    // ── run 2: revise >1% of history → class A gate demotes to review ──
    let run2 = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run 2");
    let run2_id = run2.run_id;
    let f2 = fetched(&pool, &store, run2.run_id, b"fixture table v2").await;
    let revised: Vec<ObsRow> = rows
        .iter()
        .cloned()
        .map(|mut r| {
            r.value = r.value.map(|v| v + 1.0);
            r
        })
        .collect();
    let r2 = stage(
        &pool,
        &reg,
        &profile,
        run2.run_id,
        "it-a",
        Some("v2"),
        &revised,
        f2.0,
    )
    .await
    .expect("revise");
    assert_eq!(r2.outcome, "in_review");
    assert_eq!(r2.diff.as_ref().map(|d| d.rows_changed), Some(40));
    run2.finish(RunStatus::NeedsReview, "gate")
        .await
        .expect("finish 2");
    // still serving v1: staged rows are invisible
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(
        cat.datasets["it-a"].current.as_ref().map(|c| c.dsv.clone()),
        Some(r1.dsv.clone())
    );
    let s1 = query::series(
        &pool,
        &cat,
        "it.count",
        &SeriesQuery {
            geos: vec!["ZZ11".into()],
            dims: Some("sex=male".into()),
            ..Default::default()
        },
    )
    .await
    .expect("series during review");
    assert_eq!(s1.series[0].points[0].value, Some(150.0));

    let ok = approve(&pool, &r2.dsv, "tester", Some("checked"))
        .await
        .expect("approve");
    assert_eq!(ok.outcome, "published");
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(
        cat.datasets["it-a"].current.as_ref().map(|c| c.dsv.clone()),
        Some(r2.dsv.clone())
    );
    let (closed, open, current): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE vto = 1), count(*) FILTER (WHERE vto = 2147483647),
                (SELECT count(*) FROM data.current_observations_coded WHERE dataset_id = 'it-a')
         FROM data.observations o JOIN catalog.datasets d ON d.key = o.dataset_key WHERE d.id = 'it-a'",
    )
    .fetch_one(&pool)
    .await
    .expect("ranges");
    assert_eq!(
        (closed, open, current),
        (40, 40, 40),
        "v1 rows closed at seq 1"
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM data.dataset_versions WHERE dsv = $1")
            .bind(&r1.dsv)
            .fetch_one(&pool)
            .await
            .expect("status");
    assert_eq!(status, "superseded");
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT status FROM data.current_observations_coded WHERE dataset_id = 'it-a'",
    )
    .fetch_all(&pool)
    .await
    .expect("statuses");
    assert_eq!(
        statuses,
        ["revised"],
        "changed values are stored as revised"
    );
    let p2 = query::provenance(&pool, &cat, p.obs_id)
        .await
        .expect("old provenance");
    assert_eq!(p2.closed_by_dsv.as_deref(), Some(r2.dsv.as_str()));

    // ── run 3: append one period → only the new rows are written ──
    let mut appended = revised.clone();
    for (g, v) in [("ZZ", 2000.0), ("ZZ11", 200.0)] {
        appended.push(obs(g, "2030", "female", v));
        appended.push(obs(g, "2030", "male", v + 1.0));
    }
    let r3 = stage(
        &pool,
        &reg,
        &profile,
        run2_id,
        "it-a",
        Some("v3"),
        &appended,
        f2.0,
    )
    .await
    .expect("append");
    assert_eq!((r3.outcome, r3.rows_written), ("published", 4));
    assert_eq!(
        r3.diff
            .as_ref()
            .map(|d| (d.rows_added, d.rows_changed, d.rows_removed)),
        Some((4, 0, 0))
    );

    // ── reject: a gate-tripping staging is discarded without trace ──
    let dropped: Vec<ObsRow> = appended.iter().take(10).cloned().collect();
    let r4 = stage(
        &pool,
        &reg,
        &profile,
        run2_id,
        "it-a",
        Some("v4"),
        &dropped,
        f2.0,
    )
    .await
    .expect("stage removal");
    assert_eq!(r4.outcome, "in_review");
    reject(&pool, &r4.dsv, "tester", "upstream truncated")
        .await
        .expect("reject");
    let (staged, closes): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM data.observations o JOIN data.dataset_versions v
                   ON v.dsv = $1 AND o.vfrom = v.seq JOIN catalog.datasets d ON d.key = o.dataset_key
                 WHERE d.id = 'it-a'),
                (SELECT count(*) FROM data.version_closes)",
    )
    .bind(&r4.dsv)
    .fetch_one(&pool)
    .await
    .expect("reject state");
    assert_eq!((staged, closes), (0, 0));

    // ── observations are immutable except vto ──
    let err = sqlx::query("UPDATE data.observations SET value = 0 WHERE obs_id = $1")
        .bind(p.obs_id)
        .execute(&pool)
        .await
        .expect_err("guard trigger");
    assert!(err.to_string().contains("immutable"), "{err}");

    // ── failure path: run closed as failed, an error event in the feed ──
    let run3 = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run 3");
    let run3_id = run3.run_id;
    run3.fail(&anyhow::anyhow!("upstream exploded"))
        .await
        .expect("fail");
    let (state, events): (String, i64) = sqlx::query_as(
        "SELECT last_status, (SELECT count(*) FROM ops.run_events WHERE run_id = $1 AND level = 'error')
         FROM ops.source_freshness WHERE source_id = 'it-src'",
    )
    .bind(run3_id)
    .fetch_one(&pool)
    .await
    .expect("freshness");
    assert_eq!((state.as_str(), events), ("failed", 1));
    let alerts: bool = sqlx::query_scalar("SELECT to_regclass('ops.alerts') IS NULL")
        .fetch_one(&pool)
        .await
        .expect("alerts");
    assert!(alerts, "ops.alerts is gone (ADR 0016)");
    pool.close().await;
}

fn hex_of(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ───────────────────────────── 0001 → 0002 parity ──────────────────────────

/// A 0001-shaped database: boundary set, three admin units, two datasets
/// with one published version each (and one superseded version), a release.
const LEGACY: &str = r#"
INSERT INTO catalog.licences VALUES ('cc-by-4.0', 'open', NULL, NULL);
INSERT INTO catalog.sources (id, publisher, title, homepage, access_method, endpoint, auth, licence_id,
  cadence, geo_levels, geo_join, themes, priority, effort, status, spec, spec_sha256)
VALUES ('it-src', 'Test', 'T', 'https://x', 'api', 'https://x', 'none', 'cc-by-4.0', 'annual',
  '{national}', 'pcode', '{ECON}', 'P3', 'S', 'active', '{}', '\x00');
INSERT INTO catalog.datasets (id, title, description, method, limits, publication_class, spec, spec_sha256)
VALUES ('it-a', 'A', 'Fixture A.', 'm', '{x}', 'A', '{}', '\x00'),
       ('it-b', 'B', 'Fixture B.', 'm', '{x}', 'B', '{}', '\x00');
INSERT INTO catalog.indicators (id, dataset_id, label, unit, dims, geo_levels, period_type)
VALUES ('it.count', 'it-a', 'Count', 'persons', '{sex}', '{national}', 'year'),
       ('it.snap', 'it-b', 'Snapshot', 'percent', '{}', '{national}', 'snapshot');
INSERT INTO geo.boundary_sets (id, title) VALUES ('it-set', 'Fixture');
INSERT INTO geo.admin_units (pcode, level, name_en, parent_pcode, boundary_set, geom, area_km2, geom_sha256)
VALUES ('LK', 0, 'Root', NULL, 'it-set', ST_Multi(ST_MakeEnvelope(80, 7, 81, 8, 4326)), 1, '\x01'),
       ('LK1', 1, 'One', 'LK', 'it-set', ST_Multi(ST_MakeEnvelope(80, 7, 80.5, 7.5, 4326)), 1, '\x02'),
       ('LK11', 2, 'Eleven', 'LK1', 'it-set', ST_Multi(ST_MakeEnvelope(80, 7, 80.2, 7.2, 4326)), 1, '\x03');
INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, archive_path)
VALUES (sha256('a'), 'it-src', 'https://x/a', now(), 1, 'a');
INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status, publication_class, created_at)
VALUES ('it-a', 'dsv_old0000000000a1', '\x00', 2, 'superseded', 'A', now() - interval '2 days'),
       ('it-a', 'dsv_old0000000000a2', '\x00', 3, 'published', 'A', now() - interval '1 day'),
       ('it-b', 'dsv_old0000000000b1', '\x00', 2, 'published', 'B', now());
SELECT data.ensure_obs_partition(id) FROM catalog.datasets;
INSERT INTO data.observations (dataset_id, version_id, indicator_id, geo_code, geo_level, period, period_start,
  dims, value, status, artifact_id, locator, verbatim, method, confidence, review)
SELECT 'it-a', v.id, 'it.count', g, l, p, make_date(p::int, 1, 1), d, val, s::data.obs_status,
       (SELECT id FROM ops.artifacts), '{"cell": "C1"}', val::text, 'api', 1, 'auto_accepted'
FROM data.dataset_versions v,
     (VALUES ('dsv_old0000000000a1', 'LK', 0, '2020', 'sex=female', 1.5::float8, 'reported'),
             ('dsv_old0000000000a1', 'LK', 0, '2021', 'sex=female', 2.5, 'reported'),
             ('dsv_old0000000000a2', 'LK', 0, '2020', 'sex=female', 1.5, 'reported'),
             ('dsv_old0000000000a2', 'LK', 0, '2021', 'sex=female', 2.75, 'revised'),
             ('dsv_old0000000000a2', 'LK1', 1, '2021', 'sex=male', NULL, 'not_available')) x(dsv, g, l, p, d, val, s)
WHERE v.dsv = x.dsv;
INSERT INTO data.observations (dataset_id, version_id, indicator_id, geo_code, geo_level, period, period_start,
  dims, value, status, artifact_id, locator, verbatim, method, confidence, review)
SELECT 'it-b', v.id, 'it.snap', g, l, '2024', '2024-01-01', '', val, s::data.obs_status,
       (SELECT id FROM ops.artifacts), '{}', NULL, m::data.extraction_method, 1, 'approved'
FROM data.dataset_versions v,
     (VALUES ('LK', 0, 93.28::float8, 'derived', 'derived'), ('LK11', 2, 94.6, 'estimate', 'sheet_cell')) x(g, l, val, s, m)
WHERE v.dsv = 'dsv_old0000000000b1';
INSERT INTO data.releases (data_version) VALUES ('20260101.1');
INSERT INTO data.release_datasets SELECT '20260101.1', dataset_id, id FROM data.dataset_versions
  WHERE status = 'published';
"#;

#[tokio::test(flavor = "multi_thread")]
async fn migration_0002_preserves_0001_rows() {
    with_database("mig", run_migration).await;
}

/// (indicator, geo, period, dims, value, basis, status)
type Coded = (String, String, String, String, Option<f64>, String, String);

async fn run_migration(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::MIGRATOR
        .run_to(1, &pool)
        .await
        .expect("migrate to 0001");
    sqlx::raw_sql(LEGACY)
        .execute(&pool)
        .await
        .expect("load 0001 data");
    type Row = (String, String, String, String, Option<f64>, String);
    let before: Vec<Row> = sqlx::query_as(
        "SELECT indicator_id, geo_code, period, dims, value, status::text FROM data.current_observations
         ORDER BY 1, 2, 3, 4",
    )
    .fetch_all(&pool)
    .await
    .expect("before");
    assert_eq!(before.len(), 5);

    lanka_core::db::migrate(&pool).await.expect("migrate 0002");
    let after: Vec<Coded> = sqlx::query_as(
        "SELECT indicator_id, geo_code, period, dims, value, basis, status
         FROM data.current_observations_coded ORDER BY 1, 2, 3, 4",
    )
    .fetch_all(&pool)
    .await
    .expect("after");
    let key = |a: &str, b: &str, c: &str, d: &str, v: Option<f64>| {
        (a.to_owned(), b.to_owned(), c.to_owned(), d.to_owned(), v)
    };
    assert_eq!(
        before
            .iter()
            .map(|b| key(&b.0, &b.1, &b.2, &b.3, b.4))
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|a| key(&a.0, &a.1, &a.2, &a.3, a.4))
            .collect::<Vec<_>>(),
        "(indicator, geo, period, dims, value) identical before/after"
    );
    // basis/status split per 02 §4
    for (b, a) in before.iter().zip(&after) {
        let expect = lanka_core::basis::from_legacy_status(&b.5).expect("legacy status");
        assert_eq!(
            (a.5.as_str(), a.6.as_str()),
            (expect.0.as_str(), expect.1.as_str()),
            "{}",
            b.5
        );
    }
    // superseded version rows kept as closed history; provenance moved over
    let (hist, prov, units, view, seqs): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM data.observations WHERE vto <> 2147483647),
                (SELECT count(*) FROM data.obs_provenance),
                (SELECT count(*) FROM geo.units WHERE cardinality(anc_keys) = level),
                (SELECT count(*) FROM geo.admin_units),
                (SELECT count(*) FROM data.dataset_versions WHERE legacy_dsv = dsv AND seq > 0)",
    )
    .fetch_one(&pool)
    .await
    .expect("state");
    assert_eq!((hist, prov, units, view, seqs), (2, 7, 3, 3, 3));
    pool.close().await;
}

// ───────────────────────────── range-version property ──────────────────────

/// Fixture keys: 3 geos × 6 years × 2 sexes.
fn key_space() -> Vec<(&'static str, String, &'static str)> {
    let mut k = Vec::new();
    for g in ["ZZ", "ZZ1", "ZZ11"] {
        for y in 2020..2026 {
            for s in ["female", "male"] {
                k.push((g, y.to_string(), s));
            }
        }
    }
    k
}

type Content = std::collections::BTreeMap<(String, String, String), f64>;

fn content_of(version: &[(bool, u8)]) -> (Vec<ObsRow>, Content) {
    let mut rows = Vec::new();
    let mut c = Content::new();
    for ((g, y, s), (present, v)) in key_space().into_iter().zip(version) {
        if *present {
            let v = f64::from(*v) + 1.0;
            rows.push(obs(g, &y, s, v));
            c.insert((g.to_owned(), y, format!("sex={s}")), v);
        }
    }
    (rows, c)
}

async fn rows_at(pool: &PgPool, seq: i32) -> Content {
    let rows: Vec<(String, String, String, Option<f64>)> = sqlx::query_as(
        "SELECT u.code, data.period_code(o.freq, o.time_start), ds.dims, o.value
         FROM data.observations o JOIN catalog.datasets d ON d.key = o.dataset_key
         JOIN geo.units u ON u.key = o.geo_key JOIN catalog.dim_sets ds ON ds.key = o.dims_key
         WHERE d.id = 'it-a' AND o.vfrom <= $1 AND o.vto >= $1",
    )
    .bind(seq)
    .fetch_all(pool)
    .await
    .expect("rows at seq");
    rows.into_iter()
        .map(|(g, p, d, v)| ((g, p, d), v.unwrap_or(f64::NAN)))
        .collect()
}

/// Removes every version of `it-a` (between property cases).
async fn reset_dataset(pool: &PgPool) {
    for q in [
        "DELETE FROM data.changelog WHERE dataset_id = 'it-a'",
        "DELETE FROM data.release_datasets WHERE dataset_id = 'it-a'",
        "UPDATE ops.trains SET data_version = NULL",
        "DELETE FROM data.releases r WHERE NOT EXISTS (SELECT 1 FROM data.release_datasets x WHERE x.data_version = r.data_version)",
        "DELETE FROM data.obs_provenance p USING catalog.datasets d WHERE d.id = 'it-a' AND p.dataset_key = d.key",
        "DELETE FROM data.observations o USING catalog.datasets d WHERE d.id = 'it-a' AND o.dataset_key = d.key",
        "UPDATE data.dataset_versions SET supersedes_id = NULL WHERE dataset_id = 'it-a'",
        "DELETE FROM data.dataset_versions WHERE dataset_id = 'it-a'",
        "UPDATE catalog.datasets SET obs_seq = 0 WHERE id = 'it-a'",
    ] {
        sqlx::query(q).execute(pool).await.expect(q);
    }
}

#[allow(clippy::too_many_lines)]
async fn property_case(
    pool: &PgPool,
    reg: &Registry,
    profile: &Profile,
    run_id: i64,
    artifact: i64,
    versions: &[Vec<(bool, u8)>],
) {
    reset_dataset(pool).await;
    let mut released: Vec<(i32, Content)> = Vec::new();
    let mut prev: Option<Content> = None;
    // dsv of every content seen so far: the same content must get the same
    // dsv whatever the path (A → B → A included).
    let mut seen: Vec<(Content, String)> = Vec::new();
    for v in versions {
        let (rows, content) = content_of(v);
        if rows.is_empty() {
            continue;
        }
        let mut r = stage(pool, reg, profile, run_id, "it-a", None, &rows, artifact)
            .await
            .expect("stage");
        if r.outcome == "in_review" {
            r = approve(pool, &r.dsv, "prop", None).await.expect("approve");
        }
        let (seq, stored, recomputed, n): (i32, Vec<u8>, Option<Vec<u8>>, i64) = {
            let mut conn = pool.acquire().await.expect("conn");
            let (seq, stored, n, key): (i32, Vec<u8>, i64, i32) = sqlx::query_as(
                "SELECT c.seq, v.multiset_hash, v.row_count, c.dataset_key FROM data.current_versions c
                 JOIN data.dataset_versions v ON v.id = c.version_id WHERE c.dataset_id = 'it-a'",
            )
            .fetch_one(&mut *conn)
            .await
            .expect("current");
            let (m, cnt) = multiset_at(&mut conn, profile, key, seq)
                .await
                .expect("recompute");
            assert_eq!(
                cnt,
                u64::try_from(n).expect("n"),
                "row count matches the version"
            );
            (seq, stored, Some(m.0.to_vec()), n)
        };
        assert_eq!(
            Some(stored),
            recomputed,
            "incremental multiset equals a full recomputation"
        );
        assert_eq!(n, i64::try_from(content.len()).expect("len"));
        assert_eq!(
            rows_at(pool, seq).await,
            content,
            "current rows = delivered content"
        );
        let earlier = seen
            .iter()
            .find(|(c, _)| *c == content)
            .map(|(_, d)| d.clone());
        if let Some(d) = &earlier {
            assert_eq!(&r.dsv, d, "same content, same dsv (path-independent)");
        } else {
            seen.push((content.clone(), r.dsv.clone()));
        }
        if let Some(p) = &prev {
            if *p == content {
                assert_eq!(r.outcome, "unchanged", "same content is unchanged");
                assert_eq!(r.rows_hashed, 0, "unchanged staging hashes nothing");
            } else if earlier.is_some() {
                // A → B → A: a new version reinstates A, it is not `unchanged`.
                assert_eq!(
                    r.outcome, "published",
                    "returning to earlier content publishes"
                );
                let rows_with_dsv: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM data.dataset_versions WHERE dsv = $1 AND status IN ('published', 'superseded')",
                )
                .bind(&r.dsv)
                .fetch_one(pool)
                .await
                .expect("versions by dsv");
                assert!(
                    rows_with_dsv >= 2,
                    "the reinstating version is a new version row"
                );
            }
            let changed: Vec<(String, String, String, String)> = sqlx::query_as(
                "SELECT geo_code, period, dims, status FROM data.current_observations_coded
                 WHERE dataset_id = 'it-a'",
            )
            .fetch_all(pool)
            .await
            .expect("statuses");
            for (g, per, d, st) in changed {
                let k = (g, per, d);
                if p.get(&k).is_some_and(|old| content.get(&k) != Some(old)) {
                    assert_eq!(st, "revised", "{k:?} changed value must be revised");
                }
            }
        } else {
            let statuses: Vec<String> = sqlx::query_scalar(
                "SELECT DISTINCT status FROM data.current_observations_coded WHERE dataset_id = 'it-a'",
            )
            .fetch_all(pool)
            .await
            .expect("statuses");
            assert_eq!(
                statuses,
                ["normal"],
                "a first version has nothing to revise"
            );
        }
        let open_dups: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM (SELECT 1 FROM data.observations o JOIN catalog.datasets d ON d.key = o.dataset_key
               WHERE d.id = 'it-a' AND o.vto = 2147483647
               GROUP BY indicator_key, geo_key, time_start, freq, dims_key HAVING count(*) > 1) x",
        )
        .fetch_one(pool)
        .await
        .expect("dups");
        assert_eq!(open_dups, 0, "at most one open row per key after release");
        if released.last().is_none_or(|(s, _)| *s != seq) {
            released.push((seq, content.clone()));
        }
        prev = Some(content);
    }
    // Every released seq stays reconstructible (as_of).
    for (seq, content) in &released {
        assert_eq!(&rows_at(pool, *seq).await, content, "as_of seq {seq}");
    }
    // And so does every release: the version it pins holds that content.
    let pinned: Vec<i32> = sqlx::query_scalar(
        "SELECT v.seq FROM data.release_datasets rd JOIN data.releases r USING (data_version)
         JOIN data.dataset_versions v ON v.id = rd.version_id
         WHERE rd.dataset_id = 'it-a' ORDER BY r.seq",
    )
    .fetch_all(pool)
    .await
    .expect("release history");
    for seq in pinned {
        let want = released
            .iter()
            .rev()
            .find(|(s, _)| *s <= seq)
            .map(|(_, c)| c.clone())
            .expect("released content");
        assert_eq!(rows_at(pool, seq).await, want, "release pinning seq {seq}");
    }
}

#[test]
fn range_versions_property() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};
    if std::env::var("LANKA_TEST_DATABASE_URL").is_err() {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    }
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(with_database("prop", |url| async move {
        let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
            .await
            .expect("pool");
        lanka_core::db::migrate(&pool).await.expect("migrate");
        let (dir, profile) = fixture_registry();
        let reg = Registry::load(&dir).expect("registry");
        reg.sync(&pool, &profile).await.expect("sync");
        let store = FsStore::new(tmp("prop-store"));
        let run = RunTracker::start(&pool, "it-src", "test")
            .await
            .expect("run");
        let (aid, _) = fetched(&pool, &store, run.run_id, b"prop").await;
        let units = vec![
            unit("ZZ", 0, None, square(80.0, 7.0, 1.0)),
            unit("ZZ1", 1, Some("ZZ"), square(80.0, 7.0, 0.5)),
            unit("ZZ11", 2, Some("ZZ1"), square(80.0, 7.0, 0.25)),
        ];
        let set = BoundarySet {
            id: "it-set".into(),
            title: "Fixture".into(),
            vintage: None,
            notes: None,
        };
        publish_admin_units(
            &pool,
            &profile,
            run.run_id,
            "it-geo",
            &set,
            &units,
            &[aid],
            &[],
        )
        .await
        .expect("units");
        let n = key_space().len();
        // Versions mutate a few keys at a time (so the class-A gate both
        // passes and trips), plus fully random versions.
        let version = prop::collection::vec((prop::bool::weighted(0.85), 0u8..3), n);
        // Random histories, plus histories drawn from a pool of 2–3 contents
        // so returning to earlier content (A → B → A, A → B → C → A) is
        // common.
        let random = prop::collection::vec(version.clone(), 1..7);
        let revisits = (
            prop::collection::vec(version, 2..4),
            prop::collection::vec(0usize..3, 3..8),
        )
            .prop_map(|(pool, picks)| {
                picks
                    .into_iter()
                    .map(|i| pool[i % pool.len()].clone())
                    .collect::<Vec<_>>()
            });
        let strategy = prop_oneof![random, revisits];
        let handle = tokio::runtime::Handle::current();
        let mut runner = TestRunner::new(Config {
            cases: 32,
            failure_persistence: None,
            ..Config::default()
        });
        let (pool2, reg2, profile2, run_id) =
            (pool.clone(), reg.clone(), profile.clone(), run.run_id);
        let result = tokio::task::spawn_blocking(move || {
            runner.run(&strategy, |versions| {
                handle.block_on(property_case(
                    &pool2, &reg2, &profile2, run_id, aid, &versions,
                ));
                Ok(())
            })
        })
        .await
        .expect("join");
        result.expect("range-version invariants hold");
        run.finish(RunStatus::Succeeded, "ok")
            .await
            .expect("finish");
        pool.close().await;
    }));
}

// ───────────────────────────── staging at scale ────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn staging_scales_and_rehashes_only_the_delta() {
    with_database("scale", run_scale).await;
}

#[allow(clippy::too_many_lines)]
async fn run_scale(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture_registry();
    let reg = Registry::load(&dir).expect("registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let store = FsStore::new(tmp("scale-store"));
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    let (aid, _) = fetched(&pool, &store, run.run_id, b"scale").await;
    // 1 + 9 + 81 + 4,050 units; the 4,050 divisions carry the series.
    let mut units = vec![unit("ZZ", 0, None, square(80.0, 7.0, 1.0))];
    let mut divisions = Vec::new();
    for a in 1..=9 {
        units.push(unit(
            &format!("ZZ{a}"),
            1,
            Some("ZZ"),
            square(80.0, 7.0, 0.1),
        ));
        for b in 1..=9 {
            let d = format!("ZZ{a}{b}");
            units.push(unit(
                &d,
                2,
                Some(&format!("ZZ{a}")),
                square(80.0, 7.0, 0.01),
            ));
            for c in 1..=50 {
                let code = format!("{d}{c:02}");
                units.push(unit(&code, 3, Some(&d), square(80.0, 7.0, 0.001)));
                divisions.push(code);
            }
        }
    }
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[aid],
        &[],
    )
    .await
    .expect("units");
    let day0 = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).expect("date");
    let row = |g: &str, d: i64, s: &str| {
        let date = day0 + chrono::Duration::days(d);
        ObsRow {
            indicator_id: "it.daily".into(),
            locator: json!({"row": d, "geo": g}),
            verbatim: None,
            ..obs(g, &date.format("%Y-%m-%d").to_string(), s, (d % 97) as f64)
        }
    };
    let days = 124;
    let mut rows = Vec::with_capacity(divisions.len() * days * 2);
    for g in &divisions {
        for d in 0..days {
            for s in ["female", "male"] {
                rows.push(row(g, i64::try_from(d).expect("d"), s));
            }
        }
    }
    let n = rows.len();
    assert!(n >= 1_000_000, "{n} rows");
    let t = std::time::Instant::now();
    let r1 = stage(
        &pool, &reg, &profile, run.run_id, "it-big", None, &rows, aid,
    )
    .await
    .expect("stage 1M");
    let t1 = t.elapsed();
    eprintln!("staged {n} rows in {t1:?} ({})", r1.outcome);
    assert_eq!(r1.outcome, "published");
    assert_eq!(r1.rows_written, i64::try_from(n).expect("n"));
    if !cfg!(debug_assertions) {
        assert!(
            t1.as_secs_f64() < 10.0,
            "staging {n} rows took {t1:?} (budget 10 s)"
        );
    }

    // Identical re-run: nothing hashed, nothing written.
    let t = std::time::Instant::now();
    let same = stage(
        &pool, &reg, &profile, run.run_id, "it-big", None, &rows, aid,
    )
    .await
    .expect("re-stage");
    eprintln!("unchanged re-stage in {:?}", t.elapsed());
    assert_eq!(
        (same.outcome, same.rows_hashed, same.rows_written),
        ("unchanged", 0, 0)
    );

    // Append one day for 500 divisions × 2 sexes = 1,000 rows.
    for g in divisions.iter().take(500) {
        for s in ["female", "male"] {
            rows.push(row(g, i64::try_from(days).expect("d"), s));
        }
    }
    let t = std::time::Instant::now();
    let r2 = stage(
        &pool, &reg, &profile, run.run_id, "it-big", None, &rows, aid,
    )
    .await
    .expect("append");
    eprintln!("appended 1,000 rows to {n} in {:?}", t.elapsed());
    assert_eq!(r2.outcome, "published");
    assert_eq!((r2.rows_hashed, r2.rows_written), (1000, 1000));
    assert_eq!(
        r2.diff
            .as_ref()
            .map(|d| (d.rows_added, d.rows_changed, d.rows_removed)),
        Some((1000, 0, 0))
    );
    // The incremental dsv equals a full recomputation from the database.
    let mut conn = pool.acquire().await.expect("conn");
    let (key, seq, stored): (i32, i32, Vec<u8>) = sqlx::query_as(
        "SELECT c.dataset_key, c.seq, v.multiset_hash FROM data.current_versions c
         JOIN data.dataset_versions v ON v.id = c.version_id WHERE c.dataset_id = 'it-big'",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("current");
    let (m, cnt) = multiset_at(&mut conn, &profile, key, seq)
        .await
        .expect("recompute");
    assert_eq!(
        (m.0.to_vec(), cnt),
        (stored, u64::try_from(n + 1000).expect("n"))
    );
    drop(conn);
    run.finish(RunStatus::Succeeded, "ok")
        .await
        .expect("finish");
    pool.close().await;
}

// ───────────────────────────── WP-03: operations ───────────────────────────

/// Roles, approval timing, the release train, the job queue (dedupe, claim,
/// crash → re-queue, retry backoff, cancellation), the scheduler tick, the
/// worker end to end, incidents and the status reads, the event feed order,
/// and re-staging after a rejection without `--force`.
#[tokio::test(flavor = "multi_thread")]
async fn operations_against_postgis() {
    with_database("ops", run_operations).await;
}

#[allow(clippy::too_many_lines)]
async fn run_operations(url: String) {
    use lanka_core::status;
    use lanka_ingest::jobs::{self, NewJob};
    use lanka_ingest::scheduler::{self, TickConfig};
    use lanka_ingest::worker::{self, WorkerConfig};

    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Worker)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture_registry();
    let reg = Registry::load(&dir).expect("fixture registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let store = FsStore::new(tmp("ops-store"));

    // ── roles: the API role cannot write, the worker cannot rewrite releases ──
    {
        let mut c = pool.acquire().await.expect("conn");
        for (role, sql, ok) in [
            ("lanka_api", "SELECT count(*) FROM data.observations", true),
            (
                "lanka_api",
                "SELECT count(*) FROM ops.source_freshness",
                true,
            ),
            (
                "lanka_api",
                "INSERT INTO ops.jobs (kind, dedupe_key, trigger) VALUES ('ingest', 'x', 'manual')",
                false,
            ),
            (
                "lanka_api",
                "UPDATE catalog.sources SET paused = true",
                false,
            ),
            ("lanka_api", "DELETE FROM data.releases", false),
            (
                "lanka_api",
                "SELECT data.ensure_obs_partition('it-a')",
                false,
            ),
            (
                "lanka_ingest",
                "INSERT INTO ops.jobs (kind, dedupe_key, trigger) VALUES ('ingest', 'x', 'manual')",
                true,
            ),
            (
                "lanka_ingest",
                "UPDATE data.releases SET notes = 'x'",
                false,
            ),
            ("lanka_ingest", "DELETE FROM data.releases", false),
            (
                "lanka_ingest",
                "INSERT INTO ops.run_events (level, stage, kind, message) VALUES ('info', 'job', 'x', 'forged')",
                false,
            ),
            ("lanka_ingest", "CREATE TABLE data.evil (x int)", false),
            (
                "lanka_ingest",
                "SELECT data.ensure_obs_partition('it-a')",
                true,
            ),
            (
                "lanka_recipe",
                "INSERT INTO ops.jobs (kind, dedupe_key, trigger) VALUES ('ingest', 'y', 'manual')",
                false,
            ),
        ] {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("BEGIN; SET LOCAL ROLE {role}")))
                .execute(&mut *c)
                .await
                .expect("set role");
            let r = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(&mut *c)
                .await;
            sqlx::raw_sql("ROLLBACK")
                .execute(&mut *c)
                .await
                .expect("rollback");
            assert_eq!(r.is_ok(), ok, "{role}: {sql}: {r:?}");
            if !ok {
                let e = r.expect_err("denied").to_string();
                assert!(e.contains("permission denied"), "{role}: {sql}: {e}");
            }
        }
    }

    // ── approval timing: approved versions wait for the train ──
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    let f = fetched(&pool, &store, run.run_id, b"ops fixture").await;
    let units = vec![
        unit("ZZ", 0, None, square(80.0, 7.0, 1.0)),
        unit("ZZ1", 1, Some("ZZ"), square(80.0, 7.0, 0.5)),
        unit("ZZ11", 2, Some("ZZ1"), square(80.0, 7.0, 0.25)),
    ];
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    let geo = geo_load::publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[f.0],
        &[],
    )
    .await
    .expect("units");
    assert_eq!(
        (geo.outcome, geo.data_version.as_deref()),
        ("approved", None)
    );
    let rows: Vec<ObsRow> = (2020..2025)
        .flat_map(|y| {
            [
                obs("ZZ", &y.to_string(), "female", f64::from(y)),
                obs("ZZ11", &y.to_string(), "male", f64::from(y) / 10.0),
            ]
        })
        .collect();
    let r1 = stage_only(
        &pool,
        &reg,
        &profile,
        run.run_id,
        "it-a",
        Some("v1"),
        &rows,
        f.0,
    )
    .await
    .expect("stage");
    assert_eq!((r1.outcome, r1.data_version.as_deref()), ("approved", None));
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(
        cat.data_version, None,
        "nothing is visible before the train"
    );
    let t1 = run_train(&pool, "test", None, None).await.expect("train");
    assert_eq!((t1.outcome, t1.versions.len()), ("released", 2));
    let dv1 = t1.data_version.clone().expect("dv");
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(cat.data_version.as_deref(), Some(dv1.as_str()));
    let area_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM serve.area_latest")
        .fetch_one(&pool)
        .await
        .expect("area_latest");
    assert_eq!(area_rows, 2, "latest row per (geo, indicator, dims)");
    let zz11 = query::parse_pcode(&cat, "ZZ11").expect("pcode");
    let (area, _) = query::area_profile(&pool, &cat, &zz11).await.expect("area");
    assert_eq!(area.indicators[0].period, "2024");
    assert_eq!(area.indicators[0].values[0].value, Some(202.4));
    let t2 = run_train(&pool, "test", None, None).await.expect("train 2");
    assert_eq!((t2.outcome, t2.data_version), ("no_change", None));
    run.finish(RunStatus::Succeeded, "ok")
        .await
        .expect("finish");

    // ── rejection invalidates change keys; the same content re-stages ──
    let run2 = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run 2");
    let run2_id = run2.run_id;
    run2.set_change_keys("test", Some([7; 32]), Some([8; 32]))
        .await
        .expect("keys");
    let truncated: Vec<ObsRow> = rows.iter().take(2).cloned().collect();
    let r2 = stage_only(
        &pool,
        &reg,
        &profile,
        run2.run_id,
        "it-a",
        Some("v2"),
        &truncated,
        f.0,
    )
    .await
    .expect("stage truncated");
    assert_eq!(
        r2.outcome, "in_review",
        "removing 80% of rows trips the gate"
    );
    run2.finish(RunStatus::NeedsReview, "gate")
        .await
        .expect("finish 2");
    let keys = lanka_ingest::pipeline::last_change_keys(&pool, "it-src")
        .await
        .expect("keys");
    assert_eq!(keys.0.as_deref(), Some([7u8; 32].as_slice()));
    reject(&pool, &r2.dsv, "tester", "upstream truncated")
        .await
        .expect("reject");
    let keys = lanka_ingest::pipeline::last_change_keys(&pool, "it-src")
        .await
        .expect("keys");
    assert_eq!(
        keys,
        (None, None),
        "a rejection invalidates earlier change keys"
    );
    let r3 = stage_only(
        &pool,
        &reg,
        &profile,
        run2_id,
        "it-a",
        Some("v2"),
        &truncated,
        f.0,
    )
    .await
    .expect("re-stage");
    assert_eq!(
        (r3.outcome, r3.dsv.as_str()),
        ("in_review", r2.dsv.as_str())
    );
    let (open_reviews, decided): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE state = 'open'), count(*) FILTER (WHERE state = 'rejected') FROM ops.reviews",
    )
    .fetch_one(&pool)
    .await
    .expect("reviews");
    assert_eq!((open_reviews, decided), (1, 1));
    reject(&pool, &r3.dsv, "tester", "still truncated")
        .await
        .expect("reject again");

    // ── job queue ──
    let j = NewJob::ingest("it-src", "manual", json!({}));
    let (id, created) = jobs::enqueue(&pool, &j).await.expect("enqueue");
    assert!(created);
    assert_eq!(
        jobs::enqueue(&pool, &j).await.expect("again"),
        (id, false),
        "dedupe"
    );
    let kinds = ["ingest", "release"];
    let c = jobs::claim(&pool, "w1", "batch", 4, &kinds)
        .await
        .expect("claim")
        .expect("a job");
    assert_eq!(
        (c.job.id, c.job.attempts, c.job.state.as_str()),
        (id, 1, "running")
    );
    assert!(
        jobs::claim(&pool, "w2", "batch", 4, &kinds)
            .await
            .expect("claim")
            .is_none()
    );
    assert!(
        jobs::reap_abandoned(&pool).await.expect("reap").is_empty(),
        "lock held → alive"
    );
    // the worker dies mid-job: its session goes away with the lock
    c.lock.detach().close().await.expect("close session");
    let mut reaped = Vec::new();
    for _ in 0..50 {
        reaped = jobs::reap_abandoned(&pool).await.expect("reap");
        if !reaped.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(reaped, [id], "abandoned job is re-queued");
    let c = jobs::claim(&pool, "w1", "batch", 4, &kinds)
        .await
        .expect("claim")
        .expect("again");
    assert_eq!(c.job.attempts, 2);
    assert_eq!(
        jobs::fail(&pool, &c.job, "boom", true).await.expect("fail"),
        "queued"
    );
    c.release().await;
    let wait: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM run_after - now())::float8 FROM ops.jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("run_after");
    assert!(
        (3500.0..3700.0).contains(&wait),
        "second retry backs off 1 h, got {wait}"
    );
    assert!(
        jobs::claim(&pool, "w1", "batch", 4, &kinds)
            .await
            .expect("claim")
            .is_none(),
        "backing off"
    );
    assert_eq!(jobs::cancel(&pool, id).await.expect("cancel"), "cancelled");
    let mut rj = NewJob::release("manual:test", "manual", "test");
    rj.max_attempts = 1;
    let (rid, _) = jobs::enqueue(&pool, &rj).await.expect("enqueue release");
    let c = jobs::claim(&pool, "w1", "batch", 1, &kinds)
        .await
        .expect("claim")
        .expect("release job");
    assert_eq!(c.job.id, rid);
    assert_eq!(
        jobs::cancel(&pool, rid).await.expect("cancel running"),
        "cancel_requested"
    );
    assert!(
        jobs::heartbeat(&pool, rid).await.expect("heartbeat"),
        "heartbeat sees the cancel"
    );
    jobs::mark_cancelled(&pool, &c.job, "cancel requested")
        .await
        .expect("cancelled");
    c.release().await;
    let weights_full = {
        let mut big = NewJob::ingest("it-src", "manual", json!({}));
        big.weight = 4;
        let (bid, _) = jobs::enqueue(&pool, &big).await.expect("enqueue big");
        let none = jobs::claim(&pool, "w1", "batch", 3, &kinds)
            .await
            .expect("claim");
        jobs::cancel(&pool, bid).await.expect("cancel big");
        none.is_none()
    };
    assert!(weights_full, "a job heavier than the free slots waits");

    // ── scheduler: due sources and the daily train are enqueued once ──
    sqlx::query("UPDATE catalog.sources SET schedulable = true WHERE id = 'it-src'")
        .execute(&pool)
        .await
        .expect("schedulable");
    sqlx::query("UPDATE ops.ingestion_runs SET started_at = started_at - interval '30 days'")
        .execute(&pool)
        .await
        .expect("age runs");
    let cfg = TickConfig {
        train_at: chrono::NaiveTime::MIN,
        ..TickConfig::default()
    };
    let t = scheduler::tick(&pool, &cfg).await.expect("tick");
    assert_eq!(t.enqueued.len(), 1, "{t:?}");
    assert!(t.train_job.is_some());
    let t = scheduler::tick(&pool, &cfg).await.expect("tick 2");
    assert!(
        t.enqueued.is_empty() && t.train_job.is_none(),
        "idempotent: {t:?}"
    );
    // the source has no connector in the fixture: drop its ingest, keep the train
    sqlx::query(
        "UPDATE ops.jobs SET state = 'cancelled' WHERE kind = 'ingest' AND state = 'queued'",
    )
    .execute(&pool)
    .await
    .expect("drop ingest");

    // ── the worker runs the queued train and stops (--once) ──
    let wcfg = WorkerConfig {
        lane: "batch".into(),
        slots: 4,
        tick: std::time::Duration::from_secs(60),
        registry_dir: dir.clone(),
        profile_path: Some(dir.join("profile.yaml")),
        store_root: tmp("ops-store"),
        grace: std::time::Duration::from_secs(5),
        once: true,
        tick_cfg: cfg.clone(),
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        worker::run(pool.clone(), wcfg, std::future::pending()),
    )
    .await
    .expect("worker finished")
    .expect("worker ok");
    let (state, outcome): (String, String) = sqlx::query_as(
        "SELECT j.state, t.outcome FROM ops.jobs j JOIN ops.trains t ON t.job_id = j.id
         WHERE j.kind = 'release' AND j.dedupe_key LIKE 'release:daily:%'",
    )
    .fetch_one(&pool)
    .await
    .expect("train job");
    assert_eq!(
        (state.as_str(), outcome.as_str()),
        ("succeeded", "no_change")
    );

    // ── a forced failure shows in /v2/status within one tick ──
    for _ in 0..3 {
        RunTracker::start(&pool, "it-src", "test")
            .await
            .expect("run")
            .fail(&anyhow::anyhow!("upstream exploded"))
            .await
            .expect("fail");
    }
    let t = scheduler::tick(&pool, &cfg).await.expect("tick 3");
    assert_eq!(t.incidents_opened, 1);
    let s = status::summary(&pool).await.expect("summary");
    assert_eq!(s.sources.failing, 1);
    assert_ne!(s.overall, "ok");
    assert!(
        s.reasons.iter().any(|r| r.contains("source(s) failing")),
        "{:?}",
        s.reasons
    );
    assert_eq!(s.incidents_open, 1);
    assert_eq!(s.release.data_version.as_deref(), Some(dv1.as_str()));
    assert_eq!(
        s.release.last_train.as_ref().map(|t| t.outcome.as_str()),
        Some("no_change")
    );
    let failing = status::sources(&pool, Some("failing"), None)
        .await
        .expect("sources");
    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0].consecutive_failures, 3);
    let runs = status::runs(&pool, None, Some("it-src"), Some("failed"), None, 2)
        .await
        .expect("runs");
    assert_eq!(runs.runs.len(), 2);
    let older = status::runs(
        &pool,
        None,
        Some("it-src"),
        Some("failed"),
        runs.next_cursor,
        2,
    )
    .await
    .expect("page 2");
    assert!(older.runs.iter().all(|r| r.id < runs.runs[1].id));
    // recovery closes the incident at the next tick
    RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run")
        .finish(RunStatus::NoChange, "ok")
        .await
        .expect("finish");
    let t = scheduler::tick(&pool, &cfg).await.expect("tick 4");
    assert_eq!(t.incidents_closed, 1);

    // ── the event feed is strictly ordered and resumable ──
    let all = status::events(&pool, 0, None, None, 1000)
        .await
        .expect("events");
    assert!(all.len() > 20);
    assert!(
        all.windows(2).all(|w| w[0].id < w[1].id),
        "strictly increasing ids"
    );
    let mid = all[all.len() / 2].id;
    let tail = status::events(&pool, mid, None, None, 1000)
        .await
        .expect("tail");
    assert_eq!(
        tail.first().map(|e| e.id),
        all.iter().map(|e| e.id).find(|i| *i > mid)
    );
    let errors = status::events(&pool, 0, Some("error"), None, 1000)
        .await
        .expect("errors");
    assert!(errors.iter().all(|e| e.level == "error") && !errors.is_empty());
    for k in [
        "job_queued",
        "job_abandoned",
        "job_retry",
        "job_cancelled",
        "release_cut",
        "incident_opened",
        "review_rejected",
        "worker_started",
    ] {
        assert!(all.iter().any(|e| e.kind == k), "event kind {k} missing");
    }
    pool.close().await;
}

// ───────────────────────────── frames (WP-26) ──────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn frames_against_postgis() {
    with_database("frame", run_frames).await;
}

#[allow(clippy::too_many_lines)]
async fn run_frames(url: String) {
    use lanka_core::frame::{self, Coded, FramePin, FrameQuery};
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture_registry();
    let reg = Registry::load(&dir).expect("registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let store = FsStore::new(tmp("frame-store"));
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    let (aid, _) = fetched(&pool, &store, run.run_id, b"frame fixture").await;
    let units = vec![
        unit("ZZ", 0, None, square(80.0, 7.0, 1.0)),
        unit("ZZ1", 1, Some("ZZ"), square(80.0, 7.0, 0.5)),
        unit("ZZ11", 2, Some("ZZ1"), square(80.0, 7.0, 0.25)),
        unit("ZZ12", 2, Some("ZZ1"), square(80.25, 7.0, 0.25)),
        unit("ZZ2", 1, Some("ZZ"), square(80.5, 7.0, 0.5)),
        unit("ZZ21", 2, Some("ZZ2"), square(80.5, 7.0, 0.25)),
    ];
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[aid],
        &[],
    )
    .await
    .expect("units");
    // v1: ZZ11 and ZZ21 for 2020–2022 × female/male; ZZ12 has no data.
    let mut rows = Vec::new();
    for (g, base) in [("ZZ11", 100.0), ("ZZ21", 200.0), ("ZZ", 1000.0)] {
        for (k, y) in ["2020", "2021", "2022"].into_iter().enumerate() {
            for (j, s) in ["female", "male"].into_iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                rows.push(obs(g, y, s, base + k as f64 * 10.0 + j as f64 + 0.25));
            }
        }
    }
    let v1 = stage(&pool, &reg, &profile, run.run_id, "it-a", None, &rows, aid)
        .await
        .expect("v1");
    assert_eq!(v1.outcome, "published");
    let dv1 = v1.data_version.clone().expect("release 1");
    let cat = Catalog::load(&pool).await.expect("catalog");

    let q = |periods: &[&str], dims: Option<&str>| FrameQuery {
        indicators: vec!["it.count".into()],
        level: 2,
        periods: periods.iter().map(|p| (*p).to_owned()).collect(),
        dims: dims.map(str::to_owned),
        ..FrameQuery::default()
    };

    // 1. One column equals /v2/indicators/{id}/map; codes cover the support.
    let f = frame::frame(&pool, &cat, &q(&["2021"], Some("sex=female")))
        .await
        .expect("frame");
    assert_eq!(f.codes, ["ZZ11", "ZZ12", "ZZ21"]);
    assert_eq!(
        (f.support.n, f.support.level, f.support.unmatched),
        (3, 2, 0)
    );
    assert_eq!(f.columns.len(), 1);
    let m = query::map(
        &pool,
        &cat,
        "it.count",
        &MapQuery {
            level: 2,
            period: Some("2021".into()),
            parent: None,
            dims: Some("sex=female".into()),
        },
    )
    .await
    .expect("map");
    let c = &f.columns[0];
    assert_eq!(
        (c.period.as_str(), c.dims.get("sex").map(String::as_str)),
        ("2021", Some("female"))
    );
    for (code, v) in m.pcodes.iter().zip(&m.values) {
        let i = f
            .codes
            .iter()
            .position(|x| x == code)
            .expect("code in frame");
        assert_eq!(c.values[i].map(f64::to_bits), v.map(f64::to_bits), "{code}");
    }
    assert_eq!(c.values[1], None, "ZZ12 has no value");
    assert_eq!(c.basis, Some(Coded::Uniform("reported".into())));
    assert_eq!((c.lo.is_none(), c.flags.is_none()), (true, true));
    assert_eq!(
        (c.stats.n, c.stats.min, c.stats.max),
        (2, Some(110.25), Some(210.25))
    );

    // 2. Periods × dims, ordered (indicator, dims, period); parent filter.
    let f = frame::frame(
        &pool,
        &cat,
        &FrameQuery {
            from: Some("2020".into()),
            to: Some("2022".into()),
            parent: Some("ZZ1".into()),
            ..q(&[], None)
        },
    )
    .await
    .expect("range frame");
    assert_eq!(f.codes, ["ZZ11", "ZZ12"]);
    let heads: Vec<(String, String)> = f
        .columns
        .iter()
        .map(|c| {
            (
                c.dims.get("sex").cloned().unwrap_or_default(),
                c.period.clone(),
            )
        })
        .collect();
    assert_eq!(heads.len(), 6);
    assert_eq!(heads[0], ("female".into(), "2020".into()));
    assert_eq!(heads[5], ("male".into(), "2022".into()));
    assert_eq!(f.columns[5].values[0], Some(121.25));
    assert!(
        f.columns
            .iter()
            .enumerate()
            .all(|(i, c)| c.id == format!("c{i}"))
    );
    // latest = the newest period at the level; national level has its own codes.
    let f = frame::frame(
        &pool,
        &cat,
        &FrameQuery {
            level: 0,
            ..q(&["latest"], Some("sex=male"))
        },
    )
    .await
    .expect("national");
    assert_eq!(
        (
            f.codes.as_slice(),
            f.columns[0].period.as_str(),
            f.columns[0].values[0]
        ),
        (&["ZZ".to_owned()][..], "2022", Some(1021.25))
    );

    // 3. Pinning: v2 changes ZZ11; the current frame moves, release 1 and dsv v1 do not.
    let mut rows2 = rows.clone();
    rows2[0].value = Some(999.0);
    rows2[0].verbatim = Some("999".into());
    let mut v2 = stage(&pool, &reg, &profile, run.run_id, "it-a", None, &rows2, aid)
        .await
        .expect("v2");
    if v2.outcome == "in_review" {
        v2 = approve(&pool, &v2.dsv, "it", None)
            .await
            .expect("approve v2");
    }
    assert_eq!(v2.outcome, "published");
    let cat = Catalog::load(&pool).await.expect("catalog 2");
    let at = |pin: FramePin| FrameQuery {
        pin,
        ..q(&["2020"], Some("sex=female"))
    };
    let now = frame::frame(&pool, &cat, &at(FramePin::default()))
        .await
        .expect("current");
    assert_eq!(now.columns[0].values[0], Some(999.0));
    assert_eq!(
        now.columns[0].status,
        Some(Coded::Mixed(vec![Some(3), None, Some(1)])),
        "revised vs normal"
    );
    let then = frame::frame(
        &pool,
        &cat,
        &at(FramePin {
            release: Some(dv1.clone()),
            dsvs: vec![],
        }),
    )
    .await
    .expect("release 1");
    assert_eq!(then.columns[0].values[0], Some(100.25));
    assert_eq!(then.data_version.as_deref(), Some(dv1.as_str()));
    assert_eq!(then.versions[0].dsv, v1.dsv);
    let by_dsv = frame::frame(
        &pool,
        &cat,
        &at(FramePin {
            release: None,
            dsvs: vec![v1.dsv.clone()],
        }),
    )
    .await
    .expect("dsv v1");
    assert_eq!(by_dsv.columns[0].values[0], Some(100.25));
    // 4. A → B → A: re-delivering v1's content publishes it again (same dsv).
    let v3 = stage(&pool, &reg, &profile, run.run_id, "it-a", None, &rows, aid)
        .await
        .expect("v3");
    let v3 = if v3.outcome == "in_review" {
        approve(&pool, &v3.dsv, "it", None)
            .await
            .expect("approve v3")
    } else {
        v3
    };
    assert_eq!(
        (v3.outcome, v3.dsv.as_str()),
        ("published", v1.dsv.as_str())
    );
    let cat = Catalog::load(&pool).await.expect("catalog 3");
    let back = frame::frame(&pool, &cat, &at(FramePin::default()))
        .await
        .expect("current v3");
    assert_eq!(back.columns[0].values[0], Some(100.25));
    let by_dsv = frame::frame(
        &pool,
        &cat,
        &at(FramePin {
            release: None,
            dsvs: vec![v1.dsv.clone()],
        }),
    )
    .await
    .expect("dsv v1 after A→B→A");
    assert_eq!(by_dsv.columns[0].values[0], Some(100.25));

    // 5. Errors: unknown indicator / release / dsv → 404; bad parent, too many columns → 400.
    let err = |r: Result<frame::Frame, lanka_core::CoreError>| match r {
        Err(lanka_core::CoreError::NotFound(_)) => "404",
        Err(lanka_core::CoreError::BadRequest(_)) => "400",
        Err(_) => "other",
        Ok(_) => "ok",
    };
    assert_eq!(
        err(frame::frame(
            &pool,
            &cat,
            &FrameQuery {
                indicators: vec!["nope.x".into()],
                ..q(&[], None)
            }
        )
        .await),
        "404"
    );
    assert_eq!(
        err(frame::frame(
            &pool,
            &cat,
            &at(FramePin {
                release: Some("19990101.1".into()),
                dsvs: vec![]
            })
        )
        .await),
        "404"
    );
    assert_eq!(
        err(frame::frame(
            &pool,
            &cat,
            &at(FramePin {
                release: None,
                dsvs: vec!["dsv_0000000000000000".into()]
            })
        )
        .await),
        "404"
    );
    assert_eq!(
        err(frame::frame(
            &pool,
            &cat,
            &FrameQuery {
                parent: Some("ZZ11".into()),
                ..q(&[], None)
            }
        )
        .await),
        "400"
    );
    assert_eq!(
        err(frame::frame(&pool, &cat, &q(&["2021"], Some("sex=other"))).await),
        "400"
    );
    let many: Vec<String> = (1900..1990).map(|y| y.to_string()).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(err(frame::frame(&pool, &cat, &q(&many, None)).await), "400");
    run.finish(RunStatus::Succeeded, "ok")
        .await
        .expect("finish");
    pool.close().await;
}
