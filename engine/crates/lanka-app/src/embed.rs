//! oEmbed for map URLs (10 §6.2–§6.3, 06 §7). Pure: the `/v2/oembed`
//! handler of `lanka-server` calls it and never reads `app.*` — a `/v2`
//! response does not depend on who asks or on app-plane state, so the
//! title is generic and an unknown snapshot id still yields an iframe
//! (the embed route shows "not found").

use serde::Serialize;

/// A map URL of this deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapUrl {
    /// `ms_…` snapshot or `m_…` saved map.
    pub id: String,
    /// `@{seq}`: one exact version of a saved map.
    pub seq: Option<u32>,
    /// View-state and embed parameters carried into the iframe (already
    /// filtered to known keys and safe values).
    pub query: Vec<(String, String)>,
}

/// Query keys carried from a map URL into the embed (10 §6.2).
const KEPT: &[&str] = &["v", "t", "sel", "controls", "interactive", "lang", "theme"];

fn is_map_id(s: &str) -> bool {
    crate::snapshots::is_snapshot_id(s)
        || s.strip_prefix("m_").is_some_and(|r| {
            (1..=40).contains(&r.len())
                && r.bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'_')
        })
}

/// `scheme://host[:port]` of `url`, lowercased; `None` unless http(s).
#[must_use]
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() || host.contains('@') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("https", Some((h, "443"))) | ("http", Some((h, "80"))) => h.to_owned(),
        _ => host,
    };
    Some(format!("{scheme}://{host}"))
}

/// Parses `url` as a map URL of the deployment at `base` (its origin must
/// match): `/m/{id}[@seq]`, `/embed/m/{id}[@seq]`, `/id/map/{id}`.
#[must_use]
pub fn parse_map_url(url: &str, base: &str) -> Option<MapUrl> {
    let url = url.trim();
    if origin(url)? != origin(base)? {
        return None;
    }
    let after_scheme = url.split_once("://")?.1;
    let path_and_query = after_scheme.find('/').map_or("/", |i| &after_scheme[i..]);
    let path_and_query = path_and_query.split('#').next().unwrap_or_default();
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_and_query, None),
    };
    let path = path.trim_end_matches('/');
    let tail = path
        .strip_prefix("/embed/m/")
        .or_else(|| path.strip_prefix("/m/"))
        .or_else(|| path.strip_prefix("/id/map/"))?;
    let (id, seq) = match tail.split_once('@') {
        Some((id, seq)) => (id, Some(seq.parse::<u32>().ok().filter(|s| *s > 0)?)),
        None => (tail, None),
    };
    if !is_map_id(id) || (seq.is_some() && !id.starts_with("m_")) {
        return None;
    }
    let safe = |v: &str| {
        !v.is_empty()
            && v.len() <= 200
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b',' | b'-' | b'_' | b':'))
    };
    let query = query
        .unwrap_or_default()
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, v)| KEPT.contains(k) && safe(v))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    Some(MapUrl {
        id: id.to_owned(),
        seq,
        query,
    })
}

/// The oEmbed 1.0 `rich` response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OEmbed {
    pub version: &'static str,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub title: String,
    pub provider_name: String,
    pub provider_url: String,
    pub html: String,
    pub width: u32,
    pub height: u32,
    pub cache_age: u32,
}

const DEFAULT_W: u32 = 800;
const DEFAULT_H: u32 = 600;

/// Escapes text for an HTML attribute value.
fn attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            c => o.push(c),
        }
    }
    o
}

/// The embed of `m` at `base`, sized to fit `max_width` × `max_height`
/// (4:3, at least 200 × 150).
#[must_use]
pub fn oembed(
    m: &MapUrl,
    base: &str,
    provider: &str,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> OEmbed {
    let base = base.trim_end_matches('/');
    let mut w = DEFAULT_W;
    let mut h = DEFAULT_H;
    if let Some(mw) = max_width.filter(|mw| *mw < w) {
        w = mw.max(200);
        h = w * 3 / 4;
    }
    if let Some(mh) = max_height.filter(|mh| *mh < h) {
        h = mh.max(150);
        w = h * 4 / 3;
    }
    let mut src = format!("{base}/embed/m/{}", m.id);
    if let Some(seq) = m.seq {
        src.push_str(&format!("@{seq}"));
    }
    for (i, (k, v)) in m.query.iter().enumerate() {
        src.push(if i == 0 { '?' } else { '&' });
        src.push_str(&format!("{k}={v}"));
    }
    let title = format!("Map — {provider}");
    let html = format!(
        "<iframe src=\"{}\" width=\"{w}\" height=\"{h}\" style=\"border:0\" loading=\"lazy\" \
         allowfullscreen referrerpolicy=\"strict-origin-when-cross-origin\" title=\"{}\"></iframe>",
        attr(&src),
        attr(&title)
    );
    OEmbed {
        version: "1.0",
        kind: "rich",
        title,
        provider_name: provider.to_owned(),
        provider_url: base.to_owned(),
        html,
        width: w,
        height: h,
        // snapshots are immutable; saved maps change their published version
        cache_age: if m.id.starts_with("ms_") { 86_400 } else { 300 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://lanka.example.org";

    #[test]
    fn map_urls_parse() {
        let m = parse_map_url(
            "https://Lanka.Example.org:443/m/ms_0123456789abcdef?v=79.98,6.86,9.2&t=2024&evil=<x>",
            BASE,
        )
        .expect("snapshot url");
        assert_eq!(m.id, "ms_0123456789abcdef");
        assert_eq!(m.seq, None);
        assert_eq!(
            m.query,
            [
                ("v".to_owned(), "79.98,6.86,9.2".to_owned()),
                ("t".to_owned(), "2024".to_owned())
            ]
        );
        let v = parse_map_url(&format!("{BASE}/embed/m/m_7k2q9x4w8n1c@3"), BASE).expect("version");
        assert_eq!((v.id.as_str(), v.seq), ("m_7k2q9x4w8n1c", Some(3)));
        for bad in [
            "https://other.example.org/m/ms_0123456789abcdef",
            "ftp://lanka.example.org/m/ms_0123456789abcdef",
            "https://lanka.example.org/m/ms_0123",
            "https://lanka.example.org/m/ms_0123456789abcdef@2",
            "https://lanka.example.org/datasets/census",
            "https://user@lanka.example.org/m/ms_0123456789abcdef",
            "https://lanka.example.org/m/m_UPPER",
        ] {
            assert_eq!(parse_map_url(bad, BASE), None, "{bad}");
        }
    }

    #[test]
    fn rich_response_is_escaped_and_sized() {
        let m = MapUrl {
            id: "ms_0123456789abcdef".into(),
            seq: None,
            query: vec![("t".into(), "2024".into()), ("sel".into(), "LK11".into())],
        };
        let o = oembed(&m, BASE, "Lanka \"Data\" Layer", Some(400), None);
        assert_eq!((o.width, o.height), (400, 300));
        assert!(o.html.contains(
            "src=\"https://lanka.example.org/embed/m/ms_0123456789abcdef?t=2024&amp;sel=LK11\""
        ));
        assert!(
            o.html
                .contains("title=\"Map — Lanka &quot;Data&quot; Layer\"")
        );
        let o = oembed(&m, BASE, "x", None, Some(10));
        assert_eq!((o.width, o.height), (200, 150));
    }
}
