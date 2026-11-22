//! Units of measure and measure semantics (02-standard-model.md §3).
//!
//! A unit is a case-sensitive UCUM code (ISO 4217 for money, `{annotation}`
//! for countables) with a power-of-ten multiplier on the indicator
//! (`unit_mult`, SDMX `UNIT_MULT`). `aggregation` makes roll-up data, not
//! code.

use serde::{Deserialize, Serialize};

macro_rules! text_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($var),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$var),+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$var => $s),+ }
            }

            #[must_use]
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$var),)+ _ => None }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

text_enum!(
    /// `catalog.units_of_measure.kind`.
    UnitKind {
        Count = "count",
        Ratio = "ratio",
        Currency = "currency",
        Physical = "physical",
        Index = "index",
        Score = "score",
    }
);

text_enum!(
    /// `catalog.indicators.measure_type`.
    MeasureType {
        Count = "count",
        Sum = "sum",
        Rate = "rate",
        Ratio = "ratio",
        Share = "share",
        Mean = "mean",
        Median = "median",
        Index = "index",
        Score = "score",
        Rank = "rank",
        Category = "category",
    }
);

text_enum!(
    /// `catalog.indicators.aggregation`: how a parent value follows from children.
    Aggregation {
        Sum = "sum",
        RatioOfSums = "ratio_of_sums",
        WeightedMean = "weighted_mean",
        Min = "min",
        Max = "max",
        None = "none",
    }
);

impl Aggregation {
    /// Which indicator references the aggregation needs.
    #[must_use]
    pub const fn needs(self) -> (&'static [&'static str], bool) {
        match self {
            Self::RatioOfSums => (&["numerator", "denominator"], true),
            Self::WeightedMean => (&["weight"], true),
            _ => (&[], false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid unit code {0:?}: {1}")]
pub struct UnitError(pub String, pub &'static str);

/// Syntactic check of a UCUM-style code: printable ASCII without spaces,
/// balanced `{…}` annotations and `[…]` atoms. Existence is checked against
/// `catalog.units_of_measure` / `registry/units.yaml`.
pub fn check_unit_code(code: &str) -> Result<(), UnitError> {
    let err = |why| UnitError(code.to_owned(), why);
    if code.is_empty() || code.len() > 64 {
        return Err(err("must be 1-64 characters"));
    }
    let (mut brace, mut bracket) = (0i32, 0i32);
    for c in code.chars() {
        if !c.is_ascii_graphic() {
            return Err(err("must be printable ASCII without spaces"));
        }
        match c {
            '{' => brace += 1,
            '}' => brace -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            _ => {}
        }
        if brace < 0 || bracket < 0 || brace > 1 || bracket > 1 {
            return Err(err("unbalanced {} or []"));
        }
    }
    if brace != 0 || bracket != 0 {
        return Err(err("unbalanced {} or []"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_syntax() {
        for ok in [
            "{person}",
            "%",
            "1",
            "km2",
            "/10*3",
            "USD",
            "kW.h",
            "[pH]",
            "Cel",
            "{person}/km2",
        ] {
            assert!(check_unit_code(ok).is_ok(), "{ok}");
        }
        for bad in ["", "per cent", "{person", "a}b", "[[x]]"] {
            assert!(check_unit_code(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn enums_round_trip() {
        for m in MeasureType::ALL {
            assert_eq!(MeasureType::parse(m.as_str()), Some(*m));
        }
        for a in Aggregation::ALL {
            assert_eq!(Aggregation::parse(a.as_str()), Some(*a));
        }
        for k in UnitKind::ALL {
            assert_eq!(UnitKind::parse(k.as_str()), Some(*k));
        }
        assert!(Aggregation::RatioOfSums.needs().1);
    }
}
