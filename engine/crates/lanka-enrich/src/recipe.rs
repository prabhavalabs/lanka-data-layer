//! Recipes (04 §4): `registry/recipes/<id>.yaml`, validated by
//! `registry/schema/recipe.schema.json`, then type-checked against the
//! method library and the registry (`lanka recipe check`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::dag;
use crate::method::PortKind;
use crate::methods::Methods;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    Experimental,
    Stable,
}

impl Stability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Stable => "stable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpec {
    pub dataset: String,
    /// Read only these indicators of the dataset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indicators: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub id: String,
    /// `<method>@<semver requirement>`.
    pub method: String,
    #[serde(default)]
    pub with: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    #[must_use]
    pub fn list(&self) -> Vec<&str> {
        match self {
            Self::One(s) => vec![s.as_str()],
            Self::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmitSpec {
    pub from: OneOrMany,
    /// Every row of the step(s) becomes this indicator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator: Option<String>,
    /// Step indicator → output indicator; unmapped rows are not emitted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub map: BTreeMap<String, String>,
}

impl EmitSpec {
    /// The output indicator of a step row (None = not emitted).
    #[must_use]
    pub fn target<'a>(&'a self, step_indicator: &'a str) -> Option<&'a str> {
        if let Some(i) = &self.indicator {
            Some(i)
        } else if self.map.is_empty() {
            Some(step_indicator)
        } else {
            self.map.get(step_indicator).map(String::as_str)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeSpec {
    pub id: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub stability: Stability,
    /// daily (default) | weekly | monthly | manual: heavier recipes are
    /// skipped by daily trains even when stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// The run's reference date (`MethodCtx::as_of`); never the wall clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<chrono::NaiveDate>,
    pub inputs: BTreeMap<String, InputSpec>,
    pub steps: Vec<StepSpec>,
    pub emit: Vec<EmitSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qa: Vec<serde_json::Value>,
    /// Share of existing values (percent) a re-run may move before the
    /// version is demoted to review (default 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_revision_pct: Option<f64>,
}

impl RecipeSpec {
    #[must_use]
    pub fn schedule(&self) -> &str {
        self.schedule.as_deref().unwrap_or("daily")
    }

    #[must_use]
    pub fn as_of(&self) -> chrono::NaiveDate {
        self.as_of.unwrap_or(chrono::NaiveDate::MIN)
    }

    /// The canonical JSON of the spec (what is hashed and stored).
    #[must_use]
    pub fn canonical_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// sha256 of the canonical spec.
    #[must_use]
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(lanka_core::dsv::canonical_json(&self.canonical_json()).as_bytes()).into()
    }
}

#[derive(Debug, Clone)]
pub struct LoadedRecipe {
    pub spec: RecipeSpec,
    pub path: PathBuf,
}

/// What `check` needs from the registry (built by `lanka-ingest`).
#[derive(Debug, Clone, Default)]
pub struct RegistryView {
    pub datasets: BTreeMap<String, DatasetView>,
}

#[derive(Debug, Clone, Default)]
pub struct DatasetView {
    /// `catalog.datasets.kind`.
    pub kind: String,
    pub indicators: BTreeSet<String>,
}

/// Checks that have a defined meaning in the runner's QA stage.
pub const QA_CHECKS: &[&str] = &["sum_matches", "non_negative", "min_rows", "no_missing"];

/// All recipes of a registry.
#[derive(Debug, Clone, Default)]
pub struct Recipes {
    pub list: Vec<LoadedRecipe>,
}

/// Problems found by [`Recipes::check`]; `errors` are fatal.
#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// Topological order (recipe ids) when the global DAG is acyclic.
    pub order: Vec<String>,
}

impl Recipes {
    /// Loads `root/recipes/*.yaml` and validates each against
    /// `root/schema/recipe.schema.json`. A registry without a recipes
    /// directory has no recipes.
    pub fn load(root: &Path) -> Result<Self> {
        let dir = root.join("recipes");
        if !dir.is_dir() {
            return Ok(Self::default());
        }
        let schema_path = root.join("schema/recipe.schema.json");
        let schema: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&schema_path)
                .with_context(|| format!("reading {}", schema_path.display()))?,
        )
        .with_context(|| format!("parsing {}", schema_path.display()))?;
        let validator = jsonschema::draft202012::new(&schema)
            .map_err(|e| anyhow!("compiling {}: {e}", schema_path.display()))?;
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
            .collect();
        files.sort();
        let mut errors = Vec::new();
        let mut list = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path)?;
            let json: serde_json::Value = match serde_yaml_ng::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let problems: Vec<String> = validator
                .iter_errors(&json)
                .map(|e| format!("{}: {e} (at {})", path.display(), e.instance_path()))
                .collect();
            if !problems.is_empty() {
                errors.extend(problems);
                continue;
            }
            match serde_json::from_value::<RecipeSpec>(json) {
                Ok(spec) => {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default();
                    if spec.id != stem {
                        errors.push(format!(
                            "{}: id {:?} does not match the file name",
                            path.display(),
                            spec.id
                        ));
                    }
                    list.push(LoadedRecipe { spec, path });
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        if !errors.is_empty() {
            anyhow::bail!(
                "recipes have {} error(s):\n  - {}",
                errors.len(),
                errors.join("\n  - ")
            );
        }
        Ok(Self { list })
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&RecipeSpec> {
        self.list.iter().find(|r| r.spec.id == id).map(|r| &r.spec)
    }

    /// Type-checks every recipe and proves the global DAG acyclic (04 §4).
    #[must_use]
    pub fn check(&self, reg: &RegistryView, methods: &Methods) -> CheckReport {
        let mut rep = CheckReport::default();
        let mut outputs: BTreeMap<&str, &str> = BTreeMap::new();
        for r in &self.list {
            let s = &r.spec;
            if let Some(prev) = outputs.insert(&s.output, &s.id) {
                rep.errors.push(format!(
                    "{}: output {} is already produced by {prev}",
                    s.id, s.output
                ));
            }
            check_one(s, reg, methods, &mut rep);
        }
        let specs: Vec<&RecipeSpec> = self.list.iter().map(|r| &r.spec).collect();
        match dag::order(&specs) {
            Ok(order) => rep.order = order.into_iter().map(|i| specs[i].id.clone()).collect(),
            Err(e) => rep.errors.push(e.to_string()),
        }
        rep
    }
}

fn check_one(s: &RecipeSpec, reg: &RegistryView, methods: &Methods, rep: &mut CheckReport) {
    let at = |m: String| format!("recipe {}: {m}", s.id);
    let Some(out) = reg.datasets.get(&s.output) else {
        rep.errors.push(at(format!(
            "output dataset {} is not in the registry",
            s.output
        )));
        return;
    };
    if !s.output.starts_with("derived-") {
        rep.errors.push(at(format!(
            "output dataset {} must start with derived- (04 §8)",
            s.output
        )));
    }
    if !matches!(s.schedule(), "daily" | "weekly" | "monthly" | "manual") {
        rep.errors
            .push(at(format!("unknown schedule {:?}", s.schedule())));
    }
    for (port, i) in &s.inputs {
        if i.dataset == s.output {
            rep.errors
                .push(at(format!("input {port} reads the recipe's own output")));
        }
        match reg.datasets.get(&i.dataset) {
            None => rep.errors.push(at(format!(
                "input {port}: dataset {} is not in the registry",
                i.dataset
            ))),
            Some(d) => {
                for ind in &i.indicators {
                    if !d.indicators.contains(ind) {
                        rep.errors.push(at(format!(
                            "input {port}: {ind} is not an indicator of {}",
                            i.dataset
                        )));
                    }
                }
            }
        }
    }
    let mut steps: BTreeSet<&str> = BTreeSet::new();
    for st in &s.steps {
        let sat = |m: String| at(format!("step {}: {m}", st.id));
        if s.inputs.contains_key(&st.id) || !steps.insert(&st.id) {
            rep.errors
                .push(sat("id collides with an input or an earlier step".into()));
        }
        let m = match methods.resolve(&st.method) {
            Ok(m) => m,
            Err(e) => {
                rep.errors.push(sat(e.to_string()));
                continue;
            }
        };
        match serde_json::from_str::<serde_json::Value>(m.info().params)
            .map_err(anyhow::Error::from)
            .and_then(|sc| jsonschema::draft202012::new(&sc).map_err(|e| anyhow!("{e}")))
        {
            Ok(v) => {
                for e in v.iter_errors(&st.with) {
                    rep.errors
                        .push(sat(format!("parameter {e} (at with{})", e.instance_path())));
                }
            }
            Err(e) => rep.errors.push(sat(format!("method schema: {e}"))),
        }
        let ports = match m.ports(&st.with) {
            Ok(p) => p,
            Err(e) => {
                rep.errors.push(sat(e.to_string()));
                continue;
            }
        };
        for p in ports {
            if steps.contains(p.reference.as_str()) && p.reference != st.id {
                if p.kind != PortKind::Indicators {
                    rep.errors.push(sat(format!(
                        "{} is a step (indicator rows) but the method expects {:?}",
                        p.reference, p.kind
                    )));
                }
                continue;
            }
            let Some(input) = s.inputs.get(&p.reference) else {
                rep.errors.push(sat(format!(
                    "{} is neither a declared input nor an earlier step",
                    p.reference
                )));
                continue;
            };
            let Some(d) = reg.datasets.get(&input.dataset) else {
                continue;
            };
            if d.kind != p.kind.dataset_kind() {
                let msg = format!(
                    "input {} ({}) is a {} dataset; {} expects {}",
                    p.reference,
                    input.dataset,
                    d.kind,
                    m.info().id,
                    p.kind.dataset_kind()
                );
                // Feature datasets are reshaped by WP-04 (0007_features): until a
                // dataset declares kind features this is advisory.
                if p.kind == PortKind::Features {
                    rep.warnings.push(at(msg));
                } else {
                    rep.errors.push(sat(msg));
                }
            }
            for ind in &p.indicators {
                let readable = if input.indicators.is_empty() {
                    d.indicators.contains(ind)
                } else {
                    input.indicators.contains(ind)
                };
                if !readable {
                    rep.errors.push(sat(format!(
                        "{ind} is not read from input {} ({})",
                        p.reference, input.dataset
                    )));
                }
            }
        }
    }
    if s.emit.is_empty() {
        rep.errors.push(at("emit names nothing".into()));
    }
    for e in &s.emit {
        for f in e.from.list() {
            if !steps.contains(f) {
                rep.errors.push(at(format!("emit from {f}: not a step")));
            }
        }
        let targets: Vec<&String> = e.indicator.iter().chain(e.map.values()).collect();
        for t in targets {
            if !out.indicators.contains(t) {
                rep.errors.push(at(format!(
                    "emit indicator {t} is not declared by {}",
                    s.output
                )));
            }
        }
    }
    for q in &s.qa {
        let name = q.get("check").and_then(|c| c.as_str()).unwrap_or_default();
        if !QA_CHECKS.contains(&name) {
            rep.errors.push(at(format!("unknown qa check {name:?}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> RegistryView {
        let mut v = RegistryView::default();
        v.datasets.insert(
            "in-a".into(),
            DatasetView {
                kind: "indicators".into(),
                indicators: ["x".to_owned()].into(),
            },
        );
        v.datasets.insert(
            "derived-a".into(),
            DatasetView {
                kind: "indicators".into(),
                indicators: ["dx".to_owned()].into(),
            },
        );
        v.datasets.insert(
            "derived-b".into(),
            DatasetView {
                kind: "indicators".into(),
                indicators: ["dy".to_owned()].into(),
            },
        );
        v
    }

    fn spec(id: &str, output: &str, input: &str, emit_as: &str) -> RecipeSpec {
        serde_json::from_value(serde_json::json!({
            "id": id, "output": output, "stability": "stable",
            "inputs": { "a": { "dataset": input } },
            "steps": [{ "id": "up", "method": "rollup@^1", "with": { "input": "a", "levels": [1] } }],
            "emit": [{ "from": "up", "indicator": emit_as }]
        }))
        .expect("spec")
    }

    #[test]
    fn checks_types_ports_and_the_dag() {
        let methods = Methods::builtin();
        let ok = Recipes {
            list: vec![
                LoadedRecipe {
                    spec: spec("derived-b", "derived-b", "derived-a", "dy"),
                    path: PathBuf::new(),
                },
                LoadedRecipe {
                    spec: spec("derived-a", "derived-a", "in-a", "dx"),
                    path: PathBuf::new(),
                },
            ],
        };
        let rep = ok.check(&view(), &methods);
        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
        assert_eq!(rep.order, vec!["derived-a", "derived-b"]);

        let mut bad = spec("derived-a", "derived-a", "in-a", "nope");
        bad.steps[0].with = serde_json::json!({ "input": "zz", "levels": "x" });
        let rep = Recipes {
            list: vec![LoadedRecipe {
                spec: bad,
                path: PathBuf::new(),
            }],
        }
        .check(&view(), &methods);
        let all = rep.errors.join("\n");
        assert!(all.contains("parameter"), "{all}");
        assert!(all.contains("neither a declared input"), "{all}");
        assert!(all.contains("not declared by derived-a"), "{all}");

        let cyc = Recipes {
            list: vec![
                LoadedRecipe {
                    spec: spec("derived-a", "derived-a", "derived-b", "dx"),
                    path: PathBuf::new(),
                },
                LoadedRecipe {
                    spec: spec("derived-b", "derived-b", "derived-a", "dy"),
                    path: PathBuf::new(),
                },
            ],
        }
        .check(&view(), &methods);
        assert!(
            cyc.errors.iter().any(|e| e.contains("cycle")),
            "{:?}",
            cyc.errors
        );
    }
}
