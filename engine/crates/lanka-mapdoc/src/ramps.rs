//! The ramp and tone registry (`spec/map/1/ramps.json`, 10 §2.6).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// The registry file, embedded (a byte-identical copy of `spec/map/1/ramps.json`).
pub const RAMPS_JSON: &str = include_str!("../schema/ramps.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Ramp {
    /// seq | div | cat | biv
    pub kind: String,
    pub label: String,
    /// Stops for a light basemap (`ldl.*`).
    #[serde(default)]
    pub light: Vec<String>,
    /// Stops for a dark basemap (`ldl.*`).
    #[serde(default)]
    pub dark: Vec<String>,
    /// One list for both schemes (other families).
    #[serde(default)]
    pub stops: Vec<String>,
    /// Bivariate palettes: classes per axis.
    #[serde(default)]
    pub size: Option<u8>,
}

impl Ramp {
    /// Stops for a light (`dark = false`) or dark basemap.
    #[must_use]
    pub fn stops_for(&self, dark: bool) -> &[String] {
        if !self.stops.is_empty() {
            &self.stops
        } else if dark {
            &self.dark
        } else {
            &self.light
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tone {
    pub light: String,
    pub dark: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub tones: BTreeMap<String, Tone>,
    pub ramps: BTreeMap<String, Ramp>,
}

/// The parsed registry.
///
/// # Panics
/// Never in a released build: the embedded file is checked by the tests.
#[allow(clippy::expect_used)]
pub static REGISTRY: LazyLock<Registry> =
    LazyLock::new(|| serde_json::from_str(RAMPS_JSON).expect("embedded ramps.json is valid"));

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(hex: &str) -> [f64; 3] {
        let n = u32::from_str_radix(hex.trim_start_matches('#').get(..6).unwrap_or("000000"), 16)
            .unwrap_or(0);
        [(n >> 16) & 255, (n >> 8) & 255, n & 255].map(|c| f64::from(c) / 255.0)
    }

    fn linear(c: f64) -> f64 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// CIE L* of a linear-RGB colour.
    fn lightness([r, g, b]: [f64; 3]) -> f64 {
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let f = if y > 216.0 / 24389.0 {
            y.cbrt()
        } else {
            (24389.0 / 27.0 * y + 16.0) / 116.0
        };
        116.0 * f - 16.0
    }

    /// Machado, Oliveira & Fernandes (2009), severity 1.0, in linear RGB.
    const CVD: [(&str, [[f64; 3]; 3]); 4] = [
        (
            "normal",
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        ),
        (
            "protan",
            [
                [0.152_286, 1.052_583, -0.204_868],
                [0.114_503, 0.786_281, 0.099_216],
                [-0.003_882, -0.048_116, 1.051_998],
            ],
        ),
        (
            "deutan",
            [
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_820, 0.042_940, 0.968_881],
            ],
        ),
        (
            "tritan",
            [
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_602],
                [0.004_733, 0.691_367, 0.303_900],
            ],
        ),
    ];

    fn simulated_l(hex: &str, m: &[[f64; 3]; 3]) -> f64 {
        let c = rgb(hex).map(linear);
        let s =
            [0, 1, 2].map(|i| (m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]).clamp(0.0, 1.0));
        lightness(s)
    }

    #[test]
    fn registry_parses_and_is_complete() {
        let r = &*REGISTRY;
        assert!(
            r.ramps.len() >= 10 && r.tones.contains_key("paper") && r.tones.contains_key("ink")
        );
        for (name, ramp) in &r.ramps {
            assert!(
                matches!(ramp.kind.as_str(), "seq" | "div" | "cat" | "biv"),
                "{name}"
            );
            for dark in [false, true] {
                let s = ramp.stops_for(dark);
                assert!(s.len() >= 2, "{name}");
                assert!(
                    s.iter().all(|c| c.len() == 7 && c.starts_with('#')),
                    "{name}"
                );
                if ramp.kind == "biv" {
                    let k = usize::from(ramp.size.unwrap_or(3));
                    assert_eq!(s.len(), k * k, "{name} is size × size");
                }
            }
            let prefix_ok = name.starts_with(&format!("ldl.{}.", ramp.kind))
                || name.starts_with("brewer.")
                || name.starts_with("viridis.");
            assert!(prefix_ok, "{name}: ldl ramps are named by kind");
        }
    }

    /// Sequential ramps stay ordered in lightness (monotonic L*, steps ≥ 2)
    /// for normal vision and under protan and deutan simulation, and stay
    /// ordered under tritan; diverging ramps hold this on each side of the
    /// middle.
    #[test]
    fn sequential_and_diverging_ramps_are_cvd_safe() {
        let mut failures = Vec::new();
        for (name, ramp) in &REGISTRY.ramps {
            if !matches!(ramp.kind.as_str(), "seq" | "div") {
                continue;
            }
            for dark in [false, true] {
                let stops = ramp.stops_for(dark);
                for (cvd, m) in &CVD {
                    let l: Vec<f64> = stops.iter().map(|c| simulated_l(c, m)).collect();
                    // Normal, protan and deutan (≈ 8 % of men): steps of at
                    // least 2 L*. Tritan (rare; no common ramp passes a
                    // step rule): order only.
                    let step = if *cvd == "tritan" { 0.5 } else { 2.0 };
                    let mono = |xs: &[f64]| {
                        let up = xs.windows(2).all(|w| w[1] - w[0] >= step);
                        let down = xs.windows(2).all(|w| w[0] - w[1] >= step);
                        up || down
                    };
                    let ok = if ramp.kind == "seq" {
                        mono(&l)
                    } else {
                        let mid = l.len() / 2;
                        mono(&l[..=mid]) && mono(&l[mid..])
                    };
                    if !ok {
                        failures.push(format!(
                            "{name} ({}, {cvd}): L* {l:.1?}",
                            if dark { "dark" } else { "light" }
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "not CVD-safe:\n{}",
            failures.join("\n")
        );
    }
}
