//! `lanka` — the Lanka Data Layer command-line tool.
//!
//! ```text
//! lanka db migrate | set-passwords
//! lanka registry validate | sync
//! lanka ingest run <source> [--from DIR] [--force]
//! lanka ingest restandardise <source>
//! lanka ingest status
//! lanka review list | approve <dsv> --by NAME | reject <dsv> --by NAME --note TEXT
//! lanka release cut [--note TEXT] | list
//! lanka worker [--lane batch|interactive] [--slots 4] [--once]
//! lanka jobs list | show <id> | enqueue ingest|restandardise|release … | cancel <id>
//! lanka ops tick | record-backup --kind pg|store|drill (--ok|--failed)
//! lanka serve [--bind 127.0.0.1:8601] | serve rebuild
//! lanka reverse <lat> <lon>
//! lanka area <pcode>
//! lanka map validate <file>… | hash <file> | refs <file> | resolve <file> [--freeze] | types
//! lanka tiles build [--tileset ID]… [--force] | list
//! lanka grid lookups | list | parity --v1-dir DIR [--v1-geojson FILE] | reverse-check [--n 100000]
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use lanka_core::db::{self, PoolRole};
use lanka_core::{Catalog, Profile, query};
use lanka_ingest::jobs::{self, NewJob};
use lanka_ingest::{
    Env, Registry, RunOptions, derive, implemented, publish, restandardise, run_source, scheduler,
    train, worker,
};
use lanka_store::FsStore;
use sqlx::PgPool;
use tracing_subscriber::EnvFilter;

mod grid_cmd;
mod tiles_cmd;

#[derive(Parser)]
#[command(
    name = "lanka",
    version,
    about = "Lanka Data Layer: ingest, serve and query open data for Sri Lanka"
)]
struct Cli {
    #[command(flatten)]
    global: Global,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args)]
struct Global {
    /// Postgres URL.
    #[arg(
        long,
        env = "DATABASE_URL",
        global = true,
        default_value = "postgres://lanka:lanka-local-dev@127.0.0.1:55432/lanka"
    )]
    database_url: String,
    /// Registry directory (defaults to the nearest `registry/` above the working directory).
    #[arg(long, env = "LANKA_REGISTRY_DIR", global = true)]
    registry: Option<PathBuf>,
    /// Blob store root (raw artifacts, extracts, distributions). Falls back
    /// to the deprecated `LANKA_DATA_DIR`, then `<repo>/data/v2`.
    #[arg(long, env = "LANKA_STORE_DIR", global = true)]
    store_dir: Option<PathBuf>,
    /// Country profile (defaults to the only `country/*/profile.yaml` beside the registry).
    #[arg(long, env = "LANKA_COUNTRY_PROFILE", global = true)]
    profile: Option<PathBuf>,
    /// Log as JSON lines (for journald / log shippers).
    #[arg(long, env = "LANKA_LOG_JSON", global = true)]
    log_json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Database maintenance.
    #[command(subcommand)]
    Db(DbCmd),
    /// Validate or sync the source/dataset registry.
    #[command(subcommand)]
    Registry(RegistryCmd),
    /// Run connectors and inspect ingestion state.
    #[command(subcommand)]
    Ingest(IngestCmd),
    /// Review gate for class B/C dataset versions.
    #[command(subcommand)]
    Review(ReviewCmd),
    /// Release trains: approved versions become visible in one release.
    #[command(subcommand)]
    Release(ReleaseCmd),
    /// Derived datasets: check, list, run and explain recipes.
    #[command(subcommand)]
    Recipe(RecipeCmd),
    /// Run the job worker: scheduler tick + executor over ops.jobs.
    Worker(WorkerArgs),
    /// Inspect and manage the job queue.
    #[command(subcommand)]
    Jobs(JobsCmd),
    /// Operations: scheduler tick, backup records.
    #[command(subcommand)]
    Ops(OpsCmd),
    /// Run the REST v2 server (or `serve rebuild` the serving projections).
    #[command(args_conflicts_with_subcommands = true)]
    Serve {
        #[arg(long, env = "LANKA_BIND", default_value = "127.0.0.1:8601")]
        bind: SocketAddr,
        #[command(subcommand)]
        action: Option<ServeCmd>,
    },
    /// Reverse geocode a point to admin units and the canonical grid cell.
    #[command(allow_negative_numbers = true)]
    Reverse {
        lat: f64,
        lon: f64,
        #[arg(long)]
        json: bool,
    },
    /// Print the area profile of a p-code as JSON.
    Area { pcode: String },
    /// Map documents (`ldl-map/1`): validate, hash, references, resolve.
    #[command(subcommand)]
    Map(MapCmd),
    /// Vector tilesets (PMTiles) from the current release.
    #[command(subcommand)]
    Tiles(tiles_cmd::TilesCmd),
    /// Grid engine: lookup arrays, layers, v1 parity, reverse-geocoding check.
    #[command(subcommand)]
    Grid(grid_cmd::GridCmd),
    /// HTTP GET a local URL and exit 0 on 200 (container health checks; the
    /// image has no curl). Only plain `http://host:port/path` is supported.
    Probe {
        #[arg(default_value = "http://127.0.0.1:8601/v2/health")]
        url: String,
    },
}

