//! Executes one recipe over a set of inputs (04 §4–§6): steps in order,
//! the runner's propagation rules after every step, emit, QA gates; the
//! fingerprint that decides staleness; and `explain` for one output key.
//!
//! Publication (staging the emitted rows as a dataset version, recording the
//! derivation, gating) is done by `lanka-ingest::derive`, which owns the
//! staging path; this module never writes to Postgres.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use lanka_core::{Calendars, Dims};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::method::{
    Explanation, Feature, GridInput, IndicatorMeta, Inputs, MethodCtx, MethodReport, Row, RowKey,
    Units,
};
use crate::methods::Methods;
use crate::propagate;
use crate::recipe::RecipeSpec;

/// Inputs of one step: the recipe's inputs plus the outputs of earlier steps.
pub struct StepInputs<'a> {
    pub base: &'a dyn Inputs,
    pub steps: HashMap<String, Arc<Vec<Row>>>,
}

#[async_trait]
impl Inputs for StepInputs<'_> {
    async fn obs(&self, port: &str) -> Result<Arc<Vec<Row>>> {
        match self.steps.get(port) {
            Some(rows) => Ok(rows.clone()),
            None => self.base.obs(port).await,
        }
    }
    async fn features(&self, port: &str) -> Result<Arc<Vec<Feature>>> {
        self.base.features(port).await
    }
    async fn units(&self) -> Result<Arc<Units>> {
        self.base.units().await
    }
    async fn grid(&self, port: &str) -> Result<Arc<GridInput>> {
        self.base.grid(port).await
    }
    fn indicator(&self, id: &str) -> Option<IndicatorMeta> {
        self.base.indicator(id)
    }
    fn calendars(&self) -> &Calendars {
        self.base.calendars()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StepRun {
    pub id: String,
    /// `id@version` actually used.
    pub method: String,
    pub rows: u64,
    pub ms: u128,
    pub report: MethodReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct QaResult {
    pub check: String,
    pub passed: bool,
    pub detail: Value,
}

#[derive(Debug, Clone)]
pub struct RecipeOutput {
    /// Emitted rows, output indicator ids, sorted by key.
    pub rows: Vec<Row>,
    pub steps: Vec<StepRun>,
    pub qa: Vec<QaResult>,
    /// Step rows not named by any emit.
    pub not_emitted: u64,
}

impl RecipeOutput {
    #[must_use]
    pub fn qa_passed(&self) -> bool {
        self.qa.iter().all(|q| q.passed)
    }
}

/// `[(step, method id, version)]` the recipe resolves to.
pub fn resolve_methods(
    spec: &RecipeSpec,
    methods: &Methods,
) -> Result<Vec<(String, String, String)>> {
    spec.steps
        .iter()
        .map(|s| {
            let m = methods
                .resolve(&s.method)
                .with_context(|| format!("recipe {} step {}", spec.id, s.id))?;
            Ok((
                s.id.clone(),
                m.info().id.to_owned(),
                m.info().version.to_string(),
            ))
        })
        .collect()
}

/// One pinned input as it enters the fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FpInput {
    pub port: String,
    pub dataset: String,
    pub dsv: String,
}

/// sha256(recipe spec ‖ output dataset spec ‖ method ids@versions ‖ tool
/// versions ‖ sorted input dsvs) — 04 §1. Parameters are part of the spec.
#[must_use]
pub fn fingerprint(
    spec: &RecipeSpec,
    output_spec_sha256: &[u8],
    methods: &[(String, String, String)],
    tools: &BTreeMap<String, String>,
    inputs: &[FpInput],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"lanka-recipe-fp/1\x1f");
    h.update(spec.sha256());
    h.update(b"\x1f");
    h.update(output_spec_sha256);
    for (step, id, v) in methods {
        h.update(format!("\x1fm\x1f{step}\x1f{id}@{v}").as_bytes());
    }
    for (t, v) in tools {
        h.update(format!("\x1ft\x1f{t}\x1f{v}").as_bytes());
    }
    let mut ins = inputs.to_vec();
    ins.sort();
    for i in ins {
        h.update(format!("\x1fi\x1f{}\x1f{}\x1f{}", i.port, i.dataset, i.dsv).as_bytes());
    }
    h.finalize().into()
}

