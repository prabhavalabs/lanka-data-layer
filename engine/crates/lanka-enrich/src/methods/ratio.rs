//! `ratio@1` — indicator arithmetic: ratio, share, density, per-N, change.
//!
//! ```yaml
//! with:
//!   terms:
//!     pop:  { from: [{ port: census, indicator: census24.pop_total }], select: "sex=total" }
//!     area: { from: [{ port: admin,  indicator: admin.area_km2 }] }
//!   formula: pop / area            # expr.rs grammar over the term names
//!   key: pop                       # the term whose rows define the output keys (default: leftmost in the formula)
//!   indicator: derived.per_km2     # output indicator (the recipe's emit may rename)
//!   set_dims: "source_indicator=census24.pop_total"
//!   match_period: exact            # or { nearest: { max_years: 5 } }
//!   levels: [1, 2, 3]
//! ```
//!
//! A term row matches an output key at the same geography when every
//! dimension pair it has left after `select` is in the key row's remaining
//! dimensions (so an undimensioned denominator broadcasts). The output
//! carries the key row's remaining dimensions plus `set_dims`, and the key
//! row's period; a term from another period sets `DENOM_MISMATCH`. A key
//! row with a term missing, or a zero divisor, gives a `missing` row — never
//! a silent zero. Intervals of inputs propagate by the delta method.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use lanka_core::{Basis, Dims, Flags, Status};
use serde_json::{Value, json};

use crate::expr::Expr;
use crate::method::{
    Contrib, ContribKind, Method, MethodCtx, MethodInfo, MethodReport, OutRow, OutputSink,
    PortKind, PortSpec, PortUse, Role, Row, UncertaintyKind, Version, param_levels, param_str,
};
use crate::methods::rollup::z_of;

pub struct Ratio;

static INFO: MethodInfo = MethodInfo {
    id: "ratio",
    version: Version::new(1, 0, 0),
    summary: "Indicator arithmetic: ratio, share, density, per-N, change, growth",
    citation: "Delta method for intervals: Oehlert 1992, The American Statistician 46(1)",
    inputs: &[PortSpec {
        name: "terms",
        kind: PortKind::Indicators,
        required: true,
    }],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false, "required": ["terms", "formula", "indicator"],
      "properties": {
        "terms": {"type": "object", "minProperties": 1,
          "propertyNames": {"pattern": "^[a-z_][a-z0-9_]*$"},
          "additionalProperties": {"type": "object", "additionalProperties": false, "required": ["from"],
            "properties": {
              "from": {"type": "array", "minItems": 1, "items": {"type": "object", "additionalProperties": false,
                "required": ["port", "indicator"],
                "properties": {"port": {"type": "string"}, "indicator": {"type": "string"}}}},
              "select": {"type": "string"}}}},
        "formula": {"type": "string", "minLength": 1},
        "key": {"type": "string"},
        "indicator": {"type": "string"},
        "set_dims": {"type": "string"},
        "match_period": {"oneOf": [{"const": "exact"},
          {"type": "object", "additionalProperties": false, "required": ["nearest"],
           "properties": {"nearest": {"type": "object", "additionalProperties": false, "required": ["max_years"],
             "properties": {"max_years": {"type": "integer", "minimum": 0}}}}}]},
        "levels": {"type": "array", "items": {"type": "integer", "minimum": 0}}
      }
    }"#,
    output_basis: Basis::Computed,
    uncertainty: UncertaintyKind::None,
    mandatory_flags: Flags::EMPTY,
    tools: &[],
    partial_recompute: false,
};

struct Term {
    name: String,
    sources: Vec<(String, String)>,
    select: Dims,
}

