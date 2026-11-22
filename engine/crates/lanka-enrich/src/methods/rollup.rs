//! `rollup@1` — parent values from children by each indicator's
//! `aggregation` rule (02 §3: roll-up is data, not code).
//!
//! For every target level `T` (deepest first) and every series
//! `(indicator, period, dims)`, the children are the rows at `T + 1`: the
//! input's own rows when it has that level, else the rows this step just
//! computed for `T + 1`. A parent gets a value when its children cover at
//! least `min_coverage` (default 1.0) of its child units; below that it is
//! `missing` (no silent zero), and between `min_coverage` and 1 it carries
//! `PARTIAL_COVERAGE`. Rules: `sum` adds, `min`/`max`, `ratio_of_sums`
//! recomputes `Σ numerator / Σ denominator × scale` from the indicator's
//! numerator/denominator series, `weighted_mean` uses the weight series;
//! `none` is never rolled up. Intervals add as variances (independence
//! assumed) when every child has one.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use lanka_core::{Basis, Flags, Freq, Status};
use serde_json::{Value, json};

use crate::method::{
    Contrib, ContribKind, Method, MethodCtx, MethodInfo, MethodReport, OutRow, OutputSink,
    PortKind, PortSpec, PortUse, Role, Row, UncertaintyKind, Version, param_levels, param_strs,
};

pub struct Rollup;

static INFO: MethodInfo = MethodInfo {
    id: "rollup",
    version: Version::new(1, 0, 0),
    summary: "Parent values from children by the indicator's aggregation rule",
    citation: "SDMX aggregation conventions; 02-standard-model §3",
    inputs: &[PortSpec {
        name: "input",
        kind: PortKind::Indicators,
        required: true,
    }],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false, "required": ["input", "levels"],
      "properties": {
        "input": {"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "minItems": 1}]},
        "levels": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 1},
        "indicators": {"type": "array", "items": {"type": "string"}},
        "min_coverage": {"type": "number", "exclusiveMinimum": 0, "maximum": 1},
        "scales": {"type": "object", "additionalProperties": {"type": "number"}}
      }
    }"#,
    output_basis: Basis::Computed,
    uncertainty: UncertaintyKind::None,
    mandatory_flags: Flags::EMPTY,
    tools: &[],
    partial_recompute: false,
};

/// z for a two-sided interval at `level` (0.90 / 0.95 / 0.99; default 0.95).
pub(crate) fn z_of(level: Option<f32>) -> f64 {
    match level {
        Some(l) if (l - 0.90).abs() < 1e-3 => 1.644_853_6,
        Some(l) if (l - 0.99).abs() < 1e-3 => 2.575_829_3,
        _ => 1.959_964,
    }
}

type SeriesKey = (String, Freq, NaiveDate, String);

/// A child value available for a parent: its value, interval, contributors.
#[derive(Clone)]
struct Child {
    value: Option<f64>,
    lo: Option<f64>,
    hi: Option<f64>,
    contrib: Vec<Contrib>,
}

#[async_trait]
impl Method for Rollup {
    fn info(&self) -> &MethodInfo {
        &INFO
    }

    fn ports(&self, params: &Value) -> Result<Vec<PortUse>> {
        let inds = param_strs(params, "indicators");
        Ok(param_strs(params, "input")
            .into_iter()
            .map(|reference| PortUse {
                reference,
                kind: PortKind::Indicators,
                indicators: inds.clone(),
            })
            .collect())
    }

