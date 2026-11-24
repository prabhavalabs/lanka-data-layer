//! The documents archive and its harvester (13 §1): official documents
//! (extraordinary gazettes first) listed from the publisher's site, stored as
//! rows in schema `docs`, with the PDFs and their text in the blob store.
//!
//! | module | what |
//! |---|---|
//! | [`action`] | resolve the Next.js server-action id from the site's own JavaScript (pure parsing) |
//! | [`rsc`] | parse the React Server Components response of the listing call (pure) |
//! | [`record`] | normalised record, `meta_hash`, `doc_id` derivation, the stop and withdrawal rules (pure) |
//! | [`harvest`] | listing modes `incremental` and `sweep`, upserts, revisions, `docs.harvests` |
//! | [`files`] | downloads into the store, text layer and OCR through the sandbox, `docs.search` |

pub mod action;
pub mod files;
pub mod harvest;
pub mod record;
pub mod rsc;

use std::time::Duration;

use anyhow::Result;

/// User agent of every request of the harvester: names the project and its URL.
pub const USER_AGENT: &str = concat!(
    "lanka-data-layer/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/prabhavalabs/lanka-data-layer)"
);

/// Failures the harvester distinguishes.
#[derive(Debug, thiserror::Error)]
pub enum DocsError {
    /// The site no longer answers the way the harvester expects (the server
    /// action id is gone, or the response has no `data` frame). Never silent
    /// emptiness: the run fails and is recorded.
    #[error("upstream_changed: {0}")]
    UpstreamChanged(String),
}

/// An HTTP client for the harvester (rustls, project user agent).
pub fn client(timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .timeout(timeout)
        .tls_backend_rustls()
        .build()?)
}

/// Percent-encodes everything except the RFC 3986 unreserved characters
/// (a query-parameter value: `/`, spaces and brackets are encoded).
#[must_use]
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(
            percent_encode("extra-gazette-content/G 48538 (E)_1794986733.pdf"),
            "extra-gazette-content%2FG%2048538%20%28E%29_1794986733.pdf"
        );
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(percent_encode("é"), "%C3%A9");
    }
}
