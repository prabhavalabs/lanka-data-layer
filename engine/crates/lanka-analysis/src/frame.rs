//! The aligned frame (11 §3.5): every variable as one column on one support,
//! with its history attached. The compute stage never sees raw inputs.

use std::collections::{BTreeMap, BTreeSet};

use lanka_stats::transform::Transform;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Units of one admin level (WP-29 supports; grids and points come with
/// WP-27/WP-33 alignment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub kind: String,
    pub scheme: String,
    pub level: i16,
    pub within: Option<String>,
    pub boundary_set: String,
    /// Sorted unit codes: every column lines up with them.
    pub codes: Vec<String>,
    /// Representative point of every unit (effective n, kNN weights).
    pub lon: Vec<f64>,
    pub lat: Vec<f64>,
    /// Parent code of every unit (level − 1), for fixed effects.
    pub parents: Vec<Option<String>>,
}

impl Support {
    /// `"admin:3"`.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}:{}", self.kind, self.level)
    }
}

/// What a column came from (one entry of the result's `inputs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InputRef {
    pub role: String,
    pub dataset: String,
    pub dsv: String,
    pub indicator: Option<String>,
    pub grid: Option<String>,
    /// sha256 of the grid layer file (enters the fingerprint).
    pub grid_sha256: Option<String>,
    pub period: String,
    /// official | third_party | derived | estimated
    pub class: String,
    pub licence: String,
    pub redistribution: String,
    pub measure_type: String,
    pub unit: Option<String>,
    pub label: String,
    /// Describes people, households, dwellings or properties (ECOLOGICAL).
    pub about_people: bool,
    pub synthetic: bool,
    pub community: bool,
    /// Every dataset upstream of this input in the recipe graph, itself
    /// included (SHARED_LINEAGE).
    pub upstream: BTreeSet<String>,
    pub sources: Vec<String>,
}

impl InputRef {
    #[must_use]
    pub fn extensive(&self) -> bool {
        matches!(self.measure_type.as_str(), "count" | "sum")
    }
}

/// One alignment step (an entry of the result's `alignment`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AlignmentRecord {
    pub role: String,
    pub from: String,
    pub to: String,
    pub method: String,
    pub params: Value,
    pub coverage: f64,
    pub quality: Option<String>,
    pub dropped: BTreeMap<String, u64>,
    pub period: String,
}

/// basis codes (05 §2): 0 reported, 1 computed, 2 estimated, 3 forecast.
pub const BASIS: [&str; 4] = ["reported", "computed", "estimated", "forecast"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub role: String,
    /// Term name in formulas and outputs (`x`, `y`, `c1`, …).
    pub term: String,
    pub input: InputRef,
    #[serde(with = "transform_serde")]
    pub transform: Transform,
    /// Raw (untransformed) values on the support; NaN = missing.
    pub values: Vec<f64>,
    pub lo: Option<Vec<f64>>,
    pub hi: Option<Vec<f64>>,
    /// basis code per unit (`u8::MAX` where there is no value).
    pub basis: Vec<u8>,
    /// Units whose value the publisher suppressed.
    pub suppressed: Vec<bool>,
    pub alignment: AlignmentRecord,
}

impl Column {
    /// Transformed values (NaN where the transform is undefined).
    #[must_use]
    pub fn transformed(&self) -> Vec<f64> {
        self.transform.apply(&self.values)
    }

    /// Highest basis code among the values.
    #[must_use]
    pub fn max_basis(&self) -> u8 {
        self.basis
            .iter()
            .copied()
            .filter(|b| *b != u8::MAX)
            .max()
            .unwrap_or(1)
    }

    #[must_use]
    pub fn has_intervals(&self) -> bool {
        self.lo
            .as_ref()
            .is_some_and(|l| l.iter().any(|v| v.is_finite()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedFrame {
    pub support: Support,
    pub columns: Vec<Column>,
}

impl AlignedFrame {
    #[must_use]
    pub fn col(&self, role: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.role == role)
    }

    #[must_use]
    pub fn controls(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|c| c.role.starts_with("control:"))
            .collect()
    }

    /// Rows where every listed column has a finite transformed value.
    #[must_use]
    pub fn complete(&self, roles: &[&str]) -> Vec<usize> {
        let cols: Vec<Vec<f64>> = roles
            .iter()
            .filter_map(|r| self.col(r))
            .map(Column::transformed)
            .collect();
        (0..self.support.codes.len())
            .filter(|&i| cols.iter().all(|c| c[i].is_finite()))
            .collect()
    }

    /// Units without a value per role (before transforms), and units lost
    /// to a transform (e.g. log of 0).
    #[must_use]
    pub fn drop_counts(&self, roles: &[&str]) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for r in roles {
            let Some(c) = self.col(r) else { continue };
            let t = c.transformed();
            let missing = c.values.iter().filter(|v| !v.is_finite()).count() as u64;
            let lost = c
                .values
                .iter()
                .zip(&t)
                .filter(|(v, t)| v.is_finite() && !t.is_finite())
                .count() as u64;
            let name = r.replace(':', "_");
            if missing > 0 {
                out.insert(format!("missing_{name}"), missing);
            }
            if lost > 0 {
                out.insert(format!("transform_{name}"), lost);
            }
        }
        out
    }

    /// Units excluded because a listed column was suppressed.
    #[must_use]
    pub fn suppressed(&self, roles: &[&str]) -> usize {
        (0..self.support.codes.len())
            .filter(|&i| {
                roles
                    .iter()
                    .filter_map(|r| self.col(r))
                    .any(|c| c.suppressed[i])
            })
            .count()
    }

    /// Selects rows (for scale-sensitivity sub-frames and tests).
    #[must_use]
    pub fn select(&self, rows: &[usize]) -> Self {
        let pick = |v: &[f64]| rows.iter().map(|&i| v[i]).collect::<Vec<f64>>();
        let s = &self.support;
        Self {
            support: Support {
                codes: rows.iter().map(|&i| s.codes[i].clone()).collect(),
                lon: pick(&s.lon),
                lat: pick(&s.lat),
                parents: rows.iter().map(|&i| s.parents[i].clone()).collect(),
                ..s.clone()
            },
            columns: self
                .columns
                .iter()
                .map(|c| Column {
                    values: pick(&c.values),
                    lo: c.lo.as_ref().map(|l| pick(l)),
                    hi: c.hi.as_ref().map(|h| pick(h)),
                    basis: rows.iter().map(|&i| c.basis[i]).collect(),
                    suppressed: rows.iter().map(|&i| c.suppressed[i]).collect(),
                    ..c.clone()
                })
                .collect(),
        }
    }
}

mod transform_serde {
    use lanka_stats::transform::Transform;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &Transform, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(t.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Transform, D::Error> {
        let s = String::deserialize(d)?;
        Transform::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("transform {s:?}")))
    }
}
