//! The synchronous executor and result store (11 §7–§9).
//!
//! `run`: parse → resolve/plan → fingerprint → stored? (same id, same
//! bytes) → align → guards → compute (blocking pool) → present → store.
//! Results are written once: the exact bytes go to the blob store
//! (`analysis/<id>/result.json`, outputs beside them), the jsonb copy and
//! the input versions to `analysis.results` / `analysis.result_inputs`.
//! Requests over the sync budget are refused until the interactive lane
//! (WP-30) exists.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use lanka_core::Catalog;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::frame::AlignedFrame;
use crate::guards::{self, Raised};
use crate::method::{Ctx, ExposureFrame, MethodOutput};
use crate::methods::Registry;
use crate::pipeline::{self, BadRequest, Env, Resolved, bad};
use crate::request::{Problem, Request};
use crate::result::{self, Present};

pub type CatalogFn = Arc<dyn Fn() -> Arc<Catalog> + Send + Sync>;

/// A grid layer the host already holds memory-mapped (the server's served
/// layers): the cells and the dataset that owns them.
#[derive(Clone)]
pub struct MappedLayer {
    pub grid: Arc<lanka_grid::MappedGrid>,
    pub dataset_id: String,
}

/// Looks up a mapped layer by `(id, period)`; `None` → read it from the store.
pub type MappedGridFn = Arc<dyn Fn(&str, &str) -> Option<MappedLayer> + Send + Sync>;

/// Wall-time budget of the synchronous path (11 §8: ≤ 2 s expected →
/// sync). WP-29 has no async lane yet, so up to this estimate is run
/// inline; above it the request is refused.
pub const SYNC_BUDGET_MS: u64 = 30_000;
/// Blocking pool: 2 analyses at once, 8 waiting (11 §8).
pub const WORKERS: usize = 2;
pub const QUEUE: usize = 8;

#[derive(Debug)]
pub enum EngineError {
    /// 400: schema problems or an unresolvable request (with hints).
    Bad {
        message: String,
        problems: Vec<Problem>,
        hints: Value,
    },
    /// 503: the blocking pool and its queue are full.
    Busy,
    /// 422: over the synchronous budget (needs the async lane, WP-30).
    TooCostly {
        expected_ms: u64,
    },
    Internal(anyhow::Error),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad { message, .. } => f.write_str(message),
            Self::Busy => f.write_str("analysis pool busy"),
            Self::TooCostly { expected_ms } => write!(
                f,
                "expected {expected_ms} ms exceeds the synchronous budget"
            ),
            Self::Internal(e) => write!(f, "{e:#}"),
        }
    }
}

impl From<anyhow::Error> for EngineError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<BadRequest>() {
            Ok(b) => Self::Bad {
                message: b.message,
                problems: b.problems,
                hints: b.hints,
            },
            Err(e) => Self::Internal(e),
        }
    }
}

impl From<sqlx::Error> for EngineError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

/// A computed or stored result.
#[derive(Debug, Clone)]
pub struct Ran {
    pub id: String,
    pub state: String,
    /// The exact stored bytes of the result document.
    pub bytes: Arc<Vec<u8>>,
    /// Computed by this call (false = served from the store).
    pub computed: bool,
}

pub struct Engine {
    pub pool: PgPool,
    pub catalog: CatalogFn,
    pub store_root: Option<PathBuf>,
    pub methods: Registry,
    pub base_url: String,
    pub engine: String,
    pub git_rev: String,
    /// The host's memory-mapped grid layers (`exposure` reads them in place).
    pub mapped: Option<MappedGridFn>,
    sem: Arc<Semaphore>,
    waiting: AtomicUsize,
}

fn noun_of(cat: &Catalog, level: i16) -> String {
    let Some(p) = cat.profile.as_deref() else {
        return format!("level-{level} units");
    };
    let Some(l) = p
        .spec
        .admin
        .levels
        .iter()
        .find(|l| i16::from(l.level) == level)
    else {
        return format!("level-{level} units");
    };
    let name = &l.name.en;
    let mut n = if name.contains(' ') {
        name.clone()
    } else {
        name.to_lowercase()
    };
    if let Some(stem) = n.strip_suffix('y') {
        n = format!("{stem}ies");
    } else {
        n.push('s');
    }
    n
}