fn terms(params: &Value) -> Result<Vec<Term>> {
    let obj = params
        .get("terms")
        .and_then(Value::as_object)
        .context("ratio: terms is required")?;
    obj.iter()
        .map(|(name, t)| {
            let sources = t
                .get("from")
                .and_then(Value::as_array)
                .context("ratio: term needs from")?
                .iter()
                .map(|s| {
                    Ok((
                        param_str(s, "port")?.to_owned(),
                        param_str(s, "indicator")?.to_owned(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let select = match t.get("select").and_then(Value::as_str) {
                Some(s) => Dims::parse(s).with_context(|| format!("term {name}: select"))?,
                None => Dims::new(),
            };
            Ok(Term {
                name: name.clone(),
                sources,
                select,
            })
        })
        .collect()
}

/// A candidate row of one term at one geography.
struct Cand {
    port: Arc<str>,
    idx: u32,
    rest: Dims,
    start: NaiveDate,
    level: i16,
    freq: lanka_core::Freq,
    value: Option<f64>,
    lo: Option<f64>,
    hi: Option<f64>,
}

fn subset(small: &Dims, big: &Dims) -> bool {
    small.iter().all(|(k, v)| big.get(k) == Some(v))
}

#[async_trait]
impl Method for Ratio {
    fn info(&self) -> &MethodInfo {
        &INFO
    }

    fn ports(&self, params: &Value) -> Result<Vec<PortUse>> {
        let mut by_port: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for t in terms(params)? {
            for (p, i) in t.sources {
                by_port.entry(p).or_default().push(i);
            }
        }
        Ok(by_port
            .into_iter()
            .map(|(reference, indicators)| PortUse {
                reference,
                kind: PortKind::Indicators,
                indicators,
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
        let terms = terms(params)?;
        let names: Vec<String> = terms.iter().map(|t| t.name.clone()).collect();
        let formula = Expr::parse(param_str(params, "formula")?, &names)?;
        let key_name = match params.get("key").and_then(Value::as_str) {
            Some(k) => k.to_owned(),
            None => names[formula.first_var().context("ratio: formula uses no term")?].clone(),
        };
        let key_i = names
            .iter()
            .position(|n| *n == key_name)
            .with_context(|| format!("ratio: key {key_name:?} is not a term"))?;
        let indicator = param_str(params, "indicator")?.to_owned();
        let set_dims = match params.get("set_dims").and_then(Value::as_str) {
            Some(s) => Dims::parse(s).context("set_dims")?,
            None => Dims::new(),
        };
        let max_days: i64 = match params.get("match_period") {
            None => 0,
            Some(Value::String(s)) if s == "exact" => 0,
            Some(v) => v
                .pointer("/nearest/max_years")
                .and_then(Value::as_i64)
                .context("match_period")?
                .saturating_mul(366),
        };
        let levels = param_levels(params, "levels");

        // term → geo → candidates
        let mut table: Vec<HashMap<String, Vec<Cand>>> = Vec::with_capacity(terms.len());
        let mut z = Vec::with_capacity(terms.len());
        for t in &terms {
            let mut m: HashMap<String, Vec<Cand>> = HashMap::new();
            for (port, ind) in &t.sources {
                z.push(z_of(ctx.inputs.indicator(ind).and_then(|i| i.ci_level)));
                let rows = ctx.inputs.obs(port).await?;
                let pname: Arc<str> = Arc::from(port.as_str());
                for (i, r) in rows.iter().enumerate() {
                    if r.indicator != *ind || (!levels.is_empty() && !levels.contains(&r.level)) {
                        continue;
                    }
                    let dims = Dims::parse(&r.dims)?;
                    if !subset(&t.select, &dims) {
                        continue;
                    }
                    let rest = Dims::from_pairs(
                        dims.iter()
                            .filter(|(k, _)| t.select.get(k).is_none())
                            .collect::<Vec<_>>(),
                    )?;
                    m.entry(r.geo.clone()).or_default().push(Cand {
                        port: pname.clone(),
                        idx: u32::try_from(i)?,
                        rest,
                        start: r.start,
                        level: r.level,
                        freq: r.freq,
                        value: r.value,
                        lo: r.lo,
                        hi: r.hi,
                    });
                }
            }
            table.push(m);
        }
        let z = z.into_iter().fold(1.959_964_f64, f64::max);

        // (geo, period, remaining dims, candidate index): output order is
        // independent of input order
        let mut keys: Vec<(String, NaiveDate, String, usize)> = Vec::new();
        for (geo, cands) in &table[key_i] {
            for (ci, c) in cands.iter().enumerate() {
                keys.push((geo.clone(), c.start, c.rest.canonical(), ci));
            }
        }
        keys.sort();
        let mut emitted = 0u64;
        let mut missing = 0u64;
        let mut mismatched = 0u64;
        let mut seen: BTreeMap<(String, NaiveDate, String), ()> = BTreeMap::new();
        for (geo, _, _, ci) in keys {
            let k = &table[key_i][&geo][ci];
            let (level, freq) = (k.level, k.freq);
            let mut x = vec![f64::NAN; terms.len()];
            let mut sd = vec![0.0; terms.len()];
            let mut contrib = Vec::with_capacity(terms.len());
            let mut flags = Flags::EMPTY;
            let mut complete = true;
            for (ti, m) in table.iter().enumerate() {
                let pick: Option<&Cand> = if ti == key_i {
                    Some(k)
                } else {
                    let cands: Vec<&Cand> = m
                        .get(&geo)
                        .map(|v| v.iter().filter(|c| subset(&c.rest, &k.rest)).collect())
                        .unwrap_or_default();
                    // nearest period within the window (ties: the earlier one),
                    // then the most specific dimensions
                    let best = cands
                        .iter()
                        .map(|c| ((c.start - k.start).num_days().abs(), c.start))
                        .filter(|(d, _)| *d <= max_days)
                        .min();
                    let at: Vec<&Cand> = match best {
                        Some((_, s)) => cands.into_iter().filter(|c| c.start == s).collect(),
                        None => Vec::new(),
                    };
                    let most = at.iter().map(|c| c.rest.iter().count()).max();
                    let at: Vec<&Cand> = at
                        .into_iter()
                        .filter(|c| Some(c.rest.iter().count()) == most)
                        .collect();
                    if at.len() > 1 {
                        bail!(
                            "ratio: term {} has {} candidate rows for {geo} {}; use select",
                            terms[ti].name,
                            at.len(),
                            k.start
                        );
                    }
                    at.into_iter().next()
                };
                let Some(c) = pick else {
                    complete = false;
                    continue;
                };
                if c.start != k.start {
                    flags = flags.union(Flags::DENOM_MISMATCH);
                }
                match c.value {
                    Some(v) => x[ti] = v,
                    None => complete = false,
                }
                if let (Some(lo), Some(hi)) = (c.lo, c.hi) {
                    sd[ti] = (hi - lo) / (2.0 * z);
                }
                contrib.push(Contrib {
                    port: c.port.clone(),
                    kind: ContribKind::Obs,
                    idx: c.idx,
                    role: if ti == key_i {
                        Role::Numerator
                    } else {
                        Role::Denominator
                    },
                    weight: 1.0,
                });
            }
            let value = if complete { formula.eval(&x) } else { None };
            let (lo, hi) = match value {
                Some(v) if sd.iter().any(|s| *s > 0.0) => match formula.delta_sd(&x, &sd) {
                    Some(s) => (Some(v - z * s), Some(v + z * s)),
                    None => (None, None),
                },
                _ => (None, None),
            };
            let mut dims = k.rest.clone();
            for (a, b) in set_dims.iter() {
                dims = Dims::from_pairs(
                    dims.iter()
                        .filter(|(k, _)| *k != a)
                        .chain(std::iter::once((a, b)))
                        .collect::<Vec<_>>(),
                )?;
            }
            let dims = dims.canonical();
            if seen
                .insert((geo.clone(), k.start, dims.clone()), ())
                .is_some()
            {
                bail!(
                    "ratio: two key rows map to {geo} {} {dims:?}; use select",
                    k.start
                );
            }
            if value.is_none() {
                missing += 1;
            }
            if flags.contains(Flags::DENOM_MISMATCH) {
                mismatched += 1;
            }
            let row = Row {
                lo,
                hi,
                flags,
                status: if value.is_some() {
                    Status::Normal
                } else {
                    Status::Missing
                },
                ..Row::computed(
                    indicator.clone(),
                    geo.clone(),
                    level,
                    freq,
                    k.start,
                    dims,
                    value,
                )
            };
            out.row(OutRow { row, contrib })?;
            emitted += 1;
        }
        Ok(MethodReport {
            rows: emitted,
            diagnostics: json!({ "missing": missing, "denom_mismatch": mismatched }),
        })
    }

    fn formula(&self, params: &Value, out: &OutRow) -> String {
        let terms = terms(params).unwrap_or_default();
        let bind: Vec<String> = terms
            .iter()
            .map(|t| {
                format!(
                    "{} = {}",
                    t.name,
                    t.sources
                        .iter()
                        .map(|(_, i)| i.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            })
            .collect();
        format!(
            "{}[{}, {}] = {} where {}",
            out.row.indicator,
            out.row.geo,
            out.row.start,
            params.get("formula").and_then(Value::as_str).unwrap_or("?"),
            bind.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use lanka_core::{Calendars, Freq};

    use super::*;
    use crate::method::{CollectSink, MemInputs};
    use crate::methods::rollup::tests::{meta, tree};

    fn d(y: i32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, 1, 1).expect("date")
    }

    fn r(ind: &str, geo: &str, y: i32, dims: &str, v: f64) -> Row {
        Row {
            basis: Basis::Reported,
            obs_id: Some(7),
            ..Row::computed(ind, geo, 1, Freq::A, d(y), dims, Some(v))
        }
    }

    fn inputs(rows: Vec<Row>) -> MemInputs {
        MemInputs {
            obs: HashMap::from([("in".to_owned(), Arc::new(rows))]),
            units: Arc::new(tree(&[2])),
            indicators: HashMap::from([
                ("pop".to_owned(), meta("pop", "sum")),
                ("area".to_owned(), meta("area", "sum")),
            ]),
            calendars: Calendars::default(),
            ..MemInputs::default()
        }
    }

    async fn run(inp: &MemInputs, params: Value) -> Result<Vec<OutRow>> {
        let ctx = MethodCtx {
            inputs: inp,
            rng_seed: 0,
            as_of: d(2024),
            changed_keys: None,
        };
        let mut sink = CollectSink::default();
        Ratio.run(&ctx, &params, &mut sink).await?;
        Ok(sink.rows)
    }

    fn density() -> Value {
        json!({
            "terms": {
                "pop": {"from": [{"port": "in", "indicator": "pop"}], "select": "sex=total"},
                "area": {"from": [{"port": "in", "indicator": "area"}]}
            },
            "formula": "pop / area",
            "indicator": "dens",
            "set_dims": "source_indicator=pop",
            "match_period": {"nearest": {"max_years": 2}}
        })
    }

    #[tokio::test]
    async fn density_broadcasts_and_flags_period_mismatch() {
        let inp = inputs(vec![
            r("pop", "ZZ1", 2024, "sex=total", 1000.0),
            r("pop", "ZZ1", 2024, "sex=male", 490.0),
            r("pop", "ZZ2", 2024, "sex=total", 300.0),
            r("area", "ZZ1", 2024, "", 10.0),
            r("area", "ZZ2", 2023, "", 3.0),
        ]);
        let out = run(&inp, density()).await.expect("run");
        assert_eq!(out.len(), 2);
        let z1 = out.iter().find(|o| o.row.geo == "ZZ1").expect("ZZ1");
        assert_eq!(z1.row.value, Some(100.0));
        assert_eq!(z1.row.dims, "source_indicator=pop");
        assert_eq!(z1.contrib.len(), 2);
        assert!(!z1.row.flags.contains(Flags::DENOM_MISMATCH));
        let z2 = out.iter().find(|o| o.row.geo == "ZZ2").expect("ZZ2");
        assert_eq!(z2.row.value, Some(100.0));
        assert!(z2.row.flags.contains(Flags::DENOM_MISMATCH));
    }

    #[tokio::test]
    async fn missing_term_or_zero_divisor_is_missing_not_zero() {
        let inp = inputs(vec![
            r("pop", "ZZ1", 2024, "sex=total", 1000.0),
            r("pop", "ZZ2", 2024, "sex=total", 1000.0),
            r("area", "ZZ2", 2024, "", 0.0),
        ]);
        let mut p = density();
        p["match_period"] = json!("exact");
        let out = run(&inp, p).await.expect("run");
        assert!(
            out.iter()
                .all(|o| o.row.value.is_none() && o.row.status == Status::Missing)
        );
    }

    #[tokio::test]
    async fn intervals_propagate_by_the_delta_method() {
        let mut a = r("pop", "ZZ1", 2024, "sex=total", 1000.0);
        a.lo = Some(900.0);
        a.hi = Some(1100.0);
        let inp = inputs(vec![a, r("area", "ZZ1", 2024, "", 10.0)]);
        let out = run(&inp, density()).await.expect("run");
        let o = &out[0].row;
        assert!(o.lo.is_some_and(|l| (l - 90.0).abs() < 1e-6), "{:?}", o.lo);
        assert!(o.hi.is_some_and(|h| (h - 110.0).abs() < 1e-6), "{:?}", o.hi);
    }

    #[tokio::test]
    async fn output_is_invariant_to_row_order() {
        let rows = vec![
            r("pop", "ZZ1", 2024, "sex=total", 1000.0),
            r("pop", "ZZ2", 2024, "sex=total", 300.0),
            r("area", "ZZ1", 2024, "", 10.0),
            r("area", "ZZ2", 2024, "", 3.0),
        ];
        let mut rev = rows.clone();
        rev.reverse();
        let a: Vec<Row> = run(&inputs(rows), density())
            .await
            .expect("a")
            .into_iter()
            .map(|o| o.row)
            .collect();
        let b: Vec<Row> = run(&inputs(rev), density())
            .await
            .expect("b")
            .into_iter()
            .map(|o| o.row)
            .collect();
        assert_eq!(a, b);
    }
}
