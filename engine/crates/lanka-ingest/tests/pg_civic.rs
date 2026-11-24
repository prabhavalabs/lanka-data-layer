//! The civic record against a real Postgres, on a throwaway database
//! (13 §2.1): assignments built from small baselines, the continuity of an
//! institution across them (with an alias), and the diff and structure reads.
//!
//! Gated like `pg_docs.rs`: runs only when `LANKA_TEST_DATABASE_URL` points at
//! a server where the user may CREATE DATABASE.

use chrono::NaiveDate;
use lanka_core::civic;
use lanka_core::db::{self, PoolRole};
use lanka_ingest::civic::build;
use lanka_ingest::civic::entity::Aliases;
use lanka_ingest::civic::extract;
use sqlx::{Connection, Executor, PgConnection, PgPool};

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

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("date")
}

async fn document(pool: &PgPool, doc_id: &str, issued: NaiveDate) {
    sqlx::query(
        "INSERT INTO docs.documents (doc_id, collection, number, sub_number, issued_on, title, upstream_id, meta_hash)
         VALUES ($1, 'lk-gazette-extraordinary', '1', '01', $2, $3, $1, 'h')",
    )
    .bind(doc_id)
    .bind(issued)
    .bind(serde_json::json!({"en": format!("Presidential Secretariat - {doc_id}")}))
    .execute(pool)
    .await
    .expect("document");
}

async fn instrument(
    pool: &PgPool,
    doc_id: &str,
    kind: &str,
    eff: NaiveDate,
    status: &str,
    amends: Option<&str>,
) {
    document(pool, doc_id, eff).await;
    sqlx::query(
        "INSERT INTO civic.instruments (doc_id, kind, effective_from, amends, parser, status)
         VALUES ($1, $2, $3, $4, 'test', $5)",
    )
    .bind(doc_id)
    .bind(kind)
    .bind(eff)
    .bind(amends)
    .bind(status)
    .execute(pool)
    .await
    .expect("instrument");
}

