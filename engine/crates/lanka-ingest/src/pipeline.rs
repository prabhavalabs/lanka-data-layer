//! The ingestion pipeline: one code path for every source.
//!
//! discover → fetch (bounded concurrency, retries) → archive (sha256, local
//! disk) → parse → validate → stage → publish (class A) / review (B, C).
//! Every stage writes `ops.run_events`; any error closes the run as failed
//! and raises an alert — nothing fails silently.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use lanka_core::Profile;
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;

use crate::archive::Archive;
use crate::connector::{Candidate, Ctx, Fetched, Output, RunOptions, connector_for};
use crate::geo_load::{publish_admin_units, upsert_crosswalks};
use crate::publish::{PublishResult, publish_observations};
use crate::registry::Registry;
use crate::run::{AlertConfig, RunStatus, RunTracker, Stage};

const FETCH_CONCURRENCY: usize = 6;
const FETCH_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub run_id: i64,
    pub source_id: String,
    pub status: RunStatus,
    pub artifacts: usize,
    pub published: Vec<PublishResult>,
    pub crosswalks: u64,
}

/// HTTP client used by connectors: identifies the project, follows redirects,
/// decompresses, and times out.
pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!(
            "lanka-data-layer-ingest/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()?)
}

/// Runs one source end to end. The registry must already be synced.
pub async fn run_source(
    pool: &PgPool,
    registry: &Registry,
    profile: &Profile,
    archive: &Archive,
    source_id: &str,
    opts: &RunOptions,
    alerts: &AlertConfig,
) -> Result<RunReport> {
    let connector = connector_for(source_id).with_context(|| {
        format!("no connector implemented for source {source_id:?} (see `lanka ingest status`)")
    })?;
    registry
        .source(source_id)
        .with_context(|| format!("source {source_id:?} is not in the registry"))?;
    let http = http_client()?;
    let trigger = if opts.trigger.is_empty() {
        "manual"
    } else {
        opts.trigger.as_str()
    };
    let run = RunTracker::start(pool, source_id, trigger).await?;
    let run_id = run.run_id;

    let ctx = Ctx {
        pool,
        registry,
        profile,
        archive,
        http: &http,
        run: &run,
        opts,
    };
    let result = execute(&ctx, connector.as_ref()).await;
    match result {
        Ok(mut report) => {
            report.run_id = run_id;
            let msg = report
                .published
                .iter()
                .map(|p| format!("{} {} {}", p.dataset_id, p.dsv, p.outcome))
                .collect::<Vec<_>>()
                .join("; ");
            run.finish(report.status, &msg, alerts).await?;
            Ok(report)
        }
        Err(e) => {
            run.fail(&e, alerts).await?;
            Err(e)
        }
    }
}

async fn execute(ctx: &Ctx<'_>, connector: &dyn crate::connector::Connector) -> Result<RunReport> {
    let source_id = connector.source_id();

    let candidates = connector.discover(ctx).await.context("discover")?;
    ctx.run
        .info(
            Stage::Discover,
            &format!("{} candidate(s)", candidates.len()),
            Some(json!(candidates.iter().map(|c| &c.key).collect::<Vec<_>>())),
        )
        .await;
    anyhow::ensure!(!candidates.is_empty(), "discover found nothing to fetch");

    let fetched: Vec<Fetched> =
        futures::stream::iter(candidates.iter().map(|c| fetch_one(ctx, source_id, c)))
            .buffered(FETCH_CONCURRENCY)
            .try_collect()
            .await?;
    let new_bytes: u64 = fetched.iter().map(|f| f.bytes.len() as u64).sum();
    ctx.run
        .info(
            Stage::Archive,
            &format!(
                "{} artifact(s), {} bytes archived",
                fetched.len(),
                new_bytes
            ),
            None,
        )
        .await;

    let outputs = connector.parse(ctx, &fetched).await.context("parse")?;
    let parsed: usize = outputs
        .iter()
        .map(|o| match o {
            Output::Observations { rows, .. } => rows.len(),
            Output::AdminUnits { units, .. } => units.len(),
            Output::Crosswalks { rows, .. } => rows.len(),
        })
        .sum();
    ctx.run
        .info(Stage::Parse, &format!("{parsed} record(s) parsed"), None)
        .await;

    let mut published = Vec::new();
    let mut crosswalks = 0;
    let mut written: i64 = 0;
    for out in outputs {
        match out {
            Output::Observations {
                dataset_id,
                vintage,
                rows,
            } => {
                ctx.run
                    .info(
                        Stage::Validate,
                        &format!("validating {} rows for {dataset_id}", rows.len()),
                        None,
                    )
                    .await;
                let r = publish_observations(
                    ctx.pool,
                    ctx.registry,
                    ctx.profile,
                    ctx.run.run_id,
                    &dataset_id,
                    vintage.as_deref(),
                    &rows,
                    &fetched,
                )
                .await
                .with_context(|| format!("publishing {dataset_id}"))?;
                ctx.run
                    .info(
                        Stage::Publish,
                        &format!("{dataset_id}: {} — {}", r.outcome, r.note),
                        Some(json!(r)),
                    )
                    .await;
                written += r.rows_written;
                published.push(r);
            }
            Output::AdminUnits {
                dataset_id,
                boundary_set,
                units,
            } => {
                let r = publish_admin_units(
                    ctx.pool,
                    ctx.profile,
                    ctx.run.run_id,
                    &dataset_id,
                    &boundary_set,
                    &units,
                    &fetched,
                )
                .await
                .with_context(|| format!("publishing {dataset_id}"))?;
                ctx.run
                    .info(
                        Stage::Publish,
                        &format!("{dataset_id}: {} — {}", r.outcome, r.note),
                        Some(json!(r)),
                    )
                    .await;
                written += r.rows_written;
                published.push(r);
            }
            Output::Crosswalks { source_id, rows } => {
                crosswalks += upsert_crosswalks(ctx.pool, &source_id, &rows).await?;
                ctx.run
                    .info(
                        Stage::Publish,
                        &format!("{} crosswalk row(s) upserted", rows.len()),
                        None,
                    )
                    .await;
            }
        }
    }
    ctx.run
        .add_counts(
            i32::try_from(fetched.len())?,
            i64::try_from(parsed)?,
            written,
        )
        .await?;

    let status = if published.iter().any(|p| p.outcome == "in_review") {
        RunStatus::NeedsReview
    } else if published.iter().all(|p| p.outcome == "unchanged") {
        RunStatus::NoChange
    } else {
        RunStatus::Succeeded
    };
    Ok(RunReport {
        run_id: 0,
        source_id: source_id.to_owned(),
        status,
        artifacts: fetched.len(),
        published,
        crosswalks,
    })
}

