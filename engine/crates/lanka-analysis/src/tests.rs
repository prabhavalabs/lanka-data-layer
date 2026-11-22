//! Guard rails without a database (11 §13 "Guards"): one positive and one
//! negative fixture per guard code raisable in WP-29, blocked results carry
//! no estimates, the vocabulary scan over every rendered document, and the
//! determinism/symmetry properties of the evaluation.

use std::collections::BTreeSet;
use std::sync::Arc;

use lanka_core::Catalog;
use lanka_stats::rng::Stream;
use lanka_stats::transform::Transform;
use serde_json::{Value, json};

use crate::engine::{EvalMeta, evaluate};
use crate::frame::{AlignedFrame, AlignmentRecord, Column, InputRef, Support};
use crate::guards::{REGISTRY, causal_words};
use crate::methods::Registry;
use crate::pipeline::{Resolved, ResolvedVar, Source, Version};
use crate::request::{Pin, Request, Support as SupportSpec, SupportRef, Variable, Variables};

fn support(n: usize) -> Support {
    let side = (n as f64).sqrt().ceil() as usize;
    Support {
        kind: "admin".into(),
        scheme: "admin".into(),
        level: 3,
        within: None,
        boundary_set: "test".into(),
        codes: (0..n).map(|i| format!("U{i:05}")).collect(),
        lon: (0..n).map(|i| 79.8 + (i % side) as f64 * 0.05).collect(),
        lat: (0..n).map(|i| 6.0 + (i / side) as f64 * 0.05).collect(),
        parents: (0..n).map(|i| Some(format!("P{}", i % 5))).collect(),
    }
}

fn input(role: &str, label: &str) -> InputRef {
    InputRef {
        role: role.into(),
        dataset: format!("ds-{role}"),
        dsv: format!("dsv_{role}"),
        indicator: Some(format!("ind.{role}")),
        period: "2024".into(),
        class: "official".into(),
        licence: "cc-by-4.0".into(),
        redistribution: "bulk".into(),
        measure_type: "share".into(),
        label: label.into(),
        upstream: [format!("ds-{role}")].into_iter().collect(),
        ..InputRef::default()
    }
}

fn column(role: &str, values: Vec<f64>) -> Column {
    let n = values.len();
    Column {
        role: role.into(),
        term: role.replace("control:0", "c1"),
        input: input(role, role),
        transform: Transform::Identity,
        values,
        lo: None,
        hi: None,
        basis: vec![1; n],
        suppressed: vec![false; n],
        alignment: AlignmentRecord {
            role: role.into(),
            from: "admin:3".into(),
            to: "admin:3".into(),
            method: "identity".into(),
            coverage: 1.0,
            ..AlignmentRecord::default()
        },
    }
}

/// x, y with a moderate linear relation and independent noise.
fn xy(n: usize, seed: u64) -> (Vec<f64>, Vec<f64>) {
    let mut s = Stream::new(seed, 1);
    let x: Vec<f64> = (0..n).map(|_| s.unit() * 10.0).collect();
    let y: Vec<f64> = x
        .iter()
        .map(|v| 2.0 + 0.5 * v + (s.unit() - 0.5) * 6.0)
        .collect();
    (x, y)
}

struct Case {
    method: &'static str,
    frame: AlignedFrame,
    scale: Vec<AlignedFrame>,
    overrides: Vec<String>,
    params: Value,
}

impl Case {
    fn new(method: &'static str, cols: Vec<Column>) -> Self {
        let n = cols[0].values.len();
        Self {
            method,
            frame: AlignedFrame {
                support: support(n),
                columns: cols,
            },
            scale: Vec::new(),
            overrides: Vec::new(),
            params: json!({}),
        }
    }

    fn relate(n: usize) -> Self {
        let (x, y) = xy(n, 7);
        Self::new("relate", vec![column("x", x), column("y", y)])
    }

    fn col(&mut self, role: &str) -> &mut Column {
        self.frame
            .columns
            .iter_mut()
            .find(|c| c.role == role)
            .expect("role")
    }

