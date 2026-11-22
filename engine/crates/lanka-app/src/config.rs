//! App-plane configuration (environment; 12 §2).

use std::time::Duration;

use anyhow::{Context, bail};

/// A rate: `n` events per `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub n: u32,
    pub window: Duration,
}

impl Rate {
    /// `5/h`, `30/min`, `2/s`, `100/d`.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let (n, unit) = s
            .trim()
            .split_once('/')
            .with_context(|| format!("rate {s:?}: expected <n>/<s|min|h|d>"))?;
        let n: u32 = n
            .trim()
            .parse()
            .with_context(|| format!("rate {s:?}: count is not an integer"))?;
        if n == 0 {
            bail!("rate {s:?}: count must be at least 1");
        }
        let secs = match unit.trim() {
            "s" | "sec" | "second" => 1,
            "m" | "min" | "minute" => 60,
            "h" | "hour" => 3_600,
            "d" | "day" => 86_400,
            u => bail!("rate {s:?}: unknown unit {u:?} (s, min, h, d)"),
        };
        Ok(Self {
            n,
            window: Duration::from_secs(secs),
        })
    }
}

impl std::fmt::Display for Rate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let unit = match self.window.as_secs() {
            1 => "s",
            60 => "min",
            3_600 => "h",
            _ => "d",
        };
        write!(f, "{}/{unit}", self.n)
    }
}

/// Default anonymous snapshot rate (12 §11: 5 per hour per IP).
pub const DEFAULT_SNAPSHOT_RATE: Rate = Rate {
    n: 5,
    window: Duration::from_secs(3_600),
};

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// `LANKA_APP_PLANE=on|off` (default `off`). Off: `/app/v1` is 404 and
    /// the server opens no app-plane pool; `/v2` is unaffected.
    pub enabled: bool,
    /// `LANKA_APP_DATABASE_URL`: pool C, role `lanka_app`. Unset: the
    /// server's own connection options with `SET ROLE lanka_app` on every
    /// connection (local development, where the login role is the admin).
    pub database_url: Option<String>,
    /// `LANKA_SNAPSHOT_RATE` (default `5/h`) per rate identity.
    pub snapshot_rate: Rate,
    /// `LANKA_IP_HASH_SALT`: secret behind the client-address hashes
    /// (rotated monthly by mixing in the month). Unset: random per process.
    pub ip_salt: Option<String>,
    /// `LANKA_PUBLIC_URL`: origin used in links (snapshot URLs, packages,
    /// oEmbed). Unset: the country profile's `site.base_url`.
    pub public_url: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            database_url: None,
            snapshot_rate: DEFAULT_SNAPSHOT_RATE,
            ip_salt: None,
            public_url: None,
        }
    }
}

/// `on|off` (also `true|false`, `1|0`, `yes|no`).
pub fn parse_switch(name: &str, v: &str) -> anyhow::Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" | "" => Ok(false),
        other => bail!("{name}={other:?}: expected on or off"),
    }
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let nonempty = |k: &str| get(k).filter(|v| !v.trim().is_empty());
        Ok(Self {
            enabled: match nonempty("LANKA_APP_PLANE") {
                Some(v) => parse_switch("LANKA_APP_PLANE", &v)?,
                None => false,
            },
            database_url: nonempty("LANKA_APP_DATABASE_URL"),
            snapshot_rate: match nonempty("LANKA_SNAPSHOT_RATE") {
                Some(v) => Rate::parse(&v)?,
                None => DEFAULT_SNAPSHOT_RATE,
            },
            ip_salt: nonempty("LANKA_IP_HASH_SALT"),
            public_url: nonempty("LANKA_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_parse() {
        assert_eq!(Rate::parse("5/h").ok(), Some(DEFAULT_SNAPSHOT_RATE));
        assert_eq!(
            Rate::parse("30/min").map(|r| r.window).ok(),
            Some(Duration::from_secs(60))
        );
        assert!(Rate::parse("0/h").is_err());
        assert!(Rate::parse("5/week").is_err());
        assert!(Rate::parse("five/h").is_err());
        assert_eq!(DEFAULT_SNAPSHOT_RATE.to_string(), "5/h");
    }

    #[test]
    fn switch_defaults_off() {
        let cfg = AppConfig::from_lookup(|_| None).expect("config");
        assert!(!cfg.enabled);
        let on = AppConfig::from_lookup(|k| (k == "LANKA_APP_PLANE").then(|| "on".to_owned()))
            .expect("config");
        assert!(on.enabled);
        assert!(
            AppConfig::from_lookup(|k| (k == "LANKA_APP_PLANE").then(|| "maybe".to_owned()))
                .is_err()
        );
    }
}
