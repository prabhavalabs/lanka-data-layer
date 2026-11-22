//! WP-04 features on Postgres: range-versioned staging, the changed-only
//! refresh, release closes, the gate, reject, A → B → A, derived unit and
//! grid cell, coverage, immutability and the feature reads.
//!
//! Gated like `pg_integration.rs`:
//!   LANKA_TEST_DATABASE_URL=postgres://lanka:…@127.0.0.1:55432/postgres cargo test -p lanka-ingest --test pg_features

use std::path::PathBuf;

use lanka_core::features::{FeatureQuery, NearestQuery};
use lanka_core::{Catalog, Flags, Profile};
use lanka_ingest::Registry;
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, FeatureIn};
use lanka_ingest::features::{FeatureStageInput, load_coverage, publish_features};
use lanka_ingest::geo_load::publish_admin_units;
use lanka_ingest::publish::{self, PublishResult};
use lanka_ingest::run::RunTracker;
use lanka_ingest::train::run_train;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
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
  reference_boundary_set: ft-set
  boundary_dataset: ft-geo
metric_crs: EPSG:4326
calendars: { fiscal: { start_month: 1 }, agri: { start_month: 1 } }
schemes: []
"#;

fn fixture() -> (PathBuf, Profile) {
    // One directory per call: the tests run in parallel.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lanka-ft-registry-{}-{}",
        std::process::id(),
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
        .expect("copy schema");
    }
    std::fs::write(
        dir.join("units.yaml"),
        "units:\n  - { code: '{count}', label: count, kind: count }\n",
    )
    .expect("units");
    std::fs::write(
        dir.join("sources/ft-src.yaml"),
        "id: ft-src\npublisher: Test\ntitle: Feature source\nhomepage: https://example.org\n\
         access: {method: api, endpoint: https://example.org, auth: none}\n\
         licence: {id: odbl-1.0, status: attribution, attribution: (c) test contributors}\ncadence: {frequency: daily}\n\
         geo: {levels: [point], join: spatial}\nthemes: [TRAN]\ndatasets: [ft-geo, ft-feat]\n\
         priority: P3\neffort: S\nstatus: active\n",
    )
    .expect("source");
    std::fs::write(
        dir.join("datasets/ft-geo.yaml"),
        "id: ft-geo\ntitle: Fixture boundaries\ndescription: Fixture admin units.\nsource_ids: [ft-src]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [Two squares.]\npublication_class: A\n\
         kind: boundaries\n",
    )
    .expect("geo");
    std::fs::write(
        dir.join("datasets/ft-feat.yaml"),
        "id: ft-feat\ntitle: Fixture features\ndescription: Fixture points, lines and polygons.\nsource_ids: [ft-src]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [Not real.]\npublication_class: A\n\
         kind: features\nsearchable: true\n\
         fields:\n- {name: status, type: string, enum: [open, proposed]}\n- {name: lanes, type: integer, unit: '{count}'}\n\
         tiles: {minzoom: 6, maxzoom: 12, attrs: [status]}\n",
    )
    .expect("features dataset");
    let p = dir.join("profile.yaml");
    std::fs::write(&p, PROFILE).expect("profile");
    let profile = Profile::load(&p).expect("profile");
    (dir, profile)
}

fn square(x0: f64, y0: f64, w: f64, h: f64) -> Box<serde_json::value::RawValue> {
    serde_json::value::to_raw_value(&json!({"type": "Polygon",
        "coordinates": [[[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h], [x0, y0]]]}))
    .expect("raw")
}

fn unit(
    code: &str,
    level: i16,
    parent: Option<&str>,
    g: Box<serde_json::value::RawValue>,
) -> AdminUnitIn {
    AdminUnitIn {
        pcode: code.into(),
        level,
        name_en: format!("Unit {code}"),
        name_si: None,
        name_ta: None,
        parent_pcode: parent.map(Into::into),
        geometry: g,
        artifact: 0,
    }
}

