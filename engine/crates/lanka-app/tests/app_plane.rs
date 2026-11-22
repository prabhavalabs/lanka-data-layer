//! The app plane against a real PostGIS server, on throwaway databases.
//!
//! Gated: run only when `LANKA_TEST_DATABASE_URL` points at a server where
//! the user may CREATE DATABASE (e.g. the local infra/v2 compose stack):
//!
//!   LANKA_TEST_DATABASE_URL=postgres://lanka:lanka-local-dev@127.0.0.1:55432/postgres cargo test -p lanka-app
//!
//! - `roles_are_isolated`: `lanka_app` cannot write any table of the public
//!   plane (`catalog`, `geo`, `data`, `serve`, `ops`) nor create objects
//!   there; it reads the public plane and writes `app.*`. `lanka_api`,
//!   `lanka_ingest` and `lanka_recipe` cannot read `app.*`.
//! - `snapshots_end_to_end`: the 10 §7 example and the four web-v2 starter
//!   maps through `/app/v1` on pool C (`SET ROLE lanka_app`): same document
//!   twice → one id; a 300 KB document is rejected; the snapshot rate limit;
//!   the stored form hashes to its id; the resolved example carries
//!   attribution and the estimate notice; the package lists downloads and
//!   keeps private data metadata-only; withdrawal; expiry.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use lanka_app::{AppConfig, AppPlane, Rate};
use lanka_core::Catalog;
use lanka_core::catalog::{CurrentVersion, DatasetInfo, IndicatorInfo, LicenceInfo, SourceInfo};
use serde_json::{Value, json};
use sqlx::{Connection, Executor, PgConnection};
use tower::ServiceExt;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn fixture(rel: &str) -> Value {
    let p = repo_root()
        .join("engine/crates/lanka-mapdoc/tests/fixtures")
        .join(rel);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).expect("fixture JSON")
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
    let db = format!("lanka_it_app_{tag}_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url)
        .await
        .expect("connect admin");
    for sql in [
        format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"),
        format!("CREATE DATABASE {db}"),
    ] {
        admin
            .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)))
            .await
            .expect("create");
    }
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

/// The SQLSTATE of running `sql` on `conn` (`None` = success).
async fn sqlstate(conn: &mut PgConnection, sql: &str) -> Option<String> {
    match conn
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned())))
        .await
    {
        Ok(_) => None,
        Err(sqlx::Error::Database(e)) => Some(e.code().map(|c| c.to_string()).unwrap_or_default()),
        Err(e) => panic!("{sql}: {e}"),
    }
}

const DENIED: &str = "42501";

#[tokio::test(flavor = "multi_thread")]
async fn roles_are_isolated() {
    with_database("roles", run_roles).await;
}

