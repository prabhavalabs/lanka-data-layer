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
//! `migration_0002_preserves_0001_rows` loads a 0001-shaped database and
//! asserts every `(indicator, geo, period, dims, value)` survives 0002.

use std::path::PathBuf;

use bytes::Bytes;
use lanka_core::query::{self, MapQuery, SeriesQuery};
use lanka_core::{Basis, Catalog, Flags, Method, Profile, Status};
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, Fetched, ObsRow};
use lanka_ingest::geo_load::publish_admin_units;
use lanka_ingest::publish::{approve, publish_observations, reject};
use lanka_ingest::run::{AlertConfig, RunStatus, RunTracker};
use lanka_ingest::{Archive, Registry};
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
  reference_boundary_set: it-set
  boundary_dataset: it-geo
metric_crs: EPSG:4326
calendars: { fiscal: { start_month: 1 }, agri: { start_month: 1 } }
schemes: [{ id: ed, title: Electoral district }]
"#;

fn fixture_registry() -> (PathBuf, Profile) {
    let dir = tmp("registry");
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
         geo: {levels: [national, adm2], join: pcode}\nthemes: [ECON]\ndatasets: [it-a, it-geo]\n\
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
        locator: json!({"sheet": "T1", "cell": format!("C{}", v as i64)}),
        verbatim: Some(format!("{v}")),
        method: Method::SheetCell,
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
    sqlx::query("INSERT INTO ops.run_artifacts VALUES ($1, $2) ON CONFLICT DO NOTHING")
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
    assert_eq!((sync.sources, sync.datasets, sync.indicators), (1, 2, 1));
    assert_eq!((sync.units, sync.codelists), (1, 1));
    let archive = Archive::new(tmp("archive"));
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
    let geo = publish_admin_units(&pool, &profile, run.run_id, "it-geo", &set, &units, &f)
        .await
        .expect("admin units");
    assert_eq!(geo.outcome, "published");
    let again = publish_admin_units(&pool, &profile, run.run_id, "it-geo", &set, &units, &f)
        .await
        .expect("admin units again");
    assert_eq!(
        (again.outcome, again.dsv.as_str()),
        ("unchanged", geo.dsv.as_str())
    );
    let lk_units = vec![unit("LK", 0, None, square(80.0, 7.0, 1.0))];
    assert!(
        publish_admin_units(&pool, &profile, run.run_id, "it-geo", &set, &lk_units, &f)
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
    let r1 = publish_observations(
        &pool,
        &reg,
        &profile,
        run.run_id,
        "it-a",
        Some("v1"),
        &rows,
        &f,
    )
    .await
    .expect("publish");
    assert_eq!((r1.outcome, r1.rows_written), ("published", 40));
    assert!(r1.data_version.is_some());
    // identical content → no new version, nothing written
    let again = publish_observations(
        &pool,
        &reg,
        &profile,
        run.run_id,
        "it-a",
        Some("v1"),
        &rows,
        &f,
    )
    .await
    .expect("republish");
    assert_eq!(
        (again.outcome, again.dsv.as_str(), again.rows_written),
        ("unchanged", r1.dsv.as_str(), 0)
    );
    run.finish(RunStatus::Succeeded, "ok", &alerts)
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
        let err = publish_observations(&pool, &reg, &profile, 0, "it-a", None, &bad, &f)
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

    let p = query::provenance(&pool, &cat, s.series[0].points[0].obs_id)
        .await
        .expect("provenance");
    assert_eq!(p.method.as_deref(), Some("sheet_cell"));
    assert_eq!(p.review.as_deref(), Some("auto_accepted"));
    assert_eq!(p.dsv, r1.dsv);
    assert_eq!(
        p.artifact_sha256.as_deref(),
        Some(hex_of(&f[0].sha256).as_str())
    );

    // ── run 2: revise >1% of history → class A gate demotes to review ──
    let run2 = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run 2");
    let run2_id = run2.run_id;
    let f2 = vec![fetched(&pool, &archive, run2.run_id, b"fixture table v2").await];
    let revised: Vec<ObsRow> = rows
        .iter()
        .cloned()
        .map(|mut r| {
            r.value = r.value.map(|v| v + 1.0);
            r
        })
        .collect();
    let r2 = publish_observations(
        &pool,
        &reg,
        &profile,
        run2.run_id,
        "it-a",
        Some("v2"),
        &revised,
        &f2,
    )
    .await
    .expect("revise");
    assert_eq!(r2.outcome, "in_review");
    assert_eq!(r2.diff.as_ref().map(|d| d.rows_changed), Some(40));
    run2.finish(RunStatus::NeedsReview, "gate", &alerts)
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
    let r3 = publish_observations(
        &pool,
        &reg,
        &profile,
        run2_id,
        "it-a",
        Some("v3"),
        &appended,
        &f2,
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
    let r4 = publish_observations(
        &pool,
        &reg,
        &profile,
        run2_id,
        "it-a",
        Some("v4"),
        &dropped,
        &f2,
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