    fn resolved(&self) -> Resolved {
        let methods = Registry::builtin();
        let method = methods.get(self.method).expect("method");
        let mut vars = Variables::default();
        let rvars: Vec<ResolvedVar> = self
            .frame
            .columns
            .iter()
            .map(|c| {
                let v = Variable {
                    indicator: c.input.indicator.clone(),
                    period: Some(c.input.period.clone()),
                    ..Variable::default()
                };
                match c.role.as_str() {
                    "x" => vars.x = Some(v.clone()),
                    "y" => vars.y = Some(v.clone()),
                    "group" => vars.group = Some(v.clone()),
                    _ => vars.controls.push(v.clone()),
                }
                ResolvedVar {
                    role: c.role.clone(),
                    var: v,
                    source: Source::Features {
                        dataset_id: c.input.dataset.clone(),
                        measure: "count".into(),
                        kinds: Vec::new(),
                    },
                    method: c
                        .alignment
                        .method
                        .split('@')
                        .next()
                        .unwrap_or("identity")
                        .to_owned(),
                    version: Version {
                        dataset_id: c.input.dataset.clone(),
                        dataset_key: 1,
                        version_id: 1,
                        seq: 1,
                        dsv: c.input.dsv.clone(),
                    },
                    input: c.input.clone(),
                }
            })
            .collect();
        let params = method.defaults(&self.params);
        Resolved {
            request: Request {
                spec: "ldl-analysis/1".into(),
                method: self.method.into(),
                pin: Pin::Release {
                    release: "20261122.1".into(),
                },
                support: SupportRef::Explicit(SupportSpec::Admin {
                    level: 3,
                    scheme: None,
                    within: None,
                }),
                variables: vars,
                params,
                overrides: self.overrides.clone(),
            },
            method,
            data_version: "20261122.1".into(),
            level: 3,
            within: None,
            vars: rvars,
            scale_levels: self.scale.iter().map(|f| f.support.level).collect(),
        }
    }

    fn run(&self) -> Value {
        let r = self.resolved();
        let fp = crate::result::fingerprint(&r);
        let meta = EvalMeta {
            base_url: "https://example.org".into(),
            engine: "lanka test".into(),
            git_rev: "test".into(),
        };
        let (doc, _) = evaluate(
            &r,
            &fp,
            &self.frame,
            &self.scale,
            None,
            &Catalog::default(),
            &meta,
        )
        .expect("evaluate");
        doc
    }
}

fn codes(doc: &Value) -> BTreeSet<String> {
    doc["guards"]
        .as_array()
        .expect("guards")
        .iter()
        .map(|g| g["code"].as_str().expect("code").to_owned())
        .collect()
}

fn guard<'a>(doc: &'a Value, code: &str) -> Option<&'a Value> {
    doc["guards"]
        .as_array()
        .and_then(|g| g.iter().find(|g| g["code"] == code))
}

