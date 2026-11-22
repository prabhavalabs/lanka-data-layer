//! Derived datasets: recipe sync, the release train's recompute step and
//! one-off recipe runs (04 §7, 03 §8 step 3).
//!
//! ```text
//! candidate inputs = current release ∪ approved versions
//! for recipe in topological order:
//!     schedule ≠ daily (and not forced)         → skipped_schedule
//!     a required input has no version            → waiting_inputs
//!     fingerprint has a derivation whose version
//!       is current / approved / in review        → reused (nothing runs)
//!     else run (lanka-enrich runner) → QA gates  → qa_failed (incident, nothing staged)
//!          → stage through publish_observations  → validation errors fail the recipe
//!          → derived gate: stable, not the first version, same method MAJORs and
//!            parameters as the published derivation → approved, else in_review
//!          → data.derivations + data.version_inputs
//!     an approved output enters the candidate map, so downstream recipes see it
//! ```
//!
//! A failing recipe keeps its previous version (downstream recipes then see
//! the old input and are reused); official data is never held back.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use lanka_core::{Calendars, Method, Profile};
use lanka_enrich::pg::{PgInputs, Pinned};
use lanka_enrich::provenance::seed_of;
use lanka_enrich::recipe::{DatasetView, RegistryView};
use lanka_enrich::runner::{self, FpInput};
use lanka_enrich::{Methods, RecipeSpec, Recipes};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::connector::ObsRow;
use crate::publish::{self, StageInput, publish_observations};
use crate::registry::Registry;
use crate::run::{Event, Level, RunStatus, RunTracker, Stage, emit};

/// What a recompute needs besides the pool.
pub struct DeriveEnv<'a> {
    pub pool: &'a PgPool,
    pub registry: &'a Registry,
    pub profile: &'a Profile,
    pub recipes: &'a Recipes,
    pub methods: &'a Methods,
}

/// The registry as `lanka-enrich` checks see it.
#[must_use]
pub fn registry_view(reg: &Registry) -> RegistryView {
    RegistryView {
        datasets: reg
            .datasets
            .values()
            .map(|d| {
                (
                    d.spec.id.clone(),
                    DatasetView {
                        kind: d.spec.kind().to_owned(),
                        indicators: d.spec.indicators.iter().map(|i| i.id.clone()).collect(),
                    },
                )
            })
            .collect(),
    }
}

/// Loads `registry/recipes/` and fails on any check error (warnings are returned).
pub fn load_checked(reg: &Registry, methods: &Methods) -> Result<(Recipes, Vec<String>)> {
    let recipes = Recipes::load(&reg.root)?;
    let rep = recipes.check(&registry_view(reg), methods);
    anyhow::ensure!(
        rep.errors.is_empty(),
        "recipe check failed:\n  - {}",
        rep.errors.join("\n  - ")
    );
    Ok((recipes, rep.warnings))
}

