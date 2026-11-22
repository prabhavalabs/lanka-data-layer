//! `lanka worker`: scheduler tick + executor over `ops.jobs` (ADR 0011).
//!
//! - **One leader per lane** (session advisory lock
//!   `lanka_worker_leader:<lane>`); a second instance idles as a standby and
//!   takes over when the leader's session ends.
//! - The leader ticks every 60 s (scheduler, incidents, registry re-sync)
//!   and is woken immediately by `NOTIFY lanka_jobs` on enqueue.
//! - It runs up to `slots` worth of job weight at once; every job holds its
//!   advisory lock on a dedicated session, heartbeats every 15 s, and is
//!   aborted when `cancel_requested` is set.
//! - SIGTERM stops claiming, lets running jobs finish for the grace period,
//!   then re-queues what is left (attempt not spent) — a deploy restarts the
//!   worker between jobs.
//! - All state is in Postgres: `ops.workers` (tick age, role, disk) and the
//!   event feed are what `/v2/status` reports.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lanka_core::Profile;
use lanka_store::FsStore;
use serde_json::json;
use sqlx::PgPool;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgListener, Postgres};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinSet;

use crate::connector::RunOptions;
use crate::derive;
use crate::jobs::{self, Claimed, Job};
use crate::pipeline::{Env, restandardise_for_job, run_source};
use crate::registry::Registry;
use crate::run::{Event, Level, Stage, emit};
use crate::scheduler::{self, TickConfig};
use crate::train;

/// Job kinds this engine version executes per lane; other kinds stay queued.
#[must_use]
pub fn executable_kinds(lane: &str) -> &'static [&'static str] {
    match lane {
        "batch" => &["ingest", "restandardise", "release", "recipe"],
        _ => &[],
    }
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// `batch` (ingest, release, …) or `interactive` (analysis, uploads; WP-29+).
    pub lane: String,
    /// Slot budget summed over running jobs' weights (`LANKA_WORKER_SLOTS`, 4).
    pub slots: i16,
    pub tick: Duration,
    pub registry_dir: PathBuf,
    pub profile_path: Option<PathBuf>,
    pub store_root: PathBuf,
    /// How long running jobs may finish after SIGTERM before being re-queued.
    pub grace: Duration,
    /// One tick, drain the runnable queue, exit (tests, cron-less one-shots).
    pub once: bool,
    pub tick_cfg: TickConfig,
}

struct Shared {
    pool: PgPool,
    registry_dir: PathBuf,
    profile_path: Option<PathBuf>,
    store: FsStore,
    /// Registry syncs are serialised inside the process.
    sync_lock: Mutex<()>,
}

impl Shared {
    async fn load(&self) -> Result<(Registry, Profile)> {
        let reg = Registry::load(&self.registry_dir)?;
        let path = match &self.profile_path {
            Some(p) => p.clone(),
            None => Profile::discover(&self.registry_dir)?,
        };
        let profile =
            Profile::load(&path).with_context(|| format!("loading profile {}", path.display()))?;
        {
            let _g = self.sync_lock.lock().await;
            reg.sync(&self.pool, &profile)
                .await
                .context("syncing registry")?;
        }
        Ok((reg, profile))
    }
}

/// What the executor did with a job.
enum Outcome {
    Done(Result<serde_json::Value>),
    Cancelled,
    Shutdown,
}