/// Runs the steps of `spec` up to and including `until` (all when None).
/// Returns the step tables and reports.
async fn run_steps(
    spec: &RecipeSpec,
    methods: &Methods,
    base: &dyn Inputs,
    seed: u64,
    until: Option<&str>,
) -> Result<(HashMap<String, Arc<Vec<Row>>>, Vec<StepRun>)> {
    let mut tables: HashMap<String, Arc<Vec<Row>>> = HashMap::new();
    let mut runs = Vec::new();
    for st in &spec.steps {
        let m = methods.resolve(&st.method)?;
        let inputs = StepInputs {
            base,
            steps: tables.clone(),
        };
        let ctx = MethodCtx {
            inputs: &inputs,
            rng_seed: seed,
            as_of: spec.as_of(),
            changed_keys: None,
        };
        let t0 = std::time::Instant::now();
        let mut sink = crate::method::CollectSink::default();
        let report = m
            .run(&ctx, &st.with, &mut sink)
            .await
            .with_context(|| format!("recipe {} step {} ({})", spec.id, st.id, st.method))?;
        let mut rows = sink.rows;
        propagate::enforce(m.info(), &inputs, &mut rows)
            .await
            .with_context(|| format!("recipe {} step {}: propagation", spec.id, st.id))?;
        runs.push(StepRun {
            id: st.id.clone(),
            method: m.info().reference(),
            rows: rows.len() as u64,
            ms: t0.elapsed().as_millis(),
            report,
        });
        tables.insert(
            st.id.clone(),
            Arc::new(rows.into_iter().map(|o| o.row).collect()),
        );
        if until == Some(st.id.as_str()) {
            break;
        }
    }
    Ok((tables, runs))
}

/// The emitted form of a step row: output indicator id, and the period
/// re-expressed in the output indicator's frequency (a census snapshot
/// `2024` feeding a yearly indicator becomes the year 2024).
fn emit_row(r: &Row, target: &str, base: &dyn Inputs) -> Result<Row> {
    let mut row = Row {
        indicator: target.to_owned(),
        obs_id: None,
        ..r.clone()
    };
    if let Some(meta) = base.indicator(target)
        && meta.freq != r.freq
    {
        let cal = base.calendars();
        let text = r.period(cal);
        let p = lanka_core::Period::parse(meta.freq, &text, cal).with_context(|| {
            format!(
                "{target}: period {text:?} ({}) has no {} equivalent",
                r.freq, meta.freq
            )
        })?;
        row.freq = p.freq;
        row.start = p.start;
    }
    Ok(row)
}

/// Executes `spec` over `base` and evaluates its QA gates.
pub async fn execute(
    spec: &RecipeSpec,
    methods: &Methods,
    base: &dyn Inputs,
    seed: u64,
) -> Result<RecipeOutput> {
    let (tables, steps) = run_steps(spec, methods, base, seed, None).await?;
    let mut out: BTreeMap<RowKey, Row> = BTreeMap::new();
    let mut not_emitted = 0u64;
    let mut emitted_steps: BTreeMap<&str, u64> = BTreeMap::new();
    for e in &spec.emit {
        for from in e.from.list() {
            let rows = tables
                .get(from)
                .with_context(|| format!("emit from unknown step {from}"))?;
            if e.indicator.is_some() {
                let distinct: std::collections::BTreeSet<&str> =
                    rows.iter().map(|r| r.indicator.as_str()).collect();
                if distinct.len() > 1 {
                    bail!(
                        "recipe {}: emit {from} as one indicator but the step produced {}: use map",
                        spec.id,
                        distinct.into_iter().collect::<Vec<_>>().join(", ")
                    );
                }
            }
            for r in rows.iter() {
                let Some(target) = e.target(&r.indicator) else {
                    not_emitted += 1;
                    continue;
                };
                let row = emit_row(r, target, base)?;
                *emitted_steps.entry(from).or_default() += 1;
                if out.insert(row.key(), row).is_some() {
                    bail!(
                        "recipe {}: two emitted rows share the key {target} {} {} {:?}",
                        spec.id,
                        r.geo,
                        r.start,
                        r.dims
                    );
                }
            }
        }
    }
    let rows: Vec<Row> = out.into_values().collect();
    let qa = run_qa(spec, &rows, base).await?;
    Ok(RecipeOutput {
        rows,
        steps,
        qa,
        not_emitted,
    })
}

