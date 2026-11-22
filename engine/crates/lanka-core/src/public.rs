//! The public base URL (06 §2): one origin for every absolute link the
//! engine emits — TileJSON `tiles`/`pmtiles`, analysis citations, snapshot
//! `url`/`embed`, package links, oEmbed.
//!
//! Resolution, first match wins:
//! 1. `LANKA_PUBLIC_BASE_URL` when set; **empty means relative**: links are
//!    bare paths (`/v2/tiles/admin/{z}/{x}/{y}.mvt`), for a dev proxy or any
//!    client that resolves them against the API origin it called;
//! 2. `LANKA_PUBLIC_URL` (deprecated alias, non-empty only);
//! 3. `X-Forwarded-Proto` + `X-Forwarded-Host` of the request, only when the
//!    TCP peer is listed in `LANKA_TRUSTED_PROXIES` (comma-separated IPs);
//! 4. the country profile's `site.base_url`.
//!
//! `Host` alone is never trusted. Payload links that stay inside the API
//! (`next`, analysis output `href`s) are relative whatever the base.

use std::net::IpAddr;
use std::sync::OnceLock;

/// The configured part of the public base (read once from the environment).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublicBase {
    /// `Some("")` = relative links; `Some(origin)`; `None` = not configured.
    configured: Option<String>,
    trusted_proxies: Vec<IpAddr>,
}

fn trim(s: &str) -> String {
    s.trim().trim_end_matches('/').to_owned()
}

impl PublicBase {
    /// From an environment lookup (`std::env::var` in production).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let configured = get("LANKA_PUBLIC_BASE_URL").map(|v| trim(&v)).or_else(|| {
            get("LANKA_PUBLIC_URL")
                .filter(|v| !v.trim().is_empty())
                .map(|v| trim(&v))
        });
        let trusted_proxies = get("LANKA_TRUSTED_PROXIES")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse::<IpAddr>().ok())
            .map(|ip| ip.to_canonical())
            .collect();
        Self {
            configured,
            trusted_proxies,
        }
    }

    /// The configured origin (`Some("")` = relative), before the request
    /// and profile fallbacks.
    #[must_use]
    pub fn configured(&self) -> Option<&str> {
        self.configured.as_deref()
    }

    /// The process-wide base, read from the environment on first use.
    pub fn global() -> &'static Self {
        static G: OnceLock<PublicBase> = OnceLock::new();
        G.get_or_init(|| Self::from_lookup(|k| std::env::var(k).ok()))
    }

    /// The origin for links that do not depend on a request (`""` =
    /// relative); `profile_base` is the profile's `site.base_url`.
    #[must_use]
    pub fn origin(&self, profile_base: Option<&str>) -> String {
        match &self.configured {
            Some(c) => c.clone(),
            None => profile_base.map(trim).unwrap_or_default(),
        }
    }

    /// The origin for links in a response to a request from `peer` with
    /// the given `X-Forwarded-Proto`/`X-Forwarded-Host` values.
    #[must_use]
    pub fn origin_for(
        &self,
        peer: Option<IpAddr>,
        forwarded_proto: Option<&str>,
        forwarded_host: Option<&str>,
        profile_base: Option<&str>,
    ) -> String {
        if let Some(c) = &self.configured {
            return c.clone();
        }
        let trusted = peer.is_some_and(|p| self.trusted_proxies.contains(&p.to_canonical()));
        if trusted && let Some(host) = forwarded_host.and_then(|h| h.split(',').next()) {
            let host = host.trim();
            if valid_host(host) {
                let proto = match forwarded_proto
                    .and_then(|p| p.split(',').next())
                    .map(str::trim)
                {
                    Some("http") => "http",
                    _ => "https",
                };
                return format!("{proto}://{host}");
            }
        }
        profile_base.map(trim).unwrap_or_default()
    }
}

/// `host[:port]` with no path, user info or whitespace.
fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(vars: &[(&str, &str)]) -> PublicBase {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        PublicBase::from_lookup(|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
    }

    const PROFILE: Option<&str> = Some("https://lankadata.example/");

    #[test]
    fn configured_base_wins_and_empty_means_relative() {
        let b = base(&[("LANKA_PUBLIC_BASE_URL", "https://api.example.org/")]);
        assert_eq!(b.origin(PROFILE), "https://api.example.org");
        let rel = base(&[
            ("LANKA_PUBLIC_BASE_URL", ""),
            ("LANKA_PUBLIC_URL", "https://x"),
        ]);
        assert_eq!(rel.origin(PROFILE), "");
        let peer = Some("127.0.0.1".parse().expect("ip"));
        assert_eq!(
            rel.origin_for(peer, Some("https"), Some("evil"), PROFILE),
            ""
        );
    }

    #[test]
    fn deprecated_alias_then_profile() {
        assert_eq!(
            base(&[("LANKA_PUBLIC_URL", "https://old.example")]).origin(PROFILE),
            "https://old.example"
        );
        assert_eq!(base(&[]).origin(PROFILE), "https://lankadata.example");
        assert_eq!(base(&[]).origin(None), "");
    }

    #[test]
    fn forwarded_headers_only_from_trusted_proxies() {
        let b = base(&[("LANKA_TRUSTED_PROXIES", "127.0.0.1, ::1")]);
        let proxy = Some("127.0.0.1".parse().expect("ip"));
        let other = Some("10.0.0.9".parse().expect("ip"));
        assert_eq!(
            b.origin_for(proxy, Some("https"), Some("maps.example.org"), PROFILE),
            "https://maps.example.org"
        );
        assert_eq!(
            b.origin_for(proxy, Some("http"), Some("localhost:5183"), PROFILE),
            "http://localhost:5183"
        );
        assert_eq!(
            b.origin_for(other, Some("https"), Some("evil.example"), PROFILE),
            "https://lankadata.example"
        );
        assert_eq!(
            b.origin_for(proxy, None, Some("bad host/x"), PROFILE),
            "https://lankadata.example"
        );
        // nobody trusted by default
        assert_eq!(
            base(&[]).origin_for(proxy, None, Some("evil.example"), PROFILE),
            "https://lankadata.example"
        );
    }
}
