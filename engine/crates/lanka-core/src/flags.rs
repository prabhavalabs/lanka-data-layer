//! Quality and method flags: a 32-bit mask whose bit numbers are part of the
//! standard (02-standard-model.md §4). Bits 18–27 and 30–31 are reserved; bits 28–29
//! hold an optional quality class `A|B|C|D` for estimated indicators that
//! publish a class instead of an interval.

use std::fmt;

use serde::{Serialize, Serializer};

/// `(bit, name)` — the flag registry. Never renumber.
pub const REGISTRY: &[(u8, &str)] = &[
    (0, "REBASED"),
    (1, "SPLIT_UNIT"),
    (2, "DENOM_MISMATCH"),
    (3, "SMALL_N"),
    (4, "MODEL_SPREAD"),
    (5, "INTERPOLATED_TIME"),
    (6, "EXTRAPOLATED_TIME"),
    (7, "ANCILLARY_WEAK"),
    (8, "SOURCE_AMBIGUOUS"),
    (9, "SOURCE_INCONSISTENT"),
    (10, "OCR"),
    (11, "LLM_READ"),
    (12, "PARTIAL_COVERAGE"),
    (13, "TOP_CODED"),
    (14, "UNOFFICIAL_GEOM"),
    (15, "SUPERSEDED_BOUNDARY"),
    (16, "INTERPOLATED_SPACE"),
    (17, "DISAGGREGATED"),
];

/// Flags that propagate from inputs to derived rows. All registered flags
/// currently propagate.
pub const PROPAGATING: u32 = 0x3FFFF;

const QUALITY_SHIFT: u32 = 28;
const QUALITY_MASK: u32 = 0b11 << QUALITY_SHIFT;
/// Bits that are neither registered nor the quality class.
const RESERVED_MASK: u32 = !(0x3FFFF | QUALITY_MASK);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlagError {
    #[error("unknown flag {0:?}")]
    Unknown(String),
    #[error("reserved flag bits set: {0:#010x}")]
    Reserved(u32),
    #[error("quality class must be A, B, C or D, not {0:?}")]
    Quality(String),
}

/// Flag bit set, stored as `int4` (bit pattern of the `u32`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Flags(u32);

impl Flags {
    pub const EMPTY: Self = Self(0);
    pub const REBASED: Self = Self(1 << 0);
    pub const SPLIT_UNIT: Self = Self(1 << 1);
    pub const DENOM_MISMATCH: Self = Self(1 << 2);
    pub const SMALL_N: Self = Self(1 << 3);
    pub const MODEL_SPREAD: Self = Self(1 << 4);
    pub const INTERPOLATED_TIME: Self = Self(1 << 5);
    pub const EXTRAPOLATED_TIME: Self = Self(1 << 6);
    pub const ANCILLARY_WEAK: Self = Self(1 << 7);
    pub const SOURCE_AMBIGUOUS: Self = Self(1 << 8);
    pub const SOURCE_INCONSISTENT: Self = Self(1 << 9);
    pub const OCR: Self = Self(1 << 10);
    pub const LLM_READ: Self = Self(1 << 11);
    pub const PARTIAL_COVERAGE: Self = Self(1 << 12);
    pub const TOP_CODED: Self = Self(1 << 13);
    pub const UNOFFICIAL_GEOM: Self = Self(1 << 14);
    pub const SUPERSEDED_BOUNDARY: Self = Self(1 << 15);
    pub const INTERPOLATED_SPACE: Self = Self(1 << 16);
    pub const DISAGGREGATED: Self = Self(1 << 17);

    /// From the stored `int4`; rejects reserved bits.
    pub fn from_db(v: i32) -> Result<Self, FlagError> {
        let bits = u32::from_ne_bytes(v.to_ne_bytes());
        if bits & RESERVED_MASK != 0 {
            return Err(FlagError::Reserved(bits & RESERVED_MASK));
        }
        Ok(Self(bits))
    }

    /// Lenient read for display paths: reserved bits are dropped.
    #[must_use]
    pub fn from_db_lossy(v: i32) -> Self {
        Self(u32::from_ne_bytes(v.to_ne_bytes()) & !RESERVED_MASK)
    }

