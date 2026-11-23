//! `list_analysis_methods`, `analyze_relationship`, `get_analysis` (11 §11.2).
//!
//! Requests are `ldl-analysis/1` documents run by the same engine as
//! `POST /v2/analyses`; results are the stored, immutable documents. Because
//! a language model will paraphrase, the text rendering leads with
//! `NOT_CAUSAL` and every blocking or warning guard, and only then gives
//! numbers.

use std::sync::Arc;

use lanka_analysis::{Engine, EngineError};
use lanka_core::Catalog;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Caller, Level, parse, schema};
use crate::McpContext;
use crate::out::{Next, Out, ToolError, is_estimate_indicator, num};

// ───────────────────────────── list_analysis_methods ───────────────────────

pub fn methods_schema() -> Value {
    schema(
        json!({ "tier": { "type": "integer", "minimum": 1, "maximum": 3, "description": "Only methods of this tier (1 = headline methods, 3 = restricted)" } }),
        &[],
    )
}

#[derive(Deserialize)]
struct MethodsArgs {
    tier: Option<u8>,
}

/// Methods `analyze_relationship` can run.
const RELATIONSHIP_METHODS: [&str; 2] = ["relate", "regress"];

pub async fn list_analysis_methods(
    _ctx: &McpContext,
    _cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: MethodsArgs = parse("list_analysis_methods", args)?;
    let reg = lanka_analysis::methods::Registry::builtin();
    let mut methods: Vec<Value> = Vec::new();
    let mut text = String::from("ANALYSIS METHODS\n");
    for m in reg.iter() {
        let info = m.info();
        if a.tier.is_some_and(|t| t != info.tier) {
            continue;
        }
        let mut j = info.to_json();
        j["runnable_by_mcp"] = json!(RELATIONSHIP_METHODS.contains(&info.id));
        let roles: Vec<String> = info
            .roles
            .iter()
            .map(|r| {
                format!(
                    "{}{}{}",
                    r.name,
                    if r.required { "*" } else { "" },
                    if r.many { "[]" } else { "" }
                )
            })
            .collect();
        text.push_str(&format!(
            "- {} (tier {}){}: {}\n  roles: {}; supports: {}; min n: {}; guards: {}\n",
            info.id,
            info.tier,
            if info.relationship {
                ", relationship — never causal"
            } else {
                ""
            },
            info.summary,
            roles.join(", "),
            info.supports.join("/"),
            info.min_n.map_or("none".to_owned(), |m| format!(
                "block below {}, warn below {}",
                m.block, m.warn
            )),
            info.guards.join(","),
        ));
        methods.push(j);
    }
    text.push_str("\nRoles marked * are required. analyze_relationship runs `relate` and `regress`; the other methods run through POST /v2/analyses (ldl-analysis/1).\n");
    let guards = lanka_analysis::guards::registry_json();
    Ok(
        Out::new(json!({ "methods": methods, "guards": guards }), text).next(Next::new(
            "analyze_relationship",
            json!({ "x": "census24.pop_total", "y": "census24.sex_ratio", "level": 2 }),
            "relate two indicators across districts (example)",
        )),
    )
}

// ───────────────────────────── analyze_relationship ────────────────────────

pub fn relationship_schema() -> Value {
    let variable = json!({
        "description": "An indicator id, or an object {indicator, dims?, period?, transform?, align?} (also {grid} or {dataset, field} for layers and point datasets)",
        "oneOf": [
            { "type": "string" },
            { "type": "object", "properties": {
                "indicator": { "type": "string" }, "grid": { "type": "string" }, "dataset": { "type": "string" },
                "field": { "type": "string" }, "dims": { "type": "object", "additionalProperties": { "type": "string" } },
                "period": { "type": "string" }, "transform": { "type": "string", "enum": ["log", "log1p", "sqrt", "rank", "zscore", "none"] },
                "align": { "type": "object" }
            } }
        ]
    });
    schema(
        json!({
            "x": variable.clone(),
            "y": variable.clone(),
            "controls": { "type": "array", "items": variable, "maxItems": 6, "description": "Control variables: turns the analysis into a regression with the partial correlation" },
            "level": { "type": ["integer", "string"], "description": "Admin level of the units compared: 1 provinces, 2 districts, 3 DS divisions, 4 GN divisions" },
            "support": { "type": "object", "description": "Explicit ldl-analysis/1 support instead of `level`, e.g. {\"kind\":\"admin\",\"level\":3,\"within\":\"LK1\"} or {\"kind\":\"grid\",\"factor\":10}" },
            "within": { "type": "string", "description": "With level: only units below this p-code" },
            "transforms": { "type": "object", "description": "{\"x\":\"log\",\"y\":\"log\"} transforms applied to the variables", "properties": { "x": { "type": "string" }, "y": { "type": "string" } } },
            "period": { "type": "string", "description": "Period for variables that do not name one (default: latest)" }
        }),
        &["x", "y"],
    )
}

