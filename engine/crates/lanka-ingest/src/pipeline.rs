//! The ingestion pipeline: one code path for every source (03 §1).
//!
//! ```text
//! discover ─► fetch ─► archive ─► extract ─► standardise ─► validate ─► stage ─► gate ─► publish | review
//!   │           │         L0         L1
//!   └ fingerprint└ conditional GET, per-host politeness, streamed to the store
//! ```
//!
//! Change detection is layered, cheapest first: (1) the candidate-set
//! fingerprint from `discover` equals the last successful run's → `no_change`
//! before anything is fetched; (2) the artifact set (sha256 per key) equals
//! the last successful run's → `no_change` before extract; (3) the staged
//! content equals the published version → `unchanged` with nothing written.
//! Every stage writes `ops.run_events`; any error closes the run as failed
//! (an `error` event of the status feed) — nothing fails silently.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use bytes::Bytes;
use futures::StreamExt;
use lanka_core::Profile;
use lanka_store::{BlobStore, FsStore, Ns};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Semaphore};

use crate::connector::{
    Candidate, CollectSink, Connector, DiscoverCtx, ExtractCtx, Fetched, HttpMethod, Lookup,
    ObsRow, Output, RunOptions, StdCtx, UnitRef, connector_for, implemented,
};
use crate::extract::{ExtractArtifact, ExtractOut, ExtractRef, from_parquet, to_parquet};
use crate::geo_load::{publish_admin_units, upsert_crosswalks};
use crate::publish::{PublishOpts, PublishResult, StageInput, publish_observations_with};
use crate::registry::Registry;
use crate::run::{Level, RunStatus, RunTracker, Stage};

const FETCH_ATTEMPTS: u32 = 3;
/// Concurrent requests per upstream host (03 §1).
const PER_HOST: usize = 2;
/// Longest `Retry-After` we honour inside a run.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub run_id: i64,
    pub source_id: String,
    pub status: RunStatus,
    pub artifacts: usize,
    /// Artifacts actually downloaded (not 304, not local).
    pub fetched: usize,
    pub extracts: usize,
    pub published: Vec<PublishResult>,
    pub crosswalks: u64,
    /// Why the run ended early, if it did (`fingerprint`, `artifacts`).
    pub short_circuit: Option<String>,
}

impl RunReport {
    fn empty(source_id: &str, status: RunStatus) -> Self {
        Self {
            run_id: 0,
            source_id: source_id.to_owned(),
            status,
            artifacts: 0,
            fetched: 0,
            extracts: 0,
            published: Vec::new(),
            crosswalks: 0,
            short_circuit: None,
        }
    }
}

/// User agent of every fetch: the crawler convention `Mozilla/5.0
/// (compatible; <bot>/<version>; +<url>)`. It identifies the project; some
/// government web application firewalls (CBSL) refuse agents without the
/// `Mozilla/5.0 (compatible; …)` prefix.
pub const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; lanka-data-layer-ingest/",
    env!("CARGO_PKG_VERSION"),
    "; +https://github.com/prabhavalabs/lanka-data-layer)"
);

/// HTTP client used by connectors: identifies the project, follows redirects,
/// decompresses, and times out.
pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()?)
}

/// Everything a run needs.
pub struct Env<'a> {
    pub pool: &'a PgPool,
    pub registry: &'a Registry,
    pub profile: &'a Profile,
    pub store: &'a dyn BlobStore,
}

fn connector(env: &Env<'_>, source_id: &str) -> Result<Arc<dyn Connector>> {
    env.registry
        .source(source_id)
        .with_context(|| format!("source {source_id:?} is not in the registry"))?;
    connector_for(env.registry, source_id)?.with_context(|| {
        format!(
            "no connector for source {source_id:?} (implemented: {})",
            implemented(env.registry).join(", ")
        )
    })
}

/// Runs one source end to end. The registry must already be synced.
pub async fn run_source(env: &Env<'_>, source_id: &str, opts: &RunOptions) -> Result<RunReport> {
    let c = connector(env, source_id)?;
    let trigger = if opts.trigger.is_empty() {
        "manual"
    } else {
        opts.trigger.as_str()
    };
    let run = RunTracker::start_for_job(env.pool, source_id, trigger, opts.job_id).await?;
    let result = execute(env, &run, c.as_ref(), opts).await;
    close(run, result).await
}

/// Re-runs `standardise` (and everything after it) over the extracts of the
/// source's latest extracting run — no network, no parsing of raw bytes.
/// Used after a registry, crosswalk or codelist change (03 §7).
pub async fn restandardise(env: &Env<'_>, source_id: &str) -> Result<RunReport> {
    restandardise_for_job(env, source_id, None).await
}