async fn run_qa(spec: &RecipeSpec, rows: &[Row], base: &dyn Inputs) -> Result<Vec<QaResult>> {
    let mut out = Vec::new();
    for q in &spec.qa {
        let check = q.get("check").and_then(Value::as_str).unwrap_or_default();
        let ind = q.get("indicator").and_then(Value::as_str);
        let sel = |r: &&Row| ind.is_none_or(|i| r.indicator == i);
        let res = match check {
            "non_negative" => {
                let bad: Vec<String> = rows
                    .iter()
                    .filter(sel)
                    .filter(|r| r.value.is_some_and(|v| v < 0.0))
                    .take(5)
                    .map(|r| format!("{} {} = {:?}", r.indicator, r.geo, r.value))
                    .collect();
                QaResult {
                    check: check.into(),
                    passed: bad.is_empty(),
                    detail: json!({ "negative": bad }),
                }
            }
            "min_rows" => {
                let n = rows.iter().filter(sel).count() as u64;
                let min = q.get("n").and_then(Value::as_u64).unwrap_or(1);
                QaResult {
                    check: check.into(),
                    passed: n >= min,
                    detail: json!({ "rows": n, "min": min }),
                }
            }
            "no_missing" => {
                let n = rows.iter().filter(sel).count();
                let miss = rows
                    .iter()
                    .filter(sel)
                    .filter(|r| r.value.is_none())
                    .count();
                let max = q.get("max_share").and_then(Value::as_f64).unwrap_or(0.0);
                #[allow(clippy::cast_precision_loss)]
                let share = if n == 0 { 0.0 } else { miss as f64 / n as f64 };
                QaResult {
                    check: check.into(),
                    passed: share <= max,
                    detail: json!({ "missing": miss, "rows": n, "max_share": max }),
                }
            }
            "sum_matches" => sum_matches(q, rows, base).await?,
            other => bail!("recipe {}: unknown qa check {other:?}", spec.id),
        };
        out.push(res);
    }
    Ok(out)
}

/// `{check: sum_matches, indicator, level, input, input_indicator,
/// input_level, tol_pct}`: Σ emitted rows at `level` equals Σ input rows at
/// `input_level`, per period and dims (e.g. provinces sum to the printed
/// national total).
async fn sum_matches(q: &Value, rows: &[Row], base: &dyn Inputs) -> Result<QaResult> {
    let get_s = |k: &str| q.get(k).and_then(Value::as_str);
    let get_l = |k: &str| {
        q.get(k)
            .and_then(Value::as_i64)
            .and_then(|l| i16::try_from(l).ok())
    };
    let (Some(ind), Some(level), Some(port), Some(in_ind), Some(in_level)) = (
        get_s("indicator"),
        get_l("level"),
        get_s("input"),
        get_s("input_indicator"),
        get_l("input_level"),
    ) else {
        bail!("sum_matches needs indicator, level, input, input_indicator, input_level");
    };
    let tol = q.get("tol_pct").and_then(Value::as_f64).unwrap_or(0.5);
    let mut ours: BTreeMap<(chrono::NaiveDate, String), f64> = BTreeMap::new();
    for r in rows
        .iter()
        .filter(|r| r.indicator == ind && r.level == level)
    {
        *ours.entry((r.start, r.dims.clone())).or_default() += r.value.unwrap_or(0.0);
    }
    let theirs: BTreeMap<(chrono::NaiveDate, String), f64> = base
        .obs(port)
        .await?
        .iter()
        .filter(|r| r.indicator == in_ind && r.level == in_level)
        .filter_map(|r| Some(((r.start, r.dims.clone()), r.value?)))
        .collect();
    let mut worst = 0.0f64;
    let mut compared = 0;
    let mut failures = Vec::new();
    for (k, want) in &theirs {
        let Some(got) = ours.get(k) else { continue };
        compared += 1;
        let pct = if *want == 0.0 {
            if *got == 0.0 { 0.0 } else { 100.0 }
        } else {
            ((got - want) / want).abs() * 100.0
        };
        worst = worst.max(pct);
        if pct > tol && failures.len() < 5 {
            failures.push(json!({ "dims": k.1, "period": k.0, "sum": got, "expected": want }));
        }
    }
    Ok(QaResult {
        check: "sum_matches".into(),
        passed: failures.is_empty() && compared > 0,
        detail: json!({ "compared": compared, "worst_pct": worst, "tol_pct": tol, "failures": failures }),
    })
}

