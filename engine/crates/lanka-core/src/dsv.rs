//! Dataset-version identity (05-storage-and-performance.md §3).
//!
//! ```text
//! dsv           = "dsv_" ‖ hex(sha256(schema_hash ‖ multiset_hash ‖ row_count_le64))[0:16]
//! multiset_hash = Σ sha256(canonical_row)  (mod 2^256, little-endian)
//! canonical_row = indicator ␟ geo ␟ period ␟ dims ␟ ryu(value) ␟ ryu(lo) ␟ ryu(hi)
//!                 ␟ basis ␟ status ␟ flags ␟ verbatim ␟ canonical_json(locator)
//! ```
//!
//! The sum is order-independent, incremental (add new rows, subtract
//! removed ones) and path-independent, and uses codes, never surrogate
//! keys, so a re-imported database yields identical dsvs. It detects change
//! in our own pipeline output; it is not collision-resistant against an
//! adversary choosing rows (release manifests carry per-file sha256 for that).

use sha2::{Digest, Sha256};

use crate::basis::{Basis, Status};
use crate::flags::Flags;

/// 0x1f, the ASCII unit separator.
const SEP: u8 = 0x1f;

/// One observation in canonical form (codes, not keys).
#[derive(Debug, Clone, Copy)]
pub struct CanonicalRow<'a> {
    pub indicator_id: &'a str,
    pub geo_code: &'a str,
    pub period: &'a str,
    pub dims: &'a str,
    pub value: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub basis: Basis,
    pub status: Status,
    pub flags: Flags,
    pub verbatim: Option<&'a str>,
    pub locator: Option<&'a serde_json::Value>,
}

/// Shortest round-trip float text (ryu); empty for NULL. `-0.0` → `0.0` so
/// a sign-only difference never changes identity.
#[must_use]
pub fn fmt_float(v: Option<f64>) -> String {
    match v {
        None => String::new(),
        Some(0.0) => "0.0".to_owned(),
        Some(x) if x.is_finite() => ryu::Buffer::new().format_finite(x).to_owned(),
        Some(x) => format!("{x}"),
    }
}

/// JSON with object keys sorted recursively and no insignificant whitespace.
#[must_use]
pub fn canonical_json(v: &serde_json::Value) -> String {
    fn write(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String(k.clone()).to_string());
                    out.push(':');
                    write(&m[k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    write(v, &mut s);
    s
}

impl CanonicalRow<'_> {
    /// The canonical byte string.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(160);
        for (i, part) in [
            self.indicator_id,
            self.geo_code,
            self.period,
            self.dims,
            &fmt_float(self.value),
            &fmt_float(self.lo),
            &fmt_float(self.hi),
            self.basis.as_str(),
            self.status.as_str(),
            &self.flags.bits().to_string(),
            self.verbatim.unwrap_or_default(),
            &self.locator.map(canonical_json).unwrap_or_default(),
        ]
        .iter()
        .enumerate()
        {
            if i > 0 {
                b.push(SEP);
            }
            b.extend_from_slice(part.as_bytes());
        }
        b
    }

    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.bytes()).into()
    }
}

/// A 256-bit little-endian accumulator: Σ row hashes mod 2^256.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MultisetHash(pub [u8; 32]);

impl MultisetHash {
    #[must_use]
    pub const fn empty() -> Self {
        Self([0; 32])
    }

    pub fn from_slice(b: &[u8]) -> Option<Self> {
        <[u8; 32]>::try_from(b).ok().map(Self)
    }

    pub fn add(&mut self, h: &[u8; 32]) {
        let mut carry = 0u16;
        for (a, b) in self.0.iter_mut().zip(h) {
            let s = u16::from(*a) + u16::from(*b) + carry;
            *a = (s & 0xff) as u8;
            carry = s >> 8;
        }
    }

    pub fn remove(&mut self, h: &[u8; 32]) {
        let mut borrow = 0i16;
        for (a, b) in self.0.iter_mut().zip(h) {
            let mut d = i16::from(*a) - i16::from(*b) - borrow;
            borrow = i16::from(d < 0);
            if d < 0 {
                d += 256;
            }
            *a = u8::try_from(d).unwrap_or(0);
        }
    }

