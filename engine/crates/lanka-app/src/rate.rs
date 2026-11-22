//! Rate identity and token buckets (12 §2, §6).
//!
//! A *rate identity* is what quotas are keyed by: a verified API key, or a
//! salted hash of the client address. It is never a data-access principal
//! and never appears in a `/v2` response. Raw addresses are not stored:
//! the hash mixes a server secret with the month, so the same address
//! hashes differently every month (12 §3: salted, rotated monthly).
//!
//! Client address: the server binds loopback/private addresses behind the
//! host nginx, which appends the real client to `X-Forwarded-For`. The
//! right-most entry is taken when the peer is a loopback or private
//! address (the proxy); a direct peer from anywhere else is used as is.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};

use crate::config::Rate;

/// What quotas are keyed by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateIdentity {
    /// A verified API key, by its public prefix (`ldl_live_8f3k2a7q`).
    /// Keys are verified against `app.api_keys` from WP-31; until then no
    /// request carries this identity (an unverified prefix would let a
    /// client mint unlimited identities).
    Key(String),
    /// sha256(secret ‖ month ‖ address).
    Ip([u8; 32]),
}

impl RateIdentity {
    #[must_use]
    pub fn ip_hash(&self) -> Option<&[u8; 32]> {
        match self {
            Self::Ip(h) => Some(h),
            Self::Key(_) => None,
        }
    }
}

impl std::fmt::Display for RateIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key(p) => write!(f, "key:{p}"),
            Self::Ip(h) => write!(f, "ip:{}", &hex::encode(h)[..16]),
        }
    }
}

/// Salted, monthly rotated hashing of client addresses.
pub struct IpHasher {
    secret: [u8; 32],
}

impl IpHasher {
    #[must_use]
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: Sha256::digest(secret).into(),
        }
    }

    /// A per-process secret (hashes do not survive a restart; set
    /// `LANKA_IP_HASH_SALT` in production).
    #[must_use]
    pub fn random() -> Self {
        let mut seed = Vec::with_capacity(64);
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let mut b = [0u8; 32];
            if f.read_exact(&mut b).is_ok() {
                seed.extend_from_slice(&b);
            }
        }
        // Fallback entropy where /dev/urandom is unavailable.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        seed.extend_from_slice(&t.to_le_bytes());
        seed.extend_from_slice(&std::process::id().to_le_bytes());
        Self::new(&seed)
    }

    /// The hash of `ip` in `month` (`YYYY-MM`).
    #[must_use]
    pub fn hash_in(&self, ip: IpAddr, month: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.secret);
        h.update(month.as_bytes());
        match ip.to_canonical() {
            IpAddr::V4(a) => h.update(a.octets()),
            IpAddr::V6(a) => h.update(a.octets()),
        }
        h.finalize().into()
    }

    /// The hash of `ip` this month (UTC).
    #[must_use]
    pub fn hash(&self, ip: IpAddr) -> [u8; 32] {
        self.hash_in(ip, &chrono::Utc::now().format("%Y-%m").to_string())
    }
}

fn is_proxy(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(a) => a.is_loopback() || a.is_private() || a.is_link_local(),
        IpAddr::V6(a) => a.is_loopback() || a.is_unique_local(),
    }
}

/// The client address of a request. `peer` is the TCP peer (`None` when the
/// router runs without connect info, i.e. in-process tests: headers count).
#[must_use]
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
    let behind_proxy = peer.is_none_or(|p| is_proxy(p.ip()));
    if behind_proxy {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|s| s.trim().parse::<IpAddr>().ok())
            .next_back();
        if forwarded.is_some() {
            return forwarded;
        }
        if let Some(ip) = headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
    }
    peer.map(|p| p.ip())
}

/// The rate identity of a request. Stage 0 (anonymous only): always the
/// address hash; verified keys switch to [`RateIdentity::Key`] in WP-31.
#[must_use]
pub fn identify(headers: &HeaderMap, peer: Option<SocketAddr>, hasher: &IpHasher) -> RateIdentity {
    let ip = client_ip(headers, peer).unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    RateIdentity::Ip(hasher.hash(ip))
}

/// The public prefix of a well-formed API key in `Authorization: Bearer`
/// (`ldl_live_<8 chars>_<43 chars base64url>`, 12 §6), or `None`. Shape only:
/// the secret is verified against `app.api_keys` (WP-31).
#[must_use]
pub fn key_prefix(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = v.strip_prefix("Bearer ")?.trim();
    let rest = token.strip_prefix("ldl_live_")?;
    let (id, secret) = rest.split_once('_')?;
    let id_ok = id.len() == 8 && id.bytes().all(|b| b.is_ascii_alphanumeric());
    let secret_ok = secret.len() == 43
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    (id_ok && secret_ok).then(|| format!("ldl_live_{id}"))
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: Instant,
}