/// Explains one emitted row: finds the step whose emitted rows contain
/// `key`, re-runs the steps up to it over the same inputs and resolves that
/// row's contributors.
pub async fn explain(
    spec: &RecipeSpec,
    methods: &Methods,
    base: &dyn Inputs,
    seed: u64,
    key: &RowKey,
) -> Result<Explanation> {
    let candidates: Vec<(&str, &crate::recipe::EmitSpec)> = spec
        .emit
        .iter()
        .flat_map(|e| e.from.list().into_iter().map(move |f| (f, e)))
        .filter(|(_, e)| {
            e.indicator.as_deref() == Some(key.indicator.as_str())
                || e.map.values().any(|v| *v == key.indicator)
                || (e.indicator.is_none() && e.map.is_empty())
        })
        .collect();
    anyhow::ensure!(
        !candidates.is_empty(),
        "recipe {} does not emit {}",
        spec.id,
        key.indicator
    );
    for (step_id, emit) in candidates {
        let pos = spec
            .steps
            .iter()
            .position(|s| s.id == step_id)
            .context("emit step")?;
        let (tables, _) = if pos == 0 {
            (HashMap::new(), Vec::new())
        } else {
            run_steps(spec, methods, base, seed, Some(&spec.steps[pos - 1].id)).await?
        };
        let st = &spec.steps[pos];
        let m = methods.resolve(&st.method)?;
        let inputs = StepInputs {
            base,
            steps: tables,
        };
        let ctx = MethodCtx {
            inputs: &inputs,
            rng_seed: seed,
            as_of: spec.as_of(),
            changed_keys: None,
        };
        let mut sink = crate::method::CollectSink::default();
        m.run(&ctx, &st.with, &mut sink).await?;
        let mut rows = sink.rows;
        propagate::enforce(m.info(), &inputs, &mut rows).await?;
        for out in rows {
            let Some(target) = emit.target(&out.row.indicator) else {
                continue;
            };
            if target != key.indicator {
                continue;
            }
            let emitted = emit_row(&out.row, target, base)?;
            if emitted.key() != *key {
                continue;
            }
            let (contributors, truncated) = crate::method::contributors(&inputs, &out).await?;
            let formula = m
                .formula(&st.with, &out)
                .replacen(&out.row.indicator, &key.indicator, 1);
            return Ok(Explanation {
                method: m.info().reference(),
                key: key.clone(),
                value: emitted.value,
                formula,
                contributors,
                truncated,
            });
        }
    }
    bail!("recipe {} produced no row for {key:?}", spec.id)
}