/// Mirrors the method library and the recipes into `catalog.*`.
pub async fn sync(pool: &PgPool, recipes: &Recipes, methods: &Methods) -> Result<(usize, usize)> {
    let mut tx = pool.begin().await?;
    let mut nm = 0;
    for m in methods.iter() {
        let i = m.info();
        sqlx::query(
            "INSERT INTO catalog.methods (id, version, info) VALUES ($1, $2, $3)
             ON CONFLICT (id, version) DO UPDATE SET info = EXCLUDED.info, synced_at = now()",
        )
        .bind(i.id)
        .bind(i.version.to_string())
        .bind(i.to_json())
        .execute(&mut *tx)
        .await?;
        nm += 1;
    }
    let ids: Vec<String> = recipes.list.iter().map(|r| r.spec.id.clone()).collect();
    for r in &recipes.list {
        let s = &r.spec;
        sqlx::query(
            "INSERT INTO catalog.recipes (id, output_dataset, stability, schedule, spec, spec_sha256)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET output_dataset = EXCLUDED.output_dataset,
               stability = EXCLUDED.stability, schedule = EXCLUDED.schedule, spec = EXCLUDED.spec,
               spec_sha256 = EXCLUDED.spec_sha256, synced_at = now()",
        )
        .bind(&s.id)
        .bind(&s.output)
        .bind(s.stability.as_str())
        .bind(s.schedule())
        .bind(s.canonical_json())
        .bind(s.sha256().as_slice())
        .execute(&mut *tx)
        .await
        .with_context(|| format!("syncing recipe {} (is its output dataset synced?)", s.id))?;
        sqlx::query("DELETE FROM catalog.recipe_inputs WHERE recipe_id = $1")
            .bind(&s.id)
            .execute(&mut *tx)
            .await?;
        for (port, i) in &s.inputs {
            sqlx::query(
                "INSERT INTO catalog.recipe_inputs (recipe_id, port, dataset_id, optional) VALUES ($1, $2, $3, $4)",
            )
            .bind(&s.id)
            .bind(port)
            .bind(&i.dataset)
            .bind(i.optional)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE catalog.datasets SET recipe_id = $1 WHERE id = $2")
            .bind(&s.id)
            .bind(&s.output)
            .execute(&mut *tx)
            .await?;
    }
    // Recipes removed from the registry: keep the ones with lineage (their
    // derivations reference them), drop the rest.
    sqlx::query(
        "DELETE FROM catalog.recipes r WHERE r.id <> ALL($1)
           AND NOT EXISTS (SELECT 1 FROM data.derivations d WHERE d.recipe_id = r.id)",
    )
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE catalog.datasets SET recipe_id = NULL WHERE recipe_id IS NOT NULL AND recipe_id <> ALL($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((nm, recipes.list.len()))
}

#[derive(Debug, Clone, Default)]
pub struct RecomputeOpts {
    /// `ops.ingestion_runs.trigger` of the recipe runs: schedule | manual | test.
    pub trigger: String,
    pub train_id: Option<i64>,
    pub job_id: Option<i64>,
    /// Only this recipe (`lanka recipe run <id>`); downstream ones are not run.
    pub only: Option<String>,
    /// Run even when a derivation with the fingerprint exists.
    pub force: bool,
    /// Compute fingerprints and report what would run; write nothing.
    pub dry_run: bool,
    /// Also run weekly/monthly/manual recipes.
    pub include_scheduled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipeResult {
    pub recipe: String,
    pub output: String,
    /// reused | published | approved | in_review | unchanged | would_run |
    /// waiting_inputs | skipped_schedule | qa_failed | failed
    pub outcome: &'static str,
    pub fingerprint: Option<String>,
    pub version_id: Option<i64>,
    pub dsv: Option<String>,
    pub rows: Option<i64>,
    pub ms: u128,
    pub detail: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecomputeReport {
    pub recipes: usize,
    pub ran: usize,
    pub reused: usize,
    pub failed: usize,
    pub results: Vec<RecipeResult>,
    pub ms: u128,
}

impl RecomputeReport {
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

/// A dataset version a recipe may read.
#[derive(Debug, Clone)]
struct Candidate {
    key: i32,
    version_id: i64,
    seq: i32,
    dsv: String,
    kind: String,
}

type CandRow = (String, i32, i64, i32, String, String);

async fn candidates(pool: &PgPool) -> Result<HashMap<String, Candidate>> {
    let rows: Vec<CandRow> = sqlx::query_as(
        "SELECT DISTINCT ON (d.id) d.id, d.key, v.id, v.seq, v.dsv, d.kind
         FROM catalog.datasets d JOIN data.dataset_versions v ON v.dataset_id = d.id
         WHERE v.status = 'approved' OR v.id IN (SELECT version_id FROM data.current_versions)
         ORDER BY d.id, (v.status = 'approved') DESC, v.seq DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.0,
                Candidate {
                    key: r.1,
                    version_id: r.2,
                    seq: r.3,
                    dsv: r.4,
                    kind: r.5,
                },
            )
        })
        .collect())
}

fn calendars_of(p: &Profile) -> Calendars {
    p.calendars.clone()
}

/// Runs every stale recipe in topological order (the train's step 3).
#[allow(clippy::too_many_lines)]
pub async fn recompute(env: &DeriveEnv<'_>, opts: &RecomputeOpts) -> Result<RecomputeReport> {
    let t0 = std::time::Instant::now();
    sync(env.pool, env.recipes, env.methods).await?;
    let specs: Vec<&RecipeSpec> = env.recipes.list.iter().map(|r| &r.spec).collect();
    let order = lanka_enrich::dag::order(&specs)?;
    let mut cands = candidates(env.pool).await?;
    let mut rep = RecomputeReport {
        recipes: specs.len(),
        ..RecomputeReport::default()
    };
    for i in order {
        let spec = specs[i];
        if opts.only.as_deref().is_some_and(|o| o != spec.id) {
            continue;
        }
        let r = run_one(env, opts, spec, &mut cands).await;
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                let text = format!("{e:#}");
                record_failure(env.pool, opts, spec, &text).await;
                RecipeResult {
                    recipe: spec.id.clone(),
                    output: spec.output.clone(),
                    outcome: "failed",
                    fingerprint: None,
                    version_id: None,
                    dsv: None,
                    rows: None,
                    ms: 0,
                    detail: json!({ "error": text }),
                }
            }
        };
        match r.outcome {
            "reused" => rep.reused += 1,
            "failed" | "qa_failed" => rep.failed += 1,
            "published" | "approved" | "in_review" | "unchanged" => rep.ran += 1,
            _ => {}
        }
        rep.results.push(r);
    }
    rep.ms = t0.elapsed().as_millis();
    Ok(rep)
}