#[derive(Subcommand)]
enum MapCmd {
    /// Validate documents against spec/map/1 (schema + limits + references); exit 1 on any problem.
    Validate {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print problems as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Content hash and snapshot id (RFC 8785 canonical JSON, without `id` / `meta.updated_at`).
    Hash { file: PathBuf },
    /// What a document depends on (`app.map_refs` rows).
    Refs { file: PathBuf },
    /// Resolve against the catalogue: versions, frame URLs, attribution, notices, problems.
    Resolve {
        file: PathBuf,
        /// Pin the document to the current release first.
        #[arg(long)]
        freeze: bool,
    },
    /// Print the generated TypeScript declarations (spec/map/1/index.d.ts).
    Types,
}

#[derive(Subcommand)]
enum DbCmd {
    /// Apply pending migrations (engine/migrations), ownership and grants.
    /// Run as the database admin role, never as lanka_api / lanka_ingest.
    Migrate {
        /// Also set the application roles' passwords from LANKA_API_PASSWORD,
        /// LANKA_INGEST_PASSWORD, LANKA_RECIPE_PASSWORD, LANKA_APP_PASSWORD (when set).
        #[arg(long)]
        set_role_passwords: bool,
    },
    /// Set the application roles' passwords from LANKA_API_PASSWORD,
    /// LANKA_INGEST_PASSWORD, LANKA_RECIPE_PASSWORD and LANKA_APP_PASSWORD.
    SetPasswords,
}

#[derive(Subcommand)]
enum RecipeCmd {
    /// Validate registry/recipes against the schema, the method library and
    /// the registry; prove the global DAG acyclic.
    Check,
    /// Every recipe with what the next train would do (reused / would_run / waiting_inputs …).
    List,
    /// The method library.
    Methods,
    /// Run one recipe now (stages a version; the next train releases it if approved).
    Run {
        id: String,
        /// Run even when a derivation with the current fingerprint exists.
        #[arg(long)]
        force: bool,
        /// Report the fingerprint and what would run; stage nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Lineage of a derived observation: derivation, formula, contributors.
    Explain {
        obs_id: i64,
        #[arg(long, default_value_t = 1)]
        depth: u8,
    },
}

#[derive(Subcommand)]
enum ReleaseCmd {
    /// Run a release train now (manual trigger): every approved version is
    /// released together; nothing approved = no_change.
    Cut {
        #[arg(long)]
        note: Option<String>,
    },
    /// Recent releases and trains.
    List {
        #[arg(long, default_value_t = 10)]
        limit: i64,
    },
}

#[derive(Args)]
struct WorkerArgs {
    #[arg(long, env = "LANKA_WORKER_LANE", default_value = "batch", value_parser = ["batch", "interactive"])]
    lane: String,
    /// Slot budget (sum of running jobs' weights; an ingest weighs 2).
    #[arg(long, env = "LANKA_WORKER_SLOTS", default_value_t = 4)]
    slots: i16,
    /// Scheduler tick interval in seconds.
    #[arg(long, env = "LANKA_WORKER_TICK", default_value_t = 60)]
    tick: u64,
    /// Seconds running jobs may finish after SIGTERM before being re-queued.
    #[arg(long, env = "LANKA_WORKER_GRACE", default_value_t = 25)]
    grace: u64,
    /// One tick, run what is runnable, exit.
    #[arg(long)]
    once: bool,
}

#[derive(Subcommand)]
enum JobsCmd {
    /// Recent jobs (newest first).
    List {
        #[arg(long, value_parser = ["queued", "running", "succeeded", "failed", "cancelled"])]
        state: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// One job as JSON.
    Show { id: i64 },
    /// Enqueue a job (idempotent: a live job with the same key is returned).
    #[command(subcommand)]
    Enqueue(EnqueueCmd),
    /// Cancel a job (queued: at once; running: on its next heartbeat).
    Cancel { id: i64 },
}

#[derive(Subcommand)]
enum EnqueueCmd {
    Ingest {
        source: String,
        #[arg(long)]
        from: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    Restandardise {
        source: String,
    },
    /// A manual release train.
    Release {
        #[arg(long)]
        note: Option<String>,
    },
    /// Recompute one recipe (or every stale one) outside the train.
    Recipe {
        recipe: Option<String>,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum OpsCmd {
    /// Run one scheduler tick (enqueue due work, reap abandoned jobs, sync incidents).
    Tick,
    /// Record a backup / restore-drill outcome (called by the backup scripts).
    RecordBackup {
        #[arg(long, value_parser = ["pg", "store", "drill"])]
        kind: String,
        #[arg(long, conflicts_with = "failed")]
        ok: bool,
        #[arg(long)]
        failed: bool,
        /// JSON detail (size, duration, repository …).
        #[arg(long)]
        detail: Option<String>,
    },
}

#[derive(Subcommand)]
enum ServeCmd {
    /// Rebuild the serving projections (serve.area_latest) from the current release.
    Rebuild,
}

#[derive(Subcommand)]
enum RegistryCmd {
    /// Validate every registry file and the country profile against registry/schema/*.schema.json.
    Validate,
    /// Mechanical WP-01 migration of datasets/*.yaml to the standard model
    /// (UCUM units, measure_type/aggregation/class, kind); prints a review list.
    Migrate {
        /// Rewrite the files (default: dry run).
        #[arg(long)]
        write: bool,
        /// Write the review list (CSV) here.
        #[arg(long)]
        review_out: Option<PathBuf>,
    },
    /// Mirror the registry into catalog.* (also done before every ingest run).
    Sync,
}

#[derive(Subcommand)]
enum IngestCmd {
    /// Run one source end to end (discover → fetch → archive → extract → standardise →
    /// validate → stage → publish).
    Run {
        source: String,
        /// Read inputs from a local directory instead of the network (where supported).
        #[arg(long)]
        from: Option<PathBuf>,
        #[arg(long, default_value = "manual", value_parser = ["manual", "schedule", "retry", "test", "backfill"])]
        trigger: String,
        /// Ignore the fingerprint and artifact-set short-circuits.
        #[arg(long)]
        force: bool,
    },
    /// Re-run standardise → publish over the source's stored extracts (offline:
    /// no network, no re-parsing). Accepts a source id or a dataset id.
    Restandardise { source: String },
    /// Freshness and last-run state of every source.
    Status {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ReviewCmd {
    /// Dataset versions waiting for sign-off.
    List,
    /// Approve a version: it is released by the next train.
    Approve {
        dsv: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// Reject a version (it stays archived, never published).
    Reject {
        dsv: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: String,
    },
}

fn init_tracing(json: bool) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    let b = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    if json {
        b.json().init();
    } else {
        b.compact().init();
    }
}

fn find_registry(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_owned());
    }
    let mut dir = std::env::current_dir()?;
    loop {
        let cand = dir.join("registry");
        if cand.join("schema").is_dir() {
            return Ok(cand);
        }
        if !dir.pop() {
            anyhow::bail!(
                "no registry/ directory found; pass --registry or set LANKA_REGISTRY_DIR"
            );
        }
    }
}

fn load_profile(g: &Global, registry: &Path) -> Result<Profile> {
    let path = match &g.profile {
        Some(p) => p.clone(),
        None => Profile::discover(registry)?,
    };
    Profile::load(&path).with_context(|| format!("loading country profile {}", path.display()))
}

fn store(g: &Global, registry: &Path) -> FsStore {
    match &g.store_dir {
        Some(d) => FsStore::new(d),
        None => lanka_ingest::default_store(registry),
    }
}

async fn pool(g: &Global, role: PoolRole) -> Result<PgPool> {
    db::connect(&g.database_url, role)
        .await
        .context("connecting to Postgres (is infra/v2 up?)")
}

fn read_doc(file: &Path) -> Result<serde_json::Value> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not JSON", file.display()))
}

fn checked_doc(file: &Path) -> Result<(serde_json::Value, lanka_mapdoc::MapDocument)> {
    let v = read_doc(file)?;
    match lanka_mapdoc::validate(&v) {
        Ok(d) => Ok((v, d)),
        Err(problems) => {
            for p in &problems {
                eprintln!("{}: {p}", file.display());
            }
            anyhow::bail!("{}: {} problem(s)", file.display(), problems.len())
        }
    }
}

async fn map_cmd(g: &Global, cmd: MapCmd) -> Result<()> {
    match cmd {
        MapCmd::Validate { files, json } => {
            let mut bad = 0usize;
            let mut report = Vec::new();
            for f in &files {
                let res = match read_doc(f) {
                    Ok(v) => lanka_mapdoc::validate(&v).map(|_| ()),
                    Err(e) => Err(vec![lanka_mapdoc::Problem {
                        path: String::new(),
                        code: "JSON",
                        message: format!("{e:#}"),
                    }]),
                };
                match res {
                    Ok(()) => {
                        if !json {
                            println!("{}: ok", f.display());
                        }
                        report.push(serde_json::json!({ "file": f, "ok": true }));
                    }
                    Err(problems) => {
                        bad += 1;
                        if !json {
                            for p in &problems {
                                println!("{}: {p}", f.display());
                            }
                        }
                        report.push(
                            serde_json::json!({ "file": f, "ok": false, "problems": problems }),
                        );
                    }
                }
            }
            if json {
                print_json(&report)?;
            }
            anyhow::ensure!(bad == 0, "{bad} of {} document(s) invalid", files.len());
        }
        MapCmd::Hash { file } => {
            let (v, _) = checked_doc(&file)?;
            print_json(&serde_json::json!({
                "snapshot_id": lanka_mapdoc::canon::snapshot_id(&v),
                "sha256": lanka_mapdoc::canon::content_sha256(&v).iter().map(|b| format!("{b:02x}")).collect::<String>(),
                "canonical_bytes": lanka_mapdoc::canon::jcs(&v).len(),
            }))?;
        }
        MapCmd::Refs { file } => {
            let (_, doc) = checked_doc(&file)?;
            let refs = lanka_mapdoc::refs::references(&doc);
            let cat = match pool(g, PoolRole::Api).await {
                Ok(p) => Catalog::load(&p).await.ok(),
                Err(_) => None,
            };
            let rows = lanka_mapdoc::refs::map_refs(&refs, |id| {
                cat.as_ref()
                    .and_then(|c| c.indicators.get(id).map(|i| i.dataset_id.clone()))
            });
            print_json(&serde_json::json!({ "references": refs, "map_refs": rows }))?;
        }
        MapCmd::Resolve { file, freeze } => {
            let (_, mut doc) = checked_doc(&file)?;
            let pool = pool(g, PoolRole::Api).await?;
            let cat = Catalog::load(&pool).await?;
            if freeze {
                anyhow::ensure!(
                    lanka_mapdoc::resolve::freeze(&mut doc, &cat),
                    "no release to pin to"
                );
            }
            let ctx = lanka_mapdoc::resolve::load_context(&pool, &doc).await?;
            let resolved = lanka_mapdoc::resolve::resolve(&doc, &cat, &ctx);
            print_json(&lanka_mapdoc::resolve::ResolvedMap {
                doc: &doc,
                resolved,
            })?;
        }
        MapCmd::Types => print!("{}", lanka_mapdoc::ts::declarations()),
    }
    Ok(())
}

/// Registry, profile and checked recipes.
async fn derive_parts(g: &Global) -> Result<(Registry, Profile, lanka_enrich::Recipes)> {
    let dir = find_registry(g.registry.as_deref())?;
    let reg = Registry::load(&dir)?;
    let profile = load_profile(g, &dir)?;
    let (recipes, warnings) = derive::load_checked(&reg, &lanka_enrich::Methods::builtin())?;
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok((reg, profile, recipes))
}

async fn recipe_cmd(g: &Global, cmd: RecipeCmd) -> Result<()> {
    let methods = lanka_enrich::Methods::builtin();
    match cmd {
        RecipeCmd::Check => {
            let dir = find_registry(g.registry.as_deref())?;
            let reg = Registry::load(&dir)?;
            let recipes = lanka_enrich::Recipes::load(&dir)?;
            let rep = recipes.check(&derive::registry_view(&reg), &methods);
            for w in &rep.warnings {
                eprintln!("warning: {w}");
            }
            for e in &rep.errors {
                eprintln!("error: {e}");
            }
            anyhow::ensure!(
                rep.errors.is_empty(),
                "{} recipe error(s)",
                rep.errors.len()
            );
            println!(
                "recipes OK: {} (order: {})",
                recipes.list.len(),
                rep.order.join(" → ")
            );
        }
        RecipeCmd::Methods => {
            let list: Vec<serde_json::Value> = methods.iter().map(|m| m.info().to_json()).collect();
            print_json(&list)?;
        }
        RecipeCmd::Explain { obs_id, depth } => {
            let pool = pool(g, PoolRole::Api).await?;
            let dir = find_registry(g.registry.as_deref())?;
            let profile = load_profile(g, &dir)?;
            let geo = lanka_enrich::provenance::GeoContext {
                calendars: profile.calendars.clone(),
                reference_set: profile.spec.admin.reference_boundary_set.clone(),
                scheme: profile.spec.admin.scheme.clone(),
            };
            match lanka_enrich::provenance::explain_obs(&pool, &geo, &methods, obs_id, depth)
                .await?
            {
                Some(v) => print_json(&v)?,
                None => {
                    anyhow::bail!("observation {obs_id} is not a derived row (or does not exist)")
                }
            }
        }
        cmd @ (RecipeCmd::List | RecipeCmd::Run { .. }) => {
            let (reg, profile, recipes) = derive_parts(g).await?;
            let pool = pool(g, PoolRole::Ingest).await?;
            reg.sync(&pool, &profile)
                .await
                .context("syncing registry")?;
            let env = derive::DeriveEnv {
                pool: &pool,
                registry: &reg,
                profile: &profile,
                recipes: &recipes,
                methods: &methods,
            };
            let report = match cmd {
                RecipeCmd::Run { id, force, dry_run } => {
                    anyhow::ensure!(
                        recipes.get(&id).is_some(),
                        "no recipe {id:?} in the registry"
                    );
                    derive::recompute(
                        &env,
                        &derive::RecomputeOpts {
                            trigger: "manual".into(),
                            only: Some(id),
                            force,
                            dry_run,
                            include_scheduled: true,
                            ..derive::RecomputeOpts::default()
                        },
                    )
                    .await?
                }
                _ => derive::status(&env).await?,
            };
            print_json(&report)?;
            anyhow::ensure!(report.failed == 0, "{} recipe(s) failed", report.failed);
        }
    }
    Ok(())
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.global.log_json);
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let g = &cli.global;
    match cli.cmd {
        Cmd::Db(DbCmd::SetPasswords) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            set_role_passwords(&pool).await?;
        }
        Cmd::Db(DbCmd::Migrate {
            set_role_passwords: pw,
        }) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            db::migrate(&pool).await?;
            if pw {
                set_role_passwords(&pool).await?;
            }
            let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
                .fetch_one(&pool)
                .await?;
            println!("migrations applied: {n}");
            // Migrated versions need the registry + profile to get their hashes.
            if let Ok(dir) = find_registry(g.registry.as_deref()) {
                let reg = Registry::load(&dir)?;
                let profile = load_profile(g, &dir)?;
                let r = reg.sync(&pool, &profile).await?;
                println!(
                    "registry synced ({}); {} version hash(es) backfilled",
                    reg.summary(),
                    r.hashes_backfilled
                );
                let methods = lanka_enrich::Methods::builtin();
                let (recipes, _) = derive::load_checked(&reg, &methods)?;
                let (m, n) = derive::sync(&pool, &recipes, &methods).await?;
                println!("method library synced ({m} methods); {n} recipe(s)");
            }
        }
        Cmd::Registry(RegistryCmd::Migrate { write, review_out }) => {
            let dir = find_registry(g.registry.as_deref())?;
            let profile = load_profile(g, &dir)?;
            let r = lanka_ingest::registry_migrate::migrate(&dir, &profile, write)?;
            eprintln!(
                "{} dataset file(s) {}, {} indicator(s) migrated, {} for review",
                r.files_changed,
                if write { "rewritten" } else { "would change" },
                r.indicators,
                r.review.len()
            );
            if let Some(out) = review_out {
                std::fs::write(&out, r.review_csv())?;
                eprintln!("review list: {}", out.display());
            }
        }
        Cmd::Registry(cmd) => {
            let dir = find_registry(g.registry.as_deref())?;
            let reg = Registry::load(&dir)?;
            let profile = load_profile(g, &dir)?;
            for w in &reg.warnings {
                eprintln!("warning: {w}");
            }
            match cmd {
                RegistryCmd::Validate => {
                    Registry::validate_profile(&dir, &profile)?;
                    let (recipes, warnings) =
                        derive::load_checked(&reg, &lanka_enrich::Methods::builtin())?;
                    for w in warnings {
                        eprintln!("warning: {w}");
                    }
                    println!("recipes OK: {}", recipes.list.len());
                    println!(
                        "registry OK ({}): {}; country profile {:?}",
                        dir.display(),
                        reg.summary(),
                        profile.spec.id
                    );
                }
                RegistryCmd::Migrate { .. } => unreachable!("handled above"),
                RegistryCmd::Sync => {
                    let pool = pool(g, PoolRole::Ingest).await?;
                    let r = reg.sync(&pool, &profile).await?;
                    sqlx::query("SELECT pg_notify('lanka_release', 'registry-sync')")
                        .execute(&pool)
                        .await?;
                    print_json(&r)?;
                }
            }
        }
        Cmd::Ingest(cmd @ (IngestCmd::Run { .. } | IngestCmd::Restandardise { .. })) => {
            let dir = find_registry(g.registry.as_deref())?;
            let reg = Registry::load(&dir)?;
            let profile = load_profile(g, &dir)?;
            let pool = pool(g, PoolRole::Ingest).await?;
            reg.sync(&pool, &profile)
                .await
                .context("syncing registry")?;
            let store = store(g, &dir);
            let env = Env {
                pool: &pool,
                registry: &reg,
                profile: &profile,
                store: &store,
            };
            let report = match cmd {
                IngestCmd::Run {
                    source,
                    from,
                    trigger,
                    force,
                } => {
                    let opts = RunOptions {
                        from_dir: from,
                        trigger,
                        force,
                        job_id: None,
                    };
                    run_source(&env, &source, &opts).await?
                }
                IngestCmd::Restandardise { source } => {
                    // A dataset id resolves to its (first) source.
                    let source = if reg.source(&source).is_some() {
                        source
                    } else {
                        reg.dataset(&source)
                            .and_then(|d| d.source_ids.first().cloned())
                            .with_context(|| {
                                format!("{source:?} is neither a source nor a dataset")
                            })?
                    };
                    restandardise(&env, &source).await?
                }
                IngestCmd::Status { .. } => unreachable!("matched above"),
            };
            print_json(&report)?;
            if report.published.iter().any(|p| p.outcome == "approved") {
                eprintln!(
                    "approved version(s) are released by the next train (daily, or `lanka release cut`)"
                );
            }
        }
        Cmd::Ingest(IngestCmd::Status { json }) => {
            let implemented = find_registry(g.registry.as_deref())
                .ok()
                .and_then(|d| Registry::load(&d).ok())
                .map(|r| implemented(&r))
                .unwrap_or_default();
            ingest_status(&pool(g, PoolRole::Api).await?, json, &implemented).await?
        }
        Cmd::Review(cmd) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            match cmd {
                ReviewCmd::List => print_json(&publish::pending_reviews(&pool).await?)?,
                ReviewCmd::Approve { dsv, by, note } => {
                    print_json(&publish::approve(&pool, &dsv, &by, note.as_deref()).await?)?
                }
                ReviewCmd::Reject { dsv, by, note } => {
                    publish::reject(&pool, &dsv, &by, &note).await?;
                    println!("{dsv} rejected");
                }
            }
        }
        Cmd::Serve {
            action: Some(ServeCmd::Rebuild),
            ..
        } => {
            let pool = pool(g, PoolRole::Ingest).await?;
            let n = train::rebuild_serve(&pool).await?;
            sqlx::query("SELECT pg_notify('lanka_release', 'serve-rebuild')")
                .execute(&pool)
                .await?;
            println!("serve.area_latest rebuilt: {n} rows");
        }
        Cmd::Serve { bind, action: None } => {
            let pool = pool(g, PoolRole::Api).await?;
            lanka_server::serve(pool, bind).await?;
        }
        Cmd::Release(cmd) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            match cmd {
                ReleaseCmd::Cut { note } => {
                    let parts = derive_parts(g).await;
                    if let Err(e) = &parts {
                        eprintln!("warning: derived recompute skipped: {e:#}");
                    }
                    let methods = lanka_enrich::Methods::builtin();
                    let parts = parts.ok();
                    let env = parts
                        .as_ref()
                        .map(|(reg, profile, recipes)| derive::DeriveEnv {
                            pool: &pool,
                            registry: reg,
                            profile,
                            recipes,
                            methods: &methods,
                        });
                    print_json(
                        &train::run_train_with(
                            &pool,
                            "manual",
                            None,
                            note.as_deref(),
                            env.as_ref(),
                        )
                        .await?,
                    )?;
                }
                ReleaseCmd::List { limit } => release_list(&pool, limit).await?,
            }
        }
        Cmd::Recipe(cmd) => recipe_cmd(g, cmd).await?,
        Cmd::Worker(w) => {
            let dir = find_registry(g.registry.as_deref())?;
            let pool = pool(g, PoolRole::Worker).await?;
            let cfg = worker::WorkerConfig {
                lane: w.lane,
                slots: w.slots.max(1),
                tick: std::time::Duration::from_secs(w.tick.max(1)),
                profile_path: g.profile.clone(),
                store_root: store(g, &dir).root().to_path_buf(),
                registry_dir: dir,
                grace: std::time::Duration::from_secs(w.grace),
                once: w.once,
                tick_cfg: scheduler::TickConfig::from_env(),
            };
            worker::run(pool, cfg, shutdown_signal()).await?;
        }
        Cmd::Jobs(cmd) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            match cmd {
                JobsCmd::List { state, limit } => {
                    let list = jobs::list(&pool, state.as_deref(), limit).await?;
                    println!(
                        "{:>6} {:<14} {:<34} {:<10} {:>4} {:<20} ERROR",
                        "ID", "KIND", "DEDUPE KEY", "STATE", "TRY", "RUN AFTER / FINISHED"
                    );
                    for j in list {
                        println!(
                            "{:>6} {:<14} {:<34} {:<10} {:>4} {:<20} {}",
                            j.id,
                            j.kind,
                            j.dedupe_key,
                            j.state,
                            format!("{}/{}", j.attempts, j.max_attempts),
                            j.finished_at
                                .unwrap_or(j.run_after)
                                .format("%Y-%m-%d %H:%M:%S"),
                            j.error
                                .as_deref()
                                .and_then(|e| e.lines().next())
                                .unwrap_or_default()
                        );
                    }
                }
                JobsCmd::Show { id } => match jobs::get(&pool, id).await? {
                    Some(j) => print_json(&j)?,
                    None => anyhow::bail!("no job {id}"),
                },
                JobsCmd::Cancel { id } => println!("job {id}: {}", jobs::cancel(&pool, id).await?),
                JobsCmd::Enqueue(e) => {
                    let job = match e {
                        EnqueueCmd::Ingest {
                            source,
                            from,
                            force,
                        } => NewJob::ingest(
                            &source,
                            "manual",
                            serde_json::json!({ "from": from, "force": force }),
                        ),
                        EnqueueCmd::Restandardise { source } => {
                            let mut j = NewJob::ingest(&source, "manual", serde_json::json!({}));
                            j.kind = "restandardise".into();
                            j.dedupe_key = format!("restandardise:{source}");
                            j
                        }
                        EnqueueCmd::Recipe { recipe, force } => {
                            NewJob::recipe(recipe.as_deref(), "manual", force)
                        }
                        EnqueueCmd::Release { note } => {
                            let mut j = NewJob::release(
                                &format!("manual:{}", chrono::Utc::now().format("%Y%m%dT%H%M%S")),
                                "manual",
                                "manual",
                            );
                            j.payload =
                                serde_json::json!({ "train_trigger": "manual", "note": note });
                            j
                        }
                    };
                    let (id, created) = jobs::enqueue(&pool, &job).await?;
                    println!(
                        "job {id} {} ({})",
                        if created { "queued" } else { "already live" },
                        job.dedupe_key
                    );
                }
            }
        }
        Cmd::Ops(OpsCmd::Tick) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            let mut cfg = scheduler::TickConfig::from_env();
            if let Ok(dir) = find_registry(g.registry.as_deref()) {
                cfg.store_pct = worker::store_pct(store(g, &dir).root());
            }
            print_json(&scheduler::tick(&pool, &cfg).await?)?;
        }
        Cmd::Ops(OpsCmd::RecordBackup {
            kind,
            ok,
            failed,
            detail,
        }) => {
            anyhow::ensure!(ok || failed, "pass --ok or --failed");
            let detail: Option<serde_json::Value> = detail
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .context("--detail must be JSON")?;
            let pool = pool(g, PoolRole::Ingest).await?;
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO ops.backups (kind, ok, detail) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(&kind)
            .bind(ok)
            .bind(&detail)
            .fetch_one(&pool)
            .await?;
            println!(
                "backup record {id}: {kind} {}",
                if ok { "ok" } else { "failed" }
            );
        }
        Cmd::Reverse { lat, lon, json } => {
            let pool = pool(g, PoolRole::Api).await?;
            let cat = Catalog::load(&pool).await?;
            let r = query::reverse(&pool, &cat, lat, lon).await?;
            if json {
                print_json(&r)?;
            } else {
                println!(
                    "cell {}  ({lat}, {lon})  match={}{}",
                    r.cell_id,
                    r.match_kind,
                    r.distance_m
                        .map(|d| format!(" ({d} m)"))
                        .unwrap_or_default()
                );
                for a in r.ancestors.iter().chain(std::iter::once(&r.unit)) {
                    println!("  ADM{} {:<10} {}", a.level, a.pcode, a.name_en);
                }
            }
        }
        Cmd::Probe { url } => probe(&url).await?,
        Cmd::Tiles(cmd) => tiles_cmd::run(g, cmd).await?,
        Cmd::Map(cmd) => map_cmd(g, cmd).await?,
        Cmd::Grid(cmd) => {
            let dir = find_registry(g.registry.as_deref())?;
            let reg = Registry::load(&dir)?;
            let profile = load_profile(g, &dir)?;
            let pool = pool(g, PoolRole::Ingest).await?;
            reg.sync(&pool, &profile)
                .await
                .context("syncing registry")?;
            let store = store(g, &dir);
            let env = Env {
                pool: &pool,
                registry: &reg,
                profile: &profile,
                store: &store,
            };
            grid_cmd::run(cmd, &env, &profile).await?;
        }
        Cmd::Area { pcode } => {
            let pool = pool(g, PoolRole::Api).await?;
            let cat = Catalog::load(&pool).await?;
            let p = query::parse_pcode(&cat, &pcode)?;
            let (profile, touched) = query::area_profile(&pool, &cat, &p).await?;
            let mut ds: Vec<&str> = touched.iter().map(String::as_str).collect();
            ds.extend(cat.boundary_dataset());
            print_json(&lanka_core::Envelope::ok(
                profile,
                cat.meta_for(ds, Vec::new()),
            ))?;
        }
    }
    Ok(())
}

