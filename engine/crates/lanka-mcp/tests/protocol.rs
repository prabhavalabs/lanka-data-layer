//! The MCP protocol surface over the real Streamable HTTP router: version
//! negotiation, `tools/list`, tool annotations, result rules, error hints.
//! No database needed: tools that read only the catalogue run for real; the
//! rest fail at validation, before any query.

mod common;

use axum::http::StatusCode;
use common::{Client, context, text_of};
use lanka_mcp::PROTOCOL_VERSIONS;
use serde_json::{Value, json};

#[test]
fn the_protocol_version_list_is_the_sdks() {
    let v: Vec<&str> = PROTOCOL_VERSIONS.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        v,
        [
            "2024-11-05",
            "2025-03-26",
            "2025-06-18",
            "2025-11-25",
            "2026-07-28"
        ]
    );
}

#[tokio::test]
async fn initialize_answers_with_the_clients_version_when_supported() {
    let mut c = Client::new(context());
    for version in ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"] {
        let v = c.initialize(version).await;
        assert_eq!(v["result"]["protocolVersion"], version, "{v}");
        assert_eq!(v["result"]["serverInfo"]["name"], "lanka-data-layer");
        assert_eq!(
            v["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(v["result"]["capabilities"]["tools"].is_object());
        assert!(
            v["result"]["instructions"]
                .as_str()
                .is_some_and(|s| s.contains("p-codes"))
        );
    }
}

#[tokio::test]
async fn initialize_falls_back_to_the_newest_handshake_version() {
    let mut c = Client::new(context());
    // an unsupported version, one from the future, and 2026-07-28 (which has no handshake)
    for asked in ["1999-01-01", "2030-12-31", "2026-07-28"] {
        let v = c.initialize(asked).await;
        assert_eq!(
            v["result"]["protocolVersion"], "2025-11-25",
            "asked {asked}: {v}"
        );
    }
}

#[tokio::test]
async fn server_discover_lists_every_supported_version() {
    let c = Client::new(context());
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "test-client", "version": "1.0.0" },
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let (status, v) = c
        .post(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": { "_meta": meta } }),
            Some("2026-07-28"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let listed: Vec<&str> = v["result"]["supportedVersions"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let want: Vec<&str> = PROTOCOL_VERSIONS.iter().map(|p| p.as_str()).collect();
    assert_eq!(listed, want, "{v}");
}

#[tokio::test]
async fn an_unsupported_header_version_is_refused() {
    let c = Client::new(context());
    let (status, _) = c
        .post(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            Some("1999-01-01"),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tools_list_is_the_registered_set_and_read_only() {
    let mut c = Client::new(context());
    let v = c.rpc("tools/list", json!({})).await;
    let tools = v["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(
        names,
        [
            "search_catalog",
            "describe_dataset",
            "describe_indicator",
            "resolve_place",
            "get_area_profile",
            "query_indicator",
            "rank_areas",
            "compare_areas",
            "find_features",
            "list_releases",
            "list_analysis_methods",
        ],
        "no analysis engine and no app plane in this context"
    );
    for t in tools {
        let a = &t["annotations"];
        assert_eq!(a["readOnlyHint"], true, "{}", t["name"]);
        assert_eq!(a["destructiveHint"], false, "{}", t["name"]);
        assert_eq!(a["idempotentHint"], true, "{}", t["name"]);
        assert_eq!(a["openWorldHint"], false, "{}", t["name"]);
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(
            t["description"].as_str().is_some_and(|d| d.len() > 30),
            "{}",
            t["name"]
        );
        assert!(t["title"].as_str().is_some(), "{}", t["name"]);
    }
}

fn assert_envelope(result: &Value) {
    let s = &result["structuredContent"];
    assert!(s["data_version"].is_string(), "{s}");
    for k in ["attribution", "licences", "next", "dataset_versions"] {
        assert!(s[k].is_array(), "{k} missing in {s}");
    }
}

#[tokio::test]
async fn search_catalog_finds_datasets_and_indicators() {
    let mut c = Client::new(context());
    let r = c
        .call("search_catalog", json!({ "query": "population" }))
        .await;
    assert_eq!(r["isError"], false, "{r}");
    assert_envelope(&r);
    let s = &r["structuredContent"];
    assert_eq!(s["data_version"], "20261122.3");
    let ind: Vec<&str> = s["result"]["indicators"]
        .as_array()
        .map(|a| a.iter().filter_map(|i| i["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(ind.contains(&"census.pop_total"), "{ind:?}");
    assert!(ind.contains(&"exp.flood_pop_share"), "{ind:?}");
    // attribution and licences of what was returned
    let attribution: Vec<&str> = s["attribution"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["attribution"].as_str()).collect())
        .unwrap_or_default();
    assert!(attribution.contains(&"DCS, Census 2024"), "{attribution:?}");
    assert!(
        s["licences"]
            .as_array()
            .is_some_and(|l| l.iter().any(|x| x["status"] == "permission_unclear"))
    );
    // ready-to-send next calls
    let next = s["next"].as_array().expect("next");
    assert!(
        next.iter()
            .any(|n| n["tool"] == "describe_dataset" && n["arguments"]["dataset_id"].is_string()),
        "{next:?}"
    );
    // a modelled indicator is in the list: the estimate notice leads the text
    assert!(s["estimate_notice"].is_string(), "{s}");
    let text = text_of(&r);
    assert!(text.starts_with("ESTIMATE:"), "{text}");
    assert!(
        text.contains("census.pop_total") && text.contains("data_version: 20261122.3"),
        "{text}"
    );
    assert!(
        text.contains("permission_unclear"),
        "licence status in the footer: {text}"
    );
}

#[tokio::test]
async fn search_catalog_filters_and_all_words_must_match() {
    let mut c = Client::new(context());
    let r = c
        .call(
            "search_catalog",
            json!({ "query": "population", "class": "official" }),
        )
        .await;
    let ids: Vec<&str> = r["structuredContent"]["result"]["indicators"]
        .as_array()
        .map(|a| a.iter().filter_map(|i| i["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(ids, ["census.pop_total"]);
    assert!(
        r["structuredContent"]["estimate_notice"].is_null(),
        "official only: no notice"
    );
    assert!(!text_of(&r).starts_with("ESTIMATE:"));
    let none = c
        .call("search_catalog", json!({ "query": "population zzz" }))
        .await;
    assert_eq!(
        none["structuredContent"]["result"]["totals"]["indicators"],
        0
    );
    let bad = c
        .call(
            "search_catalog",
            json!({ "query": "x", "class": "nonsense" }),
        )
        .await;
    assert_eq!(bad["isError"], true);
    assert!(bad["structuredContent"]["error"]["hints"]["valid"].is_array());
}

#[tokio::test]
async fn describe_indicator_gives_the_definition_and_next_calls() {
    let mut c = Client::new(context());
    let r = c
        .call(
            "describe_indicator",
            json!({ "indicator_id": "census.pop_total" }),
        )
        .await;
    assert_eq!(r["isError"], false, "{r}");
    assert_envelope(&r);
    let text = text_of(&r);
    assert!(
        text.contains("INDICATOR census.pop_total") && text.contains("Census reference night 2024"),
        "{text}"
    );
    let missing = c
        .call(
            "describe_indicator",
            json!({ "indicator_id": "census.nope" }),
        )
        .await;
    assert_eq!(missing["isError"], true);
    assert_eq!(missing["structuredContent"]["error"]["kind"], "not_found");
    assert!(
        missing["structuredContent"]["next"][0]["tool"] == "search_catalog",
        "{missing}"
    );
}

#[tokio::test]
async fn resolve_place_validates_before_the_database() {
    let mut c = Client::new(context());
    // neither a name nor a point
    let r = c.call("resolve_place", json!({})).await;
    assert_eq!(r["isError"], true);
    assert_eq!(r["structuredContent"]["error"]["kind"], "bad_request");
    assert_envelope(&r);
    // both
    let r = c
        .call(
            "resolve_place",
            json!({ "query": "Kandy", "lat": 7.29, "lon": 80.63 }),
        )
        .await;
    assert_eq!(r["isError"], true);
    // a point outside the grid: answered from the profile alone, with REST's hint
    let r = c
        .call("resolve_place", json!({ "lat": 0.0, "lon": 0.0 }))
        .await;
    assert_eq!(r["isError"], true, "{r}");
    assert_eq!(r["structuredContent"]["error"]["kind"], "not_in_coverage");
    assert!(r["structuredContent"]["error"]["hints"]["hint"].is_string());
    assert!(text_of(&r).starts_with("ERROR not_in_coverage"));
    // a lone coordinate
    let r = c.call("resolve_place", json!({ "lat": 7.29 })).await;
    assert_eq!(r["isError"], true);
}

#[tokio::test]
async fn data_tools_reject_bad_arguments_with_hints() {
    let mut c = Client::new(context());
    let r = c
        .call(
            "query_indicator",
            json!({ "indicator_id": "census.pop_total" }),
        )
        .await;
    assert_eq!(r["isError"], true, "needs geo or level");
    let r = c
        .call(
            "query_indicator",
            json!({ "indicator_id": "census.pop_total", "level": 9 }),
        )
        .await;
    assert_eq!(r["isError"], true);
    assert!(
        r["structuredContent"]["error"]["hints"]["valid"].is_array(),
        "{r}"
    );
    let r = c
        .call(
            "rank_areas",
            json!({ "indicator_id": "census.pop_total", "level": 2, "order": "sideways" }),
        )
        .await;
    assert_eq!(r["isError"], true);
    assert_eq!(
        r["structuredContent"]["error"]["hints"]["valid"],
        json!(["desc", "asc"])
    );
    let r = c
        .call(
            "compare_areas",
            json!({ "geo": ["LK11"], "indicator_ids": ["census.pop_total"] }),
        )
        .await;
    assert_eq!(r["isError"], true, "two areas at least");
    let r = c.call("get_area_profile", json!({ "geo": "XX1" })).await;
    assert_eq!(r["isError"], true);
    assert_eq!(r["structuredContent"]["next"][0]["tool"], "resolve_place");
    let r = c
        .call("find_features", json!({ "dataset": "hospitals-demo", "near": { "lat": 7.0, "lon": 80.0 }, "within": "LK11" }))
        .await;
    assert_eq!(r["isError"], true);
}

#[tokio::test]
async fn list_analysis_methods_runs_without_an_engine() {
    let mut c = Client::new(context());
    let r = c.call("list_analysis_methods", json!({ "tier": 1 })).await;
    assert_eq!(r["isError"], false, "{r}");
    let methods = r["structuredContent"]["result"]["methods"]
        .as_array()
        .expect("methods");
    assert!(
        methods
            .iter()
            .any(|m| m["id"] == "relate" && m["runnable_by_mcp"] == true)
    );
    assert!(methods.iter().all(|m| m["tier"] == 1));
    assert!(
        r["structuredContent"]["result"]["guards"]
            .as_array()
            .is_some_and(|g| g.len() >= 20)
    );
    assert!(text_of(&r).contains("never causal"));
}

#[tokio::test]
async fn an_unknown_tool_is_a_protocol_error_and_unregistered_tools_are_unknown() {
    let mut c = Client::new(context());
    for name in [
        "nope",
        "analyze_relationship",
        "get_analysis",
        "create_map_link",
    ] {
        let v = c
            .rpc("tools/call", json!({ "name": name, "arguments": {} }))
            .await;
        assert!(v["error"].is_object(), "{name}: {v}");
        assert!(v["error"]["data"]["tools"].is_array());
    }
}

#[tokio::test]
async fn results_stay_inside_the_limits() {
    use lanka_mcp::out::{Out, finish};
    let cat = common::catalog();
    let rows: Vec<Value> = (0..2000)
        .map(|i| json!({ "geo": format!("LK{i}"), "note": "x".repeat(600) }))
        .collect();
    let r = finish(
        &cat,
        Out::new(json!({ "rows": rows }), "many rows".into()).datasets(["census-demo"]),
    );
    let structured = r.structured_content.expect("structured");
    let size = serde_json::to_vec(&structured).map_or(0, |b| b.len())
        + r.content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| t.text.len())
            .sum::<usize>();
    assert!(size <= lanka_mcp::MAX_RESULT_BYTES, "{size} bytes");
    assert!(structured["truncated"].is_object(), "{structured}");
}
