//! GUARD and PRESENT (11 §2, §5.2, §6): the global guards evaluated
//! against the request, the aligned frame and the estimates; the result
//! document `ldl-analysis-result/1`; the fingerprint (11 §9).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::frame::{AlignedFrame, BASIS};
use crate::guards::{self, Raised};
use crate::method::{MethodInfo, MethodOutput, fmt_num, thousands};
use crate::pipeline::{Resolved, redistribution_rank};
use crate::request::RESULT_SPEC;

/// sha256 over the resolved request without `pin` (so an unrelated release
/// keeps the id), the method reference with its implementation revision,
/// the engine revision, the alignment references, the sorted input
/// versions (and grid layer hashes) and the `lanka-stats` version.
///
/// `r` must already be resolved: `pin: "latest"` turned into the current
/// release's dataset versions ([`crate::pipeline::resolve`]), so a release
/// that changed an input changes the id and one that did not keeps it.
#[must_use]
pub fn fingerprint(r: &Resolved) -> [u8; 32] {
    let mut req = serde_json::to_value(&r.request).unwrap_or(Value::Null);
    if let Some(o) = req.as_object_mut() {
        o.remove("pin");
    }
    let mut h = Sha256::new();
    h.update(lanka_mapdoc::canon::jcs(&req).as_bytes());
    h.update(b"\nmethod:");
    h.update(r.method.info().implementation().as_bytes());
    h.update(b"\nengine:r");
    h.update(crate::ENGINE_REVISION.to_string().as_bytes());
    let mut align: Vec<String> = r
        .vars
        .iter()
        .map(|v| format!("{}={}", v.role, v.method_ref()))
        .collect();
    align.sort();
    h.update(b"\nalign:");
    h.update(align.join(",").as_bytes());
    let mut inputs: Vec<String> = r
        .vars
        .iter()
        .map(|v| {
            format!(
                "{}|{}|{}|{}",
                v.role,
                v.version.dataset_id,
                v.version.dsv,
                v.input.grid_sha256.as_deref().unwrap_or("")
            )
        })
        .collect();
    inputs.sort();
    h.update(b"\ninputs:");
    h.update(inputs.join(",").as_bytes());
    h.update(b"\nlanka-stats@");
    h.update(lanka_stats::VERSION.as_bytes());
    h.finalize().into()
}

#[must_use]
pub fn id_of(fp: &[u8; 32]) -> String {
    format!("an_{}", &hex::encode(fp)[..16])
}

fn year_of(period: &str) -> Option<i32> {
    period.get(..4).and_then(|y| y.parse().ok())
}