/// Runs the worker until `shutdown` resolves (or, with `once`, until the
/// runnable queue is drained).
#[allow(clippy::too_many_lines)]
pub async fn run(
    pool: PgPool,
    cfg: WorkerConfig,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_owned());
    let pid = std::process::id();
    let worker_id = format!(
        "{host}:{pid}:{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S")
    );
    let kinds = executable_kinds(&cfg.lane);
    let shared = Arc::new(Shared {
        pool: pool.clone(),
        registry_dir: cfg.registry_dir.clone(),
        profile_path: cfg.profile_path.clone(),
        store: FsStore::new(&cfg.store_root),
        sync_lock: Mutex::new(()),
    });
    sqlx::query(
        "INSERT INTO ops.workers (id, lane, role, host, pid, engine_version, git_rev, slots, detail)
         VALUES ($1, $2, 'standby', $3, $4, $5, $6, $7, $8)",
    )
    .bind(&worker_id)
    .bind(&cfg.lane)
    .bind(&host)
    .bind(i32::try_from(pid).unwrap_or_default())
    .bind(lanka_core::ENGINE_VERSION)
    .bind(lanka_core::git_rev())
    .bind(cfg.slots)
    .bind(json!({ "kinds": kinds, "tick_s": cfg.tick.as_secs(), "once": cfg.once }))
    .execute(&pool)
    .await?;
    let ev = |kind: &'static str| Event {
        kind: Some(kind),
        data: Some(json!({ "worker": worker_id, "lane": cfg.lane })),
        ..Event::default()
    };
    emit(
        &pool,
        Level::Info,
        Stage::Worker,
        &format!(
            "worker {worker_id} started (lane {}, {} slots)",
            cfg.lane, cfg.slots
        ),
        ev("worker_started"),
    )
    .await;

    let mut leader: Option<PoolConnection<Postgres>> = None;
    let mut listener = PgListener::connect_with(&pool).await?;
    listener.listen("lanka_jobs").await?;
    let mut ticker = tokio::time::interval(cfg.tick);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut running: JoinSet<i16> = JoinSet::new();
    let mut used: i16 = 0;
    let mut last_sync = std::time::Instant::now();
    let mut startup_synced = false;
    tokio::pin!(shutdown);

    loop {
        // ── leadership ──
        if let Some(conn) = leader.as_mut()
            && sqlx::query("SELECT 1").execute(&mut **conn).await.is_err()
        {
            tracing::warn!("leader session lost; standing by");
            leader = None;
        }
        if leader.is_none() {
            let mut conn = pool.acquire().await?;
            let got: bool = sqlx::query_scalar(
                "SELECT pg_try_advisory_lock(hashtext('lanka_worker_leader:' || $1))",
            )
            .bind(&cfg.lane)
            .fetch_one(&mut *conn)
            .await?;
            if got {
                leader = Some(conn);
                sqlx::query("UPDATE ops.workers SET role = 'leader' WHERE id = $1")
                    .bind(&worker_id)
                    .execute(&pool)
                    .await?;
                emit(
                    &pool,
                    Level::Info,
                    Stage::Worker,
                    &format!("worker {worker_id} is the {} leader", cfg.lane),
                    ev("worker_leader"),
                )
                .await;
            }
        }
        let is_leader = leader.is_some();

        // ── wait for something to do ──
        let mut do_tick = false;
        tokio::select! {
            () = &mut shutdown => break,
            _ = ticker.tick() => do_tick = true,
            n = listener.recv() => {
                if let Err(e) = n {
                    tracing::warn!(error = %e, "job listener error");
                }
            }
            Some(done) = running.join_next(), if !running.is_empty() => {
                used -= done.unwrap_or(0);
            }
        }

        if do_tick {
            if is_leader && cfg.lane == "batch" {
                if !startup_synced || last_sync.elapsed() > Duration::from_secs(900) {
                    match shared.load().await {
                        Ok(_) => {
                            startup_synced = true;
                            last_sync = std::time::Instant::now();
                        }
                        Err(e) => {
                            tracing::error!(error = %format!("{e:#}"), "registry sync failed")
                        }
                    }
                }
                let mut tc = cfg.tick_cfg.clone();
                tc.store_pct = store_pct(&cfg.store_root);
                match scheduler::tick(&pool, &tc).await {
                    Ok(r) => tracing::debug!(?r, "tick"),
                    Err(e) => tracing::error!(error = %format!("{e:#}"), "scheduler tick failed"),
                }
            }
            let pct = store_pct(&cfg.store_root);
            if let Err(e) = sqlx::query(
                "UPDATE ops.workers SET last_tick_at = now(), role = $2, store_pct = $3 WHERE id = $1",
            )
            .bind(&worker_id)
            .bind(if is_leader { "leader" } else { "standby" })
            .bind(pct)
            .execute(&pool)
            .await
            {
                tracing::warn!(error = %e, "worker heartbeat failed");
            }
            let _ = sqlx::query(
                "DELETE FROM ops.workers WHERE role = 'stopped' AND stopped_at < now() - interval '7 days'",
            )
            .execute(&pool)
            .await;
        }

        // ── fill the slots ──
        while is_leader && used < cfg.slots {
            let claimed =
                match jobs::claim(&pool, &worker_id, &cfg.lane, cfg.slots - used, kinds).await {
                    Ok(Some(c)) => c,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!(error = %format!("{e:#}"), "claim failed");
                        break;
                    }
                };
            let w = claimed.job.weight;
            used += w;
            let sh = Arc::clone(&shared);
            let rx = stop_rx.clone();
            running.spawn(async move {
                execute_claimed(sh, claimed, rx).await;
                w
            });
        }

        if cfg.once && do_tick && running.is_empty() && is_leader {
            // Nothing runnable left after a tick and a drain.
            let more: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM ops.jobs WHERE state = 'queued' AND lane = $1
                                AND run_after <= now() AND kind = ANY($2))",
            )
            .bind(&cfg.lane)
            .bind(kinds)
            .fetch_one(&pool)
            .await?;
            if !more {
                break;
            }
        }
        if cfg.once && !do_tick && running.is_empty() && is_leader {
            ticker.reset_immediately();
        }
    }

    // ── shutdown: let running jobs finish, then re-queue the rest ──
    if !running.is_empty() {
        tracing::info!(
            jobs = running.len(),
            grace_s = cfg.grace.as_secs(),
            "waiting for running jobs"
        );
        let drained = tokio::time::timeout(cfg.grace, async {
            while running.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            let _ = stop_tx.send(true);
            while running.join_next().await.is_some() {}
        }
    }
    sqlx::query("UPDATE ops.workers SET role = 'stopped', stopped_at = now() WHERE id = $1")
        .bind(&worker_id)
        .execute(&pool)
        .await?;
    emit(
        &pool,
        Level::Info,
        Stage::Worker,
        &format!("worker {worker_id} stopped"),
        ev("worker_stopped"),
    )
    .await;
    drop(leader);
    Ok(())
}

