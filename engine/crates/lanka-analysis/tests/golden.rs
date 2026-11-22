//! Open-data golden tests (WP-29 acceptance; 11 §13 "Golden").
//!
//! Each case is a directory under `tests/golden/<case>/`:
//! `request.json` (the `ldl-analysis/1` request), and — written by the live
//! run with `LANKA_GOLDEN_UPDATE=1` — `resolved.json` (the resolved plan),
//! `frame.json` + `scale.json` (the aligned frames) and `result.json` (the
//! result document, `computed_at`/`elapsed_ms`/`git_rev` blanked).
//!
//! - `*_offline` (always runs): re-evaluates the committed aligned frames and
//!   requires the committed estimates, n, state and guards — the kernels,
//!   guards and templates are pinned without a database.
//! - `*_live` (`LANKA_GOLDEN_DATABASE_URL` + `LANKA_STORE_DIR` of a database
//!   holding the inputs): the whole pipeline (resolve → align → evaluate)
//!   must reproduce the committed `result.json` exactly.
//!
//! Cases: the roadmap's acceptance pair — JRC flood area share
//! (`haz.flood_area_share`, RP100, WP-27) vs census population density
//! (`derived.per_km2`, log), ADM3, island — and the open-data substitute used
//! before WP-27 merged: ESA WorldCover built-up share (`lc.class_share`,
//! class 50) vs the same density.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lanka_analysis::engine::{EvalMeta, evaluate};
use lanka_analysis::frame::AlignedFrame;
use lanka_analysis::methods::Registry;
use lanka_analysis::pipeline::Resolved;
use serde_json::{Value, json};

fn dir(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(case)
}

fn read(case: &str, f: &str) -> Option<Value> {
    let p = dir(case).join(f);
    let s = std::fs::read_to_string(&p).ok()?;
    Some(serde_json::from_str(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
}

fn write(case: &str, f: &str, v: &Value) {
    let p = dir(case).join(f);
    std::fs::write(&p, serde_json::to_string_pretty(v).expect("json") + "\n")
        .expect("write golden");
}

/// The parts of a result that do not depend on when or where it ran.
fn stable(mut doc: Value) -> Value {
    doc["provenance"]["computed_at"] = json!("");
    doc["provenance"]["elapsed_ms"] = json!(0);
    doc["provenance"]["git_rev"] = json!("");
    if let Some(t) = doc["citation"]["text"].as_str() {
        let t = t.split_once(" Analysis ").map_or_else(
            || t.to_owned(),
            |(_, r)| format!("Lanka Data Layer. Analysis {r}"),
        );
        doc["citation"]["text"] = json!(t);
    }
    doc
}

/// A result without the release it first ran against: the fingerprint (and
/// so the id) leaves out `pin`, so a later unrelated release keeps the
/// result; only the recorded release differs.
fn release_free(mut doc: Value) -> Value {
    doc["request"]["pin"] = json!(null);
    doc["provenance"]["data_version"] = json!(null);
    if let Some(t) = doc["citation"]["text"].as_str() {
        let t = t
            .split_once(" Data release ")
            .map_or(t, |(a, _)| a)
            .to_owned();
        doc["citation"]["text"] = json!(t);
    }
    doc
}

/// Catalogue-independent core of a result: id, state, n, estimates, guards.
fn core(doc: &Value) -> Value {
    json!({
        "id": doc["id"], "state": doc["state"], "n": doc["n"], "estimates": doc["estimates"],
        "guards": doc["guards"].as_array().map(|g| g.iter().map(|g| json!([g["code"], g["severity"]])).collect::<Vec<_>>()),
    })
}

fn offline(case: &str) {
    let (Some(resolved), Some(frame), Some(scale), Some(result)) = (
        read(case, "resolved.json"),
        read(case, "frame.json"),
        read(case, "scale.json"),
        read(case, "result.json"),
    ) else {
        panic!(
            "golden case {case} has no committed fixture; run the live test with LANKA_GOLDEN_UPDATE=1"
        );
    };
    let r = Resolved::from_json(&resolved, &Registry::builtin()).expect("resolved");
    let frame: AlignedFrame = serde_json::from_value(frame).expect("frame");
    let scale: Vec<AlignedFrame> = serde_json::from_value(scale).expect("scale");
    let fp = lanka_analysis::result::fingerprint(&r);
    assert_eq!(
        lanka_analysis::result::id_of(&fp),
        result["id"],
        "the resolved plan fingerprints to the committed id"
    );
    let meta = EvalMeta {
        base_url: String::new(),
        engine: String::new(),
        git_rev: String::new(),
    };
    let (doc, _) = evaluate(
        &r,
        &fp,
        &frame,
        &scale,
        None,
        &lanka_core::Catalog::default(),
        &meta,
    )
    .expect("evaluate");
    assert_eq!(
        core(&doc),
        core(&result),
        "{case}: estimates, n, state or guards changed"
    );
}

async fn live(case: &str) {
    let (Ok(url), Ok(store)) = (
        std::env::var("LANKA_GOLDEN_DATABASE_URL"),
        std::env::var("LANKA_STORE_DIR"),
    ) else {
        eprintln!(
            "skipping {case}: set LANKA_GOLDEN_DATABASE_URL and LANKA_STORE_DIR (a database holding the inputs)"
        );
        return;
    };
    let request = read(case, "request.json").expect("request.json");
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    let cat = Arc::new(lanka_core::Catalog::load(&pool).await.expect("catalog"));
    let engine = lanka_analysis::Engine::new(
        pool,
        Arc::new(move || cat.clone()),
        Some(PathBuf::from(store)),
        "https://lanka-data-layer.prabhavalabs.com".to_owned(),
        "golden".to_owned(),
    );
    let t0 = std::time::Instant::now();
    let parts = engine
        .parts(&request)
        .await
        .unwrap_or_else(|e| panic!("{case}: {e}"));
    eprintln!(
        "{case}: resolve + align + evaluate in {} ms",
        t0.elapsed().as_millis()
    );
    let _ = EvalMeta {
        base_url: String::new(),
        engine: String::new(),
        git_rev: String::new(),
    };
    let doc = stable(parts.doc);
    if std::env::var("LANKA_GOLDEN_UPDATE").is_ok_and(|v| v == "1") {
        write(case, "resolved.json", &parts.resolved.to_json());
        write(
            case,
            "frame.json",
            &serde_json::to_value(&parts.frame).expect("frame"),
        );
        write(
            case,
            "scale.json",
            &serde_json::to_value(&parts.scale).expect("scale"),
        );
        write(case, "result.json", &doc);
        eprintln!("{case}: golden files written");
        return;
    }
    let want = read(case, "result.json")
        .unwrap_or_else(|| panic!("{case}: no result.json; run with LANKA_GOLDEN_UPDATE=1"));
    assert_eq!(
        release_free(doc),
        release_free(want),
        "{case}: the result differs from the committed golden file"
    );
}

#[test]
fn builtup_share_vs_density_adm3_offline() {
    offline("builtup-share-vs-density-adm3");
}

#[tokio::test(flavor = "multi_thread")]
async fn builtup_share_vs_density_adm3_live() {
    live("builtup-share-vs-density-adm3").await;
}

/// The roadmap's acceptance pair (JRC flood hazard, release 20261122.4).
#[test]
fn flood_area_share_vs_density_adm3_offline() {
    offline("flood-area-share-vs-density-adm3");
}

#[tokio::test(flavor = "multi_thread")]
async fn flood_area_share_vs_density_adm3_live() {
    live("flood-area-share-vs-density-adm3").await;
}
