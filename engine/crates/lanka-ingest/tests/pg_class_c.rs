//! Class C against a real PostGIS server, on a throwaway database.
//!
//! Gated like `pg_integration.rs`: runs only when `LANKA_TEST_DATABASE_URL`
//! points at a server where the user may CREATE DATABASE.
//!
//! `class_c_gate_publish_and_worker_evidence` covers `lanka extract publish`
//! and the worker's evidence sheets on the `dcs-lfs` connector trimmed to
//! annual table 4.1 and its template (`tests/fixtures/pg-class-c`), in a
//! trimmed copy of the registry with the real country profile:
//!
//! - an unapproved class-C extract cannot be published, and cannot be
//!   approved while required cells are unsigned;
//! - after sign-off (one correction) the approved extract's corrected child
//!   is standardised into a class-B version left in review, with the
//!   corrected value;
//! - extracts that are not class C, and rejected ones, are refused;
//! - a worker job that leaves that version in review builds its evidence
//!   sheet (store `evidence/<dsv>/index.html`, `ops.reviews.evidence_key`);
//! - the version can then be approved (the class-C gate is satisfied).

use std::path::{Path, PathBuf};

use bytes::Bytes;
use lanka_core::Profile;
use lanka_ingest::Registry;
use lanka_ingest::classc;
use lanka_ingest::connector::{AdminUnitIn, BoundarySet};
use lanka_ingest::extract::{Cell, CellType, ExtractArtifact};
use lanka_ingest::jobs::{self, NewJob};
use lanka_ingest::pipeline::{Env, publish_extract};
use lanka_ingest::publish;
use lanka_ingest::review::{self, Decision, ExtractMeta, Target};
use lanka_ingest::run::{RunStatus, RunTracker};
use lanka_ingest::{geo_load, worker};
use lanka_store::{BlobStore, FsStore, Ns};
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};

const SOURCE: &str = "dcs-lfs";
const DATASET: &str = "dcs-lfs-annual-subnational";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lanka-classc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().expect("parent")).expect("mkdir");
    std::fs::copy(from, to).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
}

/// The trimmed `dcs-lfs` connector, its template and a trimmed dataset in a registry
/// of their own (the source paused, so the worker's tick schedules nothing).
fn registry() -> (PathBuf, Profile) {
    let real = repo_root().join("registry");
    let dir = tmp("registry");
    for e in std::fs::read_dir(real.join("schema")).expect("schema") {
        let p = e.expect("entry").path();
        copy(&p, &dir.join("schema").join(p.file_name().expect("name")));
    }
    copy(&real.join("units.yaml"), &dir.join("units.yaml"));
    copy(
        &real.join("codelists/industry.yaml"),
        &dir.join("codelists/industry.yaml"),
    );
    // The connector and its template come from tests/fixtures/pg-class-c: the
    // live dcs-lfs connector has grown tables and datasets this registry lacks.
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pg-class-c");
    copy(
        &fixture.join("connectors/dcs-lfs.yaml"),
        &dir.join("connectors/dcs-lfs.yaml"),
    );
    let tpl = "templates/dcs-lfs/annual-table-4.1-employment-by-industry.yaml";
    copy(&fixture.join(tpl), &dir.join(tpl));
    for d in ["sources", "datasets"] {
        std::fs::create_dir_all(dir.join(d)).expect("mkdir");
    }
    std::fs::write(
        dir.join("sources/dcs-lfs.yaml"),
        "id: dcs-lfs\npublisher: Department of Census and Statistics\ntitle: Labour Force Survey\n\
         homepage: https://www.statistics.gov.lk/LabourForce/StaticalInformation\n\
         access: {method: pdf, endpoint: https://www.statistics.gov.lk/x.pdf, auth: none}\n\
         licence: {id: lk-gov-unstated, status: permission_unclear}\ncadence: {frequency: annual}\n\
         paused: true\ngeo: {levels: [national], join: pcode}\nthemes: [ECON]\n\
         datasets: [dcs-lfs-annual-subnational, cod-ab-lka-boundaries]\npriority: P1\neffort: S\nstatus: active\n",
    )
    .expect("source");
    std::fs::write(
        dir.join("datasets/dcs-lfs-annual-subnational.yaml"),
        "id: dcs-lfs-annual-subnational\ntitle: Labour force (annual)\ndescription: Employment by industry.\n\
         source_ids: [dcs-lfs]\nindicators:\n  - {id: lab.employment_by_industry, label: Employment by industry,\n\
         \x20     unit: '{person}', measure_type: count, aggregation: sum, class: official, dims: [industry],\n\
         \x20     geo_levels: [national], period_type: year}\n\
         time_coverage: {from: '2015'}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: B\n",
    )
    .expect("dataset");
    std::fs::write(
        dir.join("datasets/cod-ab-lka-boundaries.yaml"),
        "id: cod-ab-lka-boundaries\ntitle: Boundaries\ndescription: The root unit only.\nsource_ids: [dcs-lfs]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [One square.]\npublication_class: A\n\
         kind: boundaries\n",
    )
    .expect("boundaries");
    let p = dir.join("profile.yaml");
    copy(&repo_root().join("country/lk/profile.yaml"), &p);
    let profile = Profile::load(&p).expect("profile");
    (dir, profile)
}