/// Runs one claimed job with heartbeats, cancellation and shutdown handling,
/// records the outcome and releases the job lock.
async fn execute_claimed(sh: Arc<Shared>, claimed: Claimed, mut stop: watch::Receiver<bool>) {
    let job = claimed.job.clone();
    let pool = sh.pool.clone();
    let outcome = {
        let fut = execute(&sh, &job);
        tokio::pin!(fut);
        let mut hb = tokio::time::interval(jobs::HEARTBEAT);
        hb.tick().await;
        loop {
            tokio::select! {
                r = &mut fut => break Outcome::Done(r),
                _ = hb.tick() => match jobs::heartbeat(&pool, job.id).await {
                    Ok(true) => break Outcome::Cancelled,
                    Ok(false) => {}
                    Err(e) => tracing::warn!(job = job.id, error = %e, "heartbeat failed"),
                },
                r = stop.changed() => if r.is_err() || *stop.borrow() { break Outcome::Shutdown },
            }
        }
    };
    let res = match outcome {
        Outcome::Done(Ok(v)) => jobs::succeed(&pool, &job, &v).await,
        Outcome::Done(Err(e)) => {
            let retryable = !format!("{e:#}").contains("not executable");
            jobs::fail(&pool, &job, &format!("{e:#}"), retryable)
                .await
                .map(|_| ())
        }
        Outcome::Cancelled => jobs::mark_cancelled(&pool, &job, "cancel requested").await,
        Outcome::Shutdown => jobs::requeue(&pool, &job, "worker shutting down").await,
    };
    if let Err(e) = res {
        tracing::error!(job = job.id, error = %format!("{e:#}"), "recording job outcome failed");
    }
    claimed.release().await;
}

