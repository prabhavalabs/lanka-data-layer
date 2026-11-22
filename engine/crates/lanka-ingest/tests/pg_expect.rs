//! WP-07 `sql` expectations at staging (03 §5.2): the query runs against
//! the staged rows (`stage_obs`), read-only, inside a savepoint; a class-A
//! failure fails the staging, a B/C failure sends the version to review.
//! The other expectation types are pure (`src/expect/tests.rs`).
//!
//! Gated like `pg_integration.rs`:
//!   LANKA_TEST_DATABASE_URL=postgres://lanka:…@127.0.0.1:55432/postgres cargo test -p lanka-ingest --test pg_expect

use std::path::PathBuf;

use lanka_core::Profile;
use lanka_ingest::Registry;
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, ObsRow};
use lanka_ingest::expect::{Expectation, Severity};
use lanka_ingest::geo_load::publish_admin_units;
use lanka_ingest::publish::{PublishOpts, StageInput, publish_observations_with};
use lanka_ingest::run::RunTracker;
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
  reference_boundary_set: ex-set
  boundary_dataset: ex-geo
metric_crs: EPSG:4326
calendars: { fiscal: { start_month: 1 }, agri: { start_month: 1 } }
schemes: []
"#;

fn fixture(class: &str) -> (PathBuf, Profile) {
    let dir =
        std::env::temp_dir().join(format!("lanka-ex-registry-{}-{class}", std::process::id()));
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
        dir.join("sources/ex-src.yaml"),
        "id: ex-src\npublisher: Test\ntitle: Source\nhomepage: https://example.org\n\
         access: {method: api, endpoint: https://example.org, auth: none}\n\
         licence: {id: cc-by-4.0, status: attribution, attribution: test}\ncadence: {frequency: annual}\n\
         geo: {levels: [national], join: pcode}\nthemes: [ECON]\ndatasets: [ex-geo, ex-obs]\n\
         priority: P3\neffort: S\nstatus: active\n",
    )
    .expect("source");
    std::fs::write(
        dir.join("datasets/ex-geo.yaml"),
        "id: ex-geo\ntitle: Boundaries\ndescription: Fixture units.\nsource_ids: [ex-src]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [None.]\npublication_class: A\n\
         kind: boundaries\n",
    )
    .expect("geo");
    std::fs::write(
        dir.join("datasets/ex-obs.yaml"),
        format!(
            "id: ex-obs\ntitle: Counts\ndescription: Fixture counts.\nsource_ids: [ex-src]\n\
             indicators:\n- {{id: ex.count, label: Count, unit: '{{count}}', measure_type: count, aggregation: sum,\n   \
             class: official, geo_levels: [national, adm1], period_type: year}}\n\
             time_coverage: {{}}\nmethod: Fixture.\nlimits: [None.]\npublication_class: {class}\n"
        ),
    )
    .expect("obs");
    let p = dir.join("profile.yaml");
    std::fs::write(&p, PROFILE).expect("profile");
    let profile = Profile::load(&p).expect("profile");
    (dir, profile)
}

fn square() -> Box<serde_json::value::RawValue> {
    serde_json::value::to_raw_value(&serde_json::json!({
        "type": "Polygon",
        "coordinates": [[[80.0, 6.0], [80.5, 6.0], [80.5, 6.5], [80.0, 6.5], [80.0, 6.0]]]
    }))
    .expect("raw")
}

fn exp(yaml: &str) -> Expectation {
    Expectation::parse(&serde_yaml_ng::from_str(yaml).expect("yaml")).expect("expectation")
}

async fn stage(
    pool: &PgPool,
    reg: &Registry,
    profile: &Profile,
    rows: &[ObsRow],
    staged: Vec<Expectation>,
) -> anyhow::Result<lanka_ingest::publish::PublishResult> {
    let run_id = RunTracker::start(pool, "ex-src", "test").await?.run_id;
    publish_observations_with(
        pool,
        reg,
        profile,
        StageInput {
            run_id,
            dataset_id: "ex-obs",
            vintage: None,
            rows,
            artifact_ids: &[],
            extract_ids: &[],
        },
        &PublishOpts {
            staged,
            ..PublishOpts::default()
        },
    )
    .await
}

async fn with_db(name: &str, f: impl AsyncFnOnce(String)) {
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_ex_{name}_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url).await.expect("admin");
    for sql in [
        format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"),
        format!("CREATE DATABASE {db}"),
    ] {
        admin
            .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)))
            .await
            .expect("admin sql");
    }
    let url = format!(
        "{}/{db}",
        admin_url
            .rsplit_once('/')
            .map_or(admin_url.as_str(), |(b, _)| b)
    );
    f(url).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {db} WITH (FORCE)"
        ))))
        .await
        .expect("drop");
}