/// [`restandardise`] executed by a worker job.
pub async fn restandardise_for_job(
    env: &Env<'_>,
    source_id: &str,
    job_id: Option<i64>,
) -> Result<RunReport> {
    let c = connector(env, source_id)?;
    let run = RunTracker::start_for_job(env.pool, source_id, "restandardise", job_id).await?;
    let result = restandardise_inner(env, &run, c.as_ref()).await;
    close(run, result).await
}

async fn close(run: RunTracker, result: Result<RunReport>) -> Result<RunReport> {
    let run_id = run.run_id;
    match result {
        Ok(mut report) => {
            report.run_id = run_id;
            let mut msg = report
                .published
                .iter()
                .map(|p| format!("{} {} {}", p.dataset_id, p.dsv, p.outcome))
                .collect::<Vec<_>>()
                .join("; ");
            if let Some(s) = &report.short_circuit {
                msg = format!("{s} unchanged");
            }
            run.finish(report.status, &msg).await?;
            Ok(report)
        }
        Err(e) => {
            run.fail(&e).await?;
            Err(e)
        }
    }
}

/// Hash of everything about the source's configuration that changes what a
/// run produces: connector version, the source/dataset registry files and
/// the connector spec.
fn config_hash(env: &Env<'_>, c: &dyn Connector) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(c.version().as_bytes());
    h.update([0]);
    if let Some(s) = c.spec_sha256() {
        h.update(s);
    }
    if let Some(src) = env.registry.sources.get(c.source_id()) {
        h.update(src.sha256);
        for d in &src.spec.datasets {
            if let Some(ds) = env.registry.datasets.get(d) {
                h.update(ds.sha256);
            }
        }
    }
    h.update(env.profile.sha256);
    h.finalize().into()
}

/// Fingerprint of local candidate files (path, size, mtime) when the
/// connector gives none and every candidate is a local file.
fn local_fingerprint(cands: &[Candidate]) -> Option<[u8; 32]> {
    let mut h = Sha256::new();
    for c in cands {
        let p = c.local_path.as_ref()?;
        let m = std::fs::metadata(p).ok()?;
        let mtime = m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        h.update(format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{mtime}\n",
            c.key,
            p.display(),
            m.len()
        ));
    }
    Some(h.finalize().into())
}

fn keyed(tag: &[u8], config: &[u8; 32], payload: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(tag);
    h.update(config);
    h.update(payload);
    h.finalize().into()
}