/// Feature `i`: points west of 80.0 (unit ZZ11), lines/polygons east (ZZ12).
fn feat(i: u32, variant: u32) -> FeatureIn {
    let x = 79.6003 + f64::from(i % 100) * 0.008;
    let y = 6.1 + f64::from(i / 100) * 0.07 + f64::from(variant) * 1e-5;
    let (g, rep, kind) = match i % 3 {
        0 => (
            format!(r#"{{"type":"Point","coordinates":[{x:.6},{y:.6}]}}"#),
            [x, y],
            "point",
        ),
        1 => (
            format!(
                r#"{{"type":"LineString","coordinates":[[{x:.6},{y:.6}],[{:.6},{:.6}]]}}"#,
                x + 0.002,
                y + 0.002
            ),
            [x + 0.001, y + 0.001],
            "line",
        ),
        _ => (
            format!(
                r#"{{"type":"Polygon","coordinates":[[[{x:.6},{y:.6}],[{:.6},{y:.6}],[{:.6},{:.6}],[{x:.6},{:.6}],[{x:.6},{y:.6}]]]}}"#,
                x + 0.003,
                x + 0.003,
                y + 0.003,
                y + 0.003
            ),
            [x + 0.0015, y + 0.0015],
            "area",
        ),
    };
    let mut attrs = serde_json::Map::new();
    attrs.insert(
        "status".into(),
        json!(if i.is_multiple_of(7) {
            "proposed"
        } else {
            "open"
        }),
    );
    attrs.insert("lanes".into(), json!(i % 4 + 1));
    FeatureIn {
        key: format!("way/{i}"),
        kind: kind.into(),
        name_en: Some(format!("Feature {i}")),
        name_si: None,
        name_ta: None,
        geometry: g,
        rep: [
            lanka_ingest::geom::round6(rep[0]),
            lanka_ingest::geom::round6(rep[1]),
        ],
        attrs,
        flags: if i.is_multiple_of(11) {
            Flags::UNOFFICIAL_GEOM
        } else {
            Flags::EMPTY
        },
        entity_id: None,
        artifact: 0,
        prov: None,
    }
}

async fn stage(
    pool: &PgPool,
    reg: &Registry,
    profile: &Profile,
    rows: &[FeatureIn],
) -> anyhow::Result<PublishResult> {
    let cov = load_coverage(pool, profile).await?;
    let run_id = RunTracker::start(pool, "ft-src", "test").await?.run_id;
    publish_features(
        pool,
        reg,
        profile,
        FeatureStageInput {
            run_id,
            dataset_id: "ft-feat",
            vintage: Some("2026-11-20"),
            rows,
            artifact_ids: &[],
            extract_ids: &[],
            coverage: cov.as_deref(),
            grid_units: None,
        },
    )
    .await
}

async fn release(pool: &PgPool) {
    let t = run_train(pool, "test", None, None).await.expect("train");
    assert_eq!(t.outcome, "released");
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .expect(sql)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn features_range_versioned_against_postgis() {
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_feat_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url).await.expect("admin");
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
    let url = format!(
        "{}/{db}",
        admin_url
            .rsplit_once('/')
            .map_or(admin_url.as_str(), |(b, _)| b)
    );
    let result = tokio::spawn(async move { run(&url).await }).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {db} WITH (FORCE)"
        ))))
        .await
        .expect("drop");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[allow(clippy::too_many_lines)]
