//! `ldl-analysis/1` requests (11 §5.1, UI contract §6.1).
//!
//! Requests are validated against the embedded JSON Schema
//! (`schema/request.schema.json`, byte-identical to
//! `spec/analysis/1/request.schema.json`) before they are deserialised, so
//! problems come back as JSON-pointer paths.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SPEC: &str = "ldl-analysis/1";
pub const RESULT_SPEC: &str = "ldl-analysis-result/1";

pub const REQUEST_SCHEMA: &str = include_str!("../schema/request.schema.json");
pub const RESULT_SCHEMA: &str = include_str!("../schema/result.schema.json");
pub const PLAN_SCHEMA: &str = include_str!("../schema/plan.schema.json");

static VALIDATOR: LazyLock<Result<jsonschema::Validator, String>> = LazyLock::new(|| {
    let schema: Value = serde_json::from_str(REQUEST_SCHEMA).map_err(|e| e.to_string())?;
    jsonschema::validator_for(&schema).map_err(|e| e.to_string())
});

/// A schema problem: JSON pointer + message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

/// Validates a request body against `ldl-analysis/1`.
pub fn validate(body: &Value) -> Result<(), Vec<Problem>> {
    let v = match VALIDATOR.as_ref() {
        Ok(v) => v,
        Err(e) => {
            return Err(vec![Problem {
                path: String::new(),
                message: format!("request schema unavailable: {e}"),
            }]);
        }
    };
    let mut problems: Vec<Problem> = v
        .iter_errors(body)
        .map(|e| Problem {
            path: e.instance_path().to_string(),
            message: e.to_string(),
        })
        .collect();
    problems.sort_by(|a, b| (&a.path, &a.message).cmp(&(&b.path, &b.message)));
    problems.dedup();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// `pin`: the current release, or a named one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum Pin {
    #[default]
    #[serde(with = "latest")]
    Latest,
    Release {
        release: String,
    },
}

mod latest {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("latest")
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
        let s = String::deserialize(d)?;
        if s == "latest" {
            Ok(())
        } else {
            Err(serde::de::Error::custom(
                "pin must be \"latest\" or {\"release\": …}",
            ))
        }
    }
}

/// `support`: `"auto"` or an explicit support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum SupportRef {
    #[default]
    #[serde(with = "auto")]
    Auto,
    Explicit(Support),
}

mod auto {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("auto")
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
        let s = String::deserialize(d)?;
        if s == "auto" {
            Ok(())
        } else {
            Err(serde::de::Error::custom(
                "support must be \"auto\" or an object",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Support {
    Admin {
        level: i16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<String>,
    },
    Grid {
        factor: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<String>,
    },
    Points {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dataset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<String>,
    },
}

/// One variable: an indicator, a grid layer or a features dataset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Variable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dims: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Value>,
}

impl Variable {
    /// What the variable names, for messages: `indicator`, `grid:<id>`,
    /// `<dataset>.<field>`.
    #[must_use]
    pub fn name(&self) -> String {
        if let Some(i) = &self.indicator {
            i.clone()
        } else if let Some(g) = &self.grid {
            format!("grid:{g}")
        } else if let (Some(d), Some(f)) = (&self.dataset, &self.field) {
            format!("{d}.{f}")
        } else if let Some(d) = &self.dataset {
            d.clone()
        } else if let Some(s) = &self.source {
            format!("source:{s}")
        } else {
            "?".to_owned()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Variables {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Variable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Variable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<Variable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<Variable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Variable>,
}

impl Variables {
    /// `(role, variable)` in a fixed order: x, y, controls, weight, group.
    #[must_use]
    pub fn roles(&self) -> Vec<(String, &Variable)> {
        let mut out = Vec::new();
        if let Some(v) = &self.x {
            out.push(("x".to_owned(), v));
        }
        if let Some(v) = &self.y {
            out.push(("y".to_owned(), v));
        }
        for (i, v) in self.controls.iter().enumerate() {
            out.push((format!("control:{i}"), v));
        }
        if let Some(v) = &self.weight {
            out.push(("weight".to_owned(), v));
        }
        if let Some(v) = &self.group {
            out.push(("group".to_owned(), v));
        }
        out
    }

    pub fn get_mut(&mut self, role: &str) -> Option<&mut Variable> {
        match role {
            "x" => self.x.as_mut(),
            "y" => self.y.as_mut(),
            "weight" => self.weight.as_mut(),
            "group" => self.group.as_mut(),
            r => r
                .strip_prefix("control:")
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| self.controls.get_mut(i)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub spec: String,
    pub method: String,
    #[serde(default)]
    pub pin: Pin,
    #[serde(default)]
    pub support: SupportRef,
    #[serde(default)]
    pub variables: Variables,
    #[serde(default = "empty_object")]
    pub params: Value,
    #[serde(default, rename = "override", skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<String>,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

impl Request {
    /// Schema validation, then deserialisation.
    pub fn parse(body: &Value) -> Result<Self, Vec<Problem>> {
        validate(body)?;
        serde_json::from_value(body.clone()).map_err(|e| {
            vec![Problem {
                path: String::new(),
                message: e.to_string(),
            }]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_documented_request_parses_and_round_trips() {
        let body = json!({
          "spec": "ldl-analysis/1", "method": "relate", "pin": "latest",
          "support": { "kind": "admin", "level": 4, "within": "LK1" },
          "variables": {
            "x": { "indicator": "exp.flood_pop_share", "dims": { "return_period": "RP100" }, "period": "latest" },
            "y": { "indicator": "@demo/re.asking_price_per_perch_median", "period": "2026", "transform": "log",
                   "align": { "method": "point_aggregate", "stat": "median", "min_n": 5 } },
            "controls": [ { "indicator": "est.pop_density", "transform": "log" } ]
          },
          "params": { "correlation": ["pearson", "spearman"], "robust": true, "bivariate": { "classes": 3, "method": "quantile" } }
        });
        let r = Request::parse(&body).expect("valid");
        assert_eq!(r.pin, Pin::Latest);
        assert!(matches!(
            r.support,
            SupportRef::Explicit(Support::Admin { level: 4, .. })
        ));
        let back = serde_json::to_value(&r).expect("ser");
        assert_eq!(Request::parse(&back).expect("again"), r);
        assert_eq!(r.variables.roles().len(), 3);
    }

    #[test]
    fn schema_problems_carry_pointers() {
        let bad = json!({ "spec": "ldl-analysis/1", "method": "relate", "support": { "kind": "admin", "level": 9 },
                          "variables": { "x": { "indicator": "a", "transform": "cube" } } });
        let p = Request::parse(&bad).expect_err("invalid");
        assert!(p.iter().any(|p| p.path.starts_with("/support")), "{p:?}");
        assert!(
            p.iter().any(|p| p.path == "/variables/x/transform"),
            "{p:?}"
        );
        let release = json!({ "spec": "ldl-analysis/1", "method": "describe", "pin": { "release": "20261122.1" },
                              "support": "auto", "variables": { "x": { "indicator": "a" } } });
        assert!(matches!(
            Request::parse(&release).expect("ok").pin,
            Pin::Release { .. }
        ));
    }
}