/// (fingerprint, input_hash) of the source's last successful run.
///
/// A reviewer's rejection of one of the source's versions invalidates every
/// change key recorded before it: the next run fetches, extracts and stages
/// again instead of short-circuiting on the inputs that produced the
/// rejected version (no `--force` needed after a reject).
pub async fn last_change_keys(
    pool: &PgPool,
    source_id: &str,
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>)> {
    Ok(sqlx::query_as(
        "SELECT r.fingerprint, r.input_hash FROM ops.ingestion_runs r
         WHERE r.source_id = $1 AND r.status IN ('succeeded', 'no_change', 'needs_review')
           AND r.trigger <> 'restandardise'
           AND NOT EXISTS (
             SELECT 1 FROM data.dataset_versions v
             JOIN catalog.dataset_sources ds ON ds.dataset_id = v.dataset_id AND ds.source_id = $1
             WHERE v.status = 'rejected' AND v.reviewed_by IS NOT NULL AND v.reviewed_at >= r.started_at)
         ORDER BY r.id DESC LIMIT 1",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or_default())
}

async fn execute(
    env: &Env<'_>,
    run: &RunTracker,
    c: &dyn Connector,
    opts: &RunOptions,
) -> Result<RunReport> {
    let source_id = c.source_id().to_owned();
    let http = http_client()?;
    let config = config_hash(env, c);
    let (prev_fp, prev_input) = last_change_keys(env.pool, &source_id).await?;

    // ── discover ──
    let dctx = DiscoverCtx {
        registry: env.registry,
        profile: env.profile,
        http: &http,
        opts,
        run: Some(run),
    };
    let disc = c.discover(&dctx).await.context("discover")?;
    ensure!(
        !disc.candidates.is_empty(),
        "discover found nothing to fetch"
    );
    let fingerprint = disc
        .fingerprint
        .or_else(|| local_fingerprint(&disc.candidates))
        .map(|f| keyed(b"lanka-fp-v1", &config, &f));
    run.info(
        Stage::Discover,
        &format!(
            "{} candidate(s); fingerprint {}",
            disc.candidates.len(),
            fingerprint.map_or_else(|| "none".to_owned(), |f| hex::encode(&f[..8]))
        ),
        Some(json!(
            disc.candidates.iter().map(|c| &c.key).collect::<Vec<_>>()
        )),
    )
    .await;
    if !opts.force
        && let Some(fp) = fingerprint
        && prev_fp.as_deref() == Some(fp.as_slice())
    {
        run.set_change_keys(
            c.version(),
            Some(fp),
            prev_input
                .as_deref()
                .and_then(|b| <[u8; 32]>::try_from(b).ok()),
        )
        .await?;
        run.info(
            Stage::Discover,
            "fingerprint unchanged: nothing fetched",
            None,
        )
        .await;
        let mut r = RunReport::empty(&source_id, RunStatus::NoChange);
        r.short_circuit = Some("fingerprint".to_owned());
        return Ok(r);
    }

    // ── fetch + archive ──
    let fetcher = Fetcher::new(env, run, &http, &source_id);
    let results = futures::future::join_all(disc.candidates.iter().map(|c| fetcher.fetch(c))).await;
    let mut fetched = Vec::with_capacity(results.len());
    let mut downloaded = 0;
    for r in results {
        let (f, dl) = r?;
        downloaded += usize::from(dl);
        fetched.push(f);
    }
    let bytes: u64 = fetched.iter().map(|f| f.bytes).sum();
    run.info(
        Stage::Archive,
        &format!(
            "{} artifact(s), {bytes} bytes ({downloaded} downloaded)",
            fetched.len()
        ),
        None,
    )
    .await;
    let mut set = Sha256::new();
    let mut keys: Vec<(&str, &[u8; 32])> = fetched
        .iter()
        .map(|f| (f.key.as_str(), &f.sha256))
        .collect();
    keys.sort_unstable();
    for (k, s) in keys {
        set.update(k.as_bytes());
        set.update([0x1f]);
        set.update(s);
    }
    let input_hash = keyed(b"lanka-in-v1", &config, &set.finalize());
    run.set_change_keys(c.version(), fingerprint, Some(input_hash))
        .await?;
    if !opts.force && prev_input.as_deref() == Some(input_hash.as_slice()) {
        run.info(
            Stage::Archive,
            "artifacts identical to the last successful run",
            None,
        )
        .await;
        let mut r = RunReport::empty(&source_id, RunStatus::NoChange);
        r.artifacts = fetched.len();
        r.fetched = downloaded;
        r.short_circuit = Some("artifacts".to_owned());
        return Ok(r);
    }

    // ── grid (rasters, WP-05): canonical arrays are the L1 of a raster ──
    if let Some(r) = c.grid() {
        let mut report = crate::grid::build_and_publish(env, run, c, r, &fetched).await?;
        report.fetched = downloaded;
        return Ok(report);
    }

    // ── extract (L1) ──
    let ectx = ExtractCtx {
        registry: env.registry,
        profile: env.profile,
    };
    let outs = c.extract(&ectx, &fetched).await.context("extract")?;
    let artifacts: Vec<ExtractArtifact> = fetched
        .iter()
        .map(|f| ExtractArtifact {
            artifact_id: f.artifact_id,
            key: f.key.clone(),
            url: f.url.clone(),
            sha256: f.sha256,
        })
        .collect();
    let mut extracts = Vec::with_capacity(outs.len());
    for out in outs {
        extracts.push(persist_extract(env, run, c, out, &artifacts).await?);
    }
    run.info(
        Stage::Extract,
        &format!(
            "{} extract(s): {}",
            extracts.len(),
            extracts
                .iter()
                .map(|e| format!("{} rows {}", e.data.len(), hex::encode(&e.sha256[..8])))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None,
    )
    .await;

    let artifact_ids: Vec<i64> = fetched.iter().map(|f| f.artifact_id).collect();
    let mut report = standardise_and_publish(env, run, c, &extracts, &artifact_ids).await?;
    report.fetched = downloaded;
    Ok(report)
}

async fn restandardise_inner(
    env: &Env<'_>,
    run: &RunTracker,
    c: &dyn Connector,
) -> Result<RunReport> {
    let ids: Vec<(i64, Vec<u8>, String, String, String)> = sqlx::query_as(
        "SELECT e.id, e.sha256, e.store_key, e.extractor, e.extractor_ver
         FROM ops.run_extracts re JOIN ops.extracts e ON e.id = re.extract_id
         WHERE re.run_id = (SELECT max(re2.run_id) FROM ops.run_extracts re2
                            JOIN ops.ingestion_runs r ON r.id = re2.run_id
                            WHERE r.source_id = $1 AND r.status <> 'failed')
         ORDER BY e.id",
    )
    .bind(c.source_id())
    .fetch_all(env.pool)
    .await?;
    ensure!(
        !ids.is_empty(),
        "no stored extracts for {}: run `lanka ingest run {}` first",
        c.source_id(),
        c.source_id()
    );
    let mut extracts = Vec::new();
    let mut artifact_ids: Vec<i64> = Vec::new();
    for (id, sha, key, extractor, ver) in ids {
        if ver != c.version() {
            run.warn(
                Stage::Extract,
                &format!(
                    "extract {id} was written by {extractor} {ver}; connector is {}",
                    c.version()
                ),
                None,
            )
            .await;
        }
        let sha256: [u8; 32] = sha.as_slice().try_into().context("extract sha256")?;
        let blob = lanka_store::BlobRef {
            ns: Ns::Extract,
            key,
            sha256,
            bytes: 0,
        };
        let bytes = env.store.get(&blob).await?;
        let data = tokio::task::spawn_blocking(move || from_parquet(bytes)).await??;
        let arts: Vec<(i32, i64, String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT x.idx, x.artifact_id, x.key, x.url, a.sha256
             FROM ops.extract_artifacts x JOIN ops.artifacts a ON a.id = x.artifact_id
             WHERE x.extract_id = $1 ORDER BY x.idx",
        )
        .bind(id)
        .fetch_all(env.pool)
        .await?;
        let mut artifacts = Vec::with_capacity(arts.len());
        for (idx, aid, key, url, sha) in arts {
            let i = usize::try_from(idx)?;
            ensure!(
                i == artifacts.len(),
                "extract {id}: artifact list has a gap at {i}"
            );
            if artifact_ids.len() <= i {
                artifact_ids.resize(i + 1, 0);
            }
            ensure!(
                artifact_ids[i] == 0 || artifact_ids[i] == aid,
                "extracts disagree about artifact {i}"
            );
            artifact_ids[i] = aid;
            artifacts.push(ExtractArtifact {
                artifact_id: aid,
                key,
                url,
                sha256: sha.as_slice().try_into().context("artifact sha256")?,
            });
        }
        sqlx::query("INSERT INTO ops.run_extracts VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(run.run_id)
            .bind(id)
            .execute(env.pool)
            .await?;
        extracts.push(ExtractRef {
            id,
            sha256,
            extractor,
            extractor_ver: ver,
            artifacts,
            data: Arc::new(data),
        });
    }
    run.info(
        Stage::Extract,
        &format!("{} stored extract(s) loaded (offline)", extracts.len()),
        None,
    )
    .await;
    standardise_and_publish(env, run, c, &extracts, &artifact_ids).await
}

/// Writes an extract to the store (content-addressed Parquet) and indexes it.
pub(crate) async fn persist_extract(
    env: &Env<'_>,
    run: &RunTracker,
    c: &dyn Connector,
    out: ExtractOut,
    artifacts: &[ExtractArtifact],
) -> Result<ExtractRef> {
    let shape = out.shape();
    let rows = i64::try_from(out.len())?;
    let (extractor, ver, source) = (
        c.extractor().to_owned(),
        c.version().to_owned(),
        c.source_id().to_owned(),
    );
    let out = Arc::new(out);
    let enc = {
        let (o, e, v, s) = (out.clone(), extractor.clone(), ver.clone(), source.clone());
        tokio::task::spawn_blocking(move || {
            to_parquet(
                &o,
                &[("extractor", &e), ("extractor_ver", &v), ("source", &s)],
            )
        })
        .await??
    };
    let (blob, created) = env.store.put_cas(Ns::Extract, Bytes::from(enc)).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.extracts (sha256, source_id, shape, extractor, extractor_ver, template_sha256, rows,
            bytes, store_key, run_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256
         RETURNING id",
    )
    .bind(blob.sha256.as_slice())
    .bind(&source)
    .bind(shape.as_str())
    .bind(&extractor)
    .bind(&ver)
    .bind(c.spec_sha256().as_ref().map(<[u8; 32]>::as_slice))
    .bind(rows)
    .bind(i64::try_from(blob.bytes)?)
    .bind(&blob.key)
    .bind(run.run_id)
    .fetch_one(env.pool)
    .await?;
    let idx: Vec<i32> = (0..artifacts.len())
        .map(i32::try_from)
        .collect::<std::result::Result<_, _>>()?;
    sqlx::query(
        "INSERT INTO ops.extract_artifacts (extract_id, idx, artifact_id, key, url)
         SELECT $1, * FROM unnest($2::int[], $3::bigint[], $4::text[], $5::text[])
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&idx)
    .bind(artifacts.iter().map(|a| a.artifact_id).collect::<Vec<_>>())
    .bind(artifacts.iter().map(|a| a.key.clone()).collect::<Vec<_>>())
    .bind(artifacts.iter().map(|a| a.url.clone()).collect::<Vec<_>>())
    .execute(env.pool)
    .await?;
    sqlx::query("INSERT INTO ops.run_extracts VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(run.run_id)
        .bind(id)
        .execute(env.pool)
        .await?;
    run.event(
        Level::Debug,
        Stage::Extract,
        &format!("{} extract {} ({rows} rows)", shape.as_str(), blob.hex()),
        Some(json!({ "extract_id": id, "new": created, "bytes": blob.bytes })),
    )
    .await;
    Ok(ExtractRef {
        id,
        sha256: blob.sha256,
        extractor,
        extractor_ver: ver,
        artifacts: artifacts.to_vec(),
        data: out,
    })
}

/// Admin units of the profile's scheme for `standardise` (read once per run),
/// and the country's land area buffered by the snap distance (features).
pub async fn load_lookup(pool: &PgPool, profile: &Profile) -> Result<Lookup> {
    let coverage = crate::features::load_coverage(pool, profile).await?;
    let units: Vec<(String, i16, String, Option<String>)> =
        sqlx::query_as("SELECT pcode, level, name_en, parent_pcode FROM geo.admin_units")
            .fetch_all(pool)
            .await?;
    Ok(Lookup {
        units: units
            .into_iter()
            .map(|(code, level, name_en, parent)| UnitRef {
                code,
                level,
                name_en,
                parent,
            })
            .collect(),
        coverage,
    })
}

async fn standardise_and_publish(
    env: &Env<'_>,
    run: &RunTracker,
    c: &dyn Connector,
    extracts: &[ExtractRef],
    artifact_ids: &[i64],
) -> Result<RunReport> {
    let lookup = load_lookup(env.pool, env.profile).await?;
    let sctx = StdCtx::new(env.registry, env.profile, &lookup);
    let mut sink = CollectSink::default();
    c.standardise(&sctx, extracts, &mut sink)
        .context("standardise")?;
    for w in sctx.take_warnings() {
        run.warn(Stage::Standardise, &w, None).await;
    }
    let outputs = sink.finish()?;
    let parsed: usize = outputs
        .iter()
        .map(|o| match o {
            Output::Observations { rows, .. } => rows.len(),
            Output::AdminUnits { units, .. } => units.len(),
            Output::Crosswalks { rows, .. } => rows.len(),
            Output::Features { rows, .. } => rows.len(),
        })
        .sum();
    run.info(
        Stage::Standardise,
        &format!("{parsed} record(s) standardised"),
        None,
    )
    .await;
    let extract_ids: Vec<i64> = extracts.iter().map(|e| e.id).collect();
    // Grid unit assignment for features (WP-05 lookups), when built.
    let grid_units = if outputs.iter().any(|o| matches!(o, Output::Features { .. })) {
        match crate::features::load_grid_units(env.pool, env.store, env.profile).await {
            Ok(g) => g,
            Err(e) => {
                run.warn(
                    Stage::Validate,
                    &format!("grid unit lookup not used (polygon path only): {e:#}"),
                    None,
                )
                .await;
                None
            }
        }
    } else {
        None
    };

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
                run.info(
                    Stage::Validate,
                    &format!("validating {} rows for {dataset_id}", rows.len()),
                    None,
                )
                .await;
                let opts = expectations(env, run, &lookup, &dataset_id, &rows).await?;
                let r = publish_observations_with(
                    env.pool,
                    env.registry,
                    env.profile,
                    StageInput {
                        run_id: run.run_id,
                        dataset_id: &dataset_id,
                        vintage: vintage.as_deref(),
                        rows: &rows,
                        artifact_ids,
                        extract_ids: &extract_ids,
                    },
                    &opts,
                )
                .await
                .with_context(|| format!("publishing {dataset_id}"))?;
                run.info(
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
                    env.pool,
                    env.profile,
                    run.run_id,
                    &dataset_id,
                    &boundary_set,
                    &units,
                    artifact_ids,
                    &extract_ids,
                )
                .await
                .with_context(|| format!("publishing {dataset_id}"))?;
                run.info(
                    Stage::Publish,
                    &format!("{dataset_id}: {} — {}", r.outcome, r.note),
                    Some(json!(r)),
                )
                .await;
                written += r.rows_written;
                published.push(r);
            }
            Output::Features {
                dataset_id,
                vintage,
                rows,
            } => {
                run.info(
                    Stage::Validate,
                    &format!("validating {} features for {dataset_id}", rows.len()),
                    None,
                )
                .await;
                {
                    let exps = crate::expect::for_dataset(env.registry, &dataset_id)?;
                    let class = env
                        .registry
                        .dataset(&dataset_id)
                        .map_or("A", |d| d.publication_class.as_str());
                    let results = crate::expect::check_geometry(
                        &exps,
                        &crate::expect::feature_items(&rows),
                        env.profile,
                        class,
                    );
                    log_results(run, &dataset_id, &results).await;
                    crate::expect::ensure_passed(&dataset_id, &results)?;
                }
                let r = crate::features::publish_features(
                    env.pool,
                    env.registry,
                    env.profile,
                    crate::features::FeatureStageInput {
                        run_id: run.run_id,
                        dataset_id: &dataset_id,
                        vintage: vintage.as_deref(),
                        rows: &rows,
                        artifact_ids,
                        extract_ids: &extract_ids,
                        coverage: lookup.coverage.as_deref(),
                        grid_units: grid_units.as_ref(),
                    },
                )
                .await
                .with_context(|| format!("publishing {dataset_id}"))?;
                run.info(
                    Stage::Publish,
                    &format!("{dataset_id}: {} — {}", r.outcome, r.note),
                    Some(json!(r)),
                )
                .await;
                written += r.rows_written;
                published.push(r);
            }
            Output::Crosswalks { source_id, rows } => {
                crosswalks += upsert_crosswalks(env.pool, &source_id, &rows).await?;
                run.info(
                    Stage::Publish,
                    &format!("{} crosswalk row(s) upserted", rows.len()),
                    None,
                )
                .await;
            }
        }
    }
    run.add_counts(
        i32::try_from(artifact_ids.len())?,
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
        source_id: c.source_id().to_owned(),
        status,
        artifacts: artifact_ids.len(),
        fetched: 0,
        extracts: extracts.len(),
        published,
        crosswalks,
        short_circuit: None,
    })
}

/// Writes expectation results to the run log.
async fn log_results(run: &RunTracker, dataset_id: &str, results: &[crate::expect::CheckResult]) {
    for r in results {
        let msg = format!("{dataset_id}: {}", r.line());
        if r.passed {
            run.info(Stage::Validate, &msg, None).await;
        } else {
            run.warn(Stage::Validate, &msg, None).await;
        }
    }
}

/// Evaluates a dataset's expectations on the standardised rows (03 §5.2).
/// Class A fails here on an error; B/C carry the failures into review.
async fn expectations(
    env: &Env<'_>,
    run: &RunTracker,
    lookup: &Lookup,
    dataset_id: &str,
    rows: &[ObsRow],
) -> Result<PublishOpts> {
    let exps = crate::expect::for_dataset(env.registry, dataset_id)?;
    if exps.is_empty() {
        return Ok(PublishOpts::default());
    }
    let class = env
        .registry
        .dataset(dataset_id)
        .map_or_else(|| "A".to_owned(), |d| d.publication_class.clone());
    let prev_rows: Option<i64> = sqlx::query_scalar(
        "SELECT row_count FROM data.dataset_versions
         WHERE dataset_id = $1 AND status = 'published' ORDER BY seq DESC LIMIT 1",
    )
    .bind(dataset_id)
    .fetch_optional(env.pool)
    .await?;
    let ctx = crate::expect::ObsCtx {
        registry: env.registry,
        profile: env.profile,
        lookup,
        class: &class,
        prev_rows,
        today: chrono::Utc::now().date_naive(),
    };
    let results = crate::expect::check_observations(&exps, rows, &ctx);
    log_results(run, dataset_id, &results).await;
    if class == "A" {
        crate::expect::ensure_passed(dataset_id, &results)?;
    }
    let staged: Vec<crate::expect::Expectation> = exps
        .into_iter()
        .filter(|e| {
            matches!(
                e.check,
                crate::expect::Check::Sql { .. } | crate::expect::Check::RevisionLimit { .. }
            )
        })
        .collect();
    Ok(PublishOpts {
        gate: crate::expect::revision_limits(
            &staged,
            (
                crate::publish::MAX_CHANGED_SHARE,
                crate::publish::MAX_REMOVED_SHARE,
            ),
        ),
        results,
        staged,
    })
}

// ───────────────────────────── fetch ───────────────────────────────────────

/// The previous artifact fetched from a URL (conditional-GET validators).
struct Prior {
    id: i64,
    sha256: [u8; 32],
    bytes: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    media_type: Option<String>,
}

/// (id, sha256, bytes, etag, last_modified, media_type)
type PriorRow = (
    i64,
    Vec<u8>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Per-host politeness: at most [`PER_HOST`] requests in flight and at most
/// `LANKA_FETCH_RPS` request starts per second (default 1).
struct HostGate {
    sem: Semaphore,
    next: Mutex<Instant>,
}

struct Fetcher<'a> {
    env: &'a Env<'a>,
    run: &'a RunTracker,
    http: &'a reqwest::Client,
    source_id: &'a str,
    hosts: std::sync::Mutex<HashMap<String, Arc<HostGate>>>,
    interval: Duration,
}

impl<'a> Fetcher<'a> {
    fn new(
        env: &'a Env<'a>,
        run: &'a RunTracker,
        http: &'a reqwest::Client,
        source_id: &'a str,
    ) -> Self {
        let rps: f64 = std::env::var("LANKA_FETCH_RPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| *v > 0.0)
            .unwrap_or(1.0);
        Self {
            env,
            run,
            http,
            source_id,
            hosts: std::sync::Mutex::new(HashMap::new()),
            interval: Duration::from_secs_f64(1.0 / rps),
        }
    }

    fn gate(&self, url: &str) -> Arc<HostGate> {
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        let mut m = self
            .hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.entry(host)
            .or_insert_with(|| {
                Arc::new(HostGate {
                    sem: Semaphore::new(PER_HOST),
                    next: Mutex::new(Instant::now()),
                })
            })
            .clone()
    }

    async fn prior(&self, url: &str) -> Result<Option<Prior>> {
        let r: Option<PriorRow> = sqlx::query_as(
            "SELECT id, sha256, bytes, etag, last_modified, media_type FROM ops.artifacts
             WHERE source_id = $1 AND url = $2 AND purged_at IS NULL
             ORDER BY fetched_at DESC, id DESC LIMIT 1",
        )
        .bind(self.source_id)
        .bind(url)
        .fetch_optional(self.env.pool)
        .await?;
        Ok(
            r.and_then(|(id, sha, bytes, etag, last_modified, media_type)| {
                Some(Prior {
                    id,
                    sha256: sha.as_slice().try_into().ok()?,
                    bytes,
                    etag,
                    last_modified,
                    media_type,
                })
            }),
        )
    }

    /// Fetches (or reads) one candidate into the store and records the
    /// artifact. Returns whether bytes were downloaded.
    async fn fetch(&self, c: &Candidate) -> Result<(Fetched, bool)> {
        let store = self.env.store;
        let (blob, created, etag, last_modified, media_type, status, downloaded) = match &c
            .local_path
        {
            Some(p) => {
                let (b, created) = store
                    .put_cas_file(Ns::Raw, p)
                    .await
                    .with_context(|| format!("archiving {}", p.display()))?;
                (b, created, None, None, c.media_type.clone(), None, false)
            }
            None => {
                let prior = self.prior(&c.url).await?;
                match self.http_get(c, prior.as_ref()).await? {
                    Got::NotModified => {
                        let p = prior.context("304 without a prior artifact")?;
                        let key = Ns::Raw.cas_key(&p.sha256);
                        if store.exists(Ns::Raw, &key).await? {
                            self.run
                                .event(
                                    Level::Debug,
                                    Stage::Fetch,
                                    &format!("{}: 304 not modified", c.key),
                                    None,
                                )
                                .await;
                            return Ok((
                                self.link(c, p.id, p.sha256, u64::try_from(p.bytes)?, p.media_type)
                                    .await?,
                                false,
                            ));
                        }
                        // Blob missing (purged or restored store): fetch it again.
                        match self.http_get(c, None).await? {
                            Got::Body(b) => b.into_parts(store).await?,
                            Got::NotModified => bail!("{}: 304 to an unconditional GET", c.url),
                        }
                    }
                    Got::Body(b) => b.into_parts(store).await?,
                }
            }
        };
        let artifact_id: i64 = sqlx::query_scalar(
            "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, media_type, etag, last_modified,
                edition, archive_path, http_status)
             VALUES ($1, $2, $3, now(), $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (sha256) DO UPDATE SET
               etag = coalesce(EXCLUDED.etag, ops.artifacts.etag),
               last_modified = coalesce(EXCLUDED.last_modified, ops.artifacts.last_modified)
             RETURNING id",
        )
        .bind(blob.sha256.as_slice())
        .bind(self.source_id)
        .bind(&c.url)
        .bind(i64::try_from(blob.bytes)?)
        .bind(&media_type)
        .bind(etag)
        .bind(last_modified)
        .bind(&c.edition)
        .bind(blob.rel_path())
        .bind(status)
        .fetch_one(self.env.pool)
        .await?;
        self.run
            .event(
                Level::Debug,
                Stage::Fetch,
                &format!("{} → {}", c.key, blob.hex()),
                Some(json!({ "url": c.url, "bytes": blob.bytes, "new": created })),
            )
            .await;
        Ok((
            self.link(c, artifact_id, blob.sha256, blob.bytes, media_type)
                .await?,
            downloaded,
        ))
    }

    async fn link(
        &self,
        c: &Candidate,
        artifact_id: i64,
        sha256: [u8; 32],
        bytes: u64,
        media_type: Option<String>,
    ) -> Result<Fetched> {
        sqlx::query("INSERT INTO ops.run_artifacts (run_id, artifact_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(self.run.run_id)
            .bind(artifact_id)
            .execute(self.env.pool)
            .await?;
        let key = Ns::Raw.cas_key(&sha256);
        let path =
            self.env.store.local_path(Ns::Raw, &key).context(
                "the store has no local path for raw artifacts (S3 cache not implemented)",
            )?;
        Ok(Fetched {
            key: c.key.clone(),
            url: c.url.clone(),
            artifact_id,
            sha256,
            path,
            bytes,
            media_type,
        })
    }

    async fn http_get(&self, c: &Candidate, prior: Option<&Prior>) -> Result<Got> {
        let gate = self.gate(&c.url);
        let _permit = gate.sem.acquire().await?;
        let mut last_err = anyhow!("no attempt made");
        for attempt in 1..=FETCH_ATTEMPTS {
            {
                let mut next = gate.next.lock().await;
                let now = Instant::now();
                if *next > now {
                    tokio::time::sleep(*next - now).await;
                }
                *next = Instant::now() + self.interval;
            }
            let mut req = match c.method {
                HttpMethod::Get => self.http.get(&c.url),
                HttpMethod::Post => self.http.post(&c.url),
            };
            for (k, v) in &c.headers {
                req = req.header(k, v);
            }
            if let Some(b) = &c.body {
                req = req.body(b.clone());
            }
            if let Some(p) = prior {
                if let Some(e) = &p.etag {
                    req = req.header(reqwest::header::IF_NONE_MATCH, e);
                }
                if let Some(lm) = &p.last_modified {
                    req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
                }
            }
            let mut wait = Duration::from_millis(500 * u64::from(attempt * attempt));
            match req.send().await {
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_MODIFIED => {
                    return Ok(Got::NotModified);
                }
                Ok(resp) if resp.status().is_success() => match self.stream_body(c, resp).await {
                    Ok(b) => return Ok(Got::Body(b)),
                    Err(e) => last_err = e,
                },
                Ok(resp) => {
                    if let Some(s) = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.trim().parse::<u64>().ok())
                    {
                        wait = Duration::from_secs(s).min(MAX_RETRY_AFTER);
                    }
                    let status = resp.status();
                    last_err = anyhow!("HTTP {status}");
                    if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        break; // 4xx other than 429 will not get better by retrying
                    }
                }
                Err(e) => last_err = e.into(),
            }
            if attempt < FETCH_ATTEMPTS {
                self.run
                    .warn(
                        Stage::Fetch,
                        &format!("{} attempt {attempt} failed: {last_err:#}", c.key),
                        None,
                    )
                    .await;
                tokio::time::sleep(wait).await;
            }
        }
        Err(last_err.context(format!("fetching {}", c.url)))
    }

    /// Streams a response body into a store scratch file, hashing as it goes.
    async fn stream_body(&self, c: &Candidate, resp: reqwest::Response) -> Result<Body> {
        let header = |n: reqwest::header::HeaderName| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let etag = header(reqwest::header::ETAG);
        let last_modified = header(reqwest::header::LAST_MODIFIED);
        let media_type = header(reqwest::header::CONTENT_TYPE).or_else(|| c.media_type.clone());
        let status = i16::try_from(resp.status().as_u16()).ok();
        let tmp = self.env.store.tmp_file(&c.key)?;
        let mut file = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("creating {}", tmp.display()))?;
        let mut h = Sha256::new();
        let mut len = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(e.into());
                }
            };
            h.update(&chunk);
            len += chunk.len() as u64;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        Ok(Body {
            tmp,
            sha256: h.finalize().into(),
            bytes: len,
            etag,
            last_modified,
            media_type,
            status,
        })
    }
}

