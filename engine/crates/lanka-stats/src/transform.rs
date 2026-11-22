//! Variable transforms (11 §3.5 `Transform`). Values a transform cannot
//! take (log of ≤ 0, sqrt of < 0) become NaN = missing, so they drop out
//! of complete cases and are counted by the caller.

use libm::{log, log1p, sqrt};

use crate::describe::{mean, ranks, variance};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transform {
    #[default]
    Identity,
    Log,
    Log1p,
    Sqrt,
    ZScore,
    Rank,
}

impl Transform {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "identity" => Self::Identity,
            "log" => Self::Log,
            "log1p" => Self::Log1p,
            "sqrt" => Self::Sqrt,
            "zscore" => Self::ZScore,
            "rank" => Self::Rank,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Log => "log",
            Self::Log1p => "log1p",
            Self::Sqrt => "sqrt",
            Self::ZScore => "zscore",
            Self::Rank => "rank",
        }
    }

    #[must_use]
    pub const fn from_code(c: u32) -> Option<Self> {
        Some(match c {
            0 => Self::Identity,
            1 => Self::Log,
            2 => Self::Log1p,
            3 => Self::Sqrt,
            4 => Self::ZScore,
            5 => Self::Rank,
            _ => return None,
        })
    }

    /// Whether the transform is applied value by value (zscore and rank
    /// depend on the whole column).
    #[must_use]
    pub const fn is_pointwise(self) -> bool {
        matches!(self, Self::Identity | Self::Log | Self::Log1p | Self::Sqrt)
    }

    /// Applies the transform; NaN stays NaN. `ZScore`/`Rank` use the finite
    /// values of the column.
    #[must_use]
    pub fn apply(self, v: &[f64]) -> Vec<f64> {
        match self {
            Self::Identity => v.to_vec(),
            Self::Log => v
                .iter()
                .map(|&x| if x > 0.0 { log(x) } else { f64::NAN })
                .collect(),
            Self::Log1p => v
                .iter()
                .map(|&x| if x > -1.0 { log1p(x) } else { f64::NAN })
                .collect(),
            Self::Sqrt => v
                .iter()
                .map(|&x| if x >= 0.0 { sqrt(x) } else { f64::NAN })
                .collect(),
            Self::ZScore => {
                let f: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
                let (m, sd) = (mean(&f), sqrt(variance(&f)));
                v.iter()
                    .map(|&x| {
                        if x.is_finite() {
                            (x - m) / sd
                        } else {
                            f64::NAN
                        }
                    })
                    .collect()
            }
            Self::Rank => {
                let idx: Vec<usize> = (0..v.len()).filter(|&i| v[i].is_finite()).collect();
                let f: Vec<f64> = idx.iter().map(|&i| v[i]).collect();
                let r = ranks(&f);
                let mut out = vec![f64::NAN; v.len()];
                for (k, &i) in idx.iter().enumerate() {
                    out[i] = r[k];
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms() {
        let v = [1.0, 0.0, -1.0, f64::NAN, 4.0];
        let l = Transform::Log.apply(&v);
        assert_eq!(l[0], 0.0);
        assert!(l[1].is_nan() && l[2].is_nan() && l[3].is_nan());
        assert_eq!(Transform::Sqrt.apply(&v)[4], 2.0);
        assert_eq!(Transform::Rank.apply(&v)[4], 4.0);
        assert_eq!(Transform::parse("log1p"), Some(Transform::Log1p));
    }
}