async fn run_roles(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    // every table of the public plane, with one column for UPDATE
    let tables: Vec<(String, String)> = sqlx::query_as(
        "SELECT format('%I.%I', n.nspname, c.relname),
                (SELECT a.attname::text FROM pg_attribute a
                 WHERE a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
                   AND a.attidentity = '' AND a.attgenerated = ''
                 ORDER BY a.attnum LIMIT 1)
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname IN ('catalog', 'geo', 'data', 'serve', 'ops') AND c.relkind IN ('r', 'p')
           AND NOT c.relispartition
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .expect("tables");
    assert!(tables.len() > 30, "{} tables", tables.len());
    for s in ["catalog", "geo", "data"] {
        assert!(
            tables.iter().any(|(t, _)| t.starts_with(&format!("{s}."))),
            "{s}"
        );
    }

    let mut app = PgConnection::connect(&url).await.expect("connect");
    app.execute("SET ROLE lanka_app").await.expect("set role");
    let who: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&mut app)
        .await
        .expect("who");
    assert_eq!(who, "lanka_app");
    let mut writable = Vec::new();
    for (t, col) in &tables {
        for sql in [
            // DEFAULT VALUES: no column analysis, so the privilege check is what fails
            format!("INSERT INTO {t} DEFAULT VALUES"),
            format!("UPDATE {t} SET \"{col}\" = \"{col}\" WHERE false"),
            format!("DELETE FROM {t} WHERE false"),
            format!("TRUNCATE {t}"),
        ] {
            let code = sqlstate(&mut app, &sql).await;
            if code.as_deref() != Some(DENIED) {
                writable.push(format!("{sql} → {code:?}"));
            }
        }
    }
    assert!(writable.is_empty(), "lanka_app may write: {writable:#?}");
    for sql in [
        "CREATE TABLE data.x (i int)",
        "CREATE TABLE catalog.x (i int)",
        "CREATE TABLE geo.x (i int)",
        "CREATE TABLE app.x (i int)",
        "SELECT ops.apply_grants()",
        "SELECT app.apply_grants()",
        "SELECT count(*) FROM ops.jobs",
    ] {
        assert_eq!(
            sqlstate(&mut app, sql).await.as_deref(),
            Some(DENIED),
            "{sql}"
        );
    }
    // …but it reads the public plane and writes its own schema
    for sql in [
        "SELECT count(*) FROM catalog.datasets",
        "SELECT count(*) FROM geo.units",
        "SELECT count(*) FROM data.releases",
        "SELECT count(*) FROM data.observations",
        "SELECT count(*) FROM serve.area_latest",
        "INSERT INTO app.map_snapshots (id, doc, doc_sha256, bytes)
         VALUES ('ms_' || left(encode(sha256('x'), 'hex'), 16),
                 jsonb_build_object('spec', 'ldl-map/1', 'id', 'ms_' || left(encode(sha256('x'), 'hex'), 16)),
                 sha256('x'), 1)",
        "UPDATE app.map_snapshots SET last_read_at = current_date",
        "DELETE FROM app.map_snapshots",
    ] {
        assert_eq!(sqlstate(&mut app, sql).await, None, "{sql}");
    }
    // the table's own checks: the id is the content hash
    assert_eq!(
        sqlstate(
            &mut app,
            "INSERT INTO app.map_snapshots (id, doc, doc_sha256, bytes)
             VALUES ('ms_0000000000000000', '{\"spec\":\"ldl-map/1\",\"id\":\"ms_0000000000000000\"}', sha256('x'), 1)"
        )
        .await
        .as_deref(),
        Some("23514")
    );

    // the public roles cannot see the app plane at all
    for role in ["lanka_api", "lanka_ingest", "lanka_recipe"] {
        let mut c = PgConnection::connect(&url).await.expect("connect");
        c.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "SET ROLE {role}"
        ))))
        .await
        .expect("set role");
        for sql in [
            "SELECT * FROM app.map_snapshots",
            "INSERT INTO app.map_snapshots (id, doc, doc_sha256, bytes) VALUES ('ms_0', '{}', '\\x00', 1)",
            "DELETE FROM app.map_snapshots",
        ] {
            assert_eq!(
                sqlstate(&mut c, sql).await.as_deref(),
                Some(DENIED),
                "{role}: {sql}"
            );
        }
        let usage: bool =
            sqlx::query_scalar("SELECT has_schema_privilege(current_user, 'app', 'USAGE')")
                .fetch_one(&mut c)
                .await
                .expect("privilege");
        assert!(!usage, "{role} has USAGE on app");
    }

    // grants survive a re-run of the migration (ops.apply_grants then app.apply_grants)
    lanka_core::db::migrate(&pool).await.expect("re-migrate");
    assert_eq!(
        sqlstate(&mut app, "DELETE FROM catalog.datasets WHERE false")
            .await
            .as_deref(),
        Some(DENIED)
    );
    assert_eq!(
        sqlstate(&mut app, "SELECT count(*) FROM app.map_snapshots").await,
        None
    );
}

// ───────────────────────────── /app/v1 ─────────────────────────────────────

