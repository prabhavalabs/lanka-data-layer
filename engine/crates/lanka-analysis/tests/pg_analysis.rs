//! The analysis engine against a real PostGIS server, on a throwaway
//! database (gated by `LANKA_TEST_DATABASE_URL`, like the ingest tests).
//!
//! A synthetic country `ZZ` (3 regions × 4 districts) with two indicator
//! datasets and an unrelated third; the engine runs as `lanka_analysis`.
//! Checks (WP-29 acceptance):
//! - the same request returns the same id and the same bytes twice, and
//!   again after an unrelated release (cache served, and a recomputation
//!   bypassing the store gives the same id and numbers);
//! - a new version of an input gives a new id;
//! - a blocked request (3 regions < MIN_N) returns no estimates;
//! - `lanka_analysis` cannot write `data.*`/`catalog.*`/`geo.*`, writes
//!   `analysis.*`; `lanka_api` reads but cannot write `analysis.*`.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use lanka_core::{Basis, Catalog, Flags, Method, Profile, Status};
use lanka_ingest::Registry;
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, ObsRow};
use lanka_ingest::geo_load;
use lanka_ingest::publish::{StageInput, publish_observations};
use lanka_ingest::run::RunTracker;
use lanka_ingest::train::run_train_with;
use serde_json::{Value, json};
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
  reference_boundary_set: it-set
  boundary_dataset: it-geo
metric_crs: EPSG:4326
calendars: { fiscal: { start_month: 1 }, agri: { start_month: 1 } }
schemes: []
"#;

fn write(dir: &std::path::Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    std::fs::write(p, text).expect("write");
}

