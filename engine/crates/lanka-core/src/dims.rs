//! Dimension combinations (02-standard-model.md §3).
//!
//! An observation's dimensions are codelist-backed `(dimension, code)`
//! pairs. The canonical text form — keys sorted, `k=v` joined by `;`, empty
//! for none — is what hashes, exports and `catalog.dim_sets.dims` use; in
//! the hot table a combination is the interned `int4` key of that string
//! (key 0 is reserved for the empty combination).

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

/// Surrogate key of the empty dimension combination.
pub const EMPTY_KEY: i32 = 0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DimsError {
    #[error("bad dimension pair {0:?}: expected key=code")]
    Pair(String),
    #[error("bad dimension id {0:?}: expected [a-z][a-z0-9_]*")]
    Key(String),
    #[error("bad code {0:?} for dimension {1:?}")]
    Code(String, String),
    #[error("dimension {0:?} given twice")]
    Duplicate(String),
}

/// A canonical set of dimension pairs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Dims(BTreeMap<String, String>);

fn valid_key(k: &str) -> bool {
    let mut b = k.bytes();
    b.next().is_some_and(|c| c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn valid_code(c: &str) -> bool {
    !c.is_empty()
        && c.len() <= 64
        && c.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+' | b'<' | b'>')
        })
}

impl Dims {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// From pairs in any order.
    pub fn from_pairs<K: AsRef<str>, V: AsRef<str>>(
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Result<Self, DimsError> {
        let mut m = BTreeMap::new();
        for (k, v) in pairs {
            let (k, v) = (k.as_ref(), v.as_ref());
            if !valid_key(k) {
                return Err(DimsError::Key(k.to_owned()));
            }
            if !valid_code(v) {
                return Err(DimsError::Code(v.to_owned(), k.to_owned()));
            }
            if m.insert(k.to_owned(), v.to_owned()).is_some() {
                return Err(DimsError::Duplicate(k.to_owned()));
            }
        }
        Ok(Self(m))
    }

    /// Parses the canonical form (`age=0-14;sex=f`) or the v2 query form
    /// (`sex:F,age:Y15T19`), in any order.
    pub fn parse(s: &str) -> Result<Self, DimsError> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Self::new());
        }
        let sep = if s.contains(';') || !s.contains(',') {
            ';'
        } else {
            ','
        };
        let mut pairs = Vec::new();
        for part in s.split(sep).map(str::trim).filter(|p| !p.is_empty()) {
            let (k, v) = part
                .split_once('=')
                .or_else(|| part.split_once(':'))
                .ok_or_else(|| DimsError::Pair(part.to_owned()))?;
            pairs.push((k.trim().to_owned(), v.trim().to_owned()));
        }
        Self::from_pairs(pairs)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Canonical text: keys sorted, `k=v` joined by `;`.
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                s.push(';');
            }
            s.push_str(k);
            s.push('=');
            s.push_str(v);
        }
        s
    }

    /// `{"sex": "f"}` — the `catalog.dim_sets.pairs` column and API form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.0
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        )
    }
}

impl fmt::Display for Dims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

/// Canonical dims key from pairs (keys sorted, `k=v` joined by `;`).
/// Panics never; invalid input is kept verbatim-sorted so validators can
/// report it with context.
#[must_use]
pub fn dims_key(pairs: &[(&str, &str)]) -> String {
    let mut p: Vec<(&str, &str)> = pairs.to_vec();
    p.sort_unstable();
    p.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_forms() {
        assert_eq!(Dims::parse("").map(|d| d.canonical()), Ok(String::new()));
        assert_eq!(
            Dims::parse("sex=f;age=0-14").map(|d| d.canonical()),
            Ok("age=0-14;sex=f".into())
        );
        assert_eq!(
            Dims::parse("sex:F,age:Y15T19").map(|d| d.canonical()),
            Ok("age=Y15T19;sex=F".into())
        );
        assert_eq!(
            Dims::parse("series_code=SP.POP.TOTL").map(|d| d.canonical()),
            Ok("series_code=SP.POP.TOTL".into())
        );
        assert_eq!(dims_key(&[("sex", "f"), ("age", "0-14")]), "age=0-14;sex=f");
        assert!(Dims::parse("sex").is_err());
        assert!(Dims::parse("Sex=f").is_err());
        assert!(Dims::parse("sex=f;sex=m").is_err());
        assert!(Dims::parse("sex=a b").is_err());
    }

    /// Property: canonicalisation is idempotent and order-independent, and
    /// `dims_key` agrees with `Dims::canonical`.
    #[test]
    fn canonicalisation_property() {
        let keys = [
            "sex",
            "age",
            "age_group",
            "ethnicity",
            "religion",
            "series_code",
            "a1",
        ];
        let codes = [
            "f",
            "m",
            "_T",
            "0-14",
            "65+",
            "SP.POP.TOTL",
            "Y15T19",
            "sinhala",
        ];
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            usize::try_from(x % 1_000_003).unwrap_or(0)
        };
        for _ in 0..5_000 {
            let n = next() % (keys.len() + 1);
            let mut pairs: Vec<(&str, &str)> = Vec::new();
            for k in keys.iter().take(n) {
                pairs.push((k, codes[next() % codes.len()]));
            }
            let d = Dims::from_pairs(pairs.iter().copied()).expect("valid");
            let mut shuffled = pairs.clone();
            for i in (1..shuffled.len()).rev() {
                shuffled.swap(i, next() % (i + 1));
            }
            let d2 = Dims::from_pairs(shuffled.iter().copied()).expect("valid");
            assert_eq!(d, d2);
            let c = d.canonical();
            assert_eq!(Dims::parse(&c).expect("reparse"), d);
            assert_eq!(Dims::parse(&c).expect("reparse").canonical(), c);
            assert_eq!(dims_key(&shuffled), c);
            let wire: Vec<String> = shuffled.iter().map(|(k, v)| format!("{k}:{v}")).collect();
            if !wire.is_empty() {
                assert_eq!(Dims::parse(&wire.join(",")).expect("wire").canonical(), c);
            }
        }
    }
}