    #[allow(clippy::too_many_lines)]
    async fn run(
        &self,
        ctx: &MethodCtx<'_>,
        params: &Value,
        out: &mut dyn OutputSink,
    ) -> Result<MethodReport> {
        let ports = param_strs(params, "input");
        let mut levels = param_levels(params, "levels");
        levels.sort_unstable_by(|a, b| b.cmp(a));
        levels.dedup();
        let only: Vec<String> = param_strs(params, "indicators");
        let min_cov = params
            .get("min_coverage")
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        let scales: HashMap<String, f64> = params
            .get("scales")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default();
        let units = ctx.inputs.units().await?;

        // series → level → geo → child (input rows only, for now)
        let mut input: BTreeMap<SeriesKey, BTreeMap<i16, BTreeMap<String, Child>>> =
            BTreeMap::new();
        let mut indicators: BTreeMap<String, ()> = BTreeMap::new();
        for port in &ports {
            let rows = ctx.inputs.obs(port).await?;
            let port_name: Arc<str> = Arc::from(port.as_str());
            for (i, r) in rows.iter().enumerate() {
                if !only.is_empty() && !only.contains(&r.indicator) {
                    continue;
                }
                indicators.insert(r.indicator.clone(), ());
                let key = (r.indicator.clone(), r.freq, r.start, r.dims.clone());
                let slot = input.entry(key).or_default().entry(r.level).or_default();
                if slot.contains_key(&r.geo) {
                    bail!(
                        "rollup: {} {} {} {:?} appears twice across inputs",
                        r.indicator,
                        r.geo,
                        r.start,
                        r.dims
                    );
                }
                slot.insert(
                    r.geo.clone(),
                    Child {
                        value: r.value,
                        lo: r.lo,
                        hi: r.hi,
                        contrib: vec![Contrib {
                            port: port_name.clone(),
                            kind: ContribKind::Obs,
                            idx: u32::try_from(i)?,
                            role: Role::SourceUnit,
                            weight: 1.0,
                        }],
                    },
                );
            }
        }

        let mut skipped: BTreeMap<String, String> = BTreeMap::new();
        let mut emitted = 0u64;
        let mut partial = 0u64;
        let mut missing = 0u64;
        for ind in indicators.keys() {
            let meta = ctx
                .inputs
                .indicator(ind)
                .with_context(|| format!("rollup: indicator {ind:?} is not in the catalogue"))?;
            let rule = meta.aggregation.as_str();
            if rule == "none" {
                skipped.insert(ind.clone(), "aggregation none".into());
                continue;
            }
            if matches!(rule, "ratio_of_sums" | "weighted_mean") {
                let need = if rule == "ratio_of_sums" {
                    [meta.numerator.as_deref(), meta.denominator.as_deref()]
                } else {
                    [meta.weight.as_deref(), None]
                };
                if need.iter().flatten().any(|n| !indicators.contains_key(*n)) {
                    skipped.insert(
                        ind.clone(),
                        format!("{rule}: numerator/denominator/weight series not in the input"),
                    );
                    continue;
                }
            }
            let z = z_of(meta.ci_level);
            let series: Vec<SeriesKey> = input.keys().filter(|k| &k.0 == ind).cloned().collect();
            for sk in series {
                // computed[level][geo]
                let mut computed: BTreeMap<i16, BTreeMap<String, Child>> = BTreeMap::new();
                for &t in &levels {
                    let child_level = t + 1;
                    let children_of = |sk: &SeriesKey,
                                       computed: &BTreeMap<i16, BTreeMap<String, Child>>|
                     -> Option<BTreeMap<String, Child>> {
                        input
                            .get(sk)
                            .and_then(|m| m.get(&child_level))
                            .or_else(|| computed.get(&child_level))
                            .cloned()
                    };
                    let Some(children) = children_of(&sk, &computed) else {
                        continue;
                    };
                    let aux = |name: Option<&str>| -> Option<BTreeMap<String, Child>> {
                        let k = (name?.to_owned(), sk.1, sk.2, sk.3.clone());
                        input.get(&k).and_then(|m| m.get(&child_level)).cloned()
                    };
                    let (num, den) = match rule {
                        "ratio_of_sums" => (
                            aux(meta.numerator.as_deref()),
                            aux(meta.denominator.as_deref()),
                        ),
                        "weighted_mean" => (aux(meta.weight.as_deref()), None),
                        _ => (None, None),
                    };
                    let mut level_out: BTreeMap<String, Child> = BTreeMap::new();
                    for parent in units.at_level(t) {
                        let kids: Vec<&str> = units
                            .children(&parent.code)
                            .map(|u| u.code.as_str())
                            .collect();
                        if kids.is_empty() {
                            continue;
                        }
                        let present: Vec<(&str, &Child)> = kids
                            .iter()
                            .filter_map(|k| children.get(*k).map(|c| (*k, c)))
                            .collect();
                        if present.is_empty() {
                            continue;
                        }
                        #[allow(clippy::cast_precision_loss)]
                        let coverage = present.len() as f64 / kids.len() as f64;
                        let mut contrib: Vec<Contrib> = present
                            .iter()
                            .flat_map(|(_, c)| c.contrib.clone())
                            .collect();
                        let vals: Vec<f64> = present.iter().filter_map(|(_, c)| c.value).collect();
                        let all_valued = vals.len() == present.len();
                        let mut value = if coverage + 1e-12 < min_cov || !all_valued {
                            None
                        } else {
                            match rule {
                                "sum" => Some(vals.iter().sum()),
                                "min" => vals.iter().copied().reduce(f64::min),
                                "max" => vals.iter().copied().reduce(f64::max),
                                "ratio_of_sums" => {
                                    let (Some(n), Some(d)) = (&num, &den) else {
                                        unreachable!("checked above")
                                    };
                                    let mut sn = 0.0;
                                    let mut sd = 0.0;
                                    let mut ok = true;
                                    for (k, _) in &present {
                                        match (
                                            n.get(*k).and_then(|c| c.value),
                                            d.get(*k).and_then(|c| c.value),
                                        ) {
                                            (Some(a), Some(b)) => {
                                                sn += a;
                                                sd += b;
                                                contrib.extend(n[*k].contrib.iter().map(|c| {
                                                    Contrib {
                                                        role: Role::Numerator,
                                                        ..c.clone()
                                                    }
                                                }));
                                                contrib.extend(d[*k].contrib.iter().map(|c| {
                                                    Contrib {
                                                        role: Role::Denominator,
                                                        ..c.clone()
                                                    }
                                                }));
                                            }
                                            _ => ok = false,
                                        }
                                    }
                                    (ok && sd != 0.0)
                                        .then(|| sn / sd * scales.get(ind).copied().unwrap_or(1.0))
                                }
                                "weighted_mean" => {
                                    let Some(w) = &num else {
                                        unreachable!("checked above")
                                    };
                                    let mut sw = 0.0;
                                    let mut swx = 0.0;
                                    let mut ok = true;
                                    for (k, c) in &present {
                                        match (c.value, w.get(*k).and_then(|c| c.value)) {
                                            (Some(x), Some(wt)) => {
                                                sw += wt;
                                                swx += wt * x;
                                                contrib.extend(w[*k].contrib.iter().map(|c| {
                                                    Contrib {
                                                        weight: wt,
                                                        ..c.clone()
                                                    }
                                                }));
                                            }
                                            _ => ok = false,
                                        }
                                    }
                                    (ok && sw != 0.0).then(|| swx / sw)
                                }
                                other => bail!("rollup: unknown aggregation {other:?} for {ind}"),
                            }
                        };
                        let mut flags = Flags::EMPTY;
                        let mut status = Status::Normal;
                        if value.is_none() {
                            status = Status::Missing;
                            missing += 1;
                        } else if coverage < 1.0 {
                            flags = Flags::PARTIAL_COVERAGE;
                            partial += 1;
                        }
                        let (mut lo, mut hi) = (None, None);
                        if rule == "sum"
                            && let Some(v) = value
                            && present
                                .iter()
                                .all(|(_, c)| c.lo.is_some() && c.hi.is_some())
                            && present
                                .iter()
                                .any(|(_, c)| c.lo != c.value || c.hi != c.value)
                        {
                            let var: f64 = present
                                .iter()
                                .map(|(_, c)| {
                                    let h = (c.hi.unwrap_or_default() - c.lo.unwrap_or_default())
                                        / (2.0 * z);
                                    h * h
                                })
                                .sum();
                            lo = Some(v - z * var.sqrt());
                            hi = Some(v + z * var.sqrt());
                        }
                        if !value.is_some_and(f64::is_finite) {
                            value = None;
                        }
                        let row = Row {
                            lo: lo.filter(|_| value.is_some()),
                            hi: hi.filter(|_| value.is_some()),
                            status,
                            flags,
                            ..Row::computed(
                                ind.clone(),
                                parent.code.clone(),
                                t,
                                sk.1,
                                sk.2,
                                sk.3.clone(),
                                value,
                            )
                        };
                        level_out.insert(
                            parent.code.clone(),
                            Child {
                                value: row.value,
                                lo: row.lo,
                                hi: row.hi,
                                contrib: contrib.clone(),
                            },
                        );
                        out.row(OutRow { row, contrib })?;
                        emitted += 1;
                    }
                    computed.insert(t, level_out);
                }
            }
        }
        Ok(MethodReport {
            rows: emitted,
            diagnostics: json!({ "partial_coverage": partial, "missing": missing, "skipped": skipped }),
        })
    }

