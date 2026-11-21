//! End-to-end test against a real PostGIS server, on a throwaway database.
//!
//! Gated: runs only when `LANKA_TEST_DATABASE_URL` points at a server where
//! the user may CREATE DATABASE (e.g. the local infra/v2 compose stack):
//!
//!   LANKA_TEST_DATABASE_URL=postgres://lanka:lanka-local-dev@127.0.0.1:55432/postgres cargo test
//!
//! Covers: migrations, registry sync, run tracking, artifact archive,
//! admin-unit load (PostGIS), class-A auto-publish + release, no-change
//! detection, the class-A revision gate demoting to review, review approval,
//! and the query core (area profile, series, map, reverse, provenance).

use std::path::PathBuf;

use bytes::Bytes;
use lanka_core::query::{self, MapQuery, SeriesQuery};
use lanka_core::{Catalog, Pcode};
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, Fetched, ObsRow};
use lanka_ingest::geo_load::publish_admin_units;
use lanka_ingest::publish::{approve, publish_observations};
use lanka_ingest::run::{AlertConfig, RunStatus, RunTracker};
use lanka_ingest::{Archive, Registry};
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn fixture_registry() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lanka-it-registry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["schema", "sources", "datasets"] {
        std::fs::create_dir_all(dir.join(d)).expect("mkdir");
    }
    for f in ["source.schema.json", "dataset.schema.json"] {
        std::fs::copy(
            repo_root().join("registry/schema").join(f),
            dir.join("schema").join(f),
        )
        .expect("copy");
    }
    std::fs::write(
        dir.join("sources/it-src.yaml"),
        "id: it-src\npublisher: Test\ntitle: Integration source\nhomepage: https://example.org\n\
         access: {method: api, endpoint: https://example.org, auth: none}\n\
         licence: {id: cc-by-4.0, status: open}\ncadence: {frequency: annual}\n\
         geo: {levels: [national, adm2], join: pcode}\nthemes: [ECON]\ndatasets: [it-a, it-geo]\n\
         priority: P3\neffort: S\nstatus: active\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-a.yaml"),
        "id: it-a\ntitle: Integration dataset\ndescription: Fixture dataset for tests.\nsource_ids: [it-src]\n\
         indicators:\n  - {id: it.count, label: Count, unit: things, dims: [sex], geo_levels: [national, adm2], period_type: year}\n\
         time_coverage: {from: 2020}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-geo.yaml"),
        "id: it-geo\ntitle: Fixture boundaries\ndescription: Fixture admin units.\nsource_ids: [it-src]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [Three squares.]\npublication_class: A\n",
    )
    .expect("write");
    dir
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
        status: "reported",
        artifact: 0,
        locator: json!({"sheet": "T1", "cell": format!("C{}", v as i64)}),
        verbatim: Some(format!("{v}")),
        method: "sheet_cell",
        confidence: Some(1.0),
    }
}