    /// Accumulates canonical byte strings.
    pub fn of_bytes<I, B>(rows: I) -> Self
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let mut m = Self::empty();
        for r in rows {
            m.add(&Sha256::digest(r.as_ref()).into());
        }
        m
    }
}

/// sha256 over sorted descriptor lines (indicators + dims + units, or a
/// boundary-set descriptor): changes when the meaning of the rows changes.
#[must_use]
pub fn schema_hash(mut lines: Vec<String>) -> [u8; 32] {
    lines.sort();
    let mut h = Sha256::new();
    for l in &lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    h.finalize().into()
}

/// The public version id.
#[must_use]
pub fn dsv(schema: &[u8; 32], multiset: &MultisetHash, row_count: u64) -> String {
    let mut h = Sha256::new();
    h.update(schema);
    h.update(multiset.0);
    h.update(row_count.to_le_bytes());
    let d: [u8; 32] = h.finalize().into();
    format!("dsv_{}", &hex::encode(d)[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(geo: &str, v: f64) -> CanonicalRow<'_> {
        CanonicalRow {
            indicator_id: "x.count",
            geo_code: geo,
            period: "2024",
            dims: "sex=f",
            value: Some(v),
            lo: None,
            hi: None,
            basis: Basis::Reported,
            status: Status::Normal,
            flags: Flags::EMPTY,
            verbatim: Some("1"),
            locator: None,
        }
    }

    #[test]
    fn floats_are_shortest_round_trip() {
        assert_eq!(fmt_float(Some(21_756_000.0)), "21756000.0");
        assert_eq!(fmt_float(Some(0.1)), "0.1");
        assert_eq!(fmt_float(Some(-0.0)), "0.0");
        assert_eq!(fmt_float(Some(2.5e-7)), "2.5e-7");
        assert_eq!(fmt_float(None), "");
    }

    #[test]
    fn canonical_json_sorts_keys() {
        let v = serde_json::json!({"b": 1, "a": {"d": [1, {"z": 0, "y": "é"}], "c": null}});
        assert_eq!(
            canonical_json(&v),
            r#"{"a":{"c":null,"d":[1,{"y":"é","z":0}]},"b":1}"#
        );
    }

    /// Property: order independence, incremental add/remove equals a fresh
    /// sum, and a value change changes the hash.
    #[test]
    fn multiset_properties() {
        let geos: Vec<String> = (0..500).map(|i| format!("G{i}")).collect();
        let rows: Vec<[u8; 32]> = geos
            .iter()
            .enumerate()
            .map(|(i, g)| row(g, f64::from(u32::try_from(i).unwrap_or(0))).hash())
            .collect();
        let mut fwd = MultisetHash::empty();
        rows.iter().for_each(|h| fwd.add(h));
        let mut rev = MultisetHash::empty();
        rows.iter().rev().for_each(|h| rev.add(h));
        assert_eq!(fwd, rev);

        // Remove the first 100, append 50 new: equals the direct sum.
        let extra: Vec<[u8; 32]> = (0..50).map(|i| row("NEW", f64::from(i)).hash()).collect();
        let mut inc = fwd;
        rows[..100].iter().for_each(|h| inc.remove(h));
        extra.iter().for_each(|h| inc.add(h));
        let mut direct = MultisetHash::empty();
        rows[100..].iter().chain(&extra).for_each(|h| direct.add(h));
        assert_eq!(inc, direct);

        // add then remove is identity, including across carries.
        let mut m = MultisetHash([0xff; 32]);
        let one = {
            let mut o = [0u8; 32];
            o[0] = 1;
            o
        };
        m.add(&one);
        assert_eq!(m, MultisetHash::empty(), "wraps mod 2^256");
        m.remove(&one);
        assert_eq!(m, MultisetHash([0xff; 32]));

        assert_ne!(row("G1", 1.0).hash(), row("G1", 1.5).hash());
        let s = schema_hash(vec!["b".into(), "a".into()]);
        assert_eq!(s, schema_hash(vec!["a".into(), "b".into()]));
        let d = dsv(&s, &fwd, 500);
        assert!(d.starts_with("dsv_") && d.len() == 20);
        assert_ne!(d, dsv(&s, &fwd, 501));
    }
}