async fn record_failure(pool: &PgPool, opts: &RecomputeOpts, spec: &RecipeSpec, text: &str) {
    let res = sqlx::query(
        "INSERT INTO ops.incidents (kind, subject, detail) VALUES ('recipe_failed', $1, $2)
         ON CONFLICT (kind, subject) WHERE closed_at IS NULL
         DO UPDATE SET last_seen_at = now(), detail = EXCLUDED.detail",
    )
    .bind(&spec.id)
    .bind(json!({ "error": text, "train_id": opts.train_id, "output": spec.output }))
    .execute(pool)
    .await;
    if let Err(e) = res {
        tracing::warn!(recipe = %spec.id, error = %e, "recording recipe_failed incident");
    }
    emit(
        pool,
        Level::Error,
        Stage::Recipe,
        &format!("recipe {} failed: {text}", spec.id),
        Event {
            kind: Some("recipe_failed"),
            job_id: opts.job_id,
            dataset_id: Some(&spec.output),
            data: Some(json!({ "recipe": spec.id, "train_id": opts.train_id })),
            ..Event::default()
        },
    )
    .await;
}

/// (version_id, status) of the derivation recorded for this fingerprint.
async fn derivation_for(
    pool: &PgPool,
    recipe: &str,
    fp: &[u8],
) -> Result<Option<(i64, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT d.version_id, v.status::text, v.dsv FROM data.derivations d
         JOIN data.dataset_versions v ON v.id = d.version_id
         WHERE d.recipe_id = $1 AND d.fingerprint = $2",
    )
    .bind(recipe)
    .bind(fp)
    .fetch_optional(pool)
    .await?)
}

