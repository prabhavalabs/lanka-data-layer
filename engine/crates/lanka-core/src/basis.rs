//! Small coded fields of an observation (02-standard-model.md §4, 05 §2):
//! `basis`, `status`, extraction `method` and `review`. Stored as `int2`
//! (not Postgres enums) so adding a code never takes a catalog lock; the
//! numbers are part of the standard and mirrored in `data.code_labels`.

use serde::{Deserialize, Serialize};

/// Declares a code enum with `int2` numbers and wire names.
macro_rules! codes {
    ($(#[$m:meta])* $name:ident, $list:literal { $($var:ident = $n:literal, $s:literal;)+ }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($var),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$var),+];
            /// Codelist id in `data.code_labels`.
            pub const CODELIST: &'static str = $list;

            #[must_use]
            pub const fn code(self) -> i16 {
                match self { $(Self::$var => $n),+ }
            }

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$var => $s),+ }
            }

            #[must_use]
            pub fn from_code(c: i16) -> Option<Self> {
                match c { $($n => Some(Self::$var),)+ _ => None }
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

codes!(
    /// How a value came to exist.
    Basis, "basis" {
        Reported = 1, "reported";
        Computed = 2, "computed";
        Estimated = 3, "estimated";
        Forecast = 4, "forecast";
    }
);

codes!(
    /// The state of a value (SDMX `CL_OBS_STATUS`-aligned).
    Status, "status" {
        Normal = 1, "normal";
        Provisional = 2, "provisional";
        Revised = 3, "revised";
        Break = 4, "break";
        LowReliability = 5, "low_reliability";
        Missing = 6, "missing";
        Nil = 7, "nil";
        Suppressed = 8, "suppressed";
    }
);

codes!(
    /// How a value was read from its artifact (`data.obs_provenance.method`).
    Method, "method" {
        Api = 1, "api";
        SheetCell = 2, "sheet_cell";
        CsvRow = 3, "csv_row";
        PdfText = 4, "pdf_text";
        PdfReviewed = 5, "pdf_reviewed";
        Ocr = 6, "ocr";
        LlmDual = 7, "llm_dual";
        Manual = 8, "manual";
        Feature = 9, "feature";
        Spatial = 10, "spatial";
        Derived = 11, "derived";
    }
);

codes!(
    /// Review state of an extracted value.
    Review, "review" {
        AutoAccepted = 1, "auto_accepted";
        Pending = 2, "pending";
        Approved = 3, "approved";
        Rejected = 4, "rejected";
    }
);

impl Status {
    /// SDMX `OBS_STATUS` code, where one exists.
    #[must_use]
    pub const fn sdmx(self) -> Option<&'static str> {
        match self {
            Self::Normal => Some("A"),
            Self::Provisional => Some("P"),
            Self::Break => Some("B"),
            Self::LowReliability => Some("U"),
            Self::Missing => Some("M"),
            Self::Nil => Some("Z"),
            Self::Suppressed => Some("Q"),
            Self::Revised => None,
        }
    }

    /// Statuses whose value is NULL by definition (`nil` may print a 0).
    #[must_use]
    pub const fn requires_null(self) -> bool {
        matches!(self, Self::Missing | Self::Suppressed)
    }
}

/// The foundation's single `data.obs_status` enum, split into
/// `(basis, status)` — the mapping migration 0002 applies.
#[must_use]
pub fn from_legacy_status(s: &str) -> Option<(Basis, Status)> {
    Some(match s {
        "reported" => (Basis::Reported, Status::Normal),
        "revised" => (Basis::Reported, Status::Revised),
        "provisional" => (Basis::Reported, Status::Provisional),
        "estimate" | "modelled" => (Basis::Estimated, Status::Normal),
        "forecast" => (Basis::Forecast, Status::Normal),
        "derived" => (Basis::Computed, Status::Normal),
        "not_available" => (Basis::Reported, Status::Missing),
        "nil" => (Basis::Reported, Status::Nil),
        "suppressed" => (Basis::Reported, Status::Suppressed),
        _ => return None,
    })
}

/// Indicator class (02 §3): what the number is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorClass {
    Official,
    ThirdParty,
    Derived,
    Estimated,
}

impl IndicatorClass {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "official" => Self::Official,
            "third_party" => Self::ThirdParty,
            "derived" => Self::Derived,
            "estimated" => Self::Estimated,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::ThirdParty => "third_party",
            Self::Derived => "derived",
            Self::Estimated => "estimated",
        }
    }

    /// The basis × class table of 02 §4. `official` may carry `computed`
    /// values only as roll-ups; `third_party` may be `estimated`/`forecast`
    /// only when the source itself says so (the connector's call).
    #[must_use]
    pub const fn allows(self, b: Basis) -> bool {
        match self {
            Self::Official => matches!(b, Basis::Reported | Basis::Computed),
            Self::ThirdParty => true,
            Self::Derived => matches!(b, Basis::Computed),
            Self::Estimated => matches!(b, Basis::Estimated | Basis::Forecast),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_are_unique() {
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            all: &[T],
            code: fn(T) -> i16,
            from: fn(i16) -> Option<T>,
            s: fn(T) -> &'static str,
            parse: fn(&str) -> Option<T>,
        ) {
            let mut seen = std::collections::BTreeSet::new();
            for v in all {
                assert!(seen.insert(code(*v)), "duplicate code {v:?}");
                assert_eq!(from(code(*v)), Some(*v));
                assert_eq!(parse(s(*v)), Some(*v));
            }
            assert_eq!(from(0), None);
        }
        check(
            Basis::ALL,
            Basis::code,
            Basis::from_code,
            Basis::as_str,
            Basis::parse,
        );
        check(
            Status::ALL,
            Status::code,
            Status::from_code,
            Status::as_str,
            Status::parse,
        );
        check(
            Method::ALL,
            Method::code,
            Method::from_code,
            Method::as_str,
            Method::parse,
        );
        check(
            Review::ALL,
            Review::code,
            Review::from_code,
            Review::as_str,
            Review::parse,
        );
    }

    #[test]
    fn legacy_mapping_covers_the_0001_enum() {
        for s in [
            "reported",
            "revised",
            "provisional",
            "estimate",
            "forecast",
            "modelled",
            "derived",
            "not_available",
            "nil",
            "suppressed",
        ] {
            assert!(from_legacy_status(s).is_some(), "{s}");
        }
        assert_eq!(
            from_legacy_status("derived"),
            Some((Basis::Computed, Status::Normal))
        );
    }

    #[test]
    fn class_rules() {
        assert!(!IndicatorClass::Official.allows(Basis::Estimated));
        assert!(IndicatorClass::Official.allows(Basis::Computed));
        assert!(!IndicatorClass::Estimated.allows(Basis::Reported));
        assert!(IndicatorClass::ThirdParty.allows(Basis::Estimated));
    }
}