/// Executes one job by kind.
async fn execute(sh: &Shared, job: &Job) -> Result<serde_json::Value> {
    match job.kind.as_str() {
        "ingest" | "restandardise" => {
            let source = job
                .payload
                .get("source")
                .and_then(|s| s.as_str())
                .context("job payload has no source")?
                .to_owned();
            let (reg, profile) = sh.load().await?;
            let env = Env {
                pool: &sh.pool,
                registry: &reg,
                profile: &profile,
                store: &sh.store,
            };
            let report = if job.kind == "ingest" {
                let trigger = if job.attempts > 1 {
                    "retry"
                } else {
                    match job.trigger.as_str() {
                        "manual" => "manual",
                        "backfill" => "backfill",
                        _ => "schedule",
                    }
                };
                let opts = RunOptions {
                    from_dir: job
                        .payload
                        .get("from")
                        .and_then(|d| d.as_str())
                        .map(PathBuf::from),
                    trigger: trigger.to_owned(),
                    force: job
                        .payload
                        .get("force")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    job_id: Some(job.id),
                };
                run_source(&env, &source, &opts).await?
            } else {
                restandardise_for_job(&env, &source, Some(job.id)).await?
            };
            Ok(json!({
                "run_id": report.run_id,
                "status": report.status,
                "short_circuit": report.short_circuit,
                "artifacts": report.artifacts,
                "fetched": report.fetched,
                "published": report.published.iter().map(|p| json!({
                    "dataset_id": p.dataset_id, "dsv": p.dsv, "outcome": p.outcome, "rows_written": p.rows_written,
                })).collect::<Vec<_>>(),
            }))
        }
        "release" => {
            let trigger = job
                .payload
                .get("train_trigger")
                .and_then(|s| s.as_str())
                .unwrap_or(if job.trigger == "manual" {
                    "manual"
                } else {
                    "schedule"
                })
                .to_owned();
            let note = job
                .payload
                .get("note")
                .and_then(|s| s.as_str())
                .map(str::to_owned);
            // A broken registry or recipe never blocks the release of
            // official data: the train then runs without derived recompute.
            let methods = lanka_enrich::Methods::builtin();
            let loaded = match sh.load().await {
                Ok((reg, profile)) => match derive::load_checked(&reg, &methods) {
                    Ok((recipes, _)) => Some((reg, profile, recipes)),
                    Err(e) => {
                        tracing::error!(error = %format!("{e:#}"), "recipes not loadable; train without recompute");
                        None
                    }
                },
                Err(e) => {
                    tracing::error!(error = %format!("{e:#}"), "registry not loadable; train without recompute");
                    None
                }
            };
            let env = loaded
                .as_ref()
                .map(|(reg, profile, recipes)| derive::DeriveEnv {
                    pool: &sh.pool,
                    registry: reg,
                    profile,
                    recipes,
                    methods: &methods,
                    store_root: Some(sh.store.root()),
                });
            let r = train::run_train_with(
                &sh.pool,
                &trigger,
                Some(job.id),
                note.as_deref(),
                env.as_ref(),
            )
            .await?;
            Ok(serde_json::to_value(&r)?)
        }
        "recipe" => {
            let (reg, profile) = sh.load().await?;
            let methods = lanka_enrich::Methods::builtin();
            let (recipes, _) = derive::load_checked(&reg, &methods)?;
            let env = derive::DeriveEnv {
                pool: &sh.pool,
                registry: &reg,
                profile: &profile,
                recipes: &recipes,
                methods: &methods,
                store_root: Some(sh.store.root()),
            };
            let opts = derive::RecomputeOpts {
                trigger: if job.trigger == "manual" {
                    "manual"
                } else {
                    "schedule"
                }
                .to_owned(),
                job_id: Some(job.id),
                only: job
                    .payload
                    .get("recipe")
                    .and_then(|r| r.as_str())
                    .map(str::to_owned),
                force: job
                    .payload
                    .get("force")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                include_scheduled: true,
                ..derive::RecomputeOpts::default()
            };
            let r = derive::recompute(&env, &opts).await?;
            if r.failed > 0 {
                anyhow::bail!("recipe failed: {}", serde_json::to_string(&r.results)?);
            }
            Ok(r.to_json())
        }
        other => anyhow::bail!("job kind {other:?} is not executable by this engine version"),
    }
}

/// Used share of the filesystem holding `root`, in percent.
#[must_use]
pub fn store_pct(root: &std::path::Path) -> Option<f32> {
    let probe = if root.exists() {
        root.to_path_buf()
    } else {
        root.parent()?.to_path_buf()
    };
    let st = nix::sys::statvfs::statvfs(&probe).ok()?;
    #[allow(clippy::cast_precision_loss, clippy::useless_conversion)]
    let (blocks, free, avail) = (
        u64::from(st.blocks()) as f64,
        u64::from(st.blocks_free()) as f64,
        u64::from(st.blocks_available()) as f64,
    );
    let used = blocks - free;
    if used + avail <= 0.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some((used / (used + avail) * 100.0) as f32)
}