fn registry() -> (PathBuf, Profile) {
    let dir = std::env::temp_dir().join(format!("lanka-it-analysis-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for f in [
        "source.schema.json",
        "dataset.schema.json",
        "codelist.schema.json",
        "units.schema.json",
        "profile.schema.json",
        "recipe.schema.json",
    ] {
        std::fs::create_dir_all(dir.join("schema")).expect("mkdir");
        std::fs::copy(
            repo_root().join("registry/schema").join(f),
            dir.join("schema").join(f),
        )
        .expect("copy schema");
    }
    write(
        &dir,
        "units.yaml",
        "units:\n  - { code: '{person}', label: persons, kind: count }\n  - { code: '%', label: percent, kind: ratio }\n",
    );
    write(
        &dir,
        "sources/it-src.yaml",
        "id: it-src\npublisher: Test\ntitle: it-src\nhomepage: https://example.org\n\
         access: {method: api, endpoint: https://example.org, auth: none}\n\
         licence: {id: cc-by-4.0, status: open}\ncadence: {frequency: annual}\n\
         geo: {levels: [national, adm2], join: pcode}\nthemes: [ECON]\ndatasets: [it-geo, it-x, it-y, it-z]\n\
         priority: P3\neffort: S\nstatus: active\n",
    );
    let ds = |id: &str, ind: &str, unit: &str, mt: &str, agg: &str| {
        format!(
            "id: {id}\ntitle: {id}\ndescription: Fixture dataset for the analysis tests.\nsource_ids: [it-src]\nindicators:\n\
             \x20 - {{id: {ind}, label: {ind}, unit: '{unit}', measure_type: {mt}, aggregation: {agg}, class: official,\n\
             \x20     dims: [], geo_levels: [national, adm1, adm2], period_type: snapshot}}\n\
             time_coverage: {{}}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\n"
        )
    };
    write(
        &dir,
        "datasets/it-geo.yaml",
        "id: it-geo\ntitle: it-geo\ndescription: Fixture dataset for the analysis tests.\nsource_ids: [it-src]\nindicators:\n  []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\nkind: boundaries\n",
    );
    write(
        &dir,
        "datasets/it-x.yaml",
        &ds("it-x", "it.share", "%", "share", "none"),
    );
    write(
        &dir,
        "datasets/it-y.yaml",
        &ds("it-y", "it.pop", "{person}", "count", "sum"),
    );
    write(
        &dir,
        "datasets/it-z.yaml",
        &ds("it-z", "it.other", "%", "share", "none"),
    );
    let p = dir.join("profile.yaml");
    std::fs::write(&p, PROFILE).expect("profile");
    let profile = Profile::load(&p).expect("profile");
    (dir, profile)
}

fn square(x0: f64, y0: f64, d: f64) -> Box<serde_json::value::RawValue> {
    let g = json!({"type": "Polygon", "coordinates": [[[x0, y0], [x0 + d, y0], [x0 + d, y0 + d], [x0, y0 + d], [x0, y0]]]});
    serde_json::value::to_raw_value(&g).expect("raw")
}

fn unit(pcode: &str, level: i16, parent: Option<&str>, x: f64, y: f64, d: f64) -> AdminUnitIn {
    AdminUnitIn {
        pcode: pcode.into(),
        level,
        name_en: pcode.into(),
        name_si: None,
        name_ta: None,
        parent_pcode: parent.map(Into::into),
        geometry: square(x, y, d),
        artifact: 0,
    }
}

fn row(ind: &str, geo: &str, v: f64) -> ObsRow {
    ObsRow {
        indicator_id: ind.into(),
        geo_code: geo.into(),
        period: "2024".into(),
        dims: String::new(),
        value: Some(v),
        lo: None,
        hi: None,
        basis: Basis::Reported,
        status: Status::Normal,
        flags: Flags::EMPTY,
        artifact: 0,
        prov: None,
        locator: json!({"cell": format!("{ind}{geo}")}),
        verbatim: Some(v.to_string()),
        method: Method::SheetCell,
        confidence: Some(1.0),
    }
}

fn districts() -> Vec<String> {
    (1..=3)
        .flat_map(|r| (1..=4).map(move |d| format!("ZZ{r}{d}")))
        .collect()
}

async fn stage(pool: &PgPool, reg: &Registry, profile: &Profile, dataset: &str, rows: &[ObsRow]) {
    let run = RunTracker::start(pool, "it-src", "test")
        .await
        .expect("run");
    let art: i64 = sqlx::query_scalar(
        "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, archive_path)
         VALUES (sha256(convert_to($1, 'UTF8')), 'it-src', 'https://example.org', now(), 1, 'x')
         ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256 RETURNING id",
    )
    .bind(format!(
        "{dataset}-{}-{}",
        rows.len(),
        rows.first().and_then(|r| r.value).unwrap_or(0.0)
    ))
    .fetch_one(pool)
    .await
    .expect("artifact");
    let r = publish_observations(
        pool,
        reg,
        profile,
        StageInput {
            run_id: run.run_id,
            dataset_id: dataset,
            vintage: None,
            rows,
            artifact_ids: &[art],
            extract_ids: &[],
        },
    )
    .await
    .expect("stage");
    if r.outcome == "in_review" {
        // a revision of > 1 % of the rows waits for a reviewer
        lanka_ingest::publish::approve(pool, &r.dsv, "it-reviewer", Some("checked"))
            .await
            .expect("approve");
    } else {
        assert_eq!(r.outcome, "approved", "{dataset}: {r:?}");
    }
    run.finish(lanka_ingest::run::RunStatus::Succeeded, "fixture")
        .await
        .expect("finish");
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
    let url = format!("{base}/{db}");
    let result = tokio::spawn(f(url)).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop)))
        .await
        .expect("drop test database");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn as_role(url: &str, role: &'static str) -> PgPool {
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    let opts: PgConnectOptions = url.parse().expect("url");
    PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |c, _| {
            Box::pin(async move {
                c.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "SET ROLE {role}"
                ))))
                .await?;
                Ok(())
            })
        })
        .connect_with(opts)
        .await
        .expect("role pool")
}

fn relate(level: i16) -> Value {
    json!({"spec": "ldl-analysis/1", "method": "relate", "support": {"kind": "admin", "level": level},
           "variables": {"x": {"indicator": "it.share"}, "y": {"indicator": "it.pop", "transform": "log"}},
           "params": {"spatial": {"weights": "knn4", "permutations": 99}}})
}

fn strip(mut d: Value) -> Value {
    d["provenance"]["computed_at"] = Value::Null;
    d["provenance"]["elapsed_ms"] = Value::Null;
    d["provenance"]["data_version"] = Value::Null;
    d["request"]["pin"] = Value::Null;
    d["citation"] = Value::Null;
    d
}

#[tokio::test(flavor = "multi_thread")]
async fn analysis_cycle() {
    with_database("analysis", run_cycle).await;
}

