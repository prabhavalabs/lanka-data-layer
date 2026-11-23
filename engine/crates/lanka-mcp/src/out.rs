//! Result and error assembly shared by every tool (06 §4 "Result rules").
//!
//! Every result — success or error — has the same envelope in its
//! structured content:
//!
//! ```text
//! { data_version, result | error, attribution[], licences[], dataset_versions[],
//!   next[], notices[], estimate_notice? }
//! ```
//!
//! and a compact text block (what a model reads first): the estimate notice
//! (when any value is modelled), the tool's own text, then a one-line
//! footer with the data version, sources and the ready-to-send follow-up
//! calls. At most [`MAX_ROWS`] rows and [`MAX_RESULT_BYTES`] bytes leave the
//! server per call.

use lanka_core::{Catalog, CoreError};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

/// Rows per result (06 §4).
pub const MAX_ROWS: usize = 500;
/// Bytes per result, structured content and text together (06 §4).
pub const MAX_RESULT_BYTES: usize = 256 * 1024;
/// Text block cap: the structured content carries the full rows.
const MAX_TEXT_BYTES: usize = 20 * 1024;

/// A ready-to-send follow-up call.
#[derive(Debug, Clone)]
pub struct Next {
    pub tool: &'static str,
    pub arguments: Value,
    pub why: String,
    /// The REST route that returns the same data (downloads, overflow).
    pub href: Option<String>,
}

impl Next {
    pub fn new(tool: &'static str, arguments: Value, why: impl Into<String>) -> Self {
        Self {
            tool,
            arguments,
            why: why.into(),
            href: None,
        }
    }

    #[must_use]
    pub fn with_href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }

    fn to_json(&self) -> Value {
        let mut v = json!({ "tool": self.tool, "arguments": self.arguments, "why": self.why });
        if let Some(h) = &self.href {
            v["href"] = json!(h);
        }
        v
    }

    fn to_text(&self) -> String {
        format!("{}({})", self.tool, compact_args(&self.arguments))
    }
}

