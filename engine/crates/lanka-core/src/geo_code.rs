//! Geography codes (docs/architecture-v2/02-standard-model.md §1, §2).
//!
//! - Admin units: bare p-codes, `<root><digits>`, where the root code and the
//!   digit count per level come from the country profile. p-codes are the
//!   only admin join key.
//! - Every other coded geography: `<scheme>:<code>` (`ed:01`, `moh:LK11-014`).
//!   The colon distinguishes them from p-codes.
//!
//! Syntax only: existence is a database question (`geo.units`).

use std::fmt;

use serde::Serialize;

use crate::profile::{AdminSpec, ProfileError};

/// A syntactically valid admin p-code under some profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(into = "String")]
pub struct Pcode {
    code: String,
    level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid p-code {code:?}: expected {expected}")]
pub struct PcodeError {
    pub code: String,
    pub expected: String,
}

/// Admin code rules derived from the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminCodes {
    root: String,
    /// `digits[level]` = digits after the root at that level (strictly increasing).
    digits: Vec<u8>,
}

impl AdminCodes {
    pub fn from_spec(a: &AdminSpec) -> Result<Self, ProfileError> {
        let root = a.root_code.trim().to_ascii_uppercase();
        if root.is_empty() || !root.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(ProfileError::Invalid(format!(
                "admin.root_code {:?} must be ASCII letters",
                a.root_code
            )));
        }
        let mut levels = a.levels.clone();
        levels.sort_by_key(|l| l.level);
        let mut digits = Vec::with_capacity(levels.len());
        for (i, l) in levels.iter().enumerate() {
            if usize::from(l.level) != i {
                return Err(ProfileError::Invalid(format!(
                    "admin levels must be 0..n without gaps (found level {})",
                    l.level
                )));
            }
            if i == 0 && l.digits != 0 {
                return Err(ProfileError::Invalid("admin level 0 has 0 digits".into()));
            }
            if i > 0 && l.digits <= digits[i - 1] {
                return Err(ProfileError::Invalid(
                    "admin level digit counts must increase".into(),
                ));
            }
            digits.push(l.digits);
        }
        if digits.is_empty() {
            return Err(ProfileError::Invalid("admin.levels is empty".into()));
        }
        let expected_pattern = format!("^{root}[0-9]*$");
        if a.pcode_pattern != expected_pattern {
            return Err(ProfileError::Invalid(format!(
                "admin.pcode_pattern {:?} disagrees with root_code (expected {expected_pattern:?})",
                a.pcode_pattern
            )));
        }
        Ok(Self { root, digits })
    }

    /// The national (level 0) code.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Deepest admin level.
    #[must_use]
    pub fn max_level(&self) -> u8 {
        u8::try_from(self.digits.len() - 1).unwrap_or(0)
    }

    fn expected(&self) -> String {
        self.digits
            .iter()
            .map(|d| format!("{}{}", self.root, "d".repeat(usize::from(*d))))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn level_for(&self, digits: usize) -> Option<u8> {
        self.digits
            .iter()
            .position(|d| usize::from(*d) == digits)
            .and_then(|l| u8::try_from(l).ok())
    }

    /// Parses a p-code, accepting lowercase input (`lk11` → `LK11`).
    pub fn parse(&self, s: &str) -> Result<Pcode, PcodeError> {
        let t = s.trim();
        let err = || PcodeError {
            code: s.to_owned(),
            expected: self.expected(),
        };
        let r = self.root.len();
        if t.len() < r || !t.is_char_boundary(r) || !t[..r].eq_ignore_ascii_case(&self.root) {
            return Err(err());
        }
        let rest = &t[r..];
        if !rest.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err());
        }
        let level = self.level_for(rest.len()).ok_or_else(err)?;
        Ok(Pcode {
            code: format!("{}{rest}", self.root),
            level,
        })
    }

    /// Parses a wire geography code: a p-code, or `scheme:code`.
    pub fn parse_geo(&self, s: &str) -> Result<GeoCode, PcodeError> {
        let t = s.trim();
        if let Some((scheme, code)) = t.split_once(':') {
            let ok_scheme = !scheme.is_empty()
                && scheme
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            let ok_code = !code.is_empty()
                && code
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
            if ok_scheme && ok_code {
                return Ok(GeoCode::Scheme {
                    scheme: scheme.to_owned(),
                    code: code.to_owned(),
                });
            }
            return Err(PcodeError {
                code: s.to_owned(),
                expected: "<scheme>:<code> or a p-code".into(),
            });
        }
        self.parse(t).map(GeoCode::Admin)
    }

    /// The enclosing p-code by code structure; `None` at level 0. Only a
    /// hint: the authoritative parent is `geo.units.parent_key`.
    #[must_use]
    pub fn structural_parent(&self, p: &Pcode) -> Option<Pcode> {
        let lvl = usize::from(p.level.checked_sub(1)?);
        let keep = self.root.len() + usize::from(self.digits[lvl]);
        Some(Pcode {
            code: p.code[..keep].to_owned(),
            level: p.level - 1,
        })
    }
}