async fn setup(url: &str, class: &str) -> (PgPool, Registry, Profile) {
    let pool = lanka_core::db::connect(url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture(class);
    let reg = Registry::load(&dir).expect("registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let units = vec![
        AdminUnitIn {
            pcode: "ZZ".into(),
            level: 0,
            name_en: "Testland".into(),
            name_si: None,
            name_ta: None,
            parent_pcode: None,
            geometry: square(),
            artifact: 0,
        },
        AdminUnitIn {
            pcode: "ZZ1".into(),
            level: 1,
            name_en: "Region".into(),
            name_si: None,
            name_ta: None,
            parent_pcode: Some("ZZ".into()),
            geometry: square(),
            artifact: 0,
        },
    ];
    let set = BoundarySet {
        id: "ex-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    let run_id = RunTracker::start(&pool, "ex-src", "test")
        .await
        .expect("run")
        .run_id;
    publish_admin_units(&pool, &profile, run_id, "ex-geo", &set, &units, &[], &[])
        .await
        .expect("units");
    (pool, reg, profile)
}

fn rows(v: &[(&str, &str, f64)]) -> Vec<ObsRow> {
    v.iter()
        .map(|(g, p, x)| ObsRow::reported("ex.count", *g, *p, String::new(), Some(*x)))
        .collect()
}

/// Region values never exceed the country's (each row is a violation).
const CHILD_LE_PARENT: &str = "{check: sql, query: \"SELECT c.period_start FROM stage_obs c \
     JOIN stage_obs p ON p.period_start = c.period_start AND p.geo_code = 'ZZ' \
     WHERE c.geo_code = 'ZZ1' AND c.value > p.value\"}";

#[tokio::test]
async fn sql_expectations_class_a() {
    with_db("a", async |url| {
        let (pool, reg, profile) = setup(&url, "A").await;
        // passing: the query returns nothing → approved, result stored
        let ok = rows(&[("ZZ", "2020", 10.0), ("ZZ1", "2020", 4.0)]);
        let r = stage(&pool, &reg, &profile, &ok, vec![exp(CHILD_LE_PARENT)])
            .await
            .expect("passes");
        assert_eq!(r.outcome, "approved", "{}", r.note);
        let v: serde_json::Value = sqlx::query_scalar(
            "SELECT validation FROM data.dataset_versions WHERE id = $1",
        )
        .bind(r.version_id)
        .fetch_one(&pool)
        .await
        .expect("validation");
        assert_eq!(v["expectations"][0]["check"], "sql");
        assert_eq!(v["expectations"][0]["passed"], true);
        // failing: a region above its country → class A staging fails, nothing written
        let bad = rows(&[("ZZ", "2021", 10.0), ("ZZ1", "2021", 40.0)]);
        let err = stage(&pool, &reg, &profile, &bad, vec![exp(CHILD_LE_PARENT)])
            .await
            .expect_err("fails");
        assert!(format!("{err:#}").contains("expectations failed"), "{err:#}");
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM data.dataset_versions WHERE dataset_id = 'ex-obs'",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(n, 1, "the failed staging wrote no version");
        // the query is read-only: a write fails (a failed check, not a write)
        let write = exp(
            "{check: sql, severity: warn, query: \"SELECT nextval('catalog.dim_sets_key_seq') AS n\"}",
        );
        assert_eq!(write.severity, Some(Severity::Warn));
        let r = stage(&pool, &reg, &profile, &bad, vec![write])
            .await
            .expect("a warning does not block");
        let v: serde_json::Value = sqlx::query_scalar(
            "SELECT validation FROM data.dataset_versions WHERE id = $1",
        )
        .bind(r.version_id)
        .fetch_one(&pool)
        .await
        .expect("validation");
        let detail = v["expectations"][0]["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("read-only"), "{detail}");
        assert_eq!(v["expectations"][0]["passed"], false);
    })
    .await;
}

#[tokio::test]
async fn sql_expectation_failure_sends_class_b_to_review() {
    with_db("b", async |url| {
        let (pool, reg, profile) = setup(&url, "B").await;
        let bad = rows(&[("ZZ", "2021", 10.0), ("ZZ1", "2021", 40.0)]);
        let e = exp(&CHILD_LE_PARENT.replace("{check: sql,", "{check: sql, severity: error,"));
        let r = stage(&pool, &reg, &profile, &bad, vec![e])
            .await
            .expect("staged for review");
        assert_eq!(r.outcome, "in_review");
        assert!(r.note.contains("expectations failed"), "{}", r.note);
    })
    .await;
}