#[allow(clippy::too_many_lines)]
async fn run_cycle(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = registry();
    let reg = Registry::load(&dir).expect("registry");
    reg.sync(&pool, &profile).await.expect("sync");

    // boundaries: 3 regions × 4 districts
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    let mut units = vec![unit("ZZ", 0, None, 80.0, 6.0, 3.0)];
    for r in 1..=3 {
        let x0 = 80.0 + f64::from(r - 1);
        units.push(unit(&format!("ZZ{r}"), 1, Some("ZZ"), x0, 6.0, 1.0));
        for d in 1..=4 {
            let (dx, dy) = (f64::from((d - 1) % 2) * 0.5, f64::from((d - 1) / 2) * 0.5);
            units.push(unit(
                &format!("ZZ{r}{d}"),
                2,
                Some(&format!("ZZ{r}")),
                x0 + dx,
                6.0 + dy,
                0.5,
            ));
        }
    }
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    geo_load::publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[],
        &[],
    )
    .await
    .expect("units");
    run.finish(lanka_ingest::run::RunStatus::Succeeded, "units")
        .await
        .expect("finish");

    let ds = districts();
    let mut xs: Vec<ObsRow> = ds
        .iter()
        .enumerate()
        .map(|(i, g)| row("it.share", g, 5.0 + 3.0 * i as f64 + (i % 3) as f64))
        .collect();
    // the share is also published per region (the blocked case reads it there)
    xs.extend((1..=3).map(|r| row("it.share", &format!("ZZ{r}"), 10.0 * f64::from(r))));
    let ys: Vec<ObsRow> = ds
        .iter()
        .enumerate()
        .map(|(i, g)| {
            row(
                "it.pop",
                g,
                1000.0 * (1.0 + i as f64) + 137.0 * (i % 4) as f64,
            )
        })
        .collect();
    stage(&pool, &reg, &profile, "it-x", &xs).await;
    stage(&pool, &reg, &profile, "it-y", &ys).await;
    let t = run_train_with(&pool, "test", None, None, None)
        .await
        .expect("train");
    assert_eq!(t.outcome, "released");

    let catalog = Arc::new(RwLock::new(Arc::new(
        Catalog::load(&pool).await.expect("catalog"),
    )));
    let reload = |c: &Arc<RwLock<Arc<Catalog>>>, fresh: Catalog| {
        *c.write().expect("lock") = Arc::new(fresh);
    };
    let analysis_pool = as_role(&url, "lanka_analysis").await;
    let who: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&analysis_pool)
        .await
        .expect("who");
    assert_eq!(who, "lanka_analysis");
    let store =
        std::env::temp_dir().join(format!("lanka-it-analysis-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&store);
    let cat2 = catalog.clone();
    let engine = lanka_analysis::Engine::new(
        analysis_pool.clone(),
        Arc::new(move || cat2.read().expect("lock").clone()),
        Some(store.clone()),
        "https://example.org".into(),
        "test".into(),
    );

    // same request twice → one id, the same bytes; second is a store read
    let a = engine.run(&relate(2)).await.expect("run");
    assert_eq!(a.state, "ready");
    assert!(a.computed);
    let b = engine.run(&relate(2)).await.expect("run again");
    assert_eq!(a.id, b.id);
    assert_eq!(a.bytes, b.bytes, "identical bytes");
    assert!(!b.computed, "served from the store");
    let doc: Value = serde_json::from_slice(&a.bytes).expect("doc");
    assert_eq!(doc["n"]["used"], 12);
    assert!(
        doc["guards"]
            .as_array()
            .expect("g")
            .iter()
            .any(|g| g["code"] == "SMALL_N")
    );
    assert!(
        doc["estimates"]["pearson"].get("p").is_none(),
        "SMALL_N: no p"
    );
    // recomputation bypassing the store: same id, same numbers
    let (id2, recomputed, _) = engine.compute_only(&relate(2)).await.expect("recompute");
    assert_eq!(id2, a.id);
    assert_eq!(strip(recomputed), strip(doc.clone()));
    // an output and the plan
    assert!(
        engine
            .output(&a.id, "bivariate")
            .await
            .expect("out")
            .is_some()
    );
    let plan = engine.plan(&relate(2)).await.expect("plan");
    assert_eq!(plan["id"], json!(a.id));
    assert_eq!(plan["support"]["n_expected"], 12);

    // an unrelated release keeps the id (and serves the same bytes)
    stage(
        &pool,
        &reg,
        &profile,
        "it-z",
        &ds.iter()
            .map(|g| row("it.other", g, 1.0))
            .collect::<Vec<_>>(),
    )
    .await;
    run_train_with(&pool, "test", None, None, None)
        .await
        .expect("train 2");
    let fresh = Catalog::load(&pool).await.expect("catalog");
    assert_ne!(fresh.data_version, catalog.read().expect("l").data_version);
    reload(&catalog, fresh);
    let c = engine
        .run(&relate(2))
        .await
        .expect("after unrelated release");
    assert_eq!(c.id, a.id);
    assert_eq!(c.bytes, a.bytes);
    let (id3, _, _) = engine.compute_only(&relate(2)).await.expect("recompute");
    assert_eq!(id3, a.id, "recomputed after the release: same fingerprint");

    // a new version of an input → a new id
    let mut xs2 = xs.clone();
    xs2[0].value = Some(99.0);
    xs2[0].verbatim = Some("99".into());
    stage(&pool, &reg, &profile, "it-x", &xs2).await;
    run_train_with(&pool, "test", None, None, None)
        .await
        .expect("train 3");
    reload(&catalog, Catalog::load(&pool).await.expect("catalog"));
    let d = engine.run(&relate(2)).await.expect("after input change");
    assert_ne!(d.id, a.id, "a changed input gives a new id");
    assert!(d.computed);
    // the old result is still served by id
    assert_eq!(
        engine.get(&a.id).await.expect("get").expect("old").bytes,
        a.bytes
    );

    // blocked: 3 regions < MIN_N 10
    let blk = engine.run(&relate(1)).await.expect("blocked run");
    assert_eq!(blk.state, "blocked");
    let bd: Value = serde_json::from_slice(&blk.bytes).expect("doc");
    assert!(bd["estimates"].is_null());
    assert_eq!(bd["outputs"], json!([]));
    assert_eq!(bd["guards"][0]["code"], "MIN_N");
    let stored: (String, i64) = sqlx::query_as(
        "SELECT r.state, (SELECT count(*) FROM analysis.result_inputs i WHERE i.result_id = r.id) FROM analysis.results r WHERE id = $1",
    )
    .bind(&blk.id)
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(stored, ("blocked".to_owned(), 2));

    // an unresolvable request is a 400 with hints, nothing stored
    let bad = json!({"spec": "ldl-analysis/1", "method": "relate", "support": {"kind": "admin", "level": 2},
                     "variables": {"x": {"indicator": "it.nope"}, "y": {"indicator": "it.pop"}}});
    assert!(matches!(
        engine.run(&bad).await,
        Err(lanka_analysis::EngineError::Bad { .. })
    ));

    // roles: lanka_analysis writes analysis.* only
    for sql in [
        "INSERT INTO data.releases (data_version) VALUES ('x')",
        "DELETE FROM data.observations",
        "UPDATE catalog.datasets SET title = 'x'",
        "DELETE FROM geo.units",
    ] {
        let e = sqlx::query(sql)
            .execute(&analysis_pool)
            .await
            .expect_err(sql);
        assert_eq!(
            e.as_database_error().and_then(|d| d.code()).as_deref(),
            Some("42501"),
            "{sql}: {e}"
        );
    }
    sqlx::query("UPDATE analysis.results SET pinned = true WHERE id = $1")
        .bind(&a.id)
        .execute(&analysis_pool)
        .await
        .expect("lanka_analysis writes analysis.*");
    let api = as_role(&url, "lanka_api").await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM analysis.results")
        .fetch_one(&api)
        .await
        .expect("api reads");
    assert!(n >= 3);
    let e = sqlx::query("DELETE FROM analysis.results")
        .execute(&api)
        .await
        .expect_err("api cannot write");
    let code = e
        .as_database_error()
        .and_then(|d| d.code())
        .unwrap_or_default()
        .into_owned();
    // lanka_api is read-only by default (25006) and has no write grant (42501)
    assert!(code == "25006" || code == "42501", "{code}: {e}");
    let e = sqlx::query("SET LOCAL transaction_read_only = off; DELETE FROM analysis.results")
        .execute(&api)
        .await;
    assert!(e.is_err(), "lanka_api has no DELETE on analysis.results");
    // grants survive a re-migrate
    lanka_core::db::migrate(&pool).await.expect("re-migrate");
    let ok: bool = sqlx::query_scalar("SELECT has_table_privilege('lanka_analysis', 'analysis.results', 'INSERT')
                                       AND NOT has_table_privilege('lanka_analysis', 'data.observations', 'INSERT')
                                       AND NOT has_table_privilege('lanka_api', 'analysis.results', 'INSERT')")
        .fetch_one(&pool)
        .await
        .expect("privileges");
    assert!(ok);
    analysis_pool.close().await;
    api.close().await;
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&dir);
}
