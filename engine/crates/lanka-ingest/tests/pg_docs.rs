//! The documents archive against a real Postgres, on a throwaway database.
//!
//! Gated like `pg_integration.rs`: runs only when `LANKA_TEST_DATABASE_URL`
//! points at a server where the user may CREATE DATABASE.
//!
//! The fixture records (`tests/fixtures/docs/p1.txt`, three real gazettes)
//! are upserted twice (one revision each, no duplicates), then one record is
//! changed (a second revision). Also covered: the doc_id collision suffix, a
//! replaced file going back to `pending`, withdrawal and reappearance, and
//! full-text search over `docs.search`.

use chrono::{Duration, Utc};
use lanka_core::db::{self, PoolRole};
use lanka_ingest::docs::harvest::{self, upsert_record};
use lanka_ingest::docs::record::{Kind, normalise};
use lanka_ingest::docs::rsc::parse_listing;
use sqlx::{Connection, Executor, PgConnection, PgPool};

const P1: &str = include_str!("fixtures/docs/p1.txt");
const COLLECTION: &str = "lk-gazette-extraordinary";

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

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .expect(sql)
}

#[tokio::test(flavor = "multi_thread")]
async fn documents_are_upserted_versioned_and_withdrawn() {
    with_database("docs", run).await;
}