    #[must_use]
    pub const fn to_db(self) -> i32 {
        i32::from_ne_bytes(self.0.to_ne_bytes())
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Quality class `A..D` (bits 28–29), if set. Bits 28–29 = 0 means "A or
    /// none"; a class is only meaningful for indicators declaring one.
    #[must_use]
    pub const fn quality(self) -> char {
        match (self.0 & QUALITY_MASK) >> QUALITY_SHIFT {
            0 => 'A',
            1 => 'B',
            2 => 'C',
            _ => 'D',
        }
    }

    pub fn with_quality(self, q: char) -> Result<Self, FlagError> {
        let v = match q {
            'A' => 0,
            'B' => 1,
            'C' => 2,
            'D' => 3,
            _ => return Err(FlagError::Quality(q.to_string())),
        };
        Ok(Self((self.0 & !QUALITY_MASK) | (v << QUALITY_SHIFT)))
    }

    /// Registered flag names, in bit order.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        REGISTRY
            .iter()
            .filter(|(b, _)| self.0 & (1 << b) != 0)
            .map(|(_, n)| *n)
            .collect()
    }

    /// Parses registered names (case-sensitive, as in the spec).
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self, FlagError> {
        let mut bits = 0;
        for n in names {
            let (b, _) = REGISTRY
                .iter()
                .find(|(_, name)| *name == n)
                .ok_or_else(|| FlagError::Unknown(n.to_owned()))?;
            bits |= 1 << b;
        }
        Ok(Self(bits))
    }
}

impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.names().join(","))
    }
}

/// Serialised as the list of flag names (`[]` when none).
impl Serialize for Flags {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.names().serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_dense_and_unique() {
        for (i, (b, n)) in REGISTRY.iter().enumerate() {
            assert_eq!(usize::from(*b), i, "{n}");
            assert!(n.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
        }
        let names: std::collections::BTreeSet<_> = REGISTRY.iter().map(|x| x.1).collect();
        assert_eq!(names.len(), REGISTRY.len());
    }

    #[test]
    fn constants_match_registry() {
        let all = [
            Flags::REBASED,
            Flags::SPLIT_UNIT,
            Flags::DENOM_MISMATCH,
            Flags::SMALL_N,
            Flags::MODEL_SPREAD,
            Flags::INTERPOLATED_TIME,
            Flags::EXTRAPOLATED_TIME,
            Flags::ANCILLARY_WEAK,
            Flags::SOURCE_AMBIGUOUS,
            Flags::SOURCE_INCONSISTENT,
            Flags::OCR,
            Flags::LLM_READ,
            Flags::PARTIAL_COVERAGE,
            Flags::TOP_CODED,
            Flags::UNOFFICIAL_GEOM,
            Flags::SUPERSEDED_BOUNDARY,
            Flags::INTERPOLATED_SPACE,
            Flags::DISAGGREGATED,
        ];
        assert_eq!(all.len(), REGISTRY.len());
        for (c, (_, name)) in all.iter().zip(REGISTRY) {
            assert_eq!(c.names(), vec![*name]);
        }
    }

    /// Property: every 18-bit flag pattern × quality class survives
    /// names → bits → db → bits.
    #[test]
    fn round_trip_property() {
        for bits in 0u32..=0x3FFFF {
            for q in ['A', 'B', 'C', 'D'] {
                let f = Flags(bits).with_quality(q).expect("quality");
                assert_eq!(Flags::from_db(f.to_db()), Ok(f));
                assert_eq!(f.quality(), q);
                let by_name = Flags::from_names(f.names()).expect("names");
                assert_eq!(by_name.bits(), bits);
            }
        }
        assert!(Flags::from_db(1 << 18).is_err());
        assert!(Flags::from_db(1 << 30).is_err());
        assert!(Flags::from_names(["NOPE"]).is_err());
        assert_eq!(
            serde_json::to_value(Flags::OCR.union(Flags::REBASED)).ok(),
            Some(serde_json::json!(["REBASED", "OCR"]))
        );
    }
}
