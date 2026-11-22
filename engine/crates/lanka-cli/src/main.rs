//! `lanka` — the Lanka Data Layer command-line tool.
//!
//! ```text
//! lanka db migrate
//! lanka registry validate | sync
//! lanka ingest run <source> [--from DIR]
//! lanka ingest status
//! lanka review list | approve <dsv> --by NAME | reject <dsv> --by NAME --note TEXT
//! lanka serve [--bind 127.0.0.1:8601]
//! lanka reverse <lat> <lon>
//! lanka area <pcode>
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use lanka_core::db::{self, PoolRole};
use lanka_core::{Catalog, Profile, query};
use lanka_ingest::{AlertConfig, Archive, IMPLEMENTED, Registry, RunOptions, publish, run_source};
use sqlx::PgPool;
use tracing_subscriber::EnvFilter;

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
    /// Raw archive root (content-addressed by sha256).
    #[arg(long, env = "LANKA_DATA_DIR", global = true)]
    data_dir: Option<PathBuf>,
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
    /// Run the REST v2 server.
    Serve {
        #[arg(long, env = "LANKA_BIND", default_value = "127.0.0.1:8601")]
        bind: SocketAddr,
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
}

#[derive(Subcommand)]
enum DbCmd {
    /// Apply pending migrations (engine/migrations).
    Migrate,
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
    /// Run one source end to end (discover → fetch → archive → parse → validate → stage → publish).
    Run {
        source: String,
        /// Read inputs from a local directory instead of the network (where supported).
        #[arg(long)]
        from: Option<PathBuf>,
        #[arg(long, default_value = "manual", value_parser = ["manual", "schedule", "retry", "test"])]
        trigger: String,
    },
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
    /// Approve and publish a version.
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

fn data_dir(g: &Global, registry: &Path) -> PathBuf {
    g.data_dir
        .clone()
        .unwrap_or_else(|| registry.parent().unwrap_or(Path::new(".")).join("data/v2"))
}

async fn pool(g: &Global, role: PoolRole) -> Result<PgPool> {
    db::connect(&g.database_url, role)
        .await
        .context("connecting to Postgres (is infra/v2 up?)")
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
        Cmd::Db(DbCmd::Migrate) => {
            let pool = pool(g, PoolRole::Ingest).await?;
            db::migrate(&pool).await?;
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
        Cmd::Ingest(IngestCmd::Run {
            source,
            from,
            trigger,
        }) => {
            let dir = find_registry(g.registry.as_deref())?;
            let reg = Registry::load(&dir)?;
            let profile = load_profile(g, &dir)?;
            let pool = pool(g, PoolRole::Ingest).await?;
            reg.sync(&pool, &profile)
                .await
                .context("syncing registry")?;
            let archive = Archive::new(data_dir(g, &dir));
            let opts = RunOptions {
                from_dir: from,
                trigger,
            };
            let report = run_source(
                &pool,
                &reg,
                &profile,
                &archive,
                &source,
                &opts,
                &AlertConfig::from_env(),
            )
            .await?;
            print_json(&report)?;
        }
        Cmd::Ingest(IngestCmd::Status { json }) => {
            ingest_status(&pool(g, PoolRole::Api).await?, json).await?
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
        Cmd::Serve { bind } => {
            let pool = pool(g, PoolRole::Api).await?;
            lanka_server::serve(pool, bind).await?;
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

/// (id, source, kind, message, created_at)
type AlertRow = (
    i64,
    Option<String>,
    String,
    String,
    chrono::DateTime<chrono::Utc>,
);

async fn ingest_status(pool: &PgPool, json: bool) -> Result<()> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM catalog.sources ORDER BY id")
        .fetch_all(pool)
        .await?;
    let fresh = query::source_freshness(pool, &ids).await?;
    let pending = publish::pending_reviews(pool).await?;
    let alerts: Vec<AlertRow> = sqlx::query_as(
        "SELECT id, source_id, kind, message, created_at FROM ops.alerts
         WHERE created_at > now() - interval '7 days' ORDER BY id DESC LIMIT 10",
    )
    .fetch_all(pool)
    .await?;
    if json {
        return print_json(&serde_json::json!({
            "sources": fresh,
            "pending_reviews": pending,
            "recent_alerts": alerts.iter().map(|a| serde_json::json!({
                "id": a.0, "source": a.1, "kind": a.2, "message": a.3, "at": a.4.to_rfc3339()
            })).collect::<Vec<_>>(),
        }));
    }
    println!(
        "{:<30} {:<10} {:<10} {:<13} {:<17} CONNECTOR",
        "SOURCE", "CADENCE", "STATE", "LAST", "LAST SUCCESS"
    );
    for f in &fresh {
        println!(
            "{:<30} {:<10} {:<10} {:<13} {:<17} {}",
            f.source_id,
            f.cadence,
            f.state,
            f.last_status.as_deref().unwrap_or("-"),
            f.last_success_at.map_or_else(
                || "-".to_owned(),
                |t| t.format("%Y-%m-%d %H:%M").to_string()
            ),
            if IMPLEMENTED.contains(&f.source_id.as_str()) {
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
    if !alerts.is_empty() {
        println!("\nalerts (7 days):");
        for a in &alerts {
            println!(
                "  #{} {} {} {}",
                a.0,
                a.1.as_deref().unwrap_or("-"),
                a.2,
                a.3.lines().next().unwrap_or_default()
            );
        }
    }
    Ok(())
}
