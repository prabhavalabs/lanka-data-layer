//! A deliberately small JSONPath subset over borrowed raw JSON: `$`, `.name`
//! and `[n]`. Numbers keep their exact serialised token (the verbatim of an
//! API value), which a parse into `serde_json::Value` would lose.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde_json::value::RawValue;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

fn steps(path: &str) -> Result<Vec<Step>> {
    let rest = path
        .strip_prefix('$')
        .map_or(path, |r| r.strip_prefix('.').unwrap_or(r));
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !cur.is_empty() {
                    out.push(Step::Key(std::mem::take(&mut cur)));
                }
            }
            '[' => {
                if !cur.is_empty() {
                    out.push(Step::Key(std::mem::take(&mut cur)));
                }
                let mut n = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    n.push(d);
                }
                out.push(Step::Index(
                    n.trim()
                        .parse()
                        .with_context(|| format!("bad index [{n}] in path {path:?}"))?,
                ));
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(Step::Key(cur));
    }
    Ok(out)
}

/// Follows `path` from `root`. `None` when a step is missing or crosses a
/// JSON `null`; an error when a step meets the wrong JSON type.
pub fn select<'a>(root: &'a RawValue, path: &str) -> Result<Option<&'a RawValue>> {
    let mut cur = root;
    for step in steps(path)? {
        if cur.get().trim() == "null" {
            return Ok(None);
        }
        cur = match step {
            Step::Key(k) => {
                let m: BTreeMap<String, &RawValue> = serde_json::from_str(cur.get())
                    .with_context(|| format!("{path:?}: expected an object before .{k}"))?;
                match m.get(&k) {
                    Some(v) => v,
                    None => return Ok(None),
                }
            }
            Step::Index(i) => {
                let a: Vec<&RawValue> = serde_json::from_str(cur.get())
                    .with_context(|| format!("{path:?}: expected an array before [{i}]"))?;
                match a.get(i) {
                    Some(v) => v,
                    None => return Ok(None),
                }
            }
        };
    }
    Ok(Some(cur))
}

/// The elements of an array (`None`/`null` → empty).
pub fn elements(v: Option<&RawValue>) -> Result<Vec<&RawValue>> {
    match v {
        None => Ok(Vec::new()),
        Some(r) if r.get().trim() == "null" => Ok(Vec::new()),
        Some(r) => serde_json::from_str(r.get()).context("expected a JSON array"),
    }
}

/// Text of a scalar: strings unescaped, numbers and booleans as serialised,
/// `null` → `None`. Objects and arrays are rejected.
pub fn text(v: Option<&RawValue>) -> Result<Option<String>> {
    let Some(r) = v else { return Ok(None) };
    let t = r.get().trim();
    match t.as_bytes().first() {
        None => Ok(None),
        Some(b'"') => Ok(Some(serde_json::from_str::<String>(t)?)),
        Some(b'{' | b'[') => bail!("expected a scalar, found {}", truncate(t, 80)),
        _ if t == "null" => Ok(None),
        _ => Ok(Some(t.to_owned())),
    }
}

#[must_use]
pub fn truncate(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_with_exact_tokens() {
        let doc: Box<RawValue> = serde_json::from_str(
            r#"[{"page":1,"lastupdated":"2026-11-11"},[{"value":2.5e7,"ind":{"value":"Pop"}},{"value":null}]]"#,
        )
        .expect("json");
        let r = select(&doc, "$[1][0].value").expect("ok");
        assert_eq!(text(r).expect("text").as_deref(), Some("2.5e7"));
        let l = select(&doc, "$[1][0].ind.value").expect("ok");
        assert_eq!(text(l).expect("text").as_deref(), Some("Pop"));
        assert_eq!(
            text(select(&doc, "$[1][1].value").expect("ok")).expect("t"),
            None
        );
        assert!(select(&doc, "$[1][5]").expect("ok").is_none());
        assert!(select(&doc, "$[0].missing").expect("ok").is_none());
        assert_eq!(
            elements(select(&doc, "$[1]").expect("ok"))
                .expect("arr")
                .len(),
            2
        );
        assert!(
            select(&doc, "$[0][1]").is_err(),
            "type mismatch is an error"
        );
        assert_eq!(
            text(select(&doc, "[0].page").expect("ok"))
                .expect("t")
                .as_deref(),
            Some("1")
        );
    }
}