/// (id, kind, subject, opened_at)
type IncidentRow = (i64, String, String, chrono::DateTime<chrono::Utc>);

async fn ingest_status(pool: &PgPool, json: bool, implemented: &[String]) -> Result<()> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM catalog.sources ORDER BY id")
        .fetch_all(pool)
        .await?;
    let fresh = query::source_freshness(pool, &ids).await?;
    let pending = publish::pending_reviews(pool).await?;
    let incidents: Vec<IncidentRow> = sqlx::query_as(
        "SELECT id, kind, subject, opened_at FROM ops.incidents WHERE closed_at IS NULL ORDER BY id DESC LIMIT 20",
    )
    .fetch_all(pool)
    .await?;
    if json {
        return print_json(&serde_json::json!({
            "sources": fresh,
            "pending_reviews": pending,
            "open_incidents": incidents.iter().map(|i| serde_json::json!({
                "id": i.0, "kind": i.1, "subject": i.2, "opened_at": i.3.to_rfc3339()
            })).collect::<Vec<_>>(),
        }));
    }
    println!(
        "{:<30} {:<10} {:<16} {:<13} {:<17} CONNECTOR",
        "SOURCE", "CADENCE", "STATE", "LAST", "LAST SUCCESS"
    );
    for f in &fresh {
        println!(
            "{:<30} {:<10} {:<16} {:<13} {:<17} {}",
            f.source_id,
            f.cadence,
            f.state,
            f.last_status.as_deref().unwrap_or("-"),
            f.last_success_at.map_or_else(
                || "-".to_owned(),
                |t| t.format("%Y-%m-%d %H:%M").to_string()
            ),
            if implemented.contains(&f.source_id) {
                "yes"
            } else {
                "-"
            }
        );
    }
    if !pending.is_empty() {
        println!("\npending review:");
        for p in &pending {
            println!(
                "  {} {} ({} rows, class {})",
                p.dsv, p.dataset_id, p.row_count, p.publication_class
            );
        }
    }
    if !incidents.is_empty() {
        println!("\nopen incidents:");
        for i in &incidents {
            println!(
                "  #{} {} {} since {}",
                i.0,
                i.1,
                i.2,
                i.3.format("%Y-%m-%d %H:%M")
            );
        }
    }
    Ok(())
}

