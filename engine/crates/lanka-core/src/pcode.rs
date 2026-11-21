//! OCHA COD-AB p-codes for Sri Lanka. p-codes are the only admin join key.
//!
//! Shape: `LK` (ADM0), `LK1` (ADM1 province), `LK11` (ADM2 district),
//! `LK1103` (ADM3 DS division), `LK1103005` (ADM4 GN division).

use std::fmt;

use serde::{Deserialize, Serialize};

/// A syntactically valid Sri Lanka p-code. Existence is a database question.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Pcode(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid p-code {0:?}: expected LK, LK<d>, LK<dd>, LK<dddd> or LK<ddddddd>")]
pub struct PcodeError(pub String);

impl Pcode {
    /// Parses a p-code, accepting lowercase input (`lk11` → `LK11`).
    pub fn parse(s: &str) -> Result<Self, PcodeError> {
        let t = s.trim();
        let b = t.as_bytes();
        let ok = b.len() >= 2
            && b[0].eq_ignore_ascii_case(&b'L')
            && b[1].eq_ignore_ascii_case(&b'K')
            && b[2..].iter().all(u8::is_ascii_digit)
            && level_for_len(b.len()).is_some();
        if ok {
            Ok(Self(t.to_ascii_uppercase()))
        } else {
            Err(PcodeError(s.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Admin level implied by the code length (0–4).
    #[must_use]
    pub fn level(&self) -> u8 {
        level_for_len(self.0.len()).unwrap_or(0)
    }

    /// The p-code of the enclosing unit by code structure, `None` for `LK`.
    /// Only a hint: the authoritative parent is `geo.admin_units.parent_pcode`.
    #[must_use]
    pub fn structural_parent(&self) -> Option<Self> {
        let cut = match self.0.len() {
            3 => 2,
            4 => 3,
            6 => 4,
            9 => 6,
            _ => return None,
        };
        Some(Self(self.0[..cut].to_owned()))
    }
}

const fn level_for_len(len: usize) -> Option<u8> {
    match len {
        2 => Some(0),
        3 => Some(1),
        4 => Some(2),
        6 => Some(3),
        9 => Some(4),
        _ => None,
    }
}

impl fmt::Display for Pcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Pcode {
    type Error = PcodeError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<Pcode> for String {
    fn from(p: Pcode) -> Self {
        p.0
    }
}

impl std::str::FromStr for Pcode {
    type Err = PcodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_levels() {
        for (s, lvl) in [
            ("LK", 0),
            ("LK1", 1),
            ("LK11", 2),
            ("LK1103", 3),
            ("LK1103005", 4),
        ] {
            assert_eq!(Pcode::parse(s).map(|p| p.level()), Ok(lvl), "{s}");
        }
        assert_eq!(
            Pcode::parse(" lk11 ").map(|p| p.to_string()),
            Ok("LK11".into())
        );
    }

    #[test]
    fn rejects_garbage() {
        for s in ["", "L", "LKA", "LK12345", "XX11", "LK11a", "LK110"] {
            assert!(Pcode::parse(s).is_err(), "{s}");
        }
    }

    #[test]
    fn structural_parent_chain() {
        let p = Pcode::parse("LK1103005").expect("valid");
        let chain: Vec<String> = std::iter::successors(Some(p), Pcode::structural_parent)
            .map(String::from)
            .collect();
        assert_eq!(chain, ["LK1103005", "LK1103", "LK11", "LK1", "LK"]);
    }
}
