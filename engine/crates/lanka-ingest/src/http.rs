//! HTTP clients of connector runs.
//!
//! Every fetch goes through rustls, except to hosts the country profile
//! lists under `fetch.legacy_tls_hosts`: those get the platform TLS stack
//! (OpenSSL on Linux, Security.framework on macOS), chosen per request by
//! the URL's host.
//!
//! Why: some government servers (www.statistics.gov.lk, IIS 8.5 / SChannel)
//! offer only TLS 1.2 CBC suites (`ECDHE-RSA-AES256-SHA384`) that rustls
//! deliberately does not implement. Such a server answers a ClientHello
//! without a suite it knows by resetting the TCP connection instead of
//! sending a `handshake_failure` alert, so the failure surfaces as
//! "Connection reset by peer" during connect, not as a TLS error. Headers
//! and the HTTP version play no part: the reset happens before any request
//! bytes are sent, and `curl --ciphers ECDHE-RSA-AES128-GCM-SHA256` is reset
//! the same way.

use std::time::Duration;

use anyhow::Result;
use lanka_core::Profile;

use crate::registry::SourceSpec;

/// User agent of every fetch: the crawler convention `Mozilla/5.0
/// (compatible; <bot>/<version>; +<url>)`. It identifies the project; some
/// government web application firewalls (CBSL) refuse agents without the
/// `Mozilla/5.0 (compatible; …)` prefix.
pub const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; lanka-data-layer-ingest/",
    env!("CARGO_PKG_VERSION"),
    "; +https://github.com/prabhavalabs/lanka-data-layer)"
);

/// TLS stack of a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tls {
    /// rustls (TLS 1.2 AEAD suites and TLS 1.3 only).
    #[default]
    Modern,
    /// The platform stack (OpenSSL / Security.framework): also TLS 1.2 CBC suites.
    Legacy,
}

/// A connector client: identifies the project, follows redirects,
/// decompresses, and times out.
pub fn client(tls: Tls) -> Result<reqwest::Client> {
    let b = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300));
    let b = match tls {
        Tls::Modern => b.tls_backend_rustls(),
        Tls::Legacy => b.tls_backend_native(),
    };
    Ok(b.build()?)
}

/// The host of `url`, lowercased (`None` when it does not parse).
fn host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
}

/// The clients of one run: rustls, plus the platform stack for the legacy hosts.
#[derive(Debug, Clone)]
pub struct Http {
    modern: reqwest::Client,
    legacy: Option<reqwest::Client>,
    legacy_hosts: Vec<String>,
}

impl Http {
    /// rustls for every host.
    pub fn modern() -> Result<Self> {
        Self::with_legacy_hosts(&[])
    }

    /// rustls, except for `hosts` (exact host names, case-insensitive).
    pub fn with_legacy_hosts(hosts: &[String]) -> Result<Self> {
        let legacy_hosts: Vec<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();
        Ok(Self {
            modern: client(Tls::Modern)?,
            legacy: if legacy_hosts.is_empty() {
                None
            } else {
                Some(client(Tls::Legacy)?)
            },
            legacy_hosts,
        })
    }

    /// The clients for one source: the profile's legacy hosts, plus every
    /// host of the source when its `access.tls` is `legacy`.
    pub fn for_source(profile: &Profile, source: Option<&SourceSpec>) -> Result<Self> {
        let mut hosts = profile.spec.fetch.legacy_tls_hosts.clone();
        if let Some(s) = source
            && s.access.tls == Some(Tls::Legacy.as_str().to_owned())
        {
            hosts.extend(
                [s.homepage.as_str(), s.access.endpoint.as_str()]
                    .into_iter()
                    .filter_map(host_of),
            );
        }
        Self::with_legacy_hosts(&hosts)
    }

    /// Which stack a request to `url` uses.
    #[must_use]
    pub fn tls_for(&self, url: &str) -> Tls {
        match host_of(url) {
            Some(h) if self.legacy.is_some() && self.legacy_hosts.contains(&h) => Tls::Legacy,
            _ => Tls::Modern,
        }
    }

    /// The client for `url`.
    #[must_use]
    pub fn client_for(&self, url: &str) -> &reqwest::Client {
        match (self.tls_for(url), &self.legacy) {
            (Tls::Legacy, Some(c)) => c,
            _ => &self.modern,
        }
    }

    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client_for(url).get(url)
    }

    pub fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client_for(url).post(url)
    }

    pub fn head(&self, url: &str) -> reqwest::RequestBuilder {
        self.client_for(url).head(url)
    }
}

impl Tls {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Modern => "modern",
            Self::Legacy => "legacy",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_pick_the_stack_by_exact_host() {
        let h = Http::with_legacy_hosts(&["WWW.Statistics.gov.lk".into()]).expect("clients");
        assert_eq!(
            h.tls_for("https://www.statistics.gov.lk/abstract2025/CHAP2/2.1a.pdf"),
            Tls::Legacy
        );
        assert_eq!(
            h.tls_for("https://WWW.STATISTICS.GOV.LK:443/x"),
            Tls::Legacy
        );
        // Sibling hosts are not covered (nada.statistics.gov.lk speaks TLS 1.3).
        assert_eq!(
            h.tls_for("https://nada.statistics.gov.lk/index.php/catalog"),
            Tls::Modern
        );
        assert_eq!(h.tls_for("https://data.humdata.org/x"), Tls::Modern);
        assert_eq!(h.tls_for("not a url"), Tls::Modern);
        let m = Http::modern().expect("clients");
        assert_eq!(m.tls_for("https://www.statistics.gov.lk/"), Tls::Modern);
    }

    #[test]
    fn profile_hosts_and_source_override() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
        let profile = Profile::discover(&root)
            .and_then(Profile::load)
            .expect("profile");
        let reg = crate::Registry::load(&root).expect("registry");
        // The profile puts every DCS source on the platform stack.
        let dcs = Http::for_source(&profile, reg.source("dcs-agriculture")).expect("clients");
        assert_eq!(
            dcs.tls_for(
                "https://www.statistics.gov.lk/Agriculture/StaticalInformation/PaddyStatistics"
            ),
            Tls::Legacy
        );
        assert_eq!(dcs.tls_for("https://api.worldbank.org/v2/"), Tls::Modern);
        // `access.tls: legacy` adds the source's own hosts.
        let mut s = reg.source("hdx-cod-ps-lka").expect("source").clone();
        let plain = Http::for_source(&profile, Some(&s)).expect("clients");
        assert_eq!(plain.tls_for(&s.access.endpoint), Tls::Modern);
        s.access.tls = Some("legacy".into());
        let legacy = Http::for_source(&profile, Some(&s)).expect("clients");
        assert_eq!(legacy.tls_for(&s.access.endpoint), Tls::Legacy);
        assert_eq!(
            legacy.tls_for("https://www.statistics.gov.lk/"),
            Tls::Legacy
        );
    }
}