async fn run(url: String) {
    let pool = db::connect(&url, PoolRole::Ingest).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");

    // the seeded collection and its settings
    let col = harvest::load_collection(&pool, COLLECTION)
        .await
        .expect("collection");
    assert_eq!(col.cfg.doc_prefix, "gze");
    assert_eq!(col.cfg.page_size, 200);
    assert!(col.cfg.file_proxy.ends_with("content-file-proxy?file="));
    let licence: String = sqlx::query_scalar("SELECT licence FROM docs.collections WHERE id = $1")
        .bind(COLLECTION)
        .fetch_one(&pool)
        .await
        .expect("licence");
    assert_eq!(licence, "public-domain-official-text");

    let page = parse_listing(P1).expect("parse").expect("frame");
    let mut recs: Vec<_> = page
        .records
        .iter()
        .map(|r| normalise(r, "gze").expect("normalise"))
        .collect();
    assert_eq!(recs.len(), 3);

    // ── first pass: everything is new ──
    let t0 = Utc::now() - Duration::hours(3);
    let mut tx = pool.begin().await.expect("tx");
    for r in &recs {
        let u = upsert_record(&mut tx, COLLECTION, r, t0)
            .await
            .expect("upsert");
        assert_eq!(u.sighting.kind, Kind::New);
    }
    tx.commit().await.expect("commit");
    assert_eq!(count(&pool, "SELECT count(*) FROM docs.documents").await, 3);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM docs.document_revisions").await,
        3
    );
    // 1 + 1 + 3 files, all pending
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM docs.files WHERE status = 'pending'"
        )
        .await,
        5
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM docs.documents WHERE doc_id = 'gze-2515-04'"
        )
        .await,
        1
    );

    // ── second pass: same records, nothing changes ──
    let t1 = t0 + Duration::hours(1);
    let mut tx = pool.begin().await.expect("tx");
    for r in &recs {
        let u = upsert_record(&mut tx, COLLECTION, r, t1)
            .await
            .expect("upsert");
        assert_eq!(u.sighting.kind, Kind::Unchanged);
        assert!(!u.sighting.reappeared);
    }
    tx.commit().await.expect("commit");
    assert_eq!(count(&pool, "SELECT count(*) FROM docs.documents").await, 3);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM docs.document_revisions").await,
        3
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM docs.files").await, 5);
    let seen: i64 = count(
        &pool,
        &format!(
            "SELECT count(*) FROM docs.documents WHERE last_seen_at = '{}'::timestamptz",
            t1.to_rfc3339()
        ),
    )
    .await;
    assert_eq!(seen, 3);

    // ── a changed record: a second revision, the row follows ──
    let mut raw = page.records[2].clone();
    raw.updated_at = Some("2026-11-24T08:00:00.000Z".into());
    raw.description_english = Some("Sri Lanka Customs - Rates of Exchange (corrected)".into());
    let changed = normalise(&raw, "gze").expect("normalise");
    assert_ne!(changed.meta_hash, recs[2].meta_hash);
    let t2 = t0 + Duration::hours(2);
    let mut tx = pool.begin().await.expect("tx");
    let u = upsert_record(&mut tx, COLLECTION, &changed, t2)
        .await
        .expect("upsert");
    tx.commit().await.expect("commit");
    assert_eq!(u.sighting.kind, Kind::Changed);
    assert_eq!(u.doc_id, "gze-2515-01");
    assert_eq!(count(&pool, "SELECT count(*) FROM docs.documents").await, 3);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM docs.document_revisions WHERE doc_id = 'gze-2515-01'"
        )
        .await,
        2
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM docs.document_revisions").await,
        4
    );
    let (title, hash): (String, String) = sqlx::query_as(
        "SELECT title ->> 'en', meta_hash FROM docs.documents WHERE doc_id = 'gze-2515-01'",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert!(title.ends_with("(corrected)"));
    assert_eq!(hash, changed.meta_hash);
    recs[2] = changed;

    // ── a different upstream record with the same number: suffix ──
    let mut dup = page.records[0].clone();
    dup.id = "ffffffff-0000-0000-0000-000000000000".into();
    let dup = normalise(&dup, "gze").expect("normalise");
    assert_eq!(dup.doc_id_base, "gze-2515-04");
    let mut tx = pool.begin().await.expect("tx");
    let u = upsert_record(&mut tx, COLLECTION, &dup, t2)
        .await
        .expect("upsert");
    tx.commit().await.expect("commit");
    assert_eq!(u.doc_id, "gze-2515-04-ffffff");
    assert_eq!(count(&pool, "SELECT count(*) FROM docs.documents").await, 4);

    // ── a replaced file goes back to pending, an unchanged one keeps its state ──
    sqlx::query("UPDATE docs.files SET status = 'ok', attempts = 1, sha256 = 'aa'")
        .execute(&pool)
        .await
        .expect("mark ok");
    let mut raw = page.records[0].clone();
    raw.updated_at = Some("2026-11-24T09:00:00.000Z".into());
    raw.contents[0].uploaded_file = Some("extra-gazette-content/replaced.pdf".into());
    let replaced = normalise(&raw, "gze").expect("normalise");
    let mut tx = pool.begin().await.expect("tx");
    upsert_record(&mut tx, COLLECTION, &replaced, t2 + Duration::minutes(1))
        .await
        .expect("upsert");
    tx.commit().await.expect("commit");
    let (status, attempts, path): (String, i32, String) = sqlx::query_as(
        "SELECT status, attempts, upstream_path FROM docs.files WHERE doc_id = 'gze-2515-04' AND lang = 'si'",
    )
    .fetch_one(&pool)
    .await
    .expect("file");
    assert_eq!((status.as_str(), attempts), ("pending", 0));
    assert_eq!(path, "extra-gazette-content/replaced.pdf");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM docs.files WHERE status = 'ok'").await,
        5
    );

    // ── withdrawal: not seen since the sweep started; cleared when it returns ──
    let sweep_started = t2 + Duration::minutes(30);
    // the sweep sees only the first two records (and the collision twin is not listed)
    let mut tx = pool.begin().await.expect("tx");
    for r in [&replaced, &recs[1]] {
        upsert_record(&mut tx, COLLECTION, r, sweep_started + Duration::seconds(1))
            .await
            .expect("upsert");
    }
    tx.commit().await.expect("commit");
    let n = harvest::withdraw_unseen(&pool, COLLECTION, sweep_started)
        .await
        .expect("withdraw");
    assert_eq!(n, 2); // gze-2515-01 and the twin
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM docs.documents WHERE withdrawn_at IS NOT NULL"
        )
        .await,
        2
    );
    // withdrawing again changes nothing
    assert_eq!(
        harvest::withdraw_unseen(&pool, COLLECTION, sweep_started)
            .await
            .expect("again"),
        0
    );
    // the withdrawn document is listed again
    let mut tx = pool.begin().await.expect("tx");
    let u = upsert_record(
        &mut tx,
        COLLECTION,
        &recs[2],
        sweep_started + Duration::hours(1),
    )
    .await
    .expect("upsert");
    tx.commit().await.expect("commit");
    assert_eq!(u.sighting.kind, Kind::Unchanged);
    assert!(u.sighting.reappeared);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM docs.documents WHERE withdrawn_at IS NOT NULL"
        )
        .await,
        1
    );
    // no revision for a mere reappearance
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM docs.document_revisions WHERE doc_id = 'gze-2515-01'"
        )
        .await,
        2
    );

    // ── full-text search ──
    let note = lanka_ingest::docs::files::index_text(
        &pool,
        "gze-2515-01",
        "en",
        "Customs Notification: Rates of Exchange effective from 17 November 2026",
    )
    .await
    .expect("index");
    assert!(note.is_none());
    lanka_ingest::docs::files::index_text(&pool, "gze-2515-04", "si", "මැතිවරණ කොමිෂන් සභාව")
        .await
        .expect("index si");
    let hits = harvest::search(&pool, "exchange rates", 10)
        .await
        .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].doc_id, "gze-2515-01");
    assert_eq!(hits[0].lang, "en");
    let hits = harvest::search(&pool, "කොමිෂන්", 10)
        .await
        .expect("search si");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].doc_id, "gze-2515-04");
    assert!(
        harvest::search(&pool, "nonexistentword", 10)
            .await
            .expect("none")
            .is_empty()
    );

    // ── status ──
    let s = harvest::status(&pool, COLLECTION).await.expect("status");
    assert_eq!(s.documents, 4);
    assert_eq!(s.withdrawn, 1);
    assert_eq!(s.searchable, 2);

    // ── roles: the api reads, the ingest role writes, the revisions are append-only ──
    let mut api = PgConnection::connect(&url).await.expect("connect");
    api.execute("SET ROLE lanka_api").await.expect("set role");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM docs.documents")
        .fetch_one(&mut api)
        .await
        .expect("api read");
    assert_eq!(n, 4);
    let denied = api
        .execute("DELETE FROM docs.documents")
        .await
        .expect_err("api must not write");
    assert!(matches!(&denied, sqlx::Error::Database(e) if e.code().as_deref() == Some("42501")));
    let mut ing = PgConnection::connect(&url).await.expect("connect");
    ing.execute("SET ROLE lanka_ingest")
        .await
        .expect("set role");
    let denied = ing
        .execute("UPDATE docs.document_revisions SET meta_hash = 'x'")
        .await
        .expect_err("revisions are append-only");
    assert!(matches!(&denied, sqlx::Error::Database(e) if e.code().as_deref() == Some("42501")));
}