async fn run(url: &str) {
    let pool = lanka_core::db::connect(url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture();
    let reg = Registry::load(&dir).expect("registry");
    reg.sync(&pool, &profile).await.expect("sync");

    // Boundaries: the country is two 0.5° squares, 79.5–80.5 E, 6.0–7.0 N.
    let units = vec![
        unit("ZZ", 0, None, square(79.5, 6.0, 1.0, 1.0)),
        unit("ZZ1", 1, Some("ZZ"), square(79.5, 6.0, 1.0, 1.0)),
        unit("ZZ11", 2, Some("ZZ1"), square(79.5, 6.0, 0.5, 1.0)),
        unit("ZZ12", 2, Some("ZZ1"), square(80.0, 6.0, 0.5, 1.0)),
    ];
    let set = BoundarySet {
        id: "ft-set".into(),
        title: "Fixture".into(),
        vintage: Some("2026".into()),
        notes: None,
    };
    let run_id = RunTracker::start(&pool, "ft-src", "test")
        .await
        .expect("run")
        .run_id;
    publish_admin_units(&pool, &profile, run_id, "ft-geo", &set, &units, &[], &[])
        .await
        .expect("units");

    // ── v1: 1,000 features, plus 2 outside the country (dropped) ──
    let v1: Vec<FeatureIn> = (0..1000).map(|i| feat(i, 0)).collect();
    let mut far = feat(5000, 0);
    far.rep = [81.5, 8.5];
    far.geometry = r#"{"type":"Point","coordinates":[81.5,8.5]}"#.into();
    let mut off_grid = feat(5001, 0);
    off_grid.rep = [70.0, 5.0];
    off_grid.geometry = r#"{"type":"Point","coordinates":[70,5]}"#.into();
    let mut with_far = v1.clone();
    with_far.push(far);
    with_far.push(off_grid);
    let r1 = stage(&pool, &reg, &profile, &with_far).await.expect("v1");
    assert_eq!(r1.outcome, "approved", "{}", r1.note);
    assert_eq!((r1.rows, r1.rows_written), (1000, 1000));
    assert!(
        r1.note.contains("2 feature(s) outside the country"),
        "{}",
        r1.note
    );
    release(&pool).await;
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM geo.features WHERE unit_key IS NULL"
        )
        .await,
        0
    );
    // Unit by representative point: x < 80.0 → ZZ11.
    let wrong: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM geo.features f JOIN geo.units u ON u.key = f.unit_key
         WHERE u.code <> CASE WHEN ST_X(f.rep) < 80.0 THEN 'ZZ11' ELSE 'ZZ12' END",
    )
    .fetch_one(&pool)
    .await
    .expect("units");
    assert_eq!(wrong, 0);
    // Cell ids equal the canonical Rust grid.
    let cells: Vec<(f64, f64, i32)> =
        sqlx::query_as("SELECT ST_Y(rep), ST_X(rep), cell_id FROM geo.features")
            .fetch_all(&pool)
            .await
            .expect("cells");
    for (lat, lon, c) in &cells {
        assert_eq!(
            lanka_core::grid::cell_id(*lat, *lon).map(|v| v as i32),
            Some(*c)
        );
    }

    // ── identical delivery: nothing copied or written ──
    let same = stage(&pool, &reg, &profile, &with_far).await.expect("same");
    assert_eq!((same.outcome, same.rows_written), ("unchanged", 0));

    // ── the daily refresh: 9 changed, 5 removed, 3 added (under the 1 % gates) ──
    let mut v2 = v1.clone();
    for f in v2.iter_mut().take(9) {
        let i: u32 = f.key[4..].parse().expect("id");
        *f = feat(i, 1);
    }
    v2.retain(|f| {
        !matches!(
            f.key.as_str(),
            "way/990" | "way/991" | "way/992" | "way/993" | "way/994"
        )
    });
    v2.extend((1000..1003).map(|i| feat(i, 0)));
    let r2 = stage(&pool, &reg, &profile, &v2).await.expect("v2");
    assert_eq!(r2.outcome, "approved", "{}", r2.note);
    assert_eq!(
        r2.rows_written, 12,
        "only changed and new features are written"
    );
    let d = r2.diff.as_ref().expect("diff");
    assert_eq!(
        (d.rows_added, d.rows_changed, d.rows_removed, d.prev_rows),
        (3, 9, 5, 1000)
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM data.version_closes").await,
        14
    );
    // Before the train the release still serves v1.
    let cat = Catalog::load(&pool).await.expect("catalog");
    let page = lanka_core::features::features(
        &pool,
        &cat,
        "ft-feat",
        &FeatureQuery {
            limit: Some(2000),
            ..FeatureQuery::default()
        },
    )
    .await
    .expect("page");
    assert_eq!(page.number_returned, 1000);
    release(&pool).await;
    assert_eq!(
        count(&pool, "SELECT count(*) FROM data.version_closes").await,
        0
    );
    let cur: i64 = count(
        &pool,
        "SELECT count(*) FROM geo.features f JOIN data.current_versions c ON c.dataset_key = f.dataset_key
         WHERE f.vto = 2147483647 AND f.vfrom <= c.seq",
    )
    .await;
    assert_eq!(cur, 998);
    // Every released seq reconstructs: seq 1 had 1,000 rows, seq 2 has 998.
    for (seq, n) in [(1, 1000), (2, 998)] {
        let k: i64 =
            sqlx::query_scalar("SELECT count(*) FROM geo.features WHERE vfrom <= $1 AND vto >= $1")
                .bind(seq)
                .fetch_one(&pool)
                .await
                .expect("seq");
        assert_eq!(k, n, "seq {seq}");
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM geo.features").await,
        1012
    );

    // ── gate: 20 % changed → review; reject deletes the staged rows ──
    let mut v3 = v2.clone();
    for f in v3.iter_mut().take(200) {
        let i: u32 = f.key[4..].parse().expect("id");
        *f = feat(i, 2);
    }
    let r3 = stage(&pool, &reg, &profile, &v3).await.expect("v3");
    assert_eq!(r3.outcome, "in_review", "{}", r3.note);
    assert_eq!(r3.rows_written, 200);
    publish::reject(&pool, &r3.dsv, "tester", "too many edits")
        .await
        .expect("reject");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM geo.features").await,
        1012
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM data.version_closes").await,
        0
    );

    // ── A → B → A: v1's content again is a new version with v1's dsv ──
    let r4 = stage(&pool, &reg, &profile, &v1).await.expect("v4");
    assert_eq!(r4.dsv, r1.dsv);
    assert_eq!(r4.outcome, "approved");
    assert_eq!(r4.rows_written, 9 + 5, "9 reverted + 5 restored");
    release(&pool).await;
    let back = lanka_core::features::features(
        &pool,
        &Catalog::load(&pool).await.expect("catalog"),
        "ft-feat",
        &FeatureQuery {
            limit: Some(2000),
            ..FeatureQuery::default()
        },
    )
    .await
    .expect("page");
    assert_eq!(back.number_returned, 1000);

    // ── immutability: only vto may change ──
    assert!(
        sqlx::query("UPDATE geo.features SET kind = 'x' WHERE feature_id = (SELECT min(feature_id) FROM geo.features)")
            .execute(&pool)
            .await
            .is_err()
    );

    // ── reads ──
    let cat = Catalog::load(&pool).await.expect("catalog");
    let mut seen = 0;
    let mut cursor = None;
    loop {
        let p = lanka_core::features::features(
            &pool,
            &cat,
            "ft-feat",
            &FeatureQuery {
                within: Some("ZZ11".into()),
                kinds: vec!["point".into()],
                cursor,
                limit: Some(50),
                ..FeatureQuery::default()
            },
        )
        .await
        .expect("page");
        seen += p.number_returned;
        assert!(
            p.features
                .iter()
                .all(|f| f.properties.unit.as_deref() == Some("ZZ11"))
        );
        match p.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    let expect = v1
        .iter()
        .filter(|f| f.kind == "point" && f.rep[0] < 80.0)
        .count();
    assert_eq!(seen, expect);
    let q = lanka_core::features::features(
        &pool,
        &cat,
        "ft-feat",
        &FeatureQuery {
            q: Some("feature 99".into()),
            bbox: Some([79.5, 6.0, 80.5, 7.0]),
            ..FeatureQuery::default()
        },
    )
    .await
    .expect("q");
    assert!(q.features.iter().any(|f| f.id == "way/99"));
    assert!(q.features.iter().all(|f| {
        f.properties
            .name
            .as_deref()
            .is_some_and(|n| n.contains("Feature 99"))
    }));
    let one = lanka_core::features::feature(&pool, &cat, "ft-feat", "way/11")
        .await
        .expect("one");
    assert_eq!(one.properties.flags, Flags::UNOFFICIAL_GEOM);
    assert_eq!(one.properties.attrs["lanes"], 4);
    // flat: fields sit beside the core properties, never under `attrs`
    let j = serde_json::to_value(&one.properties).expect("json");
    assert_eq!(j["lanes"], 4);
    assert!(j.get("attrs").is_none() && j.get("name_en").is_none());
    let near = lanka_core::features::nearest(
        &pool,
        &cat,
        &NearestQuery {
            lat: 6.1,
            lon: 79.6003,
            datasets: vec!["ft-feat".into()],
            limit: Some(3),
            ..NearestQuery::default()
        },
    )
    .await
    .expect("nearest");
    assert_eq!(near.len(), 3);
    assert_eq!(near[0].feature.id, "way/0");
    assert_eq!(near[0].feature.distance_m, Some(0.0));
    assert!(
        near.windows(2)
            .all(|w| w[0].feature.distance_m <= w[1].feature.distance_m)
    );
    assert!(
        lanka_core::features::nearest(
            &pool,
            &cat,
            &NearestQuery {
                lat: 6.1,
                lon: 79.6003,
                datasets: vec!["ft-feat".into()],
                limit: Some(51),
                ..NearestQuery::default()
            },
        )
        .await
        .is_err()
    );
}