/// Fetches (or reads) one candidate, archives it and records the artifact.
async fn fetch_one(ctx: &Ctx<'_>, source_id: &str, c: &Candidate) -> Result<Fetched> {
    let (bytes, etag, last_modified, media_type) = match &c.local_path {
        Some(p) => (
            Bytes::from(
                tokio::fs::read(p)
                    .await
                    .with_context(|| format!("reading {}", p.display()))?,
            ),
            None,
            None,
            c.media_type.clone(),
        ),
        None => http_get(ctx, c).await?,
    };
    let archive = ctx.archive.clone();
    let b = bytes.clone();
    let stored = tokio::task::spawn_blocking(move || archive.put(&b)).await??;
    let artifact_id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, media_type, etag, last_modified,
            edition, archive_path)
         VALUES ($1, $2, $3, now(), $4, $5, $6, $7, $8, $9)
         ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256
         RETURNING id",
    )
    .bind(stored.sha256.as_slice())
    .bind(source_id)
    .bind(&c.url)
    .bind(i64::try_from(stored.bytes)?)
    .bind(&media_type)
    .bind(etag)
    .bind(last_modified)
    .bind(&c.edition)
    .bind(rel_path(ctx.archive.root(), &stored.path))
    .fetch_one(ctx.pool)
    .await?;
    sqlx::query("INSERT INTO ops.run_artifacts (run_id, artifact_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(ctx.run.run_id)
        .bind(artifact_id)
        .execute(ctx.pool)
        .await?;
    ctx.run
        .event(
            crate::run::Level::Debug,
            Stage::Fetch,
            &format!("{} → {}", c.key, stored.hex()),
            Some(json!({ "url": c.url, "bytes": stored.bytes, "new": stored.created })),
        )
        .await;
    Ok(Fetched {
        key: c.key.clone(),
        url: c.url.clone(),
        artifact_id,
        sha256: stored.sha256,
        bytes,
        media_type,
    })
}

type HttpParts = (Bytes, Option<String>, Option<String>, Option<String>);

async fn http_get(ctx: &Ctx<'_>, c: &Candidate) -> Result<HttpParts> {
    let mut last_err = anyhow!("no attempt made");
    for attempt in 1..=FETCH_ATTEMPTS {
        match ctx
            .http
            .get(&c.url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(resp) => {
                let header = |n: &str| {
                    resp.headers()
                        .get(n)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned)
                };
                let etag = header("etag");
                let lm = header("last-modified");
                let ct = header("content-type").or_else(|| c.media_type.clone());
                match resp.bytes().await {
                    Ok(b) => return Ok((b, etag, lm, ct)),
                    Err(e) => last_err = e.into(),
                }
            }
            Err(e) => last_err = e.into(),
        }
        if attempt < FETCH_ATTEMPTS {
            ctx.run
                .warn(
                    Stage::Fetch,
                    &format!("{} attempt {attempt} failed: {last_err:#}", c.key),
                    None,
                )
                .await;
            tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt * attempt))).await;
        }
    }
    Err(last_err.context(format!("fetching {}", c.url)))
}

fn rel_path(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// Convenience used by the CLI and tests: a shared, loaded registry.
pub fn load_registry(dir: &Path) -> Result<Arc<Registry>> {
    Ok(Arc::new(Registry::load(dir)?))
}
