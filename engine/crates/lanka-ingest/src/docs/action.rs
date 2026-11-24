//! Resolving the Next.js server-action id (13 §1.2 step 1).
//!
//! The collection page is server-rendered; its listing is fetched by a server
//! action whose id is baked into one of the page's JavaScript chunks as
//! `createServerReference)("<40-42 hex>", …, "TableDataAction")`. The id
//! changes with every deploy, so it is read from the site's own scripts and
//! never copied by hand. Everything here is pure (fixture-testable); the
//! fetching is [`resolve`].

use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use regex::Regex;

/// The action the collection page uses for its table.
pub const TABLE_ACTION: &str = "TableDataAction";

static SCRIPT_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:src|href)\s*=\s*"([^"]*?/_next/static/chunks/[^"?#]+\.js)(?:\?[^"]*)?""#)
        .expect("script regex")
});

/// The chunk URLs a page references, in document order, deduplicated.
#[must_use]
pub fn chunk_paths(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in SCRIPT_SRC.captures_iter(html) {
        let p = c[1].to_owned();
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The id of the server action called `name` in one chunk of JavaScript.
///
/// The reference is `createServerReference)("<id>",x.callServer,void 0,
/// x.findSourceMapURL,"<Name>")`; the lazy `[^)]*?` cannot cross the closing
/// parenthesis, so a neighbouring action's id is never paired with `name`.
#[must_use]
pub fn extract_action_id(js: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"createServerReference\)\("([0-9a-f]{{40,42}})"[^)]*?"{}""#,
        regex::escape(name)
    ))
    .ok()?;
    re.captures(js).map(|c| c[1].to_owned())
}

/// Absolute URL of a chunk path found on `page_url`.
pub fn chunk_url(page_url: &str, path: &str) -> Result<String> {
    let base = reqwest::Url::parse(page_url).with_context(|| format!("page url {page_url}"))?;
    Ok(base
        .join(path)
        .with_context(|| format!("chunk path {path}"))?
        .to_string())
}

/// Fetches the page and its chunks and returns the id of `TableDataAction`.
pub async fn resolve(http: &reqwest::Client, page_url: &str) -> Result<String> {
    let html = http
        .get(page_url)
        .send()
        .await
        .with_context(|| format!("GET {page_url}"))?
        .error_for_status()
        .with_context(|| format!("GET {page_url}"))?
        .text()
        .await?;
    let chunks = chunk_paths(&html);
    if chunks.is_empty() {
        bail!("{page_url} references no /_next/static/chunks/*.js");
    }
    for path in &chunks {
        let url = chunk_url(page_url, path)?;
        let js = match http.get(&url).send().await {
            Ok(r) if r.status().is_success() => r.text().await?,
            Ok(r) => {
                tracing::warn!(url, status = %r.status(), "chunk not fetched");
                continue;
            }
            Err(e) => {
                tracing::warn!(url, error = %e, "chunk not fetched");
                continue;
            }
        };
        if let Some(id) = extract_action_id(&js, TABLE_ACTION) {
            tracing::info!(id, chunk = path, "resolved the {TABLE_ACTION} id");
            return Ok(id);
        }
    }
    bail!(
        "no {TABLE_ACTION} reference in the {} chunks of {page_url}",
        chunks.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../../tests/fixtures/docs/page.html");
    const CHUNK: &str = include_str!("../../tests/fixtures/docs/chunk.js");

    #[test]
    fn chunk_urls_come_from_the_page() {
        let c = chunk_paths(PAGE);
        assert!(c.len() >= 3, "{c:?}");
        assert!(c.iter().all(|p| p.starts_with("/_next/static/chunks/")));
        assert!(c.contains(&"/_next/static/chunks/33gf_n8651aja.js".to_owned()));
        // deduplicated
        let mut s = c.clone();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), c.len());
        assert_eq!(
            chunk_url(
                "https://documents.gov.lk/web/extra_gazettes",
                "/_next/static/chunks/a.js"
            )
            .expect("url"),
            "https://documents.gov.lk/_next/static/chunks/a.js"
        );
    }

    #[test]
    fn the_table_action_wins_over_the_decoys() {
        assert_eq!(
            extract_action_id(CHUNK, TABLE_ACTION).as_deref(),
            Some("40dd03742cae2465a031936880def365fdd014c22b")
        );
        assert_eq!(
            extract_action_id(CHUNK, "fetchDates").as_deref(),
            Some("780fee69d83a93de0e5d2d288348cd0a18dce7baae")
        );
        assert_eq!(extract_action_id(CHUNK, "NoSuchAction"), None);
    }

    #[test]
    fn a_chunk_without_the_action_yields_nothing() {
        let other = r#"(0,a.createServerReference)("60c73a2b65facf9756e337ec7ae52d2c30fc6104ac",z.callServer,void 0,z.findSourceMapURL,"SearchPagesAction")"#;
        assert_eq!(extract_action_id(other, TABLE_ACTION), None);
        assert_eq!(
            extract_action_id(other, "SearchPagesAction").as_deref(),
            Some("60c73a2b65facf9756e337ec7ae52d2c30fc6104ac")
        );
    }
}