fn cell(r: u32, c: u32, key: &str, text: &str, conf: f32) -> Cell {
    Cell {
        table_id: "t4_1".into(),
        r,
        c,
        text: text.into(),
        vtype: if r > 0 && c > 0 {
            CellType::Number
        } else {
            CellType::String
        },
        page: None,
        bbox: None,
        sheet: None,
        a1: None,
        row_header: None,
        col_header: Some(key.into()),
        artifact_idx: 0,
        conf: Some(conf),
        reader: classc::READER.into(),
    }
}

/// Table 4.1 as a class-C reading would anchor it (synthetic numbers).
/// The 2024 agriculture cell is a disagreement (0.5) the reviewer corrects.
fn cells() -> Vec<Cell> {
    let keys = ["total_no", "agri_no", "industry_no", "services_no"];
    let mut v = vec![cell(0, 0, "year", "Year", 1.0)];
    for (i, k) in keys.iter().enumerate() {
        v.push(cell(0, u32::try_from(i).expect("i") + 1, k, "No.", 1.0));
    }
    let rows = [
        ("2023", ["8,000", "2,000", "2,000", "4,000"]),
        ("2024", ["8,100", "2,190", "2,000", "4,000"]),
        ("2025*", ["8,200", "2,100", "2,000", "4,100"]),
    ];
    for (r, (label, vals)) in rows.iter().enumerate() {
        let r = u32::try_from(r).expect("r") + 1;
        v.push(cell(r, 0, "year", label, 1.0));
        for (i, (k, t)) in keys.iter().zip(vals).enumerate() {
            let conf = if r == 2 && *k == "agri_no" { 0.5 } else { 0.8 };
            v.push(cell(r, u32::try_from(i).expect("i") + 1, k, t, conf));
        }
    }
    v
}

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
    let drop = format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop.clone())))
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
    let result = tokio::spawn(f(format!("{base}/{db}"))).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop)))
        .await
        .expect("drop test database");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn value(pool: &PgPool, dsv: &str, industry: &str, period: &str) -> Option<f64> {
    sqlx::query_scalar(
        "SELECT o.value FROM data.observations o
         JOIN data.dataset_versions v ON v.dataset_id = 'dcs-lfs-annual-subnational' AND v.dsv = $1
         JOIN catalog.dim_sets ds ON ds.key = o.dims_key
         WHERE o.dataset_key = (SELECT key FROM catalog.datasets WHERE id = v.dataset_id)
           AND o.vfrom <= v.seq AND o.vto >= v.seq
           AND ds.dims = 'industry=' || $2 AND data.period_code(o.freq, o.time_start) = $3",
    )
    .bind(dsv)
    .bind(industry)
    .bind(period)
    .fetch_optional(pool)
    .await
    .expect("value")
    .flatten()
}

#[tokio::test(flavor = "multi_thread")]
async fn class_c_gate_publish_and_worker_evidence() {
    with_database("classc", run).await;
}