/// A catalogue in which the datasets of the 10 §7 example exist.
fn example_catalog() -> Catalog {
    let profile = lanka_core::Profile::discover(&repo_root().join("registry"))
        .and_then(lanka_core::Profile::load)
        .expect("repository profile");
    let mut c = Catalog {
        data_version: Some("20261122.3".into()),
        profile: Some(Arc::new(profile)),
        ..Catalog::default()
    };
    let ds = |id: &str, kind: &str, dsv: &str, key: i32, src: &str, inds: &[&str]| DatasetInfo {
        id: id.into(),
        key,
        kind: kind.into(),
        title: id.into(),
        description: String::new(),
        method: String::new(),
        limits: Vec::new(),
        publication_class: "A".into(),
        time_from: None,
        time_to: None,
        source_ids: vec![src.into()],
        indicator_ids: inds.iter().map(|s| (*s).to_owned()).collect(),
        current: Some(CurrentVersion {
            version_id: i64::from(key),
            seq: 1,
            dsv: dsv.into(),
            vintage: None,
            row_count: 10,
            published_at: None,
        }),
    };
    for d in [
        ds(
            "derived-flood-exposure",
            "indicators",
            "dsv_41c0a9e77b1d2f08",
            1,
            "ldl-derived",
            &["exp.flood_pop_share"],
        ),
        ds(
            "cod-ab-lka-boundaries",
            "boundaries",
            "dsv_9f3a1c2e77b04d15",
            2,
            "hdx-cod-ab-lka",
            &[],
        ),
        ds(
            "map-of-elsewhere",
            "indicators",
            "dsv_00000000000000aa",
            3,
            "link-only",
            &["oth.link_only"],
        ),
    ] {
        c.dataset_order.push(d.id.clone());
        c.dataset_by_key.insert(d.key, d.id.clone());
        c.datasets.insert(d.id.clone(), d);
    }
    let ind = |id: &str, ds: &str, class: &str, dims: &[&str]| IndicatorInfo {
        id: id.into(),
        key: 1,
        dataset_id: ds.into(),
        label: id.into(),
        unit: Some("1".into()),
        unit_label: None,
        unit_mult: 0,
        dims: dims.iter().map(|s| (*s).to_owned()).collect(),
        geo_levels: vec!["adm3".into(), "adm4".into()],
        period_type: "year".into(),
        freq: "A".into(),
        measure_type: "share".into(),
        aggregation: "mean".into(),
        class: class.into(),
    };
    c.indicators.insert(
        "exp.flood_pop_share".into(),
        ind(
            "exp.flood_pop_share",
            "derived-flood-exposure",
            "estimated",
            &["return_period"],
        ),
    );
    c.indicators.insert(
        "oth.link_only".into(),
        ind("oth.link_only", "map-of-elsewhere", "third_party", &[]),
    );
    let src = |id: &str, publisher: &str, lic: &str| SourceInfo {
        id: id.into(),
        publisher: publisher.into(),
        title: format!("{id} data"),
        homepage: "https://example.org".into(),
        licence_id: lic.into(),
        attribution: publisher.into(),
        cadence: "annual".into(),
        access_method: "file".into(),
        themes: Vec::new(),
        status: "active".into(),
    };
    c.sources.insert(
        "ldl-derived".into(),
        src("ldl-derived", "Lanka Data Layer (derived)", "cc-by-4.0"),
    );
    c.sources.insert(
        "hdx-cod-ab-lka".into(),
        src("hdx-cod-ab-lka", "OCHA / Survey Department", "cc-by-igo"),
    );
    c.sources.insert(
        "link-only".into(),
        src("link-only", "Elsewhere Inc.", "proprietary"),
    );
    let lic = |id: &str, status: &str, spdx: Option<&str>, redistribution: &str| LicenceInfo {
        id: id.into(),
        status: status.into(),
        url: Some(format!("https://licences.example.org/{id}")),
        spdx: spdx.map(str::to_owned),
        redistribution: redistribution.into(),
    };
    c.licences.insert(
        "cc-by-4.0".into(),
        lic("cc-by-4.0", "attribution", Some("CC-BY-4.0"), "bulk"),
    );
    c.licences.insert(
        "cc-by-igo".into(),
        lic("cc-by-igo", "attribution", Some("CC-BY-3.0-IGO"), "bulk"),
    );
    c.licences.insert(
        "proprietary".into(),
        lic("proprietary", "restricted", None, "link"),
    );
    c
}