/// One minister heading with its institutions (numbered 1..n), one function and one law.
async fn portfolio(
    pool: &PgPool,
    doc_id: &str,
    seq: i32,
    title: &str,
    key: &str,
    institutions: &[&str],
) {
    sqlx::query("INSERT INTO civic.portfolios (doc_id, seq, title, portfolio_key, first_page) VALUES ($1, $2, $3, $4, 2)")
        .bind(doc_id)
        .bind(seq)
        .bind(title)
        .bind(key)
        .execute(pool)
        .await
        .expect("portfolio");
    for (col, no, text) in [
        ("function", 1, "Formulation of policies"),
        ("law", 1, "Some Act No. 1 of 1990"),
    ] {
        sqlx::query("INSERT INTO civic.items (doc_id, seq, col, item_no, text, page) VALUES ($1, $2, $3, $4, $5, 2)")
            .bind(doc_id)
            .bind(seq)
            .bind(col)
            .bind(no)
            .bind(text)
            .execute(pool)
            .await
            .expect("item");
    }
    for (i, name) in institutions.iter().enumerate() {
        sqlx::query("INSERT INTO civic.items (doc_id, seq, col, item_no, text, page) VALUES ($1, $2, 'institution', $3, $4, 3)")
            .bind(doc_id)
            .bind(seq)
            .bind(i32::try_from(i + 1).expect("n"))
            .bind(*name)
            .execute(pool)
            .await
            .expect("institution");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn assignments_are_built_continuously_and_diffed() {
    with_database("civic", run).await;
}

async fn run(url: String) {
    let pool = db::connect(&url, PoolRole::Ingest).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");
    // the grants function exists and is idempotent
    sqlx::query("SELECT civic.apply_grants()")
        .execute(&pool)
        .await
        .expect("grants");

    // A (2020) and B (2021) are baselines; X (2022) needs review; M amends A.
    instrument(
        &pool,
        "gze-1000-01",
        "baseline",
        d(2020, 1, 1),
        "validated",
        None,
    )
    .await;
    portfolio(
        &pool,
        "gze-1000-01",
        1,
        "Minister of Alpha",
        "alpha",
        &["Alpha Authority", "Beta Board", "Eta Institute"],
    )
    .await;
    portfolio(
        &pool,
        "gze-1000-01",
        2,
        "Minister of Beta",
        "beta",
        &["Gamma Fund", "Epsilon Department"],
    )
    .await;

    instrument(
        &pool,
        "gze-2000-01",
        "baseline",
        d(2021, 1, 1),
        "validated",
        None,
    )
    .await;
    portfolio(
        &pool,
        "gze-2000-01",
        1,
        "Minister of Alpha",
        "alpha",
        &["Alpha Authority", "Gamma Fund"],
    )
    .await;
    portfolio(
        &pool,
        "gze-2000-01",
        2,
        "Minister of Beta",
        "beta",
        &["Epsilon Dept"],
    )
    .await;
    portfolio(
        &pool,
        "gze-2000-01",
        3,
        "Minister of Gamma",
        "gamma",
        &["Beta Board", "Zeta Council"],
    )
    .await;

    instrument(
        &pool,
        "gze-3000-01",
        "baseline",
        d(2022, 1, 1),
        "needs_review",
        None,
    )
    .await;
    portfolio(
        &pool,
        "gze-3000-01",
        1,
        "Minister of Beta",
        "beta",
        &["Alpha Authority"],
    )
    .await;

    instrument(
        &pool,
        "gze-1500-01",
        "amendment",
        d(2020, 6, 1),
        "validated",
        Some("gze-1000-01"),
    )
    .await;

    let aliases = Aliases::parse("\"Epsilon Dept\": \"Epsilon Department\"\n").expect("aliases");
    let r = build::build(&pool, &aliases).await.expect("build");
    assert_eq!(r.steps, 2, "the needs_review baseline is not built");
    assert_eq!(r.shared_listings, 0);

    // rebuilding changes nothing
    let r2 = build::build(&pool, &aliases).await.expect("rebuild");
    assert_eq!(
        (r2.entities, r2.assignments, r2.open),
        (r.entities, r.assignments, r.open)
    );

    let rows: Vec<(String, NaiveDate, Option<NaiveDate>, String)> = sqlx::query_as(
        "SELECT entity_key, valid_from, valid_to, portfolio_key FROM civic.assignments ORDER BY entity_key, valid_from",
    )
    .fetch_all(&pool)
    .await
    .expect("assignments");
    let find = |k: &str| -> Vec<_> { rows.iter().filter(|r| r.0 == k).cloned().collect() };
    // continuous across both baselines, in force
    assert_eq!(
        find("alpha-authority"),
        vec![(
            "alpha-authority".into(),
            d(2020, 1, 1),
            None,
            "alpha".into()
        )]
    );
    // the alias joins "Epsilon Dept" to "Epsilon Department": one row
    assert_eq!(
        find("epsilon-department"),
        vec![(
            "epsilon-department".into(),
            d(2020, 1, 1),
            None,
            "beta".into()
        )]
    );
    assert!(find("epsilon-dept").is_empty());
    // moved: two rows that meet at the new baseline's date
    assert_eq!(
        find("beta-board"),
        vec![
            (
                "beta-board".into(),
                d(2020, 1, 1),
                Some(d(2021, 1, 1)),
                "alpha".into()
            ),
            ("beta-board".into(), d(2021, 1, 1), None, "gamma".into()),
        ]
    );
    assert_eq!(
        find("gamma-fund"),
        vec![
            (
                "gamma-fund".into(),
                d(2020, 1, 1),
                Some(d(2021, 1, 1)),
                "beta".into()
            ),
            ("gamma-fund".into(), d(2021, 1, 1), None, "alpha".into()),
        ]
    );
    // removed and added
    assert_eq!(
        find("eta-institute"),
        vec![(
            "eta-institute".into(),
            d(2020, 1, 1),
            Some(d(2021, 1, 1)),
            "alpha".into()
        )]
    );
    assert_eq!(
        find("zeta-council"),
        vec![("zeta-council".into(), d(2021, 1, 1), None, "gamma".into())]
    );
    // the entity carries the latest wording and the earlier one as an alias
    let (name, kind, al): (String, String, Vec<String>) = sqlx::query_as(
        "SELECT name, kind, aliases FROM civic.entities WHERE entity_key = 'epsilon-department'",
    )
    .fetch_one(&pool)
    .await
    .expect("entity");
    assert_eq!((name.as_str(), kind.as_str()), ("Epsilon Dept", "other"));
    assert_eq!(al, vec!["Epsilon Department".to_owned()]);
    // items got their entity keys (after aliases)
    let key: String = sqlx::query_scalar(
        "SELECT entity_key FROM civic.items WHERE doc_id = 'gze-2000-01' AND text = 'Epsilon Dept'",
    )
    .fetch_one(&pool)
    .await
    .expect("item key");
    assert_eq!(key, "epsilon-department");

    // ── the diff: moved, added, removed ──
    let diff = civic::diff(&pool, d(2020, 3, 1), d(2021, 6, 1))
        .await
        .expect("diff");
    assert_eq!(
        (diff.from.doc_id.as_str(), diff.to.doc_id.as_str()),
        ("gze-1000-01", "gze-2000-01")
    );
    let moved: Vec<(&str, &str, &str)> = diff
        .moved
        .iter()
        .map(|m| {
            (
                m.entity_key.as_str(),
                m.from.portfolio_key.as_str(),
                m.to.portfolio_key.as_str(),
            )
        })
        .collect();
    assert_eq!(
        moved,
        vec![
            ("beta-board", "alpha", "gamma"),
            ("gamma-fund", "beta", "alpha")
        ]
    );
    assert_eq!(
        diff.added
            .iter()
            .map(|a| a.entity_key.as_str())
            .collect::<Vec<_>>(),
        ["zeta-council"]
    );
    assert_eq!(
        diff.removed
            .iter()
            .map(|a| a.entity_key.as_str())
            .collect::<Vec<_>>(),
        ["eta-institute"]
    );
    assert_eq!(diff.portfolios.added, vec!["Minister of Gamma".to_owned()]);
    assert!(diff.portfolios.removed.is_empty());
    assert_eq!(diff.portfolios.kept.len(), 2);
    assert_eq!(
        (diff.counts.moved, diff.counts.added, diff.counts.removed),
        (2, 1, 1)
    );
    // the same schedule on both dates changes nothing
    let same = civic::diff(&pool, d(2020, 3, 1), d(2020, 9, 1))
        .await
        .expect("same");
    assert_eq!(
        (same.counts.moved, same.counts.added, same.counts.removed),
        (0, 0, 0)
    );
    assert!(
        civic::diff(&pool, d(2021, 6, 1), d(2020, 3, 1))
            .await
            .is_err()
    );
    assert!(
        civic::diff(&pool, d(2019, 1, 1), d(2020, 3, 1))
            .await
            .is_err(),
        "before the first baseline"
    );

    // ── structure at a date, with the amendments since the baseline ──
    let s = civic::structure(&pool, d(2020, 3, 1))
        .await
        .expect("structure");
    assert_eq!(s.instrument.doc_id, "gze-1000-01");
    assert!(s.amendments_since.is_empty());
    assert_eq!(s.portfolios.len(), 2);
    assert_eq!(s.portfolios[0].institutions.len(), 3);
    assert_eq!(
        s.portfolios[0].institutions[0].entity_key.as_deref(),
        Some("alpha-authority")
    );
    assert_eq!(
        s.portfolios[0].institutions[0].kind.as_deref(),
        Some("statutory_body")
    );
    let s = civic::structure(&pool, d(2020, 7, 1))
        .await
        .expect("structure");
    assert_eq!(
        s.amendments_since
            .iter()
            .map(|a| a.doc_id.as_str())
            .collect::<Vec<_>>(),
        ["gze-1500-01"]
    );
    assert!(matches!(
        civic::structure(&pool, d(2019, 12, 31)).await,
        Err(lanka_core::CoreError::NotFound(_))
    ));
    // the needs_review baseline of 2022 is not served
    assert_eq!(
        civic::structure(&pool, d(2023, 1, 1))
            .await
            .expect("2023")
            .instrument
            .doc_id,
        "gze-2000-01"
    );

    // ── entities ──
    let hits = civic::entities(&pool, Some("EPSILON"), Some(10))
        .await
        .expect("entities");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].current.as_ref().map(|c| c.portfolio_title.as_str()),
        Some("Minister of Beta")
    );
    assert!(civic::entities(&pool, None, Some(0)).await.is_err());
    let e = civic::entity(&pool, "beta-board").await.expect("entity");
    assert_eq!(e.history.len(), 2);
    assert_eq!(e.history[0].page, Some(3));
    assert!(matches!(
        civic::entity(&pool, "nope").await,
        Err(lanka_core::CoreError::NotFound(_))
    ));
    let all = civic::instruments(&pool).await.expect("instruments");
    assert_eq!(all.len(), 4);
    assert_eq!(
        all[0].doc_id, "gze-3000-01",
        "newest first, with its status"
    );
    assert_eq!(all[0].status, "needs_review");

    // ── review: approving the 2022 baseline brings it into the build; rejecting takes it out ──
    extract::review(&pool, "gze-3000-01", true, "tester")
        .await
        .expect("approve");
    let r = build::build(&pool, &aliases)
        .await
        .expect("build with approval");
    assert_eq!(r.steps, 3);
    let aa: Vec<(NaiveDate, Option<NaiveDate>, String)> = sqlx::query_as(
        "SELECT valid_from, valid_to, portfolio_key FROM civic.assignments WHERE entity_key = 'alpha-authority' ORDER BY valid_from",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(
        aa,
        vec![
            (d(2020, 1, 1), Some(d(2022, 1, 1)), "alpha".to_owned()),
            (d(2022, 1, 1), None, "beta".to_owned()),
        ]
    );
    let checks: serde_json::Value =
        sqlx::query_scalar("SELECT checks FROM civic.instruments WHERE doc_id = 'gze-3000-01'")
            .fetch_one(&pool)
            .await
            .expect("checks");
    assert_eq!(checks[0]["check"], "review");
    assert!(
        checks[0]["detail"]
            .as_str()
            .is_some_and(|s| s.starts_with("approved by tester"))
    );
    extract::review(&pool, "gze-3000-01", false, "tester")
        .await
        .expect("reject");
    assert_eq!(build::build(&pool, &aliases).await.expect("build").steps, 2);
    assert!(
        extract::review(&pool, "gze-9999-99", true, "tester")
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_of_supplements_accumulates_into_one_structure() {
    with_database("civicsup", run_supplements).await;
}

async fn run_supplements(url: String) {
    let pool = db::connect(&url, PoolRole::Ingest).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");

    // Z: a cabinet of three; S1 and S2 appoint ministers in stages (S1 re-lists p and q, S2
    // adds r under a new heading); Z2 is a new baseline.
    instrument(
        &pool,
        "gze-1000-01",
        "baseline",
        d(2018, 1, 1),
        "validated",
        None,
    )
    .await;
    portfolio(
        &pool,
        "gze-1000-01",
        1,
        "Minister of P",
        "p",
        &["Inst One", "Inst Two"],
    )
    .await;
    portfolio(
        &pool,
        "gze-1000-01",
        2,
        "Minister of Q",
        "q",
        &["Inst Three"],
    )
    .await;
    portfolio(
        &pool,
        "gze-1000-01",
        3,
        "Minister of R",
        "r",
        &["Inst Four"],
    )
    .await;
    instrument(
        &pool,
        "gze-1100-01",
        "supplement",
        d(2018, 6, 1),
        "validated",
        None,
    )
    .await;
    portfolio(
        &pool,
        "gze-1100-01",
        1,
        "Minister of P",
        "p",
        &["Inst One", "Inst Two"],
    )
    .await;
    portfolio(
        &pool,
        "gze-1100-01",
        2,
        "Minister of Q",
        "q",
        &["Inst Three"],
    )
    .await;
    instrument(
        &pool,
        "gze-1200-01",
        "supplement",
        d(2018, 6, 3),
        "validated",
        None,
    )
    .await;
    portfolio(
        &pool,
        "gze-1200-01",
        1,
        "Minister of R and S",
        "r-and-s",
        &["Inst Four", "Inst Five"],
    )
    .await;
    instrument(
        &pool,
        "gze-2000-01",
        "baseline",
        d(2019, 1, 1),
        "validated",
        None,
    )
    .await;
    portfolio(&pool, "gze-2000-01", 1, "Minister of P", "p", &["Inst One"]).await;

    let r = build::build(&pool, &Aliases::default())
        .await
        .expect("build");
    assert_eq!((r.instruments, r.steps), (4, 4));

    // before the first supplement: the baseline
    let s = civic::structure(&pool, d(2018, 3, 1))
        .await
        .expect("baseline");
    assert_eq!(s.composed_of.len(), 1);
    assert_eq!(s.portfolios.len(), 3);
    // the first supplement replaces the structure: two ministers
    let s = civic::structure(&pool, d(2018, 6, 2)).await.expect("s1");
    assert_eq!(s.instrument.doc_id, "gze-1100-01");
    assert_eq!(
        s.portfolios
            .iter()
            .map(|p| p.portfolio_key.as_str())
            .collect::<Vec<_>>(),
        ["p", "q"]
    );
    // the second adds a heading: three, composed of both, the latest as the instrument
    let s = civic::structure(&pool, d(2018, 6, 4)).await.expect("s2");
    assert_eq!(s.instrument.doc_id, "gze-1200-01");
    assert_eq!(
        s.composed_of
            .iter()
            .map(|c| c.doc_id.as_str())
            .collect::<Vec<_>>(),
        ["gze-1100-01", "gze-1200-01"]
    );
    assert_eq!(s.portfolios.len(), 3);
    assert_eq!(s.portfolios[2].doc_id, "gze-1200-01");
    assert_eq!(s.portfolios[2].short_title, "R and S");
    // a baseline starts afresh
    assert_eq!(
        civic::structure(&pool, d(2019, 2, 1))
            .await
            .expect("z2")
            .portfolios
            .len(),
        1
    );

    // continuity: Inst One stays under p through the baseline, both supplements and Z2
    let rows: Vec<(chrono::NaiveDate, Option<chrono::NaiveDate>, String)> = sqlx::query_as(
        "SELECT valid_from, valid_to, doc_id FROM civic.assignments WHERE entity_key = 'inst-one'",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(rows, vec![(d(2018, 1, 1), None, "gze-1000-01".to_owned())]);
    // Inst Four: under r until the first supplement drops it, back under r-and-s from the second
    let rows: Vec<(chrono::NaiveDate, Option<chrono::NaiveDate>, String)> = sqlx::query_as(
        "SELECT valid_from, valid_to, doc_id FROM civic.assignments WHERE entity_key = 'inst-four' ORDER BY valid_from",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(
        rows,
        vec![
            (d(2018, 1, 1), Some(d(2018, 6, 1)), "gze-1000-01".to_owned()),
            (d(2018, 6, 3), Some(d(2019, 1, 1)), "gze-1200-01".to_owned()),
        ]
    );
    // the diff across the run sees the renamed ministry (r -> r-and-s, Inst Four kept) and the removal
    let diff = civic::diff(&pool, d(2018, 3, 1), d(2018, 6, 4))
        .await
        .expect("diff");
    assert_eq!(diff.to.doc_id, "gze-1200-01");
    assert_eq!(diff.counts.portfolios_renamed, 1);
    assert_eq!(
        diff.portfolios
            .kept
            .iter()
            .find(|k| k.renamed)
            .map(|k| k.to_key.as_str()),
        Some("r-and-s")
    );
    assert_eq!(
        diff.added
            .iter()
            .map(|a| a.entity_key.as_str())
            .collect::<Vec<_>>(),
        ["inst-five"]
    );
    assert!(
        diff.moved.is_empty(),
        "{:?}",
        diff.moved.iter().map(|m| &m.entity_key).collect::<Vec<_>>()
    );
}