/// (code, positive case, negative case)
fn cases() -> Vec<(&'static str, Case, Case)> {
    let mut out: Vec<(&'static str, Case, Case)> = Vec::new();
    out.push(("NOT_CAUSAL", Case::relate(60), {
        let (x, _) = xy(60, 3);
        Case::new("describe", vec![column("x", x)])
    }));
    out.push(("MIN_N", Case::relate(8), Case::relate(60)));
    out.push(("SMALL_N", Case::relate(20), Case::relate(60)));
    out.push(("MAUP", Case::relate(60), {
        let (x, _) = xy(60, 3);
        Case::new("describe", vec![column("x", x)])
    }));
    out.push((
        "MAUP_UNSTABLE",
        {
            let mut c = Case::relate(60);
            let (x, y) = xy(25, 9);
            let yneg: Vec<f64> = y.iter().map(|v| -v).collect();
            let mut f = AlignedFrame {
                support: support(25),
                columns: vec![column("x", x), column("y", yneg)],
            };
            f.support.level = 2;
            c.scale.push(f);
            c
        },
        {
            let mut c = Case::relate(60);
            let (x, y) = xy(60, 7);
            let mut f = AlignedFrame {
                support: support(60),
                columns: vec![column("x", x), column("y", y)],
            };
            f.support.level = 2;
            c.scale.push(f);
            c
        },
    ));
    out.push((
        "ECOLOGICAL",
        {
            let mut c = Case::relate(60);
            c.col("y").input.about_people = true;
            c
        },
        Case::relate(60),
    ));
    out.push((
        "RESIDUAL_AUTOCORR",
        {
            let mut c = Case::relate(100);
            let lat = c.frame.support.lat.clone();
            for (i, v) in c.col("y").values.iter_mut().enumerate() {
                *v += (lat[i] - 6.0) * 40.0;
            }
            c
        },
        Case::relate(100),
    ));
    out.push((
        "SIZE_CONFOUND",
        {
            let mut c = Case::relate(60);
            c.col("x").input.measure_type = "count".into();
            c.col("y").input.measure_type = "count".into();
            c
        },
        {
            let mut c = Case::relate(60);
            c.col("x").input.measure_type = "count".into();
            c
        },
    ));
    out.push((
        "SHARED_LINEAGE",
        {
            let mut c = Case::relate(60);
            c.col("x").input.upstream.insert("worldpop-pop-grid".into());
            c.col("y").input.upstream.insert("worldpop-pop-grid".into());
            c
        },
        Case::relate(60),
    ));
    out.push((
        "ESTIMATED_INPUT",
        {
            let mut c = Case::relate(60);
            c.col("x").basis = vec![2; 60];
            c
        },
        Case::relate(60),
    ));
    out.push((
        "MODEL_ON_MODEL",
        {
            let mut c = Case::relate(60);
            c.col("x").input.class = "estimated".into();
            c.col("y").input.class = "estimated".into();
            c
        },
        {
            let mut c = Case::relate(60);
            c.col("x").input.class = "estimated".into();
            c
        },
    ));
    out.push((
        "INPUT_UNCERTAINTY_IGNORED",
        {
            let mut c = Case::relate(60);
            let x = c.col("x").values.clone();
            c.col("x").lo = Some(x.iter().map(|v| v * 0.9).collect());
            c.col("x").hi = Some(x.iter().map(|v| v * 1.1).collect());
            c
        },
        Case::relate(60),
    ));
    out.push((
        "PERIOD_MISMATCH",
        {
            let mut c = Case::relate(60);
            c.col("x").input.period = "2021".into();
            c
        },
        {
            let mut c = Case::relate(60);
            c.col("x").input.period = "2023".into();
            c
        },
    ));
    out.push((
        "LOW_COVERAGE",
        {
            let mut c = Case::relate(80);
            for v in c.col("y").values.iter_mut().take(30) {
                *v = f64::NAN;
            }
            c
        },
        Case::relate(80),
    ));
    out.push((
        "PSEUDO_REPLICATION",
        {
            let mut c = Case::relate(60);
            c.col("x").alignment.method = "inherit@1.0.0".into();
            c
        },
        Case::relate(60),
    ));
    out.push((
        "OUTLIER_LEVERAGE",
        {
            let mut c = Case::relate(60);
            c.col("x").values[0] = 200.0;
            c.col("y").values[0] = -300.0;
            c
        },
        Case::relate(60),
    ));
    out.push(("NO_CONTROLS", Case::relate(60), {
        let mut c = Case::relate(60);
        let mut s = Stream::new(4, 4);
        c.frame
            .columns
            .push(column("control:0", (0..60).map(|_| s.unit()).collect()));
        c
    }));
    out.push((
        "MULTICOLLINEAR",
        {
            let mut c = Case::relate(60);
            let mut s = Stream::new(5, 5);
            let x = c.col("x").values.clone();
            c.frame.columns.push(column(
                "control:0",
                x.iter().map(|v| v + (s.unit() - 0.5) * 0.05).collect(),
            ));
            c
        },
        {
            let mut c = Case::relate(60);
            let mut s = Stream::new(4, 4);
            c.frame
                .columns
                .push(column("control:0", (0..60).map(|_| s.unit()).collect()));
            c
        },
    ));
    out.push((
        "SUPPRESSED_CELLS",
        {
            let mut c = Case::relate(60);
            c.col("y").values[3] = f64::NAN;
            c.col("y").suppressed[3] = true;
            c
        },
        Case::relate(60),
    ));
    out.push((
        "SYNTHETIC_INPUT",
        {
            let mut c = Case::relate(60);
            c.col("y").input.synthetic = true;
            c
        },
        Case::relate(60),
    ));
    out.push((
        "COMMUNITY_INPUT",
        {
            let mut c = Case::relate(60);
            c.col("y").input.community = true;
            c
        },
        Case::relate(60),
    ));
    out
}

#[test]
fn every_active_guard_has_a_positive_and_a_negative_fixture() {
    let cs = cases();
    let covered: BTreeSet<&str> = cs.iter().map(|c| c.0).collect();
    for g in REGISTRY.iter().filter(|g| g.since == "WP-29") {
        assert!(covered.contains(g.code), "no fixture for {}", g.code);
    }
    for (code, pos, neg) in &cs {
        let p = pos.run();
        let n = neg.run();
        assert!(
            codes(&p).contains(*code),
            "{code}: positive fixture did not raise it: {:?}",
            codes(&p)
        );
        assert!(
            !codes(&n).contains(*code),
            "{code}: negative fixture raised it: {:?}",
            codes(&n)
        );
        for doc in [&p, &n] {
            let text = serde_json::to_string(&json!([
                doc["guards"],
                doc["interpretation"],
                doc["estimates"]["ols"]["reading"],
                doc["estimates"]["contrasts"]
            ]))
            .expect("text");
            assert!(
                causal_words(&text).is_empty(),
                "{code}: causal wording in {text}"
            );
        }
    }
}

#[test]
fn blocked_results_carry_no_estimates_and_no_outputs() {
    let blocked = [
        Case::relate(8),
        {
            let mut c = Case::relate(60);
            c.col("x").input.period = "2014".into();
            c
        },
        {
            let mut c = Case::relate(100);
            for v in c.col("y").values.iter_mut().take(80) {
                *v = f64::NAN;
            }
            c
        },
    ];
    for c in &blocked {
        let d = c.run();
        assert_eq!(d["state"], "blocked", "{}", d["guards"]);
        assert!(d["estimates"].is_null());
        assert!(d["interpretation"].is_null());
        assert_eq!(d["outputs"], json!([]));
        assert!(d["guards"][0]["severity"] == "block");
        assert!(
            !serde_json::to_string(&d).expect("s").contains("\"p\":"),
            "no p-value anywhere"
        );
    }
    // a documented override turns PERIOD_MISMATCH's block into a recorded warn
    let mut c = Case::relate(60);
    c.col("x").input.period = "2014".into();
    c.overrides = vec!["PERIOD_MISMATCH".into()];
    let d = c.run();
    assert_eq!(d["state"], "ready");
    assert_eq!(
        guard(&d, "PERIOD_MISMATCH").expect("pm")["overridden"],
        json!(true)
    );
    assert!(
        d["citation"]["text"]
            .as_str()
            .expect("cite")
            .contains("Guards overridden: PERIOD_MISMATCH")
    );
}

#[test]
fn small_n_hides_p_values() {
    let d = Case::relate(20).run();
    assert_eq!(d["state"], "ready");
    let pe = &d["estimates"]["pearson"];
    assert!(pe["ci"].is_array());
    assert!(pe.get("p").is_none() && pe.get("p_adjusted").is_none());
    assert!(d["estimates"]["ols"]["coef"][1].get("p").is_none());
    let d = Case::relate(60).run();
    assert!(d["estimates"]["pearson"].get("p_adjusted").is_some());
    assert_eq!(d["estimates"]["significance"], "adjusted");
}

#[test]
fn evaluation_is_deterministic_and_symmetric() {
    let c = Case::relate(120);
    let a = serde_json::to_vec(&c.run()).expect("a");
    let b = serde_json::to_vec(&c.run()).expect("b");
    assert_eq!(a, b, "same inputs, same bytes");
    // relate(x, y) and relate(y, x) agree on r
    let mut sw = Case::relate(120);
    let (x, y) = (sw.col("x").values.clone(), sw.col("y").values.clone());
    sw.col("x").values = y;
    sw.col("y").values = x;
    let d1 = c.run();
    let d2 = sw.run();
    assert_eq!(
        d1["estimates"]["pearson"]["r"],
        d2["estimates"]["pearson"]["r"]
    );
    assert_eq!(
        d1["estimates"]["spearman"]["rho"],
        d2["estimates"]["spearman"]["rho"]
    );
    // row order does not change r (beyond rounding)
    let mut rev = Case::relate(120);
    let order: Vec<usize> = (0..120).rev().collect();
    rev.frame = rev.frame.select(&order);
    let r1 = d1["estimates"]["pearson"]["r"].as_f64().expect("r");
    let r2 = rev.run()["estimates"]["pearson"]["r"].as_f64().expect("r");
    assert!((r1 - r2).abs() < 1e-12);
}

#[test]
fn the_other_tier1_methods_run_on_textbook_data() {
    let (x, y) = xy(80, 11);
    for m in ["describe", "classify"] {
        let d = Case::new(m, vec![column("x", x.clone())]).run();
        assert_eq!(d["state"], "ready", "{m}: {}", d["guards"]);
    }
    let mut c = Case::new(
        "regress",
        vec![column("x", x.clone()), column("y", y.clone())],
    );
    c.params = json!({"fixed_effects": "parent"});
    let d = c.run();
    assert_eq!(d["state"], "ready", "{}", d["guards"]);
    assert_eq!(d["estimates"]["ols"]["fixed_effects"]["groups"], 5);
    let group: Vec<f64> = (0..80)
        .map(|i| if i % 3 == 0 { 1.0 } else { 0.0 })
        .collect();
    let mut g = Case::new(
        "group_compare",
        vec![column("y", y), column("group", group)],
    );
    g.params = json!({"zone": {"threshold": 0.5}, "bootstrap": 200});
    let d = g.run();
    assert_eq!(d["state"], "ready", "{}", d["guards"]);
    assert_eq!(d["estimates"]["groups"][0]["n"], 27);
    assert!(
        d["estimates"]["contrasts"][0]["reading"]
            .as_str()
            .expect("r")
            .contains("inside the zone")
    );
}

#[test]
fn methods_catalogue_is_complete() {
    let reg = Registry::builtin();
    let ids: Vec<&str> = reg.iter().map(|m| m.info().id).collect();
    assert_eq!(
        ids,
        [
            "describe",
            "relate",
            "regress",
            "group_compare",
            "exposure",
            "zonal",
            "classify"
        ]
    );
    for m in reg.iter() {
        let j = m.info().to_json();
        assert!(
            j["params"].is_object(),
            "{}: params schema parses",
            m.info().id
        );
        assert!(
            j["estimates"].is_object(),
            "{}: estimates schema parses",
            m.info().id
        );
        for g in m.info().guards {
            assert!(
                crate::guards::spec(g).is_some(),
                "{} lists unknown guard {g}",
                m.info().id
            );
        }
        let _ = Arc::clone(m);
    }
}

#[test]
fn spec_schemas_are_byte_identical_to_the_crate_copies() {
    let spec = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../spec/analysis/1");
    if !spec.exists() {
        return; // the image builds from engine/ alone
    }
    for (f, embedded) in [
        ("request.schema.json", crate::request::REQUEST_SCHEMA),
        ("result.schema.json", crate::request::RESULT_SCHEMA),
        ("plan.schema.json", crate::request::PLAN_SCHEMA),
    ] {
        let on_disk = std::fs::read_to_string(spec.join(f)).expect("spec file");
        assert_eq!(on_disk, embedded, "{f} differs from spec/analysis/1");
    }
}

#[test]
fn results_validate_against_the_result_schema() {
    let mut schema: Value = serde_json::from_str(crate::request::RESULT_SCHEMA).expect("schema");
    // the echoed request is checked against its own schema separately
    schema["properties"]["request"] = json!({"type": "object"});
    let v = jsonschema::validator_for(&schema).expect("validator");
    for d in [Case::relate(60).run(), Case::relate(8).run()] {
        crate::request::validate(&d["request"]).expect("echoed request is valid ldl-analysis/1");
        let errs: Vec<String> = v
            .iter_errors(&d)
            .map(|e| format!("{}: {e}", e.instance_path()))
            .collect();
        assert!(errs.is_empty(), "{errs:?}");
    }
}

#[test]
fn controls_make_the_partial_correlation_the_headline() {
    // x and y both follow z: the zero-order r is strong, the partial one is not
    let mut s = Stream::new(31, 2);
    let z: Vec<f64> = (0..80).map(|_| s.unit() * 10.0).collect();
    let x: Vec<f64> = z.iter().map(|v| v + s.unit()).collect();
    let y: Vec<f64> = z.iter().map(|v| 2.0 * v + s.unit()).collect();
    let d = Case::new(
        "relate",
        vec![column("x", x), column("y", y), column("control:0", z)],
    )
    .run();
    assert_eq!(d["state"], "ready", "{}", d["guards"]);
    let e = &d["estimates"];
    assert!(e["pearson"]["r"].as_f64().expect("r") > 0.9);
    let pr = &e["partial"]["pearson"];
    assert!(pr["r"].as_f64().expect("partial r").abs() < 0.3, "{pr}");
    assert_eq!(e["partial"]["k"], 1);
    assert_eq!(pr["df"], 77);
    assert!(pr.get("n_effective").is_some() && pr.get("p_adjusted").is_some());
    assert!(e["partial"]["spearman"].get("rho").is_some());
    assert_eq!(e["correlation_headline"], "partial");
    assert_eq!(e["significance"], "adjusted");
    let summary = d["interpretation"]["summary"].as_str().expect("summary");
    assert!(
        summary.contains("partial r =") && summary.contains("holding"),
        "{summary}"
    );
    // without controls nothing partial is reported
    let d = Case::relate(60).run();
    assert!(d["estimates"].get("partial").is_none());
    assert!(d["estimates"].get("correlation_headline").is_none());
}

#[test]
fn group_compare_reports_spatially_adjusted_significance() {
    let (_, y) = xy(80, 11);
    let group: Vec<f64> = (0..80)
        .map(|i| if i % 2 == 0 { 30.0 } else { 5.0 })
        .collect();
    let mut g = Case::new(
        "group_compare",
        vec![column("y", y), column("group", group)],
    );
    g.params = json!({"zone": {"threshold": 20, "label": "flood zone"}, "bootstrap": 100});
    let d = g.run();
    assert_eq!(d["state"], "ready", "{}", d["guards"]);
    let e = &d["estimates"];
    let c = &e["contrasts"][0];
    assert!(
        c.get("p_adjusted").is_some() && c.get("p_welch").is_some(),
        "{c}"
    );
    assert!(c["n_effective"].as_f64().expect("m") <= 80.0);
    let (lo, hi) = (
        c["ci"][0].as_f64().expect("lo"),
        c["ci"][1].as_f64().expect("hi"),
    );
    let (alo, ahi) = (
        c["ci_adjusted"][0].as_f64().expect("lo"),
        c["ci_adjusted"][1].as_f64().expect("hi"),
    );
    assert!(
        alo <= lo + 1e-12 && ahi >= hi - 1e-12,
        "adjusted interval is not narrower"
    );
    assert_eq!(e["significance"], "adjusted");
    assert_eq!(e["groups"][0]["label"], "inside flood zone (≥20)");
    assert_eq!(e["groups"][1]["label"], "outside flood zone (<20)");
}

#[test]
fn suggested_layers_are_valid_map_layers() {
    let ramps: Value =
        serde_json::from_str(include_str!("../../../../spec/map/1/ramps.json")).expect("ramps");
    let (x, y) = xy(80, 11);
    let group: Vec<f64> = (0..80)
        .map(|i| if i % 3 == 0 { 1.0 } else { 0.0 })
        .collect();
    let mut docs = vec![Case::relate(60).run()];
    for m in ["describe", "classify"] {
        docs.push(Case::new(m, vec![column("x", x.clone())]).run());
    }
    docs.push(
        Case::new(
            "regress",
            vec![column("x", x.clone()), column("y", y.clone())],
        )
        .run(),
    );
    let mut g = Case::new(
        "group_compare",
        vec![column("y", y), column("group", group)],
    );
    g.params = json!({"zone": {"threshold": 0.5, "label": "flood zone"}, "bootstrap": 50});
    docs.push(g.run());
    let mut n = 0;
    for d in &docs {
        for o in d["outputs"].as_array().into_iter().flatten() {
            let Some(layer) = o.get("suggested_layer") else {
                continue;
            };
            n += 1;
            let mut l = layer.clone();
            l["id"] = json!("l");
            l["source"] = json!("s");
            l["visible"] = json!(true);
            let doc = json!({
                "spec": "ldl-map/1", "title": "t", "pin": "latest",
                "view": {"center": [80, 7], "zoom": 7}, "basemap": {"style": "ldl-auto"},
                "analyses": {"a": d["request"]},
                "sources": {"s": {"kind": "analysis", "ref": "a", "output": o["id"]}},
                "layers": [l]
            });
            if let Err(p) = lanka_mapdoc::validate(&doc) {
                panic!("{} {}: {p:#?}", d["method"]["id"], o["id"]);
            }
            let fill = &layer["style"]["fill"];
            for ramp in [&fill["bivariate"]["palette"], &fill["scale"]["ramp"]] {
                if let Some(r) = ramp.as_str() {
                    assert!(ramps["ramps"].get(r).is_some(), "{r} is not in ramps.json");
                }
            }
            if let Some(b) = fill.get("bivariate") {
                for ax in ["x", "y"] {
                    assert_eq!(b[ax]["scale"]["type"], "quantile");
                    assert_eq!(b[ax]["scale"]["breaks"].as_array().map(Vec::len), Some(2));
                }
            }
        }
    }
    assert!(n >= 5, "{n} suggested layers");
}