/// The daily-refresh cost at OSM road-network scale: 240,000 features, then
/// a day's churn (0.4 % changed, 0.1 % removed, 0.1 % added) writes only the
/// churn. Timings are printed (release builds: `--release`).
#[tokio::test]
async fn daily_refresh_writes_only_the_churn() {
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_featscale_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url).await.expect("admin");
    for sql in [
        format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"),
        format!("CREATE DATABASE {db}"),
    ] {
        admin
            .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)))
            .await
            .expect("db");
    }
    let url = format!(
        "{}/{db}",
        admin_url
            .rsplit_once('/')
            .map_or(admin_url.as_str(), |(b, _)| b)
    );
    let result = tokio::spawn(async move {
        let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
            .await
            .expect("pool");
        lanka_core::db::migrate(&pool).await.expect("migrate");
        let (dir, profile) = fixture();
        let reg = Registry::load(&dir).expect("registry");
        reg.sync(&pool, &profile).await.expect("sync");
        let units = vec![
            unit("ZZ", 0, None, square(79.5, 6.0, 1.0, 1.0)),
            unit("ZZ1", 1, Some("ZZ"), square(79.5, 6.0, 1.0, 1.0)),
            unit("ZZ11", 2, Some("ZZ1"), square(79.5, 6.0, 0.5, 1.0)),
            unit("ZZ12", 2, Some("ZZ1"), square(80.0, 6.0, 0.5, 1.0)),
        ];
        let set = BoundarySet {
            id: "ft-set".into(),
            title: "Fixture".into(),
            vintage: None,
            notes: None,
        };
        let run_id = RunTracker::start(&pool, "ft-src", "test")
            .await
            .expect("run")
            .run_id;
        publish_admin_units(&pool, &profile, run_id, "ft-geo", &set, &units, &[], &[])
            .await
            .expect("units");
        // 240 rows × 1,000 columns of features inside the two squares.
        let make = |i: u32, v: u32| {
            let mut f = feat(i % 1000, v);
            let x = 79.5003 + f64::from(i % 1000) * 0.000_99;
            let y = 6.0003 + f64::from(i / 1000) * 0.0041;
            f.key = format!("way/{i}");
            f.geometry = format!(
                r#"{{"type":"LineString","coordinates":[[{x:.6},{y:.6}],[{:.6},{:.6}],[{:.6},{y:.6}]]}}"#,
                x + 0.0004,
                y + 0.0004,
                x + 0.0008
            );
            f.rep = [lanka_ingest::geom::round6(x + 0.0004), lanka_ingest::geom::round6(y + 0.0004)];
            if v > 0 {
                f.name_en = Some(format!("Renamed {i}"));
            }
            f
        };
        let base: Vec<FeatureIn> = (0..240_000).map(|i| make(i, 0)).collect();
        let t = std::time::Instant::now();
        let r = stage(&pool, &reg, &profile, &base).await.expect("first");
        eprintln!("first load: {} features in {:?}", r.rows_written, t.elapsed());
        release(&pool).await;
        let t = std::time::Instant::now();
        let r = stage(&pool, &reg, &profile, &base).await.expect("same");
        eprintln!("identical delivery: {} written in {:?}", r.rows_written, t.elapsed());
        assert_eq!(r.rows_written, 0);
        let mut day = base.clone();
        for f in day.iter_mut().step_by(250) {
            let i: u32 = f.key[4..].parse().expect("id");
            *f = make(i, 1);
        }
        day.retain(|f| !f.key.ends_with("777"));
        let removed = base.len() - day.len();
        day.extend((240_000..240_240).map(|i| {
            let mut f = make(i - 240_000, 0);
            f.key = format!("way/{i}");
            f
        }));
        let t = std::time::Instant::now();
        let r = stage(&pool, &reg, &profile, &day).await.expect("day");
        let d = r.diff.clone().expect("diff");
        eprintln!(
            "daily refresh: +{} ~{} -{} → {} written in {:?} ({})",
            d.rows_added,
            d.rows_changed,
            d.rows_removed,
            r.rows_written,
            t.elapsed(),
            r.outcome
        );
        assert_eq!(usize::try_from(d.rows_removed).expect("n"), removed);
        assert_eq!(r.rows_written, d.rows_added + d.rows_changed);
        assert!(r.rows_written < 1500, "{}", r.rows_written);
        assert_eq!(r.outcome, "approved");
    })
    .await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {db} WITH (FORCE)"
        ))))
        .await
        .expect("drop");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}
