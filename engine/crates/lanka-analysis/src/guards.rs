//! The guard registry (11 §6, ADR 0022): guards are data — code, severity,
//! whether a request may override it, the work package that makes it
//! raisable, and a message template with `{placeholders}`.
//!
//! Messages are fixed templates; no free text, no language model. The
//! vocabulary test scans every template (and the interpretation templates
//! of `result.rs`) for causal wording.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Block,
    Warn,
    Note,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Warn => "warn",
            Self::Note => "note",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GuardSpec {
    pub code: &'static str,
    pub severity: Severity,
    /// May be listed in the request's `override` (recorded in the result
    /// and the citation).
    pub overridable: bool,
    /// The work package whose methods can raise it (`WP-29` = active now).
    pub since: &'static str,
    pub trigger: &'static str,
    pub template: &'static str,
}

/// Every guard code of 11 §6.
pub static REGISTRY: &[GuardSpec] = &[
    GuardSpec {
        code: "NOT_CAUSAL",
        severity: Severity::Note,
        overridable: false,
        since: "WP-29",
        trigger: "every relationship method",
        template: "This is an association between {units}. It does not show that one variable changes the other.",
    },
    GuardSpec {
        code: "MIN_N",
        severity: Severity::Block,
        overridable: false,
        since: "WP-29",
        trigger: "n used below the method's block threshold",
        template: "Only {n} {units} have values for every variable; {method} needs at least {min}. No estimates are returned.",
    },
    GuardSpec {
        code: "SMALL_N",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "n between the block and warn thresholds",
        template: "Only {n} {units} are used (fewer than {warn}). Intervals are shown; p-values are not reported.",
    },
    GuardSpec {
        code: "MAUP",
        severity: Severity::Note,
        overridable: false,
        since: "WP-29",
        trigger: "any areal support",
        template: "Results describe {units}. The strength of an association between areas depends on how the areas are drawn{sensitivity}.",
    },
    GuardSpec {
        code: "MAUP_UNSTABLE",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "sign flip or |Δr| > 0.2 across adjacent levels",
        template: "The correlation is {r_other} at {level_other} and {r_here} at {level_here}: the result depends strongly on the units used.",
    },
    GuardSpec {
        code: "ECOLOGICAL",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "areal support and a variable that describes people, households or properties",
        template: "Results describe {units}, not individual people, households or properties; what holds for areas need not hold for those inside them.",
    },
    GuardSpec {
        code: "RESIDUAL_AUTOCORR",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "permutation p of residual Moran's I < 0.05",
        template: "Residuals are spatially clustered (Moran's I {moran}, permutation p {p}). Standard errors of the plain regression are too small; use the adjusted figures or a spatial model.",
    },
    GuardSpec {
        code: "SIZE_CONFOUND",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "both variables extensive",
        template: "Both variables are totals that grow with the size of a unit; much of their correlation reflects size. Compare rates or densities instead.",
    },
    GuardSpec {
        code: "SHARED_LINEAGE",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "the variables share an upstream input in the derivation graph",
        template: "{a} and {b} are both built from {shared}; part of this correlation is built in.",
    },
    GuardSpec {
        code: "ESTIMATED_INPUT",
        severity: Severity::Note,
        overridable: false,
        since: "WP-29",
        trigger: "any column with basis estimated or forecast",
        template: "{inputs} is a model estimate, not a measurement; the result is an estimate too.",
    },
    GuardSpec {
        code: "MODEL_ON_MODEL",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "both sides estimated",
        template: "Both sides of this comparison are model estimates; agreement between models is not confirmation.",
    },
    GuardSpec {
        code: "INPUT_UNCERTAINTY_IGNORED",
        severity: Severity::Note,
        overridable: false,
        since: "WP-29",
        trigger: "inputs have intervals and uncertainty = analytic",
        template: "{inputs} carries intervals that these estimates do not propagate.",
    },
    GuardSpec {
        code: "PERIOD_MISMATCH",
        severity: Severity::Warn,
        overridable: true,
        since: "WP-29",
        trigger: "aligned periods differ by more than the tolerance (2 years); > 5 years among time-varying inputs blocks (time-invariant inputs only warn)",
        template: "The variables refer to different periods ({periods}), {years} years apart.",
    },
    GuardSpec {
        code: "LOW_COVERAGE",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "complete cases < 80 % of the support; < 30 % blocks",
        template: "{dropped} of {total} {units} ({pct} %) lack a value for at least one variable and are excluded.",
    },
    GuardSpec {
        code: "PSEUDO_REPLICATION",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "a column aligned by inherit",
        template: "{inputs} was copied from {parents} parent units to their children; the effective number of independent values is {parents}.",
    },
    GuardSpec {
        code: "DISAGGREGATED_INPUT",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-17",
        trigger: "a column aligned by dasymetric",
        template: "{inputs} was distributed to smaller units with ancillary data (quality {quality}).",
    },
    GuardSpec {
        code: "INTERP_SOCIO",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-33",
        trigger: "an interpolated socio-economic surface (11 §3.4)",
        template: "A price surface is a smoothed description of asking prices, not a valuation of any parcel.",
    },
    GuardSpec {
        code: "OUTLIER_LEVERAGE",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "robust and OLS slopes differ by > 25 %, or max Cook's D > 1",
        template: "A few {units} weigh heavily on the fit (largest Cook's distance {cooks}; robust slope {robust} vs {ols}). The robust fit is the headline.",
    },
    GuardSpec {
        code: "NO_CONTROLS",
        severity: Severity::Note,
        overridable: false,
        since: "WP-29",
        trigger: "a regression with one predictor",
        template: "No other variables are held constant; candidate controls at this level: {candidates}.",
    },
    GuardSpec {
        code: "MULTICOLLINEAR",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "any VIF > 10",
        template: "{terms} are strongly intercorrelated (largest VIF {vif}); their separate coefficients are unstable.",
    },
    GuardSpec {
        code: "MULTIPLE_TESTING",
        severity: Severity::Note,
        overridable: false,
        since: "WP-30",
        trigger: "local statistics",
        template: "Many local tests were run; only the FDR-adjusted p is used for map classes.",
    },
    GuardSpec {
        code: "SUPPRESSED_CELLS",
        severity: Severity::Note,
        overridable: false,
        since: "WP-29",
        trigger: "units dropped because inputs were suppressed",
        template: "{n} {units} are excluded because the publisher suppressed their values.",
    },
    GuardSpec {
        code: "SYNTHETIC_INPUT",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "an input is flagged synthetic",
        template: "{inputs} is a synthetic sample for demonstration.",
    },
    GuardSpec {
        code: "COMMUNITY_INPUT",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-29",
        trigger: "an input is unreviewed workspace data",
        template: "{inputs} is unreviewed workspace data.",
    },
    GuardSpec {
        code: "EXPLORATORY",
        severity: Severity::Warn,
        overridable: false,
        since: "WP-37",
        trigger: "tier 3 methods",
        template: "This method is exploratory; treat its local estimates as hypotheses.",
    },
];