/// `{"a":1,"b":"x"}` → `a=1, b="x"`.
fn compact_args(v: &Value) -> String {
    match v {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

/// A successful tool answer, before the envelope is applied.
#[derive(Debug, Default)]
pub struct Out {
    /// The tool's structured payload (under `result`).
    pub result: Value,
    /// The tool's own text rendering.
    pub text: String,
    /// Datasets whose sources and licences the answer cites.
    pub datasets: Vec<String>,
    pub next: Vec<Next>,
    /// Any returned value is estimated, forecast or from an estimate-class indicator.
    pub estimate: bool,
    pub notices: Vec<String>,
}

impl Out {
    pub fn new(result: Value, text: String) -> Self {
        Self {
            result,
            text,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn datasets<I, S>(mut self, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.datasets.extend(ids.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn next(mut self, n: Next) -> Self {
        self.next.push(n);
        self
    }

    #[must_use]
    pub fn estimate(mut self, yes: bool) -> Self {
        self.estimate |= yes;
        self
    }

    #[must_use]
    pub fn notice(mut self, n: impl Into<String>) -> Self {
        self.notices.push(n.into());
        self
    }
}

/// The one-sentence estimate notice (04 §8): repeated in the text block so a
/// model does not paraphrase a modelled value as a fact.
pub const ESTIMATE_NOTICE: &str = "ESTIMATE: at least one value below is modelled or forecast (basis `estimated` or `forecast`, or an est./idx./acc./exp. indicator), not an official figure. Say \"estimated\" when quoting it, give the interval when there is one, and read the dataset's method page and limits first (describe_dataset).";

/// Whether an indicator id belongs to a reserved estimate namespace
/// (04 §8.2) or its class says so.
#[must_use]
pub fn is_estimate_indicator(id: &str, class: &str) -> bool {
    class == "estimated"
        || ["est.", "idx.", "acc.", "exp."]
            .iter()
            .any(|p| id.starts_with(p))
}

/// Whether a row basis is modelled.
#[must_use]
pub fn is_estimate_basis(basis: &str) -> bool {
    matches!(basis, "estimated" | "forecast")
}

fn meta_values(cat: &Catalog, datasets: &[String]) -> (lanka_core::Meta, Value, Value, Value) {
    let ids: Vec<&str> = datasets.iter().map(String::as_str).collect();
    let meta = cat.meta_for(ids, Vec::new());
    let attribution = serde_json::to_value(&meta.source).unwrap_or(Value::Array(Vec::new()));
    let licences = serde_json::to_value(&meta.licences).unwrap_or(Value::Array(Vec::new()));
    let versions = serde_json::to_value(&meta.dataset_versions).unwrap_or(Value::Array(Vec::new()));
    (meta, attribution, licences, versions)
}

fn footer(meta: &lanka_core::Meta, next: &[Next]) -> String {
    let mut f = String::new();
    f.push_str("\n—\n");
    f.push_str(&format!(
        "data_version: {}",
        meta.data_version.as_deref().unwrap_or("none")
    ));
    if !meta.source.is_empty() {
        let mut parts: Vec<String> = Vec::new();
        for s in &meta.source {
            let status = meta
                .licences
                .iter()
                .find(|l| l.id == s.licence)
                .map_or("", |l| l.status.as_str());
            parts.push(format!(
                "{} [{}{}{}]",
                s.attribution,
                s.licence,
                if status.is_empty() { "" } else { ", " },
                status
            ));
        }
        f.push_str(&format!("\nsources: {}", parts.join("; ")));
    }
    if meta
        .licences
        .iter()
        .any(|l| matches!(l.status.as_str(), "permission_unclear" | "restricted"))
    {
        f.push_str(
            "\nlicence: at least one source has `permission_unclear` or `restricted` status — cite the attribution above and check the licence before redistributing.",
        );
    }
    if !next.is_empty() {
        f.push_str("\nnext: ");
        f.push_str(
            &next
                .iter()
                .take(4)
                .map(Next::to_text)
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    f
}

fn truncate_text(mut s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    if let Some(i) = s.rfind('\n') {
        s.truncate(i);
    }
    s.push_str("\n… (text truncated; the structured content has the rest)");
    s
}

/// Halves the longest array under `v` until `size()` fits `budget`; returns
/// a description of what was dropped.
fn shrink_to_fit(v: &mut Value, other_bytes: usize, budget: usize) -> Option<Value> {
    fn longest(v: &mut Value, path: &mut Vec<String>, best: &mut (usize, Vec<String>)) {
        match v {
            Value::Array(a) => {
                if a.len() > best.0 {
                    *best = (a.len(), path.clone());
                }
                for (i, x) in a.iter_mut().enumerate().take(1) {
                    path.push(i.to_string());
                    longest(x, path, best);
                    path.pop();
                }
            }
            Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    path.push(k.clone());
                    longest(x, path, best);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut dropped: Option<Value> = None;
    for _ in 0..32 {
        let size = other_bytes + serde_json::to_vec(v).map_or(0, |b| b.len());
        if size <= budget {
            break;
        }
        let mut best = (1usize, Vec::new());
        longest(v, &mut Vec::new(), &mut best);
        if best.0 <= 1 {
            break;
        }
        let mut cur: &mut Value = v;
        for k in &best.1 {
            cur = match cur {
                Value::Array(a) => match k.parse::<usize>().ok().and_then(|i| a.get_mut(i)) {
                    Some(x) => x,
                    None => return dropped,
                },
                Value::Object(m) => match m.get_mut(k) {
                    Some(x) => x,
                    None => return dropped,
                },
                _ => return dropped,
            };
        }
        if let Value::Array(a) = cur {
            let keep = (a.len() / 2).max(1);
            let was = a.len();
            a.truncate(keep);
            dropped = Some(json!({ "at": best.1.join("."), "kept": keep, "of": was }));
        }
    }
    dropped
}

/// Applies the result envelope and the size limits.
#[must_use]
pub fn finish(cat: &Catalog, out: Out) -> CallToolResult {
    let Out {
        mut result,
        text,
        datasets,
        next,
        estimate,
        notices,
    } = out;
    let (meta, attribution, licences, versions) = meta_values(cat, &datasets);
    let mut text_block = String::new();
    if estimate {
        text_block.push_str(ESTIMATE_NOTICE);
        text_block.push_str("\n\n");
    }
    text_block.push_str(&text);
    if !notices.is_empty() {
        text_block.push_str("\nnotices:\n");
        for n in &notices {
            text_block.push_str(&format!("- {n}\n"));
        }
    }
    let mut text_block = truncate_text(text_block, MAX_TEXT_BYTES);
    text_block.push_str(&footer(&meta, &next));
    let envelope = |result: &Value, truncated: Option<&Value>| {
        let mut v = json!({
            "data_version": meta.data_version,
            "result": result,
            "attribution": attribution,
            "licences": licences,
            "dataset_versions": versions,
            "next": next.iter().map(Next::to_json).collect::<Vec<_>>(),
            "notices": notices,
        });
        if estimate {
            v["estimate_notice"] = json!(ESTIMATE_NOTICE);
        }
        if let Some(t) = truncated {
            v["truncated"] = t.clone();
        }
        v
    };
    let fixed = serde_json::to_vec(&envelope(&Value::Null, None)).map_or(0, |b| b.len());
    let budget = MAX_RESULT_BYTES.saturating_sub(text_block.len() + 512);
    let dropped = shrink_to_fit(&mut result, fixed, budget);
    if dropped.is_some() {
        text_block.push_str(
            "\n(result trimmed to the 256 KB limit: narrow the request or use the download links in `next`)",
        );
    }
    let structured = envelope(&result, dropped.as_ref());
    let mut res = CallToolResult::success(vec![ContentBlock::text(text_block)]);
    res.structured_content = Some(structured);
    res
}

// ───────────────────────────── errors ──────────────────────────────────────

/// A tool failure the model should see and can act on (`isError: true`),
/// with the REST `hints`.
#[derive(Debug)]
pub struct ToolError {
    /// `not_found` | `not_in_coverage` | `bad_request` | `unavailable` |
    /// `rate_limited` | `internal`
    pub kind: &'static str,
    pub message: String,
    /// The REST problem / hints object (`valid` values, `available_levels`, …).
    pub hints: Value,
    pub next: Vec<Next>,
    pub retry_after_s: Option<u64>,
}

impl ToolError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hints: Value::Null,
            next: Vec::new(),
            retry_after_s: None,
        }
    }

    pub fn bad(message: impl Into<String>) -> Self {
        Self::new("bad_request", message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new("unavailable", message)
    }

    #[must_use]
    pub fn hints(mut self, hints: Value) -> Self {
        self.hints = hints;
        self
    }

    #[must_use]
    pub fn next(mut self, n: Next) -> Self {
        self.next.push(n);
        self
    }
}

impl From<CoreError> for ToolError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NotFound(m) => Self::new("not_found", format!("not_found: {m}")),
            CoreError::NotInCoverage => Self::new(
                "not_in_coverage",
                "not_in_coverage: the point is outside the country's grid",
            )
            .hints(json!({ "hint": "latitude/longitude must fall inside Sri Lanka (about 5.9–9.9 N, 79.5–81.9 E)" })),
            CoreError::BadRequest(m) => Self::bad(m.into_owned()),
            CoreError::Invalid { message, problem } => Self::bad(message).hints(problem),
            CoreError::Db(err) => {
                tracing::error!(error = %err, "mcp: database error");
                Self::new("internal", "internal error")
            }
        }
    }
}

/// The error result: same envelope keys as a success.
#[must_use]
pub fn fail(cat: Option<&Catalog>, e: ToolError) -> CallToolResult {
    let data_version = cat.and_then(|c| c.data_version.clone());
    let message = e
        .message
        .strip_prefix(&format!("{}: ", e.kind))
        .unwrap_or(&e.message)
        .to_owned();
    let mut text = format!("ERROR {}: {message}", e.kind);
    if !e.hints.is_null() {
        text.push_str(&format!("\nhints: {}", e.hints));
    }
    if let Some(s) = e.retry_after_s {
        text.push_str(&format!("\nretry after {s} s"));
    }
    if !e.next.is_empty() {
        text.push_str("\ntry: ");
        text.push_str(
            &e.next
                .iter()
                .map(Next::to_text)
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    let mut structured = json!({
        "data_version": data_version,
        "error": { "kind": e.kind, "message": e.message, "hints": e.hints },
        "attribution": [],
        "licences": [],
        "dataset_versions": [],
        "next": e.next.iter().map(Next::to_json).collect::<Vec<_>>(),
        "notices": [],
    });
    if let Some(s) = e.retry_after_s {
        structured["error"]["retry_after_s"] = json!(s);
    }
    let mut res = CallToolResult::error(vec![ContentBlock::text(text)]);
    res.structured_content = Some(structured);
    res
}

// ───────────────────────────── text helpers ────────────────────────────────

/// A number for text: up to 6 significant digits, no exponent for ordinary
/// magnitudes, thousands left ungrouped (models misread separators).
#[must_use]
pub fn num(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let a = v.abs();
    if (1e-4..1e15).contains(&a) {
        let digits = 5 - a.log10().floor() as i32;
        let dp = digits.clamp(0, 8) as usize;
        let s = format!("{v:.dp$}");
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s
        }
    } else {
        format!("{v:.5e}")
    }
}

/// `value` with its unit label and power-of-ten multiplier.
#[must_use]
pub fn value_text(v: Option<f64>, unit: Option<&str>, mult: i16) -> String {
    let Some(v) = v else {
        return "no value".to_owned();
    };
    let mut s = num(v);
    if mult != 0 {
        s.push_str(&format!(" ×10^{mult}"));
    }
    if let Some(u) = unit.filter(|u| !u.is_empty()) {
        s.push(' ');
        s.push_str(u);
    }
    s
}

/// Level number → the catalogue's `geo_levels` word (`national`, `adm1` …).
#[must_use]
pub fn level_word(level: i64) -> String {
    if level == 0 {
        "national".to_owned()
    } else {
        format!("adm{level}")
    }
}

/// Caps a vector at [`MAX_ROWS`]; returns the original length when capped.
pub fn cap_rows<T>(rows: &mut Vec<T>) -> Option<usize> {
    if rows.len() > MAX_ROWS {
        let n = rows.len();
        rows.truncate(MAX_ROWS);
        Some(n)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(21_763_170.0), "21763170");
        assert_eq!(num(0.123456789), "0.123457");
        assert_eq!(num(3.0), "3");
        assert_eq!(num(1234.5678), "1234.57");
        assert_eq!(num(-2.5), "-2.5");
    }

    #[test]
    fn long_lists_are_halved_until_they_fit() {
        let rows: Vec<Value> = (0..400)
            .map(|i| json!({ "geo": format!("LK{i}"), "pad": "x".repeat(900) }))
            .collect();
        let mut v = json!({ "rows": rows });
        let dropped = shrink_to_fit(&mut v, 0, MAX_RESULT_BYTES).expect("trimmed");
        assert!(serde_json::to_vec(&v).unwrap_or_default().len() <= MAX_RESULT_BYTES);
        assert_eq!(dropped["of"], 400);
        assert!(v["rows"].as_array().is_some_and(|a| a.len() >= 100));
    }

    #[test]
    fn estimate_namespaces_are_recognised() {
        assert!(is_estimate_indicator("est.pop_total", "estimated"));
        assert!(is_estimate_indicator("exp.flood_pop_share", "derived"));
        assert!(!is_estimate_indicator("census24.pop_total", "official"));
        assert!(is_estimate_basis("forecast") && !is_estimate_basis("reported"));
    }

    #[test]
    fn errors_carry_the_envelope_keys() {
        let r = fail(
            None,
            ToolError::bad("level must be 0..4").hints(json!({"valid": [0, 1]})),
        );
        assert_eq!(r.is_error, Some(true));
        let s = r.structured_content.expect("structured");
        for k in ["data_version", "attribution", "licences", "next", "error"] {
            assert!(s.get(k).is_some(), "{k}");
        }
        assert_eq!(s["error"]["hints"]["valid"][1], 1);
    }
}