/// In-memory token buckets, one per identity: `rate.n` tokens, refilled
/// continuously over `rate.window`.
pub struct Limiter {
    capacity: f64,
    per_sec: f64,
    buckets: Mutex<HashMap<RateIdentity, Bucket>>,
}

/// Buckets kept before full ones are pruned.
const PRUNE_AT: usize = 100_000;

impl Limiter {
    #[must_use]
    pub fn new(rate: Rate) -> Self {
        let capacity = f64::from(rate.n);
        Self {
            capacity,
            per_sec: capacity / rate.window.as_secs_f64().max(1.0),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Takes one token for `id`; `Err(retry_after)` when the bucket is empty.
    pub fn check(&self, id: &RateIdentity) -> Result<(), Duration> {
        self.check_at(id, Instant::now())
    }

    pub fn check_at(&self, id: &RateIdentity, now: Instant) -> Result<(), Duration> {
        let mut map = match self.buckets.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if map.len() >= PRUNE_AT {
            let (cap, rate) = (self.capacity, self.per_sec);
            map.retain(|_, b| {
                b.tokens + now.saturating_duration_since(b.at).as_secs_f64() * rate < cap
            });
        }
        let b = map.entry(id.clone()).or_insert(Bucket {
            tokens: self.capacity,
            at: now,
        });
        let refill = now.saturating_duration_since(b.at).as_secs_f64() * self.per_sec;
        b.tokens = (b.tokens + refill).min(self.capacity);
        b.at = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - b.tokens) / self.per_sec;
            Err(Duration::from_secs_f64(wait.max(1.0)).max(Duration::from_secs(1)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn buckets_refill_over_the_window() {
        let l = Limiter::new(Rate::parse("5/h").expect("rate"));
        let a = RateIdentity::Ip([1; 32]);
        let b = RateIdentity::Ip([2; 32]);
        let t0 = Instant::now();
        for _ in 0..5 {
            assert!(l.check_at(&a, t0).is_ok());
        }
        let wait = l.check_at(&a, t0).expect_err("sixth is limited");
        assert!((719..=721).contains(&wait.as_secs()), "{wait:?}");
        assert!(l.check_at(&b, t0).is_ok(), "other identities unaffected");
        assert!(l.check_at(&a, t0 + Duration::from_secs(700)).is_err());
        assert!(l.check_at(&a, t0 + Duration::from_secs(721)).is_ok());
    }

    #[test]
    fn hashes_rotate_monthly_and_hide_the_address() {
        let h = IpHasher::new(b"secret");
        let ip: IpAddr = "203.0.113.7".parse().expect("ip");
        assert_eq!(h.hash_in(ip, "2026-11"), h.hash_in(ip, "2026-11"));
        assert_ne!(h.hash_in(ip, "2026-11"), h.hash_in(ip, "2026-12"));
        assert_ne!(
            h.hash_in(ip, "2026-11"),
            IpHasher::new(b"other").hash_in(ip, "2026-11")
        );
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().expect("ip");
        assert_eq!(h.hash_in(ip, "2026-11"), h.hash_in(mapped, "2026-11"));
        assert!(
            RateIdentity::Ip(h.hash_in(ip, "2026-11"))
                .to_string()
                .starts_with("ip:")
        );
    }

    #[test]
    fn forwarded_for_only_from_the_proxy() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("10.9.9.9, 198.51.100.4"),
        );
        let proxy: SocketAddr = "127.0.0.1:50000".parse().expect("addr");
        let direct: SocketAddr = "192.0.2.1:50000".parse().expect("addr");
        assert_eq!(
            client_ip(&h, Some(proxy)),
            "198.51.100.4".parse::<IpAddr>().ok()
        );
        assert_eq!(client_ip(&h, Some(direct)), Some(direct.ip()));
        assert_eq!(client_ip(&HeaderMap::new(), Some(proxy)), Some(proxy.ip()));
    }

    #[test]
    fn key_prefixes_are_shape_checked() {
        let mut h = HeaderMap::new();
        let secret = "A".repeat(43);
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer ldl_live_8f3k2a7q_{secret}")).expect("value"),
        );
        assert_eq!(key_prefix(&h).as_deref(), Some("ldl_live_8f3k2a7q"));
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ldl_live_short_x"),
        );
        assert_eq!(key_prefix(&h), None);
    }
}
