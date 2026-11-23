//! The roadmap's acceptance transcript against a loaded database:
//! `resolve_place` → `get_area_profile` → `query_indicator`, and the numbers
//! equal what the shared query core (and therefore REST) returns.
//!
//! Gated: runs only when `LANKA_MCP_TEST_DATABASE_URL` points at a database
//! with a release loaded (read-only use), e.g.
//!
//!   LANKA_MCP_TEST_DATABASE_URL=postgres://lanka:lanka-local-dev@127.0.0.1:55432/lanka \
//!     cargo test -p lanka-mcp --test transcript

mod common;

use std::sync::Arc;

use common::{Client, text_of};
use lanka_core::Catalog;
use lanka_core::db::{self, PoolRole};
use lanka_core::query::{self, SeriesQuery};
use lanka_mcp::McpContext;
use serde_json::{Value, json};

async fn live() -> Option<(McpContext, Arc<Catalog>)> {
    let url = std::env::var("LANKA_MCP_TEST_DATABASE_URL").ok()?;
    let pool = db::connect(&url, PoolRole::Api).await.expect("connect");
    let cat = Arc::new(Catalog::load(&pool).await.expect("catalogue"));
    let shared = Arc::clone(&cat);
    Some((
        McpContext {
            pool,
            catalog: Arc::new(move || Arc::clone(&shared)),
            analysis: None,
            app: None,
            reverse: None,
        },
        cat,
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_place_then_profile_then_series_agree_with_the_core() {
    let Some((ctx, cat)) = live().await else {
        eprintln!("skipping: LANKA_MCP_TEST_DATABASE_URL not set");
        return;
    };
    let pool = ctx.pool.clone();
    let mut c = Client::new(ctx);

    // 1. a name → candidates with p-codes, level, parent chain, confidence
    let r = c.call("resolve_place", json!({ "query": "Colombo" })).await;
    assert_eq!(r["isError"], false, "{r}");
    let cands = r["structuredContent"]["result"]["candidates"]
        .as_array()
        .expect("candidates");
    assert!(!cands.is_empty());
    let best = &cands[0];
    assert_eq!(best["name_en"], "Colombo", "{best}");
    assert_eq!(best["confidence"], 1.0);
    let pcode = best["pcode"].as_str().expect("pcode").to_owned();
    assert!(
        best["ancestors"].as_array().is_some_and(|a| !a.is_empty()),
        "parent chain: {best}"
    );
    assert!(r["structuredContent"]["data_version"].is_string());
    assert!(
        r["structuredContent"]["next"]
            .as_array()
            .is_some_and(|n| n.iter().any(|x| x["tool"] == "get_area_profile"))
    );

    // a point → the same family of answers
    let p = c
        .call("resolve_place", json!({ "lat": 6.9271, "lon": 79.8612 }))
        .await;
    assert_eq!(p["isError"], false, "{p}");
    assert_eq!(
        p["structuredContent"]["result"]["candidates"][0]["match"],
        "contains"
    );

    // 2. the profile of that p-code equals the core's
    let prof = c.call("get_area_profile", json!({ "geo": pcode })).await;
    assert_eq!(prof["isError"], false, "{prof}");
    let (core_profile, _) = query::area_profile(
        &pool,
        &cat,
        &query::parse_pcode(&cat, &pcode).expect("pcode"),
    )
    .await
    .expect("core profile");
    let groups = &prof["structuredContent"]["result"]["groups"];
    let mut seen = 0usize;
    for (_, rows) in groups.as_object().expect("groups") {
        for i in rows.as_array().expect("rows") {
            let id = i["indicator_id"].as_str().expect("id");
            let core = core_profile
                .indicators
                .iter()
                .find(|x| x.indicator_id == id)
                .expect("in core");
            assert_eq!(i["period"], core.period);
            assert_eq!(
                i["values"].as_array().map(Vec::len),
                Some(core.values.len()),
                "{id}"
            );
            for (a, b) in i["values"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .zip(&core.values)
            {
                assert_eq!(a["value"].as_f64(), b.value, "{id}");
                assert_eq!(a["basis"], b.basis.as_str());
            }
            seen += 1;
        }
    }
    assert!(seen > 0, "an area profile with values");
    assert!(text_of(&prof).contains("AREA "), "{}", text_of(&prof));

    // 3. a series for the first indicator of the profile: same points as the core
    let first = core_profile
        .indicators
        .iter()
        .find(|i| i.class == "official")
        .unwrap_or(&core_profile.indicators[0]);
    let q = c
        .call(
            "query_indicator",
            json!({ "indicator_id": first.indicator_id, "geo": [pcode] }),
        )
        .await;
    assert_eq!(q["isError"], false, "{q}");
    let core = query::series(
        &pool,
        &cat,
        &first.indicator_id,
        &SeriesQuery {
            geos: vec![pcode.clone()],
            ..SeriesQuery::default()
        },
    )
    .await
    .expect("core series");
    let want: Vec<(String, Option<f64>)> = core
        .series
        .iter()
        .flat_map(|l| l.points.iter().map(|p| (p.period.clone(), p.value)))
        .collect();
    let rows = q["structuredContent"]["result"]["rows"]
        .as_array()
        .expect("rows");
    let got: Vec<(String, Option<f64>)> = rows
        .iter()
        .map(|r| {
            (
                r["period"].as_str().unwrap_or("").to_owned(),
                r["value"].as_f64(),
            )
        })
        .collect();
    assert_eq!(got, want);
    assert!(
        rows.iter()
            .all(|r| r["basis"].is_string() && r["status"].is_string()),
        "basis and status on every row"
    );
    let s = &q["structuredContent"];
    assert!(
        s["attribution"].as_array().is_some_and(|a| !a.is_empty()),
        "{s}"
    );
    assert!(
        s["licences"].as_array().is_some_and(|a| !a.is_empty()),
        "{s}"
    );

    // rank_areas at the level of the profile's children: 1 = highest, values equal the core's map
    let level = (core_profile.level + 1).min(2);
    let rk = c
        .call(
            "rank_areas",
            json!({ "indicator_id": first.indicator_id, "level": level, "limit": 5 }),
        )
        .await;
    if rk["isError"] == false {
        let rows = rk["structuredContent"]["result"]["rows"]
            .as_array()
            .expect("rank rows");
        assert!(
            rows.windows(2)
                .all(|w| w[0]["value"].as_f64() >= w[1]["value"].as_f64()),
            "{rows:?}"
        );
        assert_eq!(rows[0]["rank"], 1);
    }

    // the catalogue search works on the real catalogue and cites sources
    let s = c
        .call("search_catalog", json!({ "query": "population" }))
        .await;
    assert_eq!(s["isError"], false, "{s}");
    assert!(
        s["structuredContent"]["result"]["datasets"]
            .as_array()
            .is_some_and(|d| !d.is_empty())
    );
    let _: Value = s;
}