/// Words association templates must never contain (11 §6).
pub const CAUSAL_VOCABULARY: &[&str] = &[
    "causes",
    "leads to",
    "due to",
    "because of",
    "effect of",
    "impact of",
];

#[must_use]
pub fn spec(code: &str) -> Option<&'static GuardSpec> {
    REGISTRY.iter().find(|g| g.code == code)
}

/// A raised guard before rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct Raised {
    pub code: &'static str,
    /// Severity override (PERIOD_MISMATCH/LOW_COVERAGE escalate to block).
    pub severity: Option<Severity>,
    pub vars: BTreeMap<&'static str, String>,
    pub detail: Option<Value>,
    pub action: Option<Value>,
}

impl Raised {
    #[must_use]
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            severity: None,
            vars: BTreeMap::new(),
            detail: None,
            action: None,
        }
    }

    #[must_use]
    pub fn var(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.vars.insert(k, v.into());
        self
    }

    #[must_use]
    pub fn detail(mut self, d: Value) -> Self {
        self.detail = Some(d);
        self
    }

    #[must_use]
    pub fn action(mut self, a: Value) -> Self {
        self.action = Some(a);
        self
    }

    #[must_use]
    pub fn block(mut self) -> Self {
        self.severity = Some(Severity::Block);
        self
    }

    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
            .or_else(|| spec(self.code).map(|s| s.severity))
            .unwrap_or(Severity::Note)
    }
}