#[allow(clippy::too_many_lines)]
async fn run_one(
    env: &DeriveEnv<'_>,
    opts: &RecomputeOpts,
    spec: &RecipeSpec,
    cands: &mut HashMap<String, Candidate>,
) -> Result<RecipeResult> {
    let t0 = std::time::Instant::now();
    let mut res = RecipeResult {
        recipe: spec.id.clone(),
        output: spec.output.clone(),
        outcome: "would_run",
        fingerprint: None,
        version_id: None,
        dsv: None,
        rows: None,
        ms: 0,
        detail: Value::Null,
    };
    if spec.schedule() != "daily" && !opts.include_scheduled && !opts.force && opts.only.is_none() {
        res.outcome = "skipped_schedule";
        res.detail = json!({ "schedule": spec.schedule() });
        return Ok(res);
    }
    // 1. pin inputs
    let mut pins = Vec::new();
    let mut missing = Vec::new();
    for (port, inp) in &spec.inputs {
        match cands.get(&inp.dataset) {
            Some(c) => pins.push(Pinned {
                port: port.clone(),
                dataset_id: inp.dataset.clone(),
                dataset_key: c.key,
                version_id: c.version_id,
                seq: c.seq,
                dsv: c.dsv.clone(),
                kind: c.kind.clone(),
                indicators: inp.indicators.clone(),
            }),
            None if inp.optional => {}
            None => missing.push(format!("{port} ({})", inp.dataset)),
        }
    }
    if !missing.is_empty() {
        res.outcome = "waiting_inputs";
        res.detail = json!({ "missing": missing });
        return Ok(res);
    }
    // 2. fingerprint
    let out_sha: Vec<u8> =
        sqlx::query_scalar("SELECT spec_sha256 FROM catalog.datasets WHERE id = $1")
            .bind(&spec.output)
            .fetch_optional(env.pool)
            .await?
            .with_context(|| format!("output dataset {} is not synced", spec.output))?;
    let methods = runner::resolve_methods(spec, env.methods)?;
    let tools: BTreeMap<String, String> = BTreeMap::new();
    let fp_inputs: Vec<FpInput> = pins
        .iter()
        .map(|p| FpInput {
            port: p.port.clone(),
            dataset: p.dataset_id.clone(),
            dsv: p.dsv.clone(),
        })
        .collect();
    let fp = runner::fingerprint(spec, &out_sha, &methods, &tools, &fp_inputs);
    res.fingerprint = Some(hex::encode(&fp[..8]));
    // 3. reuse
    let output_cand = cands.get(&spec.output).map(|c| c.version_id);
    if !opts.force
        && let Some((vid, status, dsv)) = derivation_for(env.pool, &spec.id, &fp).await?
        && (Some(vid) == output_cand
            || matches!(status.as_str(), "in_review" | "staged" | "approved"))
    {
        res.outcome = "reused";
        res.version_id = Some(vid);
        res.dsv = Some(dsv);
        res.detail = json!({ "version_status": status });
        res.ms = t0.elapsed().as_millis();
        return Ok(res);
    }
    if opts.dry_run {
        res.detail = json!({ "inputs": fp_inputs });
        return Ok(res);
    }
    // 4. run
    let ds = env
        .registry
        .dataset(&spec.output)
        .with_context(|| format!("output dataset {} not in the registry", spec.output))?;
    let source = ds
        .source_ids
        .first()
        .with_context(|| format!("{} lists no source", spec.output))?;
    let trigger = match opts.trigger.as_str() {
        t @ ("manual" | "test" | "retry") => t,
        _ => "schedule",
    };
    let tracker = RunTracker::start_for_job(env.pool, source, trigger, opts.job_id).await?;
    tracker
        .info(
            Stage::Recipe,
            &format!("recipe {} started", spec.id),
            Some(json!({ "recipe": spec.id, "fingerprint": hex::encode(fp), "inputs": fp_inputs })),
        )
        .await;
    let started = chrono::Utc::now();
    let outcome = execute_and_stage(env, spec, &pins, &fp, tracker.run_id).await;
    let staged = match outcome {
        Ok(s) => s,
        Err(e) => {
            tracker.fail(&e).await?;
            return Err(e);
        }
    };
    let Staged {
        output,
        result,
        gate,
    } = staged;
    let qa = json!(output.qa);
    let steps = json!(output.steps);
    let Some(result) = result else {
        let text = format!(
            "QA gate failed: {}",
            output
                .qa
                .iter()
                .filter(|q| !q.passed)
                .map(|q| format!("{} {}", q.check, q.detail))
                .collect::<Vec<_>>()
                .join("; ")
        );
        tracker.finish(RunStatus::Failed, &text).await?;
        record_failure(env.pool, opts, spec, &text).await;
        res.outcome = "qa_failed";
        res.rows = Some(i64::try_from(output.rows.len())?);
        res.detail = json!({ "qa": qa, "steps": steps });
        res.ms = t0.elapsed().as_millis();
        return Ok(res);
    };
    let deriv_outcome = if result.outcome == "unchanged" {
        "unchanged"
    } else {
        "new"
    };
    let report = json!({
        "steps": steps, "qa": qa, "not_emitted": output.not_emitted,
        "publish": { "outcome": result.outcome, "rows": result.rows, "rows_written": result.rows_written,
                     "diff": result.diff, "note": result.note },
        "gate": gate,
    });
    let methods_json: Vec<Value> = methods
        .iter()
        .map(|(s, id, v)| json!({ "step": s, "id": id, "version": v }))
        .collect();
    sqlx::query(
        "INSERT INTO data.derivations (version_id, recipe_id, fingerprint, outcome, methods, params, inputs,
            tools, git_rev, engine_version, report, job_id, train_id, started_at, finished_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, now())
         ON CONFLICT (recipe_id, fingerprint) DO UPDATE SET version_id = EXCLUDED.version_id,
            outcome = EXCLUDED.outcome, methods = EXCLUDED.methods, params = EXCLUDED.params,
            inputs = EXCLUDED.inputs, tools = EXCLUDED.tools, git_rev = EXCLUDED.git_rev,
            engine_version = EXCLUDED.engine_version, report = EXCLUDED.report, job_id = EXCLUDED.job_id,
            train_id = EXCLUDED.train_id, started_at = EXCLUDED.started_at, finished_at = now()",
    )
    .bind(result.version_id)
    .bind(&spec.id)
    .bind(fp.as_slice())
    .bind(deriv_outcome)
    .bind(json!(methods_json))
    .bind(spec.canonical_json())
    .bind(serde_json::to_value(&pins)?)
    .bind(json!(tools))
    .bind(lanka_core::git_rev())
    .bind(lanka_core::ENGINE_VERSION)
    .bind(&report)
    .bind(opts.job_id)
    .bind(opts.train_id)
    .bind(started)
    .execute(env.pool)
    .await?;
    if deriv_outcome == "new" {
        for p in &pins {
            sqlx::query(
                "INSERT INTO data.version_inputs (version_id, input_version_id, port) VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(result.version_id)
            .bind(p.version_id)
            .bind(&p.port)
            .execute(env.pool)
            .await?;
        }
    }
    sqlx::query(
        "UPDATE ops.incidents SET closed_at = now() WHERE kind = 'recipe_failed' AND subject = $1 AND closed_at IS NULL",
    )
    .bind(&spec.id)
    .execute(env.pool)
    .await?;
    let final_outcome: &'static str =
        match (result.outcome, gate.get("review").and_then(Value::as_bool)) {
            ("unchanged", _) => "unchanged",
            ("approved", Some(false)) => "approved",
            _ => "in_review",
        };
    let status = match final_outcome {
        "unchanged" => RunStatus::NoChange,
        "approved" => RunStatus::Succeeded,
        _ => RunStatus::NeedsReview,
    };
    tracker
        .finish(
            status,
            &format!("recipe {} → {} ({final_outcome})", spec.id, result.dsv),
        )
        .await?;
    emit(
        env.pool,
        Level::Info,
        Stage::Recipe,
        &format!(
            "recipe {} → {} {} ({final_outcome})",
            spec.id, spec.output, result.dsv
        ),
        Event {
            kind: Some("recipe_finished"),
            job_id: opts.job_id,
            dataset_id: Some(&spec.output),
            data: Some(
                json!({ "recipe": spec.id, "outcome": final_outcome, "dsv": result.dsv,
                               "train_id": opts.train_id }),
            ),
            ..Event::default()
        },
    )
    .await;
    if final_outcome == "approved" {
        let (seq, key): (i32, i32) = sqlx::query_as(
            "SELECT v.seq, d.key FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
             WHERE v.id = $1",
        )
        .bind(result.version_id)
        .fetch_one(env.pool)
        .await?;
        cands.insert(
            spec.output.clone(),
            Candidate {
                key,
                version_id: result.version_id,
                seq,
                dsv: result.dsv.clone(),
                kind: "indicators".into(),
            },
        );
    }
    res.outcome = final_outcome;
    res.version_id = Some(result.version_id);
    res.dsv = Some(result.dsv);
    res.rows = Some(result.rows);
    res.detail = json!({ "gate": gate, "rows_written": result.rows_written, "qa": qa });
    res.ms = t0.elapsed().as_millis();
    Ok(res)
}