impl Pcode {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.code
    }

    /// Admin level implied by the code length.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }
}

impl fmt::Display for Pcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code)
    }
}

impl From<Pcode> for String {
    fn from(p: Pcode) -> Self {
        p.code
    }
}

/// Any coded geography as it appears on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GeoCode {
    Admin(Pcode),
    Scheme { scheme: String, code: String },
}

impl GeoCode {
    /// The code as stored in `geo.units.code`.
    #[must_use]
    pub fn to_code(&self) -> String {
        match self {
            Self::Admin(p) => p.code.clone(),
            Self::Scheme { scheme, code } => format!("{scheme}:{code}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::tests::repo_profile;

    #[test]
    fn parses_all_levels() {
        let a = repo_profile().admin;
        let root = a.root().to_owned();
        let cases = [
            (root.clone(), 0),
            (format!("{root}1"), 1),
            (format!("{root}11"), 2),
            (format!("{root}1103"), 3),
            (format!("{root}1103005"), 4),
        ];
        for (s, lvl) in &cases {
            assert_eq!(a.parse(s).map(|p| p.level()), Ok(*lvl), "{s}");
        }
        let lower = format!(" {}11 ", root.to_ascii_lowercase());
        assert_eq!(
            a.parse(&lower).map(|p| p.to_string()),
            Ok(format!("{root}11"))
        );
    }

    #[test]
    fn rejects_garbage() {
        let a = repo_profile().admin;
        let root = a.root().to_owned();
        for s in [
            String::new(),
            root[..1].to_owned(),
            format!("{root}A"),
            format!("{root}12345"),
            "XX11".to_owned(),
            format!("{root}11a"),
            format!("{root}110"),
        ] {
            assert!(a.parse(&s).is_err(), "{s}");
        }
    }

    #[test]
    fn structural_parent_chain() {
        let a = repo_profile().admin;
        let p = a.parse(&format!("{}1103005", a.root())).expect("valid");
        let chain: Vec<String> = std::iter::successors(Some(p), |p| a.structural_parent(p))
            .map(String::from)
            .collect();
        let r = a.root();
        assert_eq!(
            chain,
            [
                format!("{r}1103005"),
                format!("{r}1103"),
                format!("{r}11"),
                format!("{r}1"),
                r.to_owned()
            ]
        );
    }

    #[test]
    fn scheme_codes() {
        let a = repo_profile().admin;
        assert_eq!(
            a.parse_geo("ed:01").map(|g| g.to_code()),
            Ok("ed:01".into())
        );
        assert!(a.parse_geo("ED:01").is_err());
        assert!(a.parse_geo("ed:").is_err());
        assert!(matches!(a.parse_geo(a.root()), Ok(GeoCode::Admin(_))));
    }

    #[test]
    fn profile_rules_are_checked() {
        let mut spec = repo_profile().spec.admin;
        spec.pcode_pattern = "^XX[0-9]*$".into();
        assert!(AdminCodes::from_spec(&spec).is_err());
    }
}