impl Engine {
    /// The public origin of citation permalinks (`""` = relative).
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn new(
        pool: PgPool,
        catalog: CatalogFn,
        store_root: Option<PathBuf>,
        base_url: String,
        git_rev: String,
    ) -> Self {
        Self {
            pool,
            catalog,
            store_root,
            methods: Registry::builtin(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            engine: format!("lanka {}", env!("CARGO_PKG_VERSION")),
            git_rev,
            mapped: None,
            sem: Arc::new(Semaphore::new(WORKERS)),
            waiting: AtomicUsize::new(0),
        }
    }

    /// Reads grid layers from the host's memory-mapped set when it has them.
    #[must_use]
    pub fn with_mapped_grids(mut self, f: MappedGridFn) -> Self {
        self.mapped = Some(f);
        self
    }

    fn env(&self) -> Env {
        Env {
            pool: self.pool.clone(),
            catalog: (self.catalog)(),
            store_root: self.store_root.clone(),
            methods: self.methods.clone(),
            mapped: self.mapped.clone(),
        }
    }

    /// `GET /v2/analysis/methods`.
    #[must_use]
    pub fn methods_json(&self) -> Value {
        json!({
            "methods": self.methods.iter().map(|m| m.info().to_json()).collect::<Vec<_>>(),
            "guards": guards::registry_json(),
        })
    }

    fn parse(body: &Value) -> Result<Request, EngineError> {
        Request::parse(body).map_err(|problems| EngineError::Bad {
            message: "the request is not a valid ldl-analysis/1 document".to_owned(),
            problems,
            hints: json!({}),
        })
    }

    /// `POST /v2/analyses/plan`: the dry run.
    pub async fn plan(&self, body: &Value) -> Result<Value, EngineError> {
        let req = Self::parse(body)?;
        let env = self.env();
        let r = pipeline::resolve(&env, &req).await?;
        let fp = result::fingerprint(&r);
        let cat = &env.catalog;
        let noun = noun_of(cat, r.level);
        let n_expected = self.support_size(&r).await?;
        let expected_ms = r.method.cost_ms(n_expected, &r.request.params);
        let raised = result::plan_guards(&r, &noun, cat.boundary_dataset());
        let alignment: Vec<Value> = r
            .vars
            .iter()
            .map(|v| json!({"role": v.role, "from": from_label(v), "to": format!("admin:{}", r.level),
                            "method": v.method_ref(), "params": v.var.align, "coverage": 1.0, "period": v.input.period}))
            .collect();
        Ok(json!({
            "id": result::id_of(&fp),
            "request": r.request,
            "support": {"kind": "admin", "level": r.level, "within": r.within, "n_expected": n_expected},
            "alignment": alignment,
            "cost": {"mode": if expected_ms <= 2_000 { "sync" } else { "async" }, "expected_ms": expected_ms,
                     "within_quota": expected_ms <= SYNC_BUDGET_MS},
            "guards": guards::finish(&raised, &r.request.overrides),
        }))
    }

    async fn support_size(&self, r: &Resolved) -> Result<usize> {
        let set = (self.catalog)().reference_set.clone().unwrap_or_default();
        let n: i64 = match &r.within {
            None => sqlx::query_scalar(
                "SELECT count(*) FROM geo.units u JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $1
                 WHERE u.level = $2 AND u.valid_to IS NULL",
            )
            .bind(&set)
            .bind(r.level)
            .fetch_one(&self.pool)
            .await?,
            Some(w) => sqlx::query_scalar(
                "SELECT count(*) FROM geo.units u JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $1
                 WHERE u.level = $2 AND u.valid_to IS NULL
                   AND (SELECT key FROM geo.units WHERE code = $3) = ANY (u.anc_keys)",
            )
            .bind(&set)
            .bind(r.level)
            .bind(w)
            .fetch_one(&self.pool)
            .await?,
        };
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// A stored result's bytes (touches `last_read_at` at most daily).
    pub async fn get(&self, id: &str) -> Result<Option<Ran>, EngineError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT state FROM analysis.results WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some((state,)) = row else { return Ok(None) };
        let bytes = self.read_blob(&format!("{id}/result.json")).await?;
        let _ = sqlx::query("UPDATE analysis.results SET last_read_at = current_date WHERE id = $1 AND last_read_at < current_date")
            .bind(id)
            .execute(&self.pool)
            .await;
        Ok(Some(Ran {
            id: id.to_owned(),
            state,
            bytes: Arc::new(bytes),
            computed: false,
        }))
    }

    /// An output's data (`{codes, columns}` for frames, the chart data).
    pub async fn output(&self, id: &str, output: &str) -> Result<Option<Value>, EngineError> {
        if !output
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || output.is_empty()
        {
            return Ok(None);
        }
        let exists: Option<Value> =
            sqlx::query_scalar("SELECT outputs FROM analysis.results WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(list) = exists else { return Ok(None) };
        if !list
            .as_array()
            .is_some_and(|a| a.iter().any(|o| o["id"] == output))
        {
            return Ok(None);
        }
        let b = self
            .read_blob(&format!("{id}/outputs/{output}.json"))
            .await?;
        Ok(Some(serde_json::from_slice(&b).context("output json")?))
    }

    fn store(&self) -> Result<lanka_store::FsStore> {
        Ok(lanka_store::FsStore::new(self.store_root.clone().context(
            "analysis needs the blob store (LANKA_STORE_DIR)",
        )?))
    }

    async fn read_blob(&self, key: &str) -> Result<Vec<u8>> {
        let p = self.store()?.path(lanka_store::Ns::Analysis, key)?;
        tokio::fs::read(&p)
            .await
            .with_context(|| format!("reading {}", p.display()))
    }

    async fn write_blob(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let p = self.store()?.path(lanka_store::Ns::Analysis, key)?;
        // A result is published by its `analysis.results` row; a blob left
        // by a run that never got its row is simply replaced.
        if tokio::fs::read(&p)
            .await
            .is_ok_and(|existing| existing == bytes)
        {
            return Ok(());
        }
        let dir = p.parent().context("parent")?;
        tokio::fs::create_dir_all(dir).await?;
        let tmp = dir.join(format!(
            ".{}.tmp-{}",
            p.file_name().and_then(|s| s.to_str()).unwrap_or("x"),
            std::process::id()
        ));
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::File::open(&tmp).await?.sync_all().await?;
        tokio::fs::rename(&tmp, &p).await?;
        Ok(())
    }

    /// `POST /v2/analyses`: the stored result, or compute and store it.
    pub async fn run(&self, body: &Value) -> Result<Ran, EngineError> {
        let req = Self::parse(body)?;
        let env = self.env();
        let r = pipeline::resolve(&env, &req).await?;
        let fp = result::fingerprint(&r);
        let id = result::id_of(&fp);
        if let Some(stored) = self.get(&id).await? {
            return Ok(stored);
        }
        let (doc, outputs) = self.compute(&env, &r, &fp).await?;
        self.persist(&r, &fp, &id, &doc, &outputs).await
    }

    /// Computes without consulting or writing the store (determinism tests,
    /// `lanka analyze`).
    pub async fn compute_only(
        &self,
        body: &Value,
    ) -> Result<(String, Value, Vec<(String, Value)>), EngineError> {
        let req = Self::parse(body)?;
        let env = self.env();
        let r = pipeline::resolve(&env, &req).await?;
        let fp = result::fingerprint(&r);
        let (doc, outputs) = self.compute(&env, &r, &fp).await?;
        Ok((result::id_of(&fp), doc, outputs))
    }

    #[allow(clippy::too_many_lines)]
    async fn compute(
        &self,
        env: &Env,
        r: &Resolved,
        fp: &[u8; 32],
    ) -> Result<(Value, Vec<(String, Value)>), EngineError> {
        let t0 = Instant::now();
        let cat = env.catalog.clone();
        let n_support = self.support_size(r).await?;
        let expected = r.method.cost_ms(n_support, &r.request.params);
        if expected > SYNC_BUDGET_MS {
            return Err(EngineError::TooCostly {
                expected_ms: expected,
            });
        }
        if self.waiting.fetch_add(1, Ordering::SeqCst) >= WORKERS + QUEUE {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            return Err(EngineError::Busy);
        }
        let permit = self.sem.clone().acquire_owned().await;
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        let _permit = permit.map_err(|e| EngineError::Internal(e.into()))?;

        let (frame, scale, exposure) = pipeline::align(env, r).await?;
        let meta = EvalMeta {
            base_url: self.base_url.clone(),
            engine: self.engine.clone(),
            git_rev: self.git_rev.clone(),
        };
        let (r2, cat2, fp2) = (r.clone(), cat.clone(), *fp);
        let (mut doc, outputs) = tokio::task::spawn_blocking(move || {
            evaluate(&r2, &fp2, &frame, &scale, exposure.as_ref(), &cat2, &meta)
        })
        .await
        .map_err(|e| EngineError::Internal(e.into()))??;
        doc["provenance"]["elapsed_ms"] =
            json!(u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX));
        doc["provenance"]["computed_at"] =
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        let year = doc["provenance"]["computed_at"]
            .as_str()
            .and_then(|s| s.get(..4))
            .unwrap_or("2026")
            .to_owned();
        if let Some(t) = doc["citation"]["text"].as_str() {
            doc["citation"]["text"] = json!(t.replacen(
                "Lanka Data Layer (0000)",
                &format!("Lanka Data Layer ({year})"),
                1
            ));
        }
        Ok((doc, outputs))
    }

    /// Resolve + align + evaluate without the store, returning the parts
    /// (golden fixtures are written from them).
    pub async fn parts(&self, body: &Value) -> Result<Parts, EngineError> {
        let req = Self::parse(body)?;
        let env = self.env();
        let r = pipeline::resolve(&env, &req).await?;
        let fp = result::fingerprint(&r);
        let (frame, scale, exposure) = pipeline::align(&env, &r).await?;
        let meta = EvalMeta {
            base_url: self.base_url.clone(),
            engine: self.engine.clone(),
            git_rev: self.git_rev.clone(),
        };
        let (doc, outputs) = evaluate(
            &r,
            &fp,
            &frame,
            &scale,
            exposure.as_ref(),
            &env.catalog,
            &meta,
        )?;
        Ok(Parts {
            resolved: r,
            frame,
            scale,
            exposure,
            doc,
            outputs,
        })
    }

    async fn persist(
        &self,
        r: &Resolved,
        fp: &[u8; 32],
        id: &str,
        doc: &Value,
        outputs: &[(String, Value)],
    ) -> Result<Ran, EngineError> {
        let bytes = serde_json::to_vec(doc).map_err(|e| EngineError::Internal(e.into()))?;
        let mut listed = Vec::new();
        for (oid, data) in outputs {
            let b = serde_json::to_vec(data).map_err(|e| EngineError::Internal(e.into()))?;
            self.write_blob(&format!("{id}/outputs/{oid}.json"), &b)
                .await?;
            let kind = doc["outputs"]
                .as_array()
                .and_then(|a| a.iter().find(|o| o["id"] == oid.as_str()))
                .map_or(Value::Null, |o| o["kind"].clone());
            listed.push(
                json!({"id": oid, "kind": kind, "blob": format!("analysis/{id}/outputs/{oid}.json"),
                               "bytes": b.len(), "sha256": hex::encode(Sha256::digest(&b))}),
            );
        }
        self.write_blob(&format!("{id}/result.json"), &bytes)
            .await?;
        let state = doc["state"].as_str().unwrap_or("ready").to_owned();
        let basis: i16 = match doc["basis"].as_str() {
            Some("reported") => 0,
            Some("estimated") => 2,
            Some("forecast") => 3,
            _ => 1,
        };
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO analysis.results (id, fingerprint, method, method_version, request, state, result, result_sha256,
                                           outputs, basis, licence_id, redistribution, data_version, cost, elapsed_ms)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(fp.as_slice())
        .bind(r.method.info().id)
        .bind(r.method.info().version)
        .bind(serde_json::to_value(&r.request).map_err(|e| EngineError::Internal(e.into()))?)
        .bind(&state)
        .bind(doc)
        .bind(Sha256::digest(&bytes).as_slice())
        .bind(Value::Array(listed))
        .bind(basis)
        .bind(doc["licence"]["id"].as_str())
        .bind(doc["licence"]["redistribution"].as_str())
        .bind(&r.data_version)
        .bind(i32::try_from(r.method.cost_ms(r.vars.len(), &r.request.params)).unwrap_or(i32::MAX))
        .bind(i32::try_from(doc["provenance"]["elapsed_ms"].as_u64().unwrap_or(0)).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 1 {
            for v in &r.vars {
                sqlx::query(
                    "INSERT INTO analysis.result_inputs (result_id, dataset_ref, dsv, role) VALUES ($1, $2, $3, $4)
                     ON CONFLICT DO NOTHING",
                )
                .bind(id)
                .bind(&v.version.dataset_id)
                .bind(&v.version.dsv)
                .bind(&v.role)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        if inserted == 0 {
            // a concurrent twin stored it first: serve its bytes
            if let Some(s) = self.get(id).await? {
                return Ok(s);
            }
        }
        Ok(Ran {
            id: id.to_owned(),
            state,
            bytes: Arc::new(bytes),
            computed: true,
        })
    }
}

fn from_label(v: &pipeline::ResolvedVar) -> String {
    match &v.source {
        pipeline::Source::Indicator { source_level, .. } => format!("admin:{source_level}"),
        pipeline::Source::Grid { .. } => "grid:1".to_owned(),
        pipeline::Source::Features { dataset_id, .. } => format!("features:{dataset_id}"),
    }
}

/// First letter upper-case (labels are lower-case inside sentences).
fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// What `evaluate` needs besides the data.
#[derive(Debug, Clone)]
pub struct EvalMeta {
    pub base_url: String,
    pub engine: String,
    pub git_rev: String,
}

pub struct Parts {
    pub resolved: Resolved,
    pub frame: AlignedFrame,
    pub scale: Vec<AlignedFrame>,
    pub exposure: Option<ExposureFrame>,
    pub doc: Value,
    pub outputs: Vec<(String, Value)>,
}

/// GUARD → COMPUTE → PRESENT over an aligned frame: pure and
/// deterministic (no database, no clock). `provenance.computed_at` is
/// `"0000-00-00T00:00:00Z"` and `elapsed_ms` 0; the caller stamps them.
pub fn evaluate(
    r: &Resolved,
    fp: &[u8; 32],
    frame: &AlignedFrame,
    scale: &[AlignedFrame],
    exposure: Option<&ExposureFrame>,
    cat: &Catalog,
    meta: &EvalMeta,
) -> Result<(Value, Vec<(String, Value)>), EngineError> {
    let noun = noun_of(cat, r.level);
    let id = result::id_of(fp);
    let seed = lanka_stats::rng::seed_of(fp);
    let info = r.method.info();
    let mut raised: Vec<Raised> = result::plan_guards(r, &noun, cat.boundary_dataset());
    let roles: Vec<String> = r
        .vars
        .iter()
        .filter(|v| v.role != "weight")
        .map(|v| v.role.clone())
        .collect();
    let role_refs: Vec<&str> = roles.iter().map(String::as_str).collect();
    let complete = frame.complete(&role_refs);
    raised.extend(result::frame_guards(
        info,
        frame,
        complete.len(),
        &noun,
        &role_refs,
    ));
    let pre_blocked = guards::blocks(&guards::finish(&raised, &r.request.overrides));
    let mut out: MethodOutput = if pre_blocked {
        MethodOutput {
            estimates: Value::Null,
            n_used: complete.len(),
            dropped: frame.drop_counts(&role_refs),
            ..MethodOutput::default()
        }
    } else {
        let level_nouns: Vec<(i16, String)> = (0..=4).map(|l| (l, noun_of(cat, l))).collect();
        let noun_of = |l: i16| {
            level_nouns
                .iter()
                .find(|x| x.0 == l)
                .map_or_else(|| format!("level {l}"), |x| x.1.clone())
        };
        let ctx = Ctx {
            frame,
            scale,
            params: &r.request.params,
            seed,
            noun: &noun,
            noun_of: &noun_of,
            exposure,
        };
        r.method
            .run(&ctx)
            .map_err(|e| EngineError::from(bad(e.to_string(), json!({}))))?
    };
    raised.extend(out.raised.iter().cloned());
    out.summary = capitalise(&out.summary);
    let present = Present {
        resolved: r,
        id: &id,
        fingerprint: fp,
        seed,
        frame: Some(frame),
        noun: &noun,
        base_url: &meta.base_url,
        engine: &meta.engine,
        git_rev: &meta.git_rev,
        computed_at: "0000-00-00T00:00:00Z",
        elapsed_ms: 0,
        catalog: cat,
    };
    let doc = result::document(&present, Some(&out), &raised);
    let outputs = if doc["state"] == "blocked" {
        Vec::new()
    } else {
        out.outputs
            .iter()
            .map(|o| (o.id.to_owned(), o.data.clone()))
            .collect()
    };
    Ok((doc, outputs))
}