/// (data_version, created_at, train id, notes)
type ReleaseRow = (
    String,
    chrono::DateTime<chrono::Utc>,
    Option<i64>,
    Option<String>,
);

/// (id, trigger, started_at, outcome, data_version, versions)
type TrainRow = (
    i64,
    String,
    chrono::DateTime<chrono::Utc>,
    String,
    Option<String>,
    i32,
);

async fn release_list(pool: &PgPool, limit: i64) -> Result<()> {
    let rel: Vec<ReleaseRow> = sqlx::query_as(
        "SELECT data_version, created_at, train_id, notes FROM data.releases ORDER BY seq DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    println!("releases:");
    for r in rel {
        println!(
            "  {:<12} {}  train {:<6} {}",
            r.0,
            r.1.format("%Y-%m-%d %H:%M:%S"),
            r.2.map_or_else(|| "-".to_owned(), |t| t.to_string()),
            r.3.unwrap_or_default()
        );
    }
    let trains: Vec<TrainRow> = sqlx::query_as(
        "SELECT id, trigger, started_at, outcome, data_version, versions FROM ops.trains
             ORDER BY id DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    println!("trains:");
    for t in trains {
        println!(
            "  #{:<5} {:<8} {}  {:<9} {} ({} version(s))",
            t.0,
            t.1,
            t.2.format("%Y-%m-%d %H:%M:%S"),
            t.3,
            t.4.unwrap_or_else(|| "-".to_owned()),
            t.5
        );
    }
    Ok(())
}

/// ALTER ROLE … PASSWORD for the application roles, from the environment.
/// The password travels as a bound parameter into a transaction-local
/// setting and is quoted server-side, so no statement text (client or
/// server log) ever contains it.
async fn set_role_passwords(pool: &PgPool) -> Result<()> {
    for (role, var) in [
        ("lanka_api", "LANKA_API_PASSWORD"),
        ("lanka_ingest", "LANKA_INGEST_PASSWORD"),
        ("lanka_recipe", "LANKA_RECIPE_PASSWORD"),
        ("lanka_app", "LANKA_APP_PASSWORD"),
    ] {
        let Some(pw) = std::env::var(var).ok().filter(|v| !v.is_empty()) else {
            continue;
        };
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('lanka.role', $1, true), set_config('lanka.pw', $2, true)")
            .bind(role)
            .bind(&pw)
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(
            "DO $$ BEGIN
               EXECUTE format('ALTER ROLE %I PASSWORD %L', current_setting('lanka.role'), current_setting('lanka.pw'));
             END $$",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        println!("password set for {role}");
    }
    Ok(())
}

/// Minimal HTTP/1.0 GET over TCP: succeeds on a 200 status line.
async fn probe(url: &str) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url
        .strip_prefix("http://")
        .context("probe supports http:// URLs only")?;
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
    let fut = async {
        let mut s = tokio::net::TcpStream::connect(hostport).await?;
        s.write_all(format!("GET /{path} HTTP/1.0\r\nHost: {hostport}\r\n\r\n").as_bytes())
            .await?;
        let mut buf = vec![0u8; 64];
        let n = s.read(&mut buf).await?;
        anyhow::Ok(String::from_utf8_lossy(&buf[..n]).into_owned())
    };
    let head = tokio::time::timeout(std::time::Duration::from_secs(5), fut)
        .await
        .context("probe timed out")??;
    let line = head.lines().next().unwrap_or_default();
    anyhow::ensure!(
        line.split_whitespace().nth(1) == Some("200"),
        "probe: {line}"
    );
    Ok(())
}

/// Resolves on SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { () = ctrl_c => {}, () = term => {} }
    tracing::info!("shutdown requested");
}