/// Fills `{name}` placeholders; unknown ones render empty.
#[must_use]
pub fn render(template: &str, vars: &BTreeMap<&'static str, String>) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        if let Some(j) = tail.find('}') {
            let key = &tail[..j];
            if let Some(v) = vars.get(key) {
                out.push_str(v);
            }
            rest = &tail[j + 1..];
        } else {
            out.push_str(&rest[i..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// Rendered guards, sorted block → warn → note, `NOT_CAUSAL` first among
/// notes, then by code; one entry per code (the first raised wins).
/// `overrides` turns an overridable block back into a warn, marked
/// `overridden: true`.
#[must_use]
pub fn finish(raised: &[Raised], overrides: &[String]) -> Vec<Value> {
    let mut seen = std::collections::BTreeSet::new();
    let mut list: Vec<(Severity, u8, &'static str, Value)> = Vec::new();
    for r in raised {
        if !seen.insert(r.code) {
            continue;
        }
        let Some(s) = spec(r.code) else { continue };
        let mut sev = r.severity();
        let mut overridden = false;
        if sev == Severity::Block && s.overridable && overrides.iter().any(|o| o == r.code) {
            sev = Severity::Warn;
            overridden = true;
        }
        let mut g = json!({
            "code": r.code,
            "severity": sev.as_str(),
            "message": render(s.template, &r.vars),
        });
        if let Some(d) = &r.detail {
            g["detail"] = d.clone();
        }
        if let Some(a) = &r.action {
            g["action"] = a.clone();
        }
        if overridden {
            g["overridden"] = json!(true);
        }
        let first = u8::from(r.code != "NOT_CAUSAL");
        list.push((sev, first, r.code, g));
    }
    list.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    list.into_iter().map(|x| x.3).collect()
}

/// Whether any finished guard blocks.
#[must_use]
pub fn blocks(guards: &[Value]) -> bool {
    guards.iter().any(|g| g["severity"] == "block")
}

/// Registry as served by `/v2/analysis/methods` (and the method pages).
#[must_use]
pub fn registry_json() -> Value {
    Value::Array(
        REGISTRY
            .iter()
            .map(|g| {
                json!({"code": g.code, "severity": g.severity.as_str(), "overridable": g.overridable,
                       "since": g.since, "trigger": g.trigger, "template": g.template})
            })
            .collect(),
    )
}

/// Every causal word found in `text` (lower-cased).
#[must_use]
pub fn causal_words(text: &str) -> Vec<&'static str> {
    let t = text.to_lowercase();
    CAUSAL_VOCABULARY
        .iter()
        .copied()
        .filter(|w| t.contains(w))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_scan_of_the_registry() {
        for g in REGISTRY {
            assert!(
                causal_words(g.template).is_empty(),
                "{}: {}",
                g.code,
                g.template
            );
        }
        assert_eq!(
            causal_words("Flood risk leads to lower prices"),
            vec!["leads to"]
        );
    }

    #[test]
    fn every_code_of_the_contract_is_registered_once() {
        let codes: std::collections::BTreeSet<_> = REGISTRY.iter().map(|g| g.code).collect();
        assert_eq!(codes.len(), REGISTRY.len());
        let schema: Value =
            serde_json::from_str(crate::request::RESULT_SCHEMA).expect("result schema");
        let listed: std::collections::BTreeSet<String> =
            schema["$defs"]["guard"]["properties"]["code"]["enum"]
                .as_array()
                .expect("enum")
                .iter()
                .map(|v| v.as_str().expect("code").to_owned())
                .collect();
        assert_eq!(listed, codes.iter().map(|c| (*c).to_owned()).collect());
    }

    #[test]
    fn sorting_overrides_and_rendering() {
        let r = vec![
            Raised::new("MAUP").var("units", "districts"),
            Raised::new("PERIOD_MISMATCH")
                .block()
                .var("periods", "2014, 2024")
                .var("years", "10"),
            Raised::new("NOT_CAUSAL").var("units", "districts"),
            Raised::new("ECOLOGICAL").var("units", "districts"),
            Raised::new("MAUP"),
        ];
        let g = finish(&r, &[]);
        let codes: Vec<&str> = g
            .iter()
            .map(|g| g["code"].as_str().expect("code"))
            .collect();
        assert_eq!(
            codes,
            vec!["PERIOD_MISMATCH", "ECOLOGICAL", "NOT_CAUSAL", "MAUP"]
        );
        assert!(blocks(&g));
        assert!(
            g[0]["message"]
                .as_str()
                .expect("m")
                .contains("10 years apart")
        );
        let o = finish(&r, &["PERIOD_MISMATCH".to_owned()]);
        assert!(!blocks(&o));
        let pm = o
            .iter()
            .find(|g| g["code"] == "PERIOD_MISMATCH")
            .expect("pm");
        assert_eq!(pm["overridden"], json!(true));
        assert_eq!(pm["severity"], "warn");
        // MIN_N is not overridable
        let m = finish(&[Raised::new("MIN_N")], &["MIN_N".to_owned()]);
        assert!(blocks(&m));
    }
}