    fn formula(&self, _params: &Value, out: &OutRow) -> String {
        format!(
            "{}[{}] = Σ children at level {} ({} contributing rows)",
            out.row.indicator,
            out.row.geo,
            out.row.level + 1,
            out.contrib.len()
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use lanka_core::Calendars;
    use proptest::prelude::*;

    use super::*;
    use crate::method::{CollectSink, IndicatorMeta, MemInputs, Unit, Units};

    pub(crate) fn tree(fanout: &[usize]) -> Units {
        let mut list = vec![Unit {
            code: "ZZ".into(),
            level: 0,
            parent: None,
            name: "ZZ".into(),
            area_km2: Some(100.0),
        }];
        let mut frontier = vec!["ZZ".to_owned()];
        for (l, n) in fanout.iter().enumerate() {
            let mut next = Vec::new();
            for p in &frontier {
                for i in 0..*n {
                    let code = format!("{p}{}", i + 1);
                    list.push(Unit {
                        code: code.clone(),
                        level: i16::try_from(l + 1).expect("level"),
                        parent: Some(p.clone()),
                        name: code.clone(),
                        area_km2: Some(1.0),
                    });
                    next.push(code);
                }
            }
            frontier = next;
        }
        Units::new(list)
    }

    pub(crate) fn meta(id: &str, agg: &str) -> IndicatorMeta {
        IndicatorMeta {
            id: id.into(),
            dataset_id: "it".into(),
            freq: Freq::A,
            measure_type: "count".into(),
            aggregation: agg.into(),
            class: "official".into(),
            numerator: None,
            denominator: None,
            weight: None,
            ci_level: None,
            dims: vec![],
        }
    }

    fn d2024() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 1, 1).expect("date")
    }