async fn fetched(pool: &PgPool, archive: &Archive, run_id: i64, body: &'static [u8]) -> Fetched {
    let stored = archive.put(body).expect("archive");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, archive_path)
         VALUES ($1, 'it-src', 'https://example.org/t1', now(), $2, 'x')
         ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256 RETURNING id",
    )
    .bind(stored.sha256.as_slice())
    .bind(stored.bytes as i64)
    .fetch_one(pool)
    .await
    .expect("artifact");
    sqlx::query("INSERT INTO ops.run_artifacts VALUES ($1, $2)")
        .bind(run_id)
        .bind(id)
        .execute(pool)
        .await
        .expect("link");
    Fetched {
        key: "t1".into(),
        url: "https://example.org/t1".into(),
        artifact_id: id,
        sha256: stored.sha256,
        bytes: Bytes::from_static(body),
        media_type: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn full_cycle_against_postgis() {
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url)
        .await
        .expect("connect admin");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {db}"
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

    let result = tokio::spawn(run_cycle(url)).await;

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

async fn run_cycle(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let reg = Registry::load(fixture_registry()).expect("fixture registry");
    let sync = reg.sync(&pool).await.expect("sync");
    assert_eq!((sync.sources, sync.datasets, sync.indicators), (1, 2, 1));
    let archive =
        Archive::new(std::env::temp_dir().join(format!("lanka-it-archive-{}", std::process::id())));
    let alerts = AlertConfig::default();

    // ── run 1: boundaries + first data version (class A → auto-publish) ──
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    assert!(
        RunTracker::start(&pool, "it-src", "test").await.is_err(),
        "second concurrent run must be refused"
    );
    let f = vec![fetched(&pool, &archive, run.run_id, b"fixture table v1").await];
    let units = vec![
        unit("LK", 0, None, square(80.0, 7.0, 1.0)),
        unit("LK1", 1, Some("LK"), square(80.0, 7.0, 0.5)),
        unit("LK11", 2, Some("LK1"), square(80.0, 7.0, 0.25)),
    ];
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    let geo = publish_admin_units(&pool, run.run_id, "it-geo", &set, &units, &f)
        .await
        .expect("admin units");
    assert_eq!(geo.outcome, "published");

    let mut rows = Vec::new();
    for y in 2020..2030 {
        for (g, base) in [("LK", 1000.0), ("LK11", 100.0)] {
            rows.push(obs(g, &y.to_string(), "female", base + f64::from(y - 2020)));
            rows.push(obs(
                g,
                &y.to_string(),
                "male",
                base + 50.0 + f64::from(y - 2020),
            ));
        }
    }
    let r1 = publish_observations(&pool, &reg, run.run_id, "it-a", Some("v1"), &rows, &f)
        .await
        .expect("publish");
    assert_eq!(r1.outcome, "published");
    assert!(r1.data_version.is_some());
    // identical content → no new version
    let again = publish_observations(&pool, &reg, run.run_id, "it-a", Some("v1"), &rows, &f)
        .await
        .expect("republish");
    assert_eq!(
        (again.outcome, again.dsv.as_str()),
        ("unchanged", r1.dsv.as_str())
    );
    run.finish(RunStatus::Succeeded, "ok", &alerts)
        .await
        .expect("finish");

    // bad geo code fails validation, nothing written
    let mut bad = rows.clone();
    bad.push(obs("LK99", "2020", "female", 1.0));
    assert!(
        publish_observations(&pool, &reg, 0, "it-a", None, &bad, &f)
            .await
            .is_err()
    );

    // ── query core over the published release ──
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(cat.data_version, r1.data_version);
    let (profile, touched) =
        query::area_profile(&pool, &cat, &Pcode::parse("LK11").expect("pcode"))
            .await
            .expect("area");
    assert_eq!(
        profile
            .ancestors
            .iter()
            .map(|a| a.pcode.as_str())
            .collect::<Vec<_>>(),
        ["LK", "LK1"]
    );
    assert_eq!(profile.indicators.len(), 1);
    assert_eq!(profile.indicators[0].period, "2029");
    assert_eq!(profile.indicators[0].values.len(), 2);
    assert_eq!(touched, ["it-a"]);
    assert!(profile.area_km2.is_some_and(|a| a > 0.0));

    let s = query::series(
        &pool,
        &cat,
        "it.count",
        &SeriesQuery {
            geos: vec!["LK".into(), "LK11".into()],
            from: Some("2025".into()),
            dims: Some("sex=male".into()),
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
        (["LK11".to_owned()].as_slice(), "2029", "sex=female")
    );

    let rev = query::reverse(&pool, 7.1, 80.1).await.expect("reverse");
    assert_eq!(rev.unit.pcode, "LK11");
    assert!(matches!(
        query::reverse(&pool, 4.0, 80.0).await,
        Err(lanka_core::CoreError::NotInCoverage)
    ));

    let p = query::provenance(&pool, s.series[0].points[0].obs_id)
        .await
        .expect("provenance");
    assert_eq!(p.method, "sheet_cell");
    assert_eq!(p.review, "auto_accepted");
    assert_eq!(
        p.artifact_sha256.as_deref(),
        Some(hex_of(&f[0].sha256).as_str())
    );

    // ── run 2: revise >1% of history → class A gate demotes to review ──
    let run2 = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run 2");
    let f2 = vec![fetched(&pool, &archive, run2.run_id, b"fixture table v2").await];
    let revised: Vec<ObsRow> = rows
        .iter()
        .cloned()
        .map(|mut r| {
            r.value = r.value.map(|v| v + 1.0);
            r
        })
        .collect();
    let r2 = publish_observations(&pool, &reg, run2.run_id, "it-a", Some("v2"), &revised, &f2)
        .await
        .expect("revise");
    assert_eq!(r2.outcome, "in_review");
    assert_eq!(r2.diff.as_ref().map(|d| d.rows_changed), Some(40));
    run2.finish(RunStatus::NeedsReview, "gate", &alerts)
        .await
        .expect("finish 2");
    // still serving v1
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(
        cat.datasets["it-a"].current.as_ref().map(|c| c.dsv.clone()),
        Some(r1.dsv.clone())
    );

    let ok = approve(&pool, &r2.dsv, "tester", Some("checked"))
        .await
        .expect("approve");
    assert_eq!(ok.outcome, "published");
    let cat = Catalog::load(&pool).await.expect("catalog");
    assert_eq!(
        cat.datasets["it-a"].current.as_ref().map(|c| c.dsv.clone()),
        Some(r2.dsv.clone())
    );
    let changelog: i64 =
        sqlx::query_scalar("SELECT count(*) FROM data.changelog WHERE dataset_id = 'it-a'")
            .fetch_one(&pool)
            .await
            .expect("changelog");
    assert_eq!(changelog, 2);
    let status: String =
        sqlx::query_scalar("SELECT status::text FROM data.dataset_versions WHERE dsv = $1")
            .bind(&r1.dsv)
            .fetch_one(&pool)
            .await
            .expect("status");
    assert_eq!(status, "superseded");

    // ── failure path: run closed as failed + alert recorded ──
    let run3 = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run 3");
    run3.fail(&anyhow::anyhow!("upstream exploded"), &alerts)
        .await
        .expect("fail");
    let (state, alerts_n): (String, i64) = sqlx::query_as(
        "SELECT last_status::text, (SELECT count(*) FROM ops.alerts WHERE kind = 'run_failed')
         FROM ops.source_health WHERE source_id = 'it-src'",
    )
    .fetch_one(&pool)
    .await
    .expect("health");
    assert_eq!((state.as_str(), alerts_n), ("failed", 1));
    pool.close().await;
}

fn hex_of(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