enum Got {
    NotModified,
    Body(Body),
}

struct Body {
    tmp: std::path::PathBuf,
    sha256: [u8; 32],
    bytes: u64,
    etag: Option<String>,
    last_modified: Option<String>,
    media_type: Option<String>,
    status: Option<i16>,
}

type Parts = (
    lanka_store::BlobRef,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i16>,
    bool,
);

impl Body {
    async fn into_parts(self, store: &dyn BlobStore) -> Result<Parts> {
        let (b, created) = store
            .adopt_tmp(Ns::Raw, &self.tmp, self.sha256, self.bytes)
            .await?;
        Ok((
            b,
            created,
            self.etag,
            self.last_modified,
            self.media_type,
            self.status,
            true,
        ))
    }
}

/// The store root a CLI or test uses: `LANKA_STORE_DIR`, else the
/// deprecated `LANKA_DATA_DIR`, else `<repo>/data/v2` beside the registry.
#[must_use]
pub fn default_store(registry_dir: &Path) -> FsStore {
    FsStore::new(lanka_store::root_from_env(
        registry_dir
            .parent()
            .unwrap_or(Path::new("."))
            .join("data/v2"),
    ))
}

/// Convenience used by the CLI and tests: a shared, loaded registry.
pub fn load_registry(dir: &Path) -> Result<Arc<Registry>> {
    Ok(Arc::new(Registry::load(dir)?))
}