    fn leaf_rows(units: &Units, level: i16, f: impl Fn(&str) -> f64) -> Vec<Row> {
        units
            .at_level(level)
            .map(|u| Row {
                basis: Basis::Reported,
                obs_id: Some(1),
                ..Row::computed(
                    "x",
                    u.code.clone(),
                    level,
                    Freq::A,
                    d2024(),
                    "sex=total",
                    Some(f(&u.code)),
                )
            })
            .collect()
    }

    fn inputs(units: Units, rows: Vec<Row>, agg: &str) -> MemInputs {
        MemInputs {
            obs: HashMap::from([("in".to_owned(), Arc::new(rows))]),
            units: Arc::new(units),
            indicators: HashMap::from([("x".to_owned(), meta("x", agg))]),
            calendars: Calendars::default(),
            ..MemInputs::default()
        }
    }

    async fn run(inp: &MemInputs, params: Value) -> Vec<OutRow> {
        let ctx = MethodCtx {
            inputs: inp,
            rng_seed: 0,
            as_of: d2024(),
            changed_keys: None,
        };
        let mut sink = CollectSink::default();
        Rollup.run(&ctx, &params, &mut sink).await.expect("run");
        sink.rows
    }

    #[tokio::test]
    async fn sums_two_levels_with_contributors() {
        let units = tree(&[2, 3]);
        let rows = leaf_rows(&units, 2, |c| c.len() as f64 + 1.0);
        let inp = inputs(units, rows, "sum");
        let out = run(&inp, json!({"input": "in", "levels": [1, 0]})).await;
        let zz = out.iter().find(|r| r.row.geo == "ZZ").expect("root");
        assert_eq!(zz.row.value, Some(6.0 * 5.0));
        assert_eq!(zz.contrib.len(), 6, "root lists its leaf contributors");
        assert_eq!(out.iter().filter(|r| r.row.level == 1).count(), 2);
    }