/// Canonical dims text (helper for callers building keys from user input).
pub fn canonical_dims(s: &str) -> Result<String> {
    Ok(Dims::parse(s)?.canonical())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;
    use lanka_core::{Basis, Flags, Freq, Status};

    use super::*;
    use crate::method::MemInputs;
    use crate::methods::rollup::tests::{meta, tree};

    fn spec() -> RecipeSpec {
        serde_json::from_value(json!({
            "id": "derived-t", "output": "derived-t", "stability": "stable",
            "inputs": { "census": { "dataset": "c" }, "admin": { "dataset": "b" } },
            "steps": [
                { "id": "up", "method": "rollup@^1", "with": { "input": "census", "levels": [1, 0] } },
                { "id": "area", "method": "unit_metrics@^1",
                  "with": { "boundaries": "admin", "period": "2024", "area": "area", "levels": [0, 1] } },
                { "id": "dens", "method": "ratio@^1", "with": {
                    "terms": { "p": { "from": [{ "port": "up", "indicator": "pop" }] },
                               "a": { "from": [{ "port": "area", "indicator": "area" }] } },
                    "formula": "p / a", "indicator": "dens", "match_period": { "nearest": { "max_years": 1 } } } }
            ],
            "emit": [ { "from": "up", "map": { "pop": "out.pop" } }, { "from": "dens", "indicator": "out.dens" } ],
            "qa": [ { "check": "sum_matches", "indicator": "out.pop", "level": 1, "input": "census",
                      "input_indicator": "pop", "input_level": 0, "tol_pct": 0.5 },
                    { "check": "non_negative" } ]
        }))
        .expect("spec")
    }

    fn inputs(national: f64, flag: Flags, status: Status) -> MemInputs {
        let units = tree(&[2, 2]);
        let d = NaiveDate::from_ymd_opt(2024, 1, 1).expect("d");
        let mut rows: Vec<Row> = units
            .at_level(2)
            .map(|u| Row {
                basis: Basis::Reported,
                obs_id: Some(100),
                ..Row::computed("pop", u.code.clone(), 2, Freq::A, d, "", Some(10.0))
            })
            .collect();
        rows[0].flags = flag;
        rows[0].status = status;
        rows.push(Row {
            basis: Basis::Reported,
            obs_id: Some(1),
            ..Row::computed("pop", "ZZ", 0, Freq::A, d, "", Some(national))
        });
        MemInputs {
            obs: HashMap::from([("census".to_owned(), Arc::new(rows))]),
            units: Arc::new(units),
            indicators: HashMap::from([
                ("pop".to_owned(), meta("pop", "sum")),
                ("area".to_owned(), meta("area", "sum")),
            ]),
            ..MemInputs::default()
        }
    }

    #[tokio::test]
    async fn steps_chain_emit_and_gate() {
        let inp = inputs(40.0, Flags::EMPTY, Status::Normal);
        let out = execute(&spec(), &Methods::builtin(), &inp, 0)
            .await
            .expect("run");
        assert!(out.qa_passed(), "{:?}", out.qa);
        let get = |i: &str, g: &str| out.rows.iter().find(|r| r.indicator == i && r.geo == g);
        assert_eq!(get("out.pop", "ZZ1").and_then(|r| r.value), Some(20.0));
        // ZZ1 has area 1.0 (tree fixture), 20 people
        assert_eq!(get("out.dens", "ZZ1").and_then(|r| r.value), Some(20.0));
        assert!(get("out.dens", "ZZ1").is_some_and(|r| r.basis == Basis::Computed));

        let bad = inputs(41.0, Flags::EMPTY, Status::Normal);
        let out = execute(&spec(), &Methods::builtin(), &bad, 0)
            .await
            .expect("run");
        assert!(
            !out.qa_passed(),
            "the provinces no longer sum to the national total"
        );
    }

    #[tokio::test]
    async fn propagation_flows_through_steps() {
        let inp = inputs(40.0, Flags::DISAGGREGATED, Status::Provisional);
        let out = execute(&spec(), &Methods::builtin(), &inp, 0)
            .await
            .expect("run");
        let dens = out
            .rows
            .iter()
            .find(|r| r.indicator == "out.dens" && r.geo == "ZZ1")
            .expect("dens");
        assert!(
            dens.flags.contains(Flags::DISAGGREGATED),
            "flag through rollup → ratio"
        );
        assert_eq!(dens.status, Status::Provisional);
        let other = out
            .rows
            .iter()
            .find(|r| r.indicator == "out.dens" && r.geo == "ZZ2")
            .expect("dens");
        assert!(!other.flags.contains(Flags::DISAGGREGATED));
    }

    #[tokio::test]
    async fn explain_returns_contributors() {
        let inp = inputs(40.0, Flags::EMPTY, Status::Normal);
        let s = spec();
        let key = RowKey {
            indicator: "out.pop".into(),
            geo: "ZZ1".into(),
            freq: Freq::A,
            start: NaiveDate::from_ymd_opt(2024, 1, 1).expect("d"),
            dims: String::new(),
        };
        let e = explain(&s, &Methods::builtin(), &inp, 0, &key)
            .await
            .expect("explain");
        assert_eq!(e.value, Some(20.0));
        assert_eq!(e.contributors.len(), 2);
        assert!(e.contributors.iter().all(|c| c.obs_id == Some(100)));
        assert!(e.formula.starts_with("out.pop[ZZ1]"), "{}", e.formula);
        let key = RowKey {
            indicator: "out.dens".into(),
            ..key
        };
        let e = explain(&s, &Methods::builtin(), &inp, 0, &key)
            .await
            .expect("explain");
        assert_eq!(
            e.contributors.len(),
            2,
            "numerator from step up, area from step area"
        );
        assert!(e.contributors.iter().any(|c| c.port == "up"));
    }

    #[test]
    fn fingerprint_changes_with_any_input() {
        let s = spec();
        let m = resolve_methods(&s, &Methods::builtin()).expect("methods");
        let ins = vec![FpInput {
            port: "census".into(),
            dataset: "c".into(),
            dsv: "dsv_1".into(),
        }];
        let base = fingerprint(&s, b"x", &m, &BTreeMap::new(), &ins);
        assert_eq!(base, fingerprint(&s, b"x", &m, &BTreeMap::new(), &ins));
        let mut ins2 = ins.clone();
        ins2[0].dsv = "dsv_2".into();
        assert_ne!(base, fingerprint(&s, b"x", &m, &BTreeMap::new(), &ins2));
        assert_ne!(base, fingerprint(&s, b"y", &m, &BTreeMap::new(), &ins));
        let tools = BTreeMap::from([("gdal".to_owned(), "3.11.0".to_owned())]);
        assert_ne!(base, fingerprint(&s, b"x", &m, &tools, &ins));
        let mut s2 = s.clone();
        s2.steps[0].with["min_coverage"] = json!(0.9);
        assert_ne!(base, fingerprint(&s2, b"x", &m, &BTreeMap::new(), &ins));
    }
}