#[allow(clippy::too_many_lines)]
async fn run(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = registry();
    let reg = Registry::load(&dir).expect("fixture registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let store_root = tmp("store");
    let store = FsStore::new(&store_root);
    let env = Env {
        pool: &pool,
        registry: &reg,
        profile: &profile,
        store: &store,
    };

    // ── the root unit (observations are published on it) ──
    let run = RunTracker::start(&pool, SOURCE, "test").await.expect("run");
    let root = AdminUnitIn {
        pcode: "LK".into(),
        level: 0,
        name_en: "Sri Lanka".into(),
        name_si: None,
        name_ta: None,
        parent_pcode: None,
        geometry: serde_json::value::RawValue::from_string(
            r#"{"type":"Polygon","coordinates":[[[79.5,5.9],[81.9,5.9],[81.9,9.9],[79.5,9.9],[79.5,5.9]]]}"#
                .into(),
        )
        .expect("geometry"),
        artifact: 0,
    };
    let set = BoundarySet {
        id: profile.reference_boundary_set().into(),
        title: "Root".into(),
        vintage: None,
        notes: None,
    };
    geo_load::publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        profile.boundary_dataset(),
        &set,
        &[root],
        &[],
        &[],
    )
    .await
    .expect("root unit");
    run.finish(RunStatus::Succeeded, "root")
        .await
        .expect("finish");

    // ── a class-C extract of the document, with its review ──
    let (blob, _) = store
        .put_cas(Ns::Raw, Bytes::from_static(b"%PDF-1.4 scanned fixture"))
        .await
        .expect("archive");
    let artifact_id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, archive_path)
         VALUES ($1, $2, 'https://www.statistics.gov.lk/x.pdf', now(), $3, $4) RETURNING id",
    )
    .bind(blob.sha256.as_slice())
    .bind(SOURCE)
    .bind(i64::try_from(blob.bytes).expect("bytes"))
    .bind(blob.rel_path())
    .fetch_one(&pool)
    .await
    .expect("artifact");
    let artifacts = [ExtractArtifact {
        artifact_id,
        key: "lfs-annual-2025".into(),
        url: "https://www.statistics.gov.lk/x.pdf".into(),
        sha256: blob.sha256,
    }];
    let cells = cells();
    let x = review::persist_cells(
        &pool,
        &store,
        SOURCE,
        classc::READER,
        "test",
        &ExtractMeta {
            model: Some("replay:fixture"),
            ..ExtractMeta::default()
        },
        cells.clone(),
        &artifacts,
    )
    .await
    .expect("extract");
    let target = Target::Extract(x);
    review::open_review(&pool, &target, "class C")
        .await
        .expect("review");

    // ── the gate: not approved → not published; unsigned → not approvable ──
    let err = publish_extract(&env, x).await.expect_err("unapproved");
    assert!(err.to_string().contains("is not approved"), "{err:#}");
    let err = review::approve_extract(&pool, &store, x, "owner", None)
        .await
        .expect_err("unsigned cells");
    assert!(err.to_string().contains("have no decision yet"), "{err:#}");
    let versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM data.dataset_versions WHERE dataset_id = $1")
            .bind(DATASET)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(versions, 0, "nothing was staged");

    // ── sign every value cell (one correction), approve ──
    for (i, c) in cells.iter().enumerate() {
        if c.r == 0 || c.c == 0 {
            continue;
        }
        let i = u32::try_from(i).expect("i");
        if c.r == 2 && c.col_header.as_deref() == Some("agri_no") {
            review::sign(
                &pool,
                &target,
                x,
                i,
                Decision::Correct,
                Some("2,100"),
                "owner",
            )
            .await
            .expect("correct");
        } else {
            review::sign(&pool, &target, x, i, Decision::Accept, None, "owner")
                .await
                .expect("accept");
        }
    }
    let d = review::approve_extract(&pool, &store, x, "owner", None)
        .await
        .expect("approve");
    assert_eq!(d.corrected, 1);
    assert_ne!(d.approved_extract_id, x, "a corrected child is written");

    // ── publish: the corrected child → a class-B version in review ──
    let r = publish_extract(&env, x).await.expect("publish");
    assert_eq!(r.published.len(), 1, "{r:?}");
    let p = &r.published[0];
    assert_eq!((p.dataset_id.as_str(), p.outcome), (DATASET, "in_review"));
    assert_eq!(p.rows, 12);
    let dsv = p.dsv.clone();
    let linked: Vec<i64> = sqlx::query_scalar(
        "SELECT ve.extract_id FROM data.dataset_version_extracts ve
         JOIN data.dataset_versions v ON v.id = ve.version_id WHERE v.dsv = $1",
    )
    .bind(&dsv)
    .fetch_all(&pool)
    .await
    .expect("extracts");
    assert_eq!(linked, vec![d.approved_extract_id]);
    assert_eq!(
        value(&pool, &dsv, "agriculture", "2024").await,
        Some(2100.0)
    );
    assert_eq!(value(&pool, &dsv, "services", "2025").await, Some(4100.0));
    // The child, named directly, resolves to itself.
    assert_eq!(
        lanka_ingest::pipeline::approved_class_c(&pool, d.approved_extract_id)
            .await
            .expect("child"),
        d.approved_extract_id
    );

    // ── not class C, or rejected: refused ──
    let mut b_cells = cells.clone();
    for c in &mut b_cells {
        c.reader = "pdftotext".into();
        c.conf = Some(1.0);
    }
    let b = review::persist_cells(
        &pool,
        &store,
        SOURCE,
        SOURCE,
        "test",
        &ExtractMeta::default(),
        b_cells,
        &artifacts,
    )
    .await
    .expect("class B extract");
    let err = publish_extract(&env, b).await.expect_err("class B");
    assert!(
        err.to_string().contains("is not a class-C extract"),
        "{err:#}"
    );
    let mut c2 = cells.clone();
    c2[1].text = "No. (2)".into(); // different content → a new extract
    let rejected = review::persist_cells(
        &pool,
        &store,
        SOURCE,
        classc::READER,
        "test",
        &ExtractMeta::default(),
        c2,
        &artifacts,
    )
    .await
    .expect("extract 2");
    review::open_review(&pool, &Target::Extract(rejected), "class C")
        .await
        .expect("review 2");
    review::reject_extract(&pool, rejected, "owner", "misread")
        .await
        .expect("reject");
    let err = publish_extract(&env, rejected).await.expect_err("rejected");
    assert!(err.to_string().contains("is not approved"), "{err:#}");

    // ── the worker leaves the version in review and builds its evidence ──
    let job = NewJob {
        kind: "restandardise".into(),
        lane: "batch".into(),
        dedupe_key: format!("restandardise:{SOURCE}"),
        payload: json!({ "source": SOURCE }),
        priority: 50,
        weight: 2,
        max_attempts: 1,
        trigger: "manual".into(),
        run_after: None,
    };
    let (job_id, _) = jobs::enqueue(&pool, &job).await.expect("enqueue");
    let wcfg = worker::WorkerConfig {
        lane: "batch".into(),
        slots: 4,
        tick: std::time::Duration::from_secs(60),
        registry_dir: dir.clone(),
        profile_path: Some(dir.join("profile.yaml")),
        store_root: store_root.clone(),
        grace: std::time::Duration::from_secs(5),
        once: true,
        tick_cfg: lanka_ingest::scheduler::TickConfig {
            train_at: chrono::NaiveTime::from_hms_opt(23, 59, 59).expect("time"),
            live_trains: false,
            ..lanka_ingest::scheduler::TickConfig::default()
        },
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(120),
        worker::run(pool.clone(), wcfg, std::future::pending()),
    )
    .await
    .expect("worker finished")
    .expect("worker ok");
    let (state, result): (String, Option<serde_json::Value>) =
        sqlx::query_as("SELECT state, result FROM ops.jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&pool)
            .await
            .expect("job");
    assert_eq!(state, "succeeded", "{result:?}");
    let result = result.expect("result");
    assert_eq!(result["published"][0]["outcome"], "in_review", "{result}");
    assert_eq!(result["published"][0]["dsv"], dsv.as_str(), "{result}");
    let ev = &result["evidence"][0];
    assert_eq!(ev["dsv"], dsv.as_str(), "{result}");
    let key = ev["key"].as_str().unwrap_or_else(|| panic!("{result}"));
    assert!(
        store_root.join(key).is_file(),
        "{} is written",
        store_root.join(key).display()
    );
    let recorded: Option<String> = sqlx::query_scalar(
        "SELECT r.evidence_key FROM ops.reviews r JOIN data.dataset_versions v ON v.id = r.version_id
         WHERE v.dsv = $1 ORDER BY r.id DESC LIMIT 1",
    )
    .bind(&dsv)
    .fetch_one(&pool)
    .await
    .expect("review row");
    assert_eq!(recorded.as_deref(), Some(key));

    // ── the version's class-C extracts are approved: it can be approved ──
    let a = publish::approve(&pool, &dsv, "owner", None)
        .await
        .expect("approve version");
    assert_eq!(a.outcome, "approved");
}