struct Staged {
    output: runner::RecipeOutput,
    /// None when a QA gate failed (nothing was staged).
    result: Option<publish::PublishResult>,
    gate: Value,
}

/// Runs the recipe over its pinned inputs, stages the output and applies
/// the derived-publication gate (04 §7 "Publication of derived data").
async fn execute_and_stage(
    env: &DeriveEnv<'_>,
    spec: &RecipeSpec,
    pins: &[Pinned],
    fp: &[u8; 32],
    run_id: i64,
) -> Result<Staged> {
    let inputs = PgInputs::new(
        env.pool.clone(),
        calendars_of(env.profile),
        &env.profile.spec.admin.reference_boundary_set,
        &env.profile.spec.admin.scheme,
        pins.to_vec(),
    )
    .await?;
    let output = runner::execute(spec, env.methods, &inputs, seed_of(fp)).await?;
    if !output.qa_passed() {
        return Ok(Staged {
            output,
            result: None,
            gate: json!({ "review": true, "reason": "qa_failed" }),
        });
    }
    let cal = calendars_of(env.profile);
    let rows: Vec<ObsRow> = output
        .rows
        .iter()
        .map(|r| ObsRow {
            indicator_id: r.indicator.clone(),
            geo_code: r.geo.clone(),
            period: r.period(&cal),
            dims: r.dims.clone(),
            value: r.value,
            lo: r.lo,
            hi: r.hi,
            basis: r.basis,
            status: r.status,
            flags: r.flags,
            // No artifact: derived rows get no obs_provenance (ADR 0005).
            artifact: usize::MAX,
            prov: None,
            locator: Value::Null,
            verbatim: None,
            method: Method::Derived,
            confidence: None,
        })
        .collect();
    anyhow::ensure!(!rows.is_empty(), "recipe {} emitted no rows", spec.id);
    let had_version: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM data.current_versions WHERE dataset_id = $1)",
    )
    .bind(&spec.output)
    .fetch_one(env.pool)
    .await?;
    // The published derivation this run would replace.
    let prev: Option<(Value, Value)> = sqlx::query_as(
        "SELECT d.methods, d.params FROM data.derivations d
         JOIN data.current_versions c ON c.version_id = d.version_id
         WHERE d.recipe_id = $1 ORDER BY d.id DESC LIMIT 1",
    )
    .bind(&spec.id)
    .fetch_optional(env.pool)
    .await?;
    let result = publish_observations(
        env.pool,
        env.registry,
        env.profile,
        StageInput {
            run_id,
            dataset_id: &spec.output,
            vintage: None,
            rows: &rows,
            artifact_ids: &[],
            extract_ids: &[],
        },
    )
    .await?;
    let methods = runner::resolve_methods(spec, env.methods)?;
    let majors = |m: &Value| -> Vec<(String, String)> {
        m.as_array()
            .map(|a| {
                a.iter()
                    .map(|x| {
                        (
                            x["id"].as_str().unwrap_or_default().to_owned(),
                            x["version"]
                                .as_str()
                                .unwrap_or_default()
                                .split('.')
                                .next()
                                .unwrap_or_default()
                                .to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let now_majors: Vec<(String, String)> = methods
        .iter()
        .map(|(_, id, v)| {
            (
                id.clone(),
                v.split('.').next().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let mut reasons: Vec<String> = Vec::new();
    if spec.stability != lanka_enrich::recipe::Stability::Stable {
        reasons.push("recipe is experimental".into());
    }
    if !had_version {
        reasons.push("first version of the dataset".into());
    }
    if let Some((pm, pp)) = &prev {
        if majors(pm) != now_majors {
            reasons.push("method MAJOR version changed".into());
        }
        if strip_doc(pp) != strip_doc(&spec.canonical_json()) {
            reasons.push("recipe parameters changed".into());
        }
    }
    if result.outcome == "in_review" {
        reasons.push(result.note.clone());
    }
    let review = !reasons.is_empty() && result.outcome != "unchanged";
    if review && result.outcome == "approved" {
        let mut tx = env.pool.begin().await?;
        sqlx::query(
            "UPDATE data.dataset_versions SET status = 'in_review', approved_at = NULL, review_note = NULL
             WHERE id = $1",
        )
        .bind(result.version_id)
        .execute(&mut *tx)
        .await?;
        publish::open_review(&mut tx, result.version_id, &reasons.join("; ")).await?;
        tx.commit().await?;
    }
    Ok(Staged {
        output,
        result: Some(result),
        gate: json!({ "review": review, "reasons": reasons }),
    })
}

/// The spec without fields that do not change outputs (title).
fn strip_doc(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("title");
    }
    v
}

/// Recipe status for `lanka recipe list`: what the next train would do.
pub async fn status(env: &DeriveEnv<'_>) -> Result<RecomputeReport> {
    recompute(
        env,
        &RecomputeOpts {
            dry_run: true,
            trigger: "manual".into(),
            ..RecomputeOpts::default()
        },
    )
    .await
}