#[derive(Deserialize)]
struct RelArgs {
    x: Value,
    y: Value,
    #[serde(default)]
    controls: Vec<Value>,
    level: Option<Level>,
    support: Option<Value>,
    within: Option<String>,
    transforms: Option<Map<String, Value>>,
    period: Option<String>,
}

fn variable(
    v: Value,
    role: &str,
    period: Option<&str>,
    transform: Option<&Value>,
) -> Result<Value, ToolError> {
    let mut o = match v {
        Value::String(s) if !s.trim().is_empty() => {
            let mut m = Map::new();
            m.insert("indicator".to_owned(), Value::String(s.trim().to_owned()));
            m
        }
        Value::Object(m) if !m.is_empty() => m,
        _ => {
            return Err(ToolError::bad(format!(
                "{role}: give an indicator id or an object like {{\"indicator\": \"census24.pop_total\"}}"
            )));
        }
    };
    if let Some(p) = period
        && !o.contains_key("period")
    {
        o.insert("period".to_owned(), json!(p));
    }
    if let Some(t) = transform
        && !o.contains_key("transform")
        && t != "none"
    {
        o.insert("transform".to_owned(), t.clone());
    }
    Ok(Value::Object(o))
}

pub async fn analyze_relationship(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: RelArgs = parse("analyze_relationship", args)?;
    let Some(engine) = ctx.analysis.clone() else {
        return Err(ToolError::unavailable(
            "the analysis engine is not available on this server",
        ));
    };
    let tf = a.transforms.clone().unwrap_or_default();
    let period = a.period.as_deref();
    let x = variable(a.x, "x", period, tf.get("x"))?;
    let y = variable(a.y, "y", period, tf.get("y"))?;
    let mut controls = Vec::new();
    for (i, c) in a.controls.into_iter().enumerate() {
        controls.push(variable(
            c,
            &format!("controls[{i}]"),
            period,
            tf.get("controls"),
        )?);
    }
    let support = match (a.support, a.level) {
        (Some(s), _) => s,
        (None, Some(l)) => {
            let mut s = json!({ "kind": "admin", "level": l.0 });
            if let Some(w) = a.within.as_deref().filter(|w| !w.trim().is_empty()) {
                s["within"] = json!(w.trim());
            }
            s
        }
        (None, None) => {
            if a.within.is_some() {
                return Err(ToolError::bad(
                    "`within` needs `level` (or an explicit `support`)",
                ));
            }
            json!("auto")
        }
    };
    let method = if controls.is_empty() {
        "relate"
    } else {
        "regress"
    };
    let mut variables = json!({ "x": x, "y": y });
    if !controls.is_empty() {
        variables["controls"] = Value::Array(controls);
    }
    let body = json!({
        "spec": "ldl-analysis/1", "method": method, "pin": "latest",
        "support": support, "variables": variables,
    });
    let ran = engine.run(&body).await.map_err(engine_error)?;
    let doc: Value = serde_json::from_slice(&ran.bytes)
        .map_err(|e| ToolError::new("internal", format!("stored result is not JSON: {e}")))?;
    render(ctx, cat, &engine, ran.id, doc)
}

// ───────────────────────────── get_analysis ────────────────────────────────

pub fn get_schema() -> Value {
    schema(
        json!({ "id": { "type": "string", "description": "Analysis id: an_ + 16 hex, from analyze_relationship" } }),
        &["id"],
    )
}

#[derive(Deserialize)]
struct GetArgs {
    id: String,
}