    #[tokio::test]
    async fn partial_children_are_missing_or_flagged() {
        let units = tree(&[2, 3]);
        let mut rows = leaf_rows(&units, 2, |_| 1.0);
        rows.retain(|r| r.geo != "ZZ13");
        let inp = inputs(units, rows, "sum");
        let out = run(&inp, json!({"input": "in", "levels": [1]})).await;
        let z1 = out.iter().find(|r| r.row.geo == "ZZ1").expect("ZZ1");
        assert_eq!((z1.row.value, z1.row.status), (None, Status::Missing));
        let out = run(
            &inp,
            json!({"input": "in", "levels": [1], "min_coverage": 0.5}),
        )
        .await;
        let z1 = out.iter().find(|r| r.row.geo == "ZZ1").expect("ZZ1");
        assert_eq!(z1.row.value, Some(2.0));
        assert!(z1.row.flags.contains(Flags::PARTIAL_COVERAGE));
        let z2 = out.iter().find(|r| r.row.geo == "ZZ2").expect("ZZ2");
        assert!(!z2.row.flags.contains(Flags::PARTIAL_COVERAGE));
    }

    #[tokio::test]
    async fn none_is_never_rolled_up() {
        let units = tree(&[2]);
        let rows = leaf_rows(&units, 1, |_| 1.0);
        let inp = inputs(units, rows, "none");
        assert!(
            run(&inp, json!({"input": "in", "levels": [0]}))
                .await
                .is_empty()
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// The roll-up of a `sum` indicator equals the sum of the leaves at
        /// every level, whatever the tree and the values, and is invariant to
        /// the order of the input rows.
        #[test]
        fn sum_rollup_equals_sum_of_leaves(
            fan in prop::collection::vec(1usize..4, 1..4),
            seed in prop::collection::vec(0u32..10_000, 1..200),
            reverse in any::<bool>(),
        ) {
            let units = tree(&fan);
            let deepest = i16::try_from(fan.len()).expect("depth");
            let mut rows = leaf_rows(&units, deepest, |_| 0.0);
            for (i, r) in rows.iter_mut().enumerate() {
                r.value = Some(f64::from(seed[i % seed.len()]));
            }
            let total: f64 = rows.iter().filter_map(|r| r.value).sum();
            if reverse { rows.reverse(); }
            let inp = inputs(units, rows, "sum");
            let levels: Vec<i16> = (0..deepest).collect();
            let rt = tokio::runtime::Builder::new_current_thread().build().expect("rt");
            let out = rt.block_on(run(&inp, json!({"input": "in", "levels": levels})));
            for l in 0..deepest {
                let s: f64 = out.iter().filter(|r| r.row.level == l).filter_map(|r| r.row.value).sum();
                prop_assert!((s - total).abs() <= 1e-9 * total.max(1.0), "level {l}: {s} vs {total}");
            }
        }
    }
}