struct Client {
    router: Router,
}

impl Client {
    async fn send(&self, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
        let res = self.router.clone().oneshot(req).await.expect("response");
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.expect("body").to_bytes();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(
        &self,
        uri: &str,
        ip: &str,
        body: &Value,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        self.send(
            Request::post(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-forwarded-for", ip)
                .body(Body::from(serde_json::to_vec(body).expect("json")))
                .expect("request"),
        )
        .await
    }

    async fn get(&self, uri: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
        self.send(Request::get(uri).body(Body::empty()).expect("request"))
            .await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshots_end_to_end() {
    with_database("snap", run_snapshots).await;
}

#[allow(clippy::too_many_lines)]
async fn run_snapshots(url: String) {
    let admin = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&admin).await.expect("migrate");
    let cfg = AppConfig {
        enabled: true,
        snapshot_rate: Rate::parse("5/h").expect("rate"),
        ip_salt: Some("it".into()),
        public_url: Some("https://maps.example.org".into()),
        ..AppConfig::default()
    };
    // pool C through the fallback path: the admin's options + SET ROLE lanka_app
    let pool = lanka_app::connect(&cfg, &admin.connect_options())
        .await
        .expect("app pool");
    let cat = Arc::new(example_catalog());
    let plane = AppPlane::new(pool.clone(), Arc::new(move || Arc::clone(&cat)), cfg);
    let c = Client {
        router: plane.router(),
    };
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM app.map_snapshots")
            .fetch_one(&admin)
            .await
            .expect("count")
    };

    // 1. the 10 §7 example: same document twice → one id
    let example = fixture("valid/flood-prices.json");
    let (status, h, v) = c
        .post("/app/v1/maps/snapshots", "198.51.100.1", &example)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let id = v["payload"]["id"].as_str().expect("id").to_owned();
    assert_eq!(id, lanka_mapdoc::canon::snapshot_id(&example));
    assert_eq!(
        v["payload"]["url"],
        format!("https://maps.example.org/m/{id}")
    );
    assert_eq!(v["payload"]["created"], true);
    assert_eq!(
        h.get(header::LOCATION).and_then(|v| v.to_str().ok()),
        Some(format!("/app/v1/maps/{id}").as_str())
    );
    let (status, _, v) = c
        .post("/app/v1/maps/snapshots", "198.51.100.2", &example)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (
            v["payload"]["id"].as_str(),
            v["payload"]["created"].as_bool()
        ),
        (Some(id.as_str()), Some(false))
    );
    // `id`, `meta.updated_at` and null members are not content
    let mut same = example.clone();
    same["id"] = json!("m_another");
    same["meta"]["updated_at"] = json!("2027-04-01T00:00:00Z");
    same["description"] = example["description"].clone();
    same["x-note"] = Value::Null;
    let (status, _, v) = c
        .post("/app/v1/maps/snapshots", "198.51.100.3", &same)
        .await;
    assert_eq!(
        (status, v["payload"]["id"].as_str()),
        (StatusCode::OK, Some(id.as_str()))
    );
    assert_eq!(count().await, 1);
    let (ip_hash, bytes): (Option<Vec<u8>>, i32) =
        sqlx::query_as("SELECT created_ip_hash, bytes FROM app.map_snapshots WHERE id = $1")
            .bind(&id)
            .fetch_one(&admin)
            .await
            .expect("row");
    assert_eq!(
        ip_hash.map(|h| h.len()),
        Some(32),
        "salted hash, never the address"
    );
    assert!(bytes > 1000);

    // 2. the stored document comes back with its id and hashes to it
    let (status, h, v) = c.get(&format!("/app/v1/maps/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["payload"]["id"], id.as_str());
    assert_eq!(lanka_mapdoc::canon::snapshot_id(&v["payload"]), id);
    assert_eq!(
        lanka_mapdoc::canon::jcs(&lanka_mapdoc::canon::content_value(&v["payload"])),
        lanka_mapdoc::canon::jcs(&lanka_mapdoc::canon::content_value(&example)),
        "stored content = posted content"
    );
    let etag = h.get(header::ETAG).expect("etag").clone();
    assert!(
        h.get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("private"))
    );
    let (status, _, _) = c
        .send(
            Request::get(format!("/app/v1/maps/{id}"))
                .header(header::IF_NONE_MATCH, etag)
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    let (status, _, _) = c.get("/app/v1/maps/ms_ffffffffffffffff").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 3. the resolved example carries attribution and the estimate notice
    let (status, h, v) = c.get(&format!("/app/v1/maps/{id}/resolved")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let r = &v["payload"]["resolved"];
    let notices: Vec<&str> = r["notices"]
        .as_array()
        .expect("notices")
        .iter()
        .filter_map(|n| n["code"].as_str())
        .collect();
    assert!(notices.contains(&"ESTIMATE"), "{notices:?}");
    assert!(notices.contains(&"PRIVATE_DATA"), "{notices:?}");
    let attribution: Vec<&str> = r["attribution"]
        .as_array()
        .expect("attribution")
        .iter()
        .filter_map(|a| a["source_id"].as_str())
        .collect();
    assert!(attribution.contains(&"ldl-derived"), "{attribution:?}");
    assert!(attribution.contains(&"hdx-cod-ab-lka"), "{attribution:?}");
    assert_eq!(r["sources"]["exposure"]["shape"], "frame");
    assert_eq!(r["sources"]["exposure"]["class"], "estimated");
    assert_eq!(r["sources"]["prices"]["access"], "denied");
    assert_eq!(v["payload"]["title"]["en"], example["title"]["en"]);
    // the example's release does not exist here: a repair problem, not a failure
    assert!(
        r["problems"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p["path"] == "/pin"))
    );
    assert!(h.get(header::ETAG).is_some());

    // 4. the package: downloads for public data, metadata only for private data
    let (status, _, pkg) = c.get(&format!("/app/v1/maps/{id}/package")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pkg["profile"], "data-package");
    let only: Vec<&str> = pkg["ldl:metadata_only"]
        .as_array()
        .expect("metadata_only")
        .iter()
        .filter_map(|m| m["source"].as_str())
        .collect();
    assert!(
        only.contains(&"zone_pts") && only.contains(&"dsd"),
        "{only:?}"
    );
    assert!(
        !pkg.to_string()
            .contains("@demo/property-asking-prices-sample\",\"path")
    );
    for r in pkg["resources"].as_array().expect("resources") {
        assert!(
            r["path"]
                .as_str()
                .is_some_and(|p| p.starts_with("https://maps.example.org/v2/"))
        );
        assert!(
            r["licenses"].as_array().is_some_and(|l| !l.is_empty()),
            "{r}"
        );
    }

    // 5. a 300 KB document is rejected and nothing is stored
    let mut big = example.clone();
    big["title"] = json!("big");
    big["x-padding"] = json!("x".repeat(300 * 1024));
    let (status, _, v) = c.post("/app/v1/maps/snapshots", "198.51.100.4", &big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert!(
        v["payload"]["problems"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p["code"] == "TOO_LARGE"))
    );
    assert_eq!(count().await, 1);
    let (status, _, _) = c.post("/app/v1/maps/resolve", "198.51.100.4", &big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // 6. rate limit: 5 new snapshots per hour per identity
    let mut doc = fixture("web/web-m_glance-light.json");
    for i in 0..5 {
        doc["title"] = json!(format!("rate {i}"));
        let (status, _, v) = c.post("/app/v1/maps/snapshots", "203.0.113.9", &doc).await;
        assert_eq!(status, StatusCode::CREATED, "{i}: {v}");
    }
    doc["title"] = json!("rate 5");
    let (status, h, v) = c.post("/app/v1/maps/snapshots", "203.0.113.9", &doc).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert_eq!(v["payload"]["type"], "rate_limited");
    let retry: u64 = h
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .expect("Retry-After");
    assert!((600..=720).contains(&retry), "{retry}");
    // re-posting stored content is free; other identities are unaffected
    doc["title"] = json!("rate 0");
    let (status, _, _) = c.post("/app/v1/maps/snapshots", "203.0.113.9", &doc).await;
    assert_eq!(status, StatusCode::OK);
    doc["title"] = json!("rate 5");
    let (status, _, _) = c.post("/app/v1/maps/snapshots", "203.0.113.10", &doc).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(count().await, 7);

    // 7. the four web-v2 starter maps round-trip and resolve
    for f in [
        "web/web-m_flood_prices-light.json",
        "web/web-m_glance-light.json",
        "web/web-m_hospital_access-light.json",
        "web/web-m_population_rail-light.json",
    ] {
        let d = fixture(f);
        let ip = format!("192.0.2.{}", f.len());
        let (status, _, v) = c.post("/app/v1/maps/snapshots", &ip, &d).await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{f}: {v}"
        );
        let sid = v["payload"]["id"].as_str().expect("id").to_owned();
        let (_, _, back) = c.get(&format!("/app/v1/maps/{sid}")).await;
        assert_eq!(
            lanka_mapdoc::canon::snapshot_id(&back["payload"]),
            sid,
            "{f}"
        );
        let (status, _, r) = c.post("/app/v1/maps/resolve", &ip, &d).await;
        assert_eq!(status, StatusCode::OK, "{f}");
        assert!(r["payload"]["resolved"]["attribution"].is_array(), "{f}");
        let (status, _, r2) = c.get(&format!("/app/v1/maps/{sid}/resolved")).await;
        assert_eq!(status, StatusCode::OK, "{f}");
        assert_eq!(r["payload"]["resolved"], r2["payload"]["resolved"], "{f}");
    }

    // 8. invalid documents: JSON-pointer problems, nothing stored
    let mut bad = example.clone();
    bad["layers"][0]["source"] = json!("nope");
    let before = count().await;
    let (status, _, v) = c.post("/app/v1/maps/snapshots", "198.51.100.5", &bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        v["payload"]["problems"]
            .as_array()
            .is_some_and(|p| p.iter().any(|p| p["path"]
                .as_str()
                .is_some_and(|s| s.starts_with("/layers/0"))))
    );
    assert_eq!(count().await, before);

    // 9. takedown: withdrawn snapshots are 410 and cannot be re-created
    sqlx::query("UPDATE app.map_snapshots SET withdrawn = true WHERE id = $1")
        .bind(&id)
        .execute(&admin)
        .await
        .expect("withdraw");
    for uri in [
        format!("/app/v1/maps/{id}"),
        format!("/app/v1/maps/{id}/resolved"),
        format!("/app/v1/maps/{id}/package"),
    ] {
        let (status, _, _) = c.get(&uri).await;
        assert_eq!(status, StatusCode::GONE, "{uri}");
    }
    let (status, _, _) = c
        .post("/app/v1/maps/snapshots", "198.51.100.6", &example)
        .await;
    assert_eq!(status, StatusCode::GONE);

    // 10. reads touch last_read_at; anonymous snapshots unread for 24 months expire
    sqlx::query("UPDATE app.map_snapshots SET last_read_at = current_date - 800")
        .execute(&admin)
        .await
        .expect("age");
    let first = fixture("web/web-m_glance-light.json");
    let sid = lanka_mapdoc::canon::snapshot_id(&first);
    let (status, _, _) = c.get(&format!("/app/v1/maps/{sid}")).await;
    assert_eq!(status, StatusCode::OK);
    let n = lanka_app::snapshots::expire(&pool).await.expect("expire");
    assert_eq!(
        i64::try_from(n).expect("n"),
        before - 1,
        "all but the one just read"
    );
}