pub async fn get_analysis(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: GetArgs = parse("get_analysis", args)?;
    let Some(engine) = ctx.analysis.clone() else {
        return Err(ToolError::unavailable(
            "the analysis engine is not available on this server",
        ));
    };
    let id = a.id.trim();
    let valid = id.len() == 19
        && id.starts_with("an_")
        && id[3..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    if !valid {
        return Err(ToolError::bad(format!(
            "{id:?} is not an analysis id (an_ + 16 lower-case hex)"
        )));
    }
    let Some(ran) = engine.get(id).await.map_err(engine_error)? else {
        return Err(ToolError::new(
            "not_found",
            format!("not_found: no analysis \"{id}\""),
        ));
    };
    let doc: Value = serde_json::from_slice(&ran.bytes)
        .map_err(|e| ToolError::new("internal", format!("stored result is not JSON: {e}")))?;
    render(ctx, cat, &engine, ran.id, doc)
}

fn engine_error(e: EngineError) -> ToolError {
    match e {
        EngineError::Bad { message, problems, hints } => {
            ToolError::bad(message).hints(json!({ "problems": problems, "hints": hints }))
        }
        EngineError::Busy => {
            let mut t = ToolError::unavailable("the analysis pool is busy; retry shortly");
            t.retry_after_s = Some(2);
            t
        }
        EngineError::TooCostly { expected_ms } => ToolError::bad(
            "this analysis is over the synchronous budget: narrow `within` or use a coarser `level`",
        )
        .hints(json!({ "expected_ms": expected_ms, "budget_ms": lanka_analysis::engine::SYNC_BUDGET_MS })),
        EngineError::Internal(err) => {
            tracing::error!(error = %format!("{err:#}"), "mcp: analysis failed");
            ToolError::new("internal", "internal error")
        }
    }
}

// ───────────────────────────── rendering ───────────────────────────────────

fn dataset_ids(doc: &Value) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if k == "dataset" || k == "dataset_id" {
                        if let Some(s) = x.as_str() {
                            out.push(s.to_owned());
                        }
                    } else {
                        walk(x, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&doc["inputs"], &mut out);
    out
}

/// Keys that carry a p-value or a significance verdict.
fn is_p_key(k: &str) -> bool {
    k == "p"
        || k.starts_with("p_")
        || k.ends_with("_p")
        || k.contains("p_value")
        || k == "significance"
}

/// Compact `key: value` lines of an estimates object (depth ≤ 2).
fn estimate_lines(v: &Value, prefix: &str, depth: usize, out: &mut Vec<String>) {
    const MAX_LINES: usize = 28;
    if out.len() >= MAX_LINES {
        return;
    }
    if let Value::Object(m) = v {
        for (k, x) in m {
            // significance is shown in the engine's own words (interpretation, guards): no bare p-values
            if is_p_key(k) {
                continue;
            }
            let key = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            match x {
                Value::Object(_) if depth < 2 => estimate_lines(x, &key, depth + 1, out),
                Value::Object(_) => {}
                Value::Array(a)
                    if a.len() <= 4 && a.iter().all(|e| e.is_number() || e.is_null()) =>
                {
                    out.push(format!(
                        "{key}: [{}]",
                        a.iter()
                            .map(|e| e.as_f64().map_or("null".to_owned(), num))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                Value::Array(_) => {}
                Value::Number(n) => {
                    out.push(format!("{key}: {}", n.as_f64().map_or(n.to_string(), num)))
                }
                Value::String(s) if s.len() <= 80 => out.push(format!("{key}: {s}")),
                Value::Bool(b) => out.push(format!("{key}: {b}")),
                _ => {}
            }
        }
    }
}

fn guard_line(g: &Value) -> String {
    if g["code"] == "NOT_CAUSAL" {
        return format!("NOT_CAUSAL: {}", g["message"].as_str().unwrap_or(""));
    }
    format!(
        "{} {}: {}{}",
        g["severity"].as_str().unwrap_or("note").to_uppercase(),
        g["code"].as_str().unwrap_or("?"),
        g["message"].as_str().unwrap_or(""),
        if g["overridden"] == true {
            " (overridden)"
        } else {
            ""
        }
    )
}

fn render(
    ctx: &McpContext,
    _cat: &Catalog,
    engine: &Arc<Engine>,
    id: String,
    mut doc: Value,
) -> Result<Out, ToolError> {
    lanka_analysis::result::relink(&mut doc, engine.base_url());
    let base = ctx.public_base();
    let state = doc["state"].as_str().unwrap_or("ready").to_owned();
    let method = doc["method"]
        .as_str()
        .or_else(|| doc.pointer("/request/method").and_then(Value::as_str))
        .unwrap_or("?")
        .to_owned();
    let url = format!("{base}/v2/analyses/{id}");
    let guards: Vec<Value> = doc["guards"].as_array().cloned().unwrap_or_default();
    let by = |pred: &dyn Fn(&Value) -> bool| {
        guards
            .iter()
            .filter(|g| pred(g))
            .map(guard_line)
            .collect::<Vec<_>>()
    };
    let not_causal = by(&|g| g["code"] == "NOT_CAUSAL");
    let blocks = by(&|g| g["severity"] == "block" && g["code"] != "NOT_CAUSAL");
    let warns = by(&|g| g["severity"] == "warn" && g["code"] != "NOT_CAUSAL");
    let notes = by(&|g| g["severity"] == "note" && g["code"] != "NOT_CAUSAL");

    let mut text = String::new();
    // guards first: NOT_CAUSAL, then blocks, then warnings
    if method == "relate" || method == "regress" || !not_causal.is_empty() {
        if not_causal.is_empty() {
            text.push_str("NOT_CAUSAL: this is an association between areas, not evidence that one thing causes the other.\n");
        } else {
            for l in &not_causal {
                text.push_str(l);
                text.push('\n');
            }
        }
    }
    for l in blocks.iter().chain(warns.iter()) {
        text.push_str(l);
        text.push('\n');
    }
    text.push_str(&format!(
        "\nANALYSIS {id} — method {method}, state {state}\n"
    ));
    if let Some(r) = doc.pointer("/request/variables") {
        let name = |k: &str| {
            r.get(k)
                .and_then(|v| {
                    v["indicator"]
                        .as_str()
                        .or(v["grid"].as_str())
                        .or(v["dataset"].as_str())
                })
                .unwrap_or("?")
                .to_owned()
        };
        text.push_str(&format!("x = {}; y = {}\n", name("x"), name("y")));
    }
    match state.as_str() {
        "queued" | "running" => {
            text.push_str("The result is not ready. Repeat get_analysis with this id shortly");
            if let Some(s) = doc["retry_after_s"].as_i64() {
                text.push_str(&format!(" (about {s} s)"));
            }
            text.push_str(".\n");
        }
        "blocked" | "failed" => {
            text.push_str("No estimates: the analysis was blocked or failed (see the guards above and `interpretation`).\n");
        }
        _ => {}
    }
    if let (Some(used), Some(dropped)) =
        (doc["n"]["used"].as_i64(), doc["n"]["dropped"].as_object())
    {
        let d: Vec<String> = dropped
            .iter()
            .filter(|(_, v)| v.as_i64().unwrap_or(0) > 0)
            .map(|(k, v)| format!("{k} {v}"))
            .collect();
        text.push_str(&format!(
            "n used: {used}{}\n",
            if d.is_empty() {
                String::new()
            } else {
                format!(" (dropped: {})", d.join(", "))
            }
        ));
    }
    if let Some(s) = doc["interpretation"]["summary"].as_str() {
        text.push_str(&format!("\n{s}\n"));
        for c in doc["interpretation"]["caveats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            text.push_str(&format!("- caveat: {c}\n"));
        }
    }
    let mut lines = Vec::new();
    if doc["estimates"].is_object() {
        estimate_lines(&doc["estimates"], "", 0, &mut lines);
    }
    if !lines.is_empty() {
        text.push_str("\nESTIMATES (full detail in the structured content)\n");
        for l in &lines {
            text.push_str(&format!("{l}\n"));
        }
    }
    if !notes.is_empty() {
        text.push_str("\nNOTES\n");
        for l in &notes {
            text.push_str(&format!("{l}\n"));
        }
    }
    text.push_str(&format!(
        "\nresult: {url} (immutable; bundle {url}/bundle)\n"
    ));

    let estimate = guards
        .iter()
        .any(|g| g["code"] == "ESTIMATED_INPUT" || g["code"] == "MODEL_ON_MODEL")
        || doc["inputs"].as_array().into_iter().flatten().any(|i| {
            i["class"]
                .as_str()
                .is_some_and(|c| is_estimate_indicator(i["indicator"].as_str().unwrap_or(""), c))
        });
    let result = json!({
        "id": id, "state": state, "method": method, "url": url,
        "guards_first": { "not_causal": not_causal, "block": blocks, "warn": warns },
        "analysis": doc,
    });
    let mut out = Out::new(result, text)
        .datasets(dataset_ids(&doc))
        .estimate(estimate);
    if matches!(state.as_str(), "queued" | "running") {
        out = out.next(Next::new(
            "get_analysis",
            json!({ "id": id }),
            "check again",
        ));
    } else {
        out = out
            .next(
                Next::new(
                    "get_analysis",
                    json!({ "id": id }),
                    "fetch this result again later",
                )
                .with_href(format!("/v2/analyses/{id}")),
            )
            .next(Next::new(
                "list_analysis_methods",
                json!({}),
                "other methods and their guards",
            ));
    }
    Ok(out)
}