/// Guards knowable from the resolved request alone (the plan shows them).
#[must_use]
pub fn plan_guards(r: &Resolved, noun: &str, boundary_dataset: Option<&str>) -> Vec<Raised> {
    let info = r.method.info();
    let mut g = Vec::new();
    let var = |role: &str| r.vars.iter().find(|v| v.role == role);
    if info.relationship {
        g.push(Raised::new("NOT_CAUSAL").var("units", noun));
    }
    let pair = [var("x"), var("y")];
    let main: Vec<_> = r.vars.iter().filter(|v| v.role != "weight").collect();
    if info.relationship && main.iter().any(|v| v.input.about_people) {
        g.push(Raised::new("ECOLOGICAL").var("units", noun));
    }
    if matches!(info.id, "relate" | "regress")
        && let [Some(x), Some(y)] = pair
    {
        if x.input.extensive() && y.input.extensive() {
            g.push(
                Raised::new("SIZE_CONFOUND")
                    .action(json!({"label": "normalise by population or area", "method": info.id})),
            );
        }
        let shared: Vec<String> = x
            .input
            .upstream
            .intersection(&y.input.upstream)
            .filter(|d| Some(d.as_str()) != boundary_dataset)
            .cloned()
            .collect();
        if !shared.is_empty() {
            g.push(
                Raised::new("SHARED_LINEAGE")
                    .var("a", x.input.label.clone())
                    .var("b", y.input.label.clone())
                    .var("shared", shared.join(", "))
                    .detail(json!({"shared": shared})),
            );
        }
    }
    if info.relationship
        && let [Some(x), Some(y)] = pair
        && x.input.class == "estimated"
        && y.input.class == "estimated"
    {
        g.push(Raised::new("MODEL_ON_MODEL"));
    }
    // periods
    let periods: Vec<(String, i32)> = main
        .iter()
        .filter_map(|v| {
            year_of(&v.input.period).map(|y| (format!("{} {}", v.input.label, v.input.period), y))
        })
        .collect();
    if periods.len() > 1 {
        let lo = periods.iter().map(|p| p.1).min().unwrap_or(0);
        let hi = periods.iter().map(|p| p.1).max().unwrap_or(0);
        let years = hi - lo;
        if years > 2 {
            let mut p = Raised::new("PERIOD_MISMATCH")
                .var(
                    "periods",
                    periods
                        .iter()
                        .map(|p| p.0.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .var("years", years.to_string())
                .detail(json!({"years": years, "tolerance": 2, "block_above": 5}));
            if years > 5 {
                p = p.block();
            }
            g.push(p);
        }
    }
    for v in &main {
        if v.input.synthetic {
            g.push(Raised::new("SYNTHETIC_INPUT").var("inputs", v.input.label.clone()));
        }
        if v.input.community {
            g.push(Raised::new("COMMUNITY_INPUT").var("inputs", v.input.label.clone()));
        }
    }
    g
}

/// Guards that need the aligned frame and the method's n.
#[must_use]
pub fn frame_guards(
    info: &MethodInfo,
    f: &AlignedFrame,
    n_used: usize,
    noun: &str,
    roles: &[&str],
) -> Vec<Raised> {
    let mut g = Vec::new();
    let total = f.support.codes.len();
    if let Some(m) = info.min_n {
        if n_used < m.block {
            g.push(
                Raised::new("MIN_N")
                    .var("n", n_used.to_string())
                    .var("units", noun)
                    .var("method", info.id)
                    .var("min", m.block.to_string())
                    .detail(json!({"n": n_used, "min": m.block}))
                    .action(json!({"label": "use a finer level or a wider area"})),
            );
        } else if n_used < m.warn {
            g.push(
                Raised::new("SMALL_N")
                    .var("n", n_used.to_string())
                    .var("units", noun)
                    .var("warn", m.warn.to_string()),
            );
        }
    }
    if total > 0 && info.min_n.is_some() {
        #[allow(clippy::cast_precision_loss)]
        let cov = n_used as f64 / total as f64;
        if cov < 0.8 {
            let dropped = total - n_used;
            let mut r = Raised::new("LOW_COVERAGE")
                .var("dropped", thousands(dropped))
                .var("total", thousands(total))
                .var("units", noun)
                .var("pct", fmt_num((1.0 - cov) * 100.0, 2))
                .detail(json!({"coverage": cov, "dropped": dropped, "by": f.drop_counts(roles)}));
            if cov < 0.3 {
                r = r.block();
            }
            g.push(r);
        }
    }
    for c in roles.iter().filter_map(|r| f.col(r)) {
        if c.alignment.method.starts_with("inherit") {
            let mut parents: Vec<u64> = c
                .values
                .iter()
                .filter(|v| v.is_finite())
                .map(|v| v.to_bits())
                .collect();
            parents.sort_unstable();
            parents.dedup();
            g.push(
                Raised::new("PSEUDO_REPLICATION")
                    .var("inputs", c.input.label.clone())
                    .var("parents", parents.len().to_string())
                    .detail(json!({"n_effective": parents.len()})),
            );
        }
    }
    let sup = f.suppressed(roles);
    if sup > 0 {
        g.push(
            Raised::new("SUPPRESSED_CELLS")
                .var("n", sup.to_string())
                .var("units", noun),
        );
    }
    let est: Vec<String> = f
        .columns
        .iter()
        .filter(|c| c.max_basis() >= 2 || c.input.class == "estimated")
        .map(|c| c.input.label.clone())
        .collect();
    if !est.is_empty() {
        g.push(Raised::new("ESTIMATED_INPUT").var("inputs", est.join(" and ")));
    }
    if info.relationship
        && let (Some(x), Some(y)) = (f.col("x"), f.col("y"))
        && (x.max_basis() >= 2 || x.input.class == "estimated")
        && (y.max_basis() >= 2 || y.input.class == "estimated")
    {
        g.push(Raised::new("MODEL_ON_MODEL"));
    }
    let iv: Vec<String> = roles
        .iter()
        .filter_map(|r| f.col(r))
        .filter(|c| c.has_intervals())
        .map(|c| c.input.label.clone())
        .collect();
    if !iv.is_empty() && info.relationship {
        g.push(
            Raised::new("INPUT_UNCERTAINTY_IGNORED")
                .var("inputs", iv.join(" and "))
                .action(json!({"label": "rerun with uncertainty \"mc\" (WP-30)"})),
        );
    }
    g
}

/// Everything the document needs besides the method output.
pub struct Present<'a> {
    pub resolved: &'a Resolved,
    pub id: &'a str,
    pub fingerprint: &'a [u8; 32],
    pub seed: u64,
    pub frame: Option<&'a AlignedFrame>,
    pub noun: &'a str,
    pub base_url: &'a str,
    pub engine: &'a str,
    pub git_rev: &'a str,
    pub computed_at: &'a str,
    pub elapsed_ms: u64,
    pub catalog: &'a lanka_core::Catalog,
}

/// The result document. `out = None` → blocked before running.
#[allow(clippy::too_many_lines)]
/// Rewrites the links of a stored result for serving: output `href`s
/// relative (results stored before they were), the citation permalink on
/// the current public origin (`""` = relative).
pub fn relink(doc: &mut Value, base: &str) {
    let Some(id) = doc["id"].as_str().map(str::to_owned) else {
        return;
    };
    if let Some(outs) = doc["outputs"].as_array_mut() {
        for o in outs {
            if let Some(oid) = o["id"].as_str().map(str::to_owned) {
                o["href"] = json!(format!("/v2/analyses/{id}/outputs/{oid}"));
            }
        }
    }
    if doc["citation"].is_object() {
        doc["citation"]["url"] = json!(format!("{}/id/analysis/{id}", base.trim_end_matches('/')));
    }
}

#[must_use]
pub fn document(p: &Present<'_>, out: Option<&MethodOutput>, raised: &[Raised]) -> Value {
    let r = p.resolved;
    let info = r.method.info();
    let guards = guards::finish(raised, &r.request.overrides);
    let blocked = guards::blocks(&guards);
    let inputs: Vec<Value> = r
        .vars
        .iter()
        .map(|v| {
            let mut i = json!({
                "role": v.role, "dataset": v.version.dataset_id, "dsv": v.version.dsv,
                "period": v.input.period, "class": v.input.class, "licence": v.input.licence,
                "visibility": "public",
            });
            if let Some(ind) = &v.input.indicator {
                i["indicator"] = json!(ind);
            }
            if let Some(g) = &v.input.grid {
                i["grid"] = json!(g);
            }
            if v.input.synthetic {
                i["synthetic"] = json!(true);
            }
            i
        })
        .collect();
    let alignment: Vec<Value> = match p.frame {
        Some(f) => f
            .columns
            .iter()
            .map(|c| {
                let a = &c.alignment;
                let mut j = json!({"role": a.role, "from": a.from, "to": a.to, "method": a.method,
                                   "params": if a.params.is_null() { json!({}) } else { a.params.clone() }, "coverage": a.coverage, "dropped": a.dropped, "period": a.period});
                if let Some(q) = &a.quality {
                    j["quality"] = json!(q);
                }
                j
            })
            .collect(),
        None => Vec::new(),
    };
    let units = p.frame.map_or(0, |f| f.support.codes.len());
    let (used, dropped) = out.map_or((0, BTreeMap::new()), |o| (o.n_used, o.dropped.clone()));
    let basis = p.frame.map_or(1, |f| {
        f.columns
            .iter()
            .map(crate::frame::Column::max_basis)
            .max()
            .unwrap_or(1)
    });
    let lic = r.vars.iter().max_by_key(|v| {
        (
            redistribution_rank(&v.input.redistribution),
            v.input.licence.clone(),
        )
    });
    let licence_ids: BTreeSet<&str> = r.vars.iter().map(|v| v.input.licence.as_str()).collect();
    let licence = json!({
        "id": if licence_ids.len() == 1 { lic.map_or("unknown", |l| l.input.licence.as_str()).to_owned() }
              else { licence_ids.iter().copied().collect::<Vec<_>>().join(" AND ") },
        "redistribution": lic.map_or("link", |l| l.input.redistribution.as_str()),
    });
    let mut attribution: Vec<Value> = Vec::new();
    let mut seen = BTreeSet::new();
    for v in &r.vars {
        for s in &v.input.sources {
            if !seen.insert(s.clone()) {
                continue;
            }
            if let Some(src) = p.catalog.sources.get(s) {
                attribution.push(json!({
                    "source": s, "publisher": src.publisher, "title": src.title, "licence": src.licence_id,
                    "text": src.attribution, "url": src.homepage,
                }));
            }
        }
    }
    // Payload links inside the API are relative (06 §2); only the citation
    // permalink carries the public origin.
    let href = |o: &str| format!("/v2/analyses/{}/outputs/{o}", p.id);
    let outputs: Vec<Value> = if blocked {
        Vec::new()
    } else {
        out.map(|o| {
            o.outputs
                .iter()
                .map(|x| {
                    let mut j = json!({"id": x.id, "kind": x.kind, "href": href(x.id)});
                    if !x.columns.is_empty() {
                        j["columns"] = json!(x.columns);
                    }
                    if let Some(e) = x.encoding {
                        j["encoding"] = json!(e);
                    }
                    if let Some(s) = &x.suggested_layer {
                        j["suggested_layer"] = s.clone();
                    }
                    j
                })
                .collect()
        })
        .unwrap_or_default()
    };
    let caveats: Vec<Value> = guards
        .iter()
        .filter(|g| g["severity"] == "warn")
        .map(|g| g["message"].clone())
        .collect();
    let interpretation = if blocked {
        Value::Null
    } else {
        out.map_or(
            Value::Null,
            |o| json!({"summary": o.summary, "caveats": caveats}),
        )
    };
    let names: Vec<String> = r
        .vars
        .iter()
        .map(|v| {
            v.input
                .indicator
                .clone()
                .or_else(|| v.input.grid.clone())
                .unwrap_or_else(|| v.version.dataset_id.clone())
        })
        .collect();
    let year = p.computed_at.get(..4).unwrap_or("2026");
    let within = r
        .within
        .as_deref()
        .map(|w| format!(", {w}"))
        .unwrap_or_default();
    let overrides = if r.request.overrides.is_empty() {
        String::new()
    } else {
        format!(" Guards overridden: {}.", r.request.overrides.join(", "))
    };
    let citation = json!({
        "text": format!("Lanka Data Layer ({year}). Analysis {}: {}({}), {}{within}. Data release {}.{overrides}",
                        p.id, info.id, names.join(", "), p.noun, r.data_version),
        "url": format!("{}/id/analysis/{}", p.base_url, p.id),
    });
    json!({
        "spec": RESULT_SPEC,
        "id": p.id,
        "state": if blocked { "blocked" } else { "ready" },
        "method": {"id": info.id, "version": info.version, "revision": info.revision, "citations": info.citations},
        "request": serde_json::to_value(&r.request).unwrap_or(Value::Null),
        "inputs": inputs,
        "alignment": alignment,
        "n": {"units": units, "used": used, "dropped": dropped},
        "estimates": if blocked { Value::Null } else { out.map_or(Value::Null, |o| o.estimates.clone()) },
        "guards": guards,
        "interpretation": interpretation,
        "outputs": outputs,
        "basis": BASIS[usize::from(basis.min(3))],
        "licence": licence,
        "attribution": attribution,
        "provenance": {
            "fingerprint": hex::encode(p.fingerprint), "seed": p.seed, "engine": p.engine, "git_rev": p.git_rev,
            "data_version": r.data_version, "computed_at": p.computed_at, "elapsed_ms": p.elapsed_ms,
            "stats": format!("lanka-stats {}", lanka_stats::VERSION),
            "engine_revision": crate::ENGINE_REVISION,
        },
        "citation": citation,
    })
}
