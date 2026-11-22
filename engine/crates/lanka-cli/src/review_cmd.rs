//! `lanka review list | approve | reject | open | sign | evidence` (03 §6).
//!
//! `list`, `approve <dsv>` and `reject <dsv>` behave as before WP-08. A
//! subject may now also be a class-C extract (`x<id>`): approving it needs
//! every required per-cell decision. `open` builds the evidence sheet and
//! serves it on a loopback address with per-cell sign-off forms.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use axum::extract::{Form, Path as AxPath, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use clap::Subcommand;
use lanka_core::db::PoolRole;
use lanka_extractd::Sandbox;
use lanka_ingest::review::{self, Decision, Target};
use lanka_ingest::{Registry, publish};
use lanka_store::FsStore;
use serde::Deserialize;
use sqlx::PgPool;

use crate::{Global, find_registry, pool, print_json, store};

#[derive(Subcommand)]
pub enum ReviewCmd {
    /// Dataset versions waiting for sign-off.
    List,
    /// Approve a version (released by the next train) or a class-C
    /// extract (`x<id>`, once every required cell is signed).
    Approve {
        /// `dsv_…` or `x<extract id>`.
        dsv: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// Reject a version (it stays archived, never published) or an extract.
    Reject {
        dsv: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: String,
    },
    /// Build the evidence sheet and serve it on 127.0.0.1 (per-cell sign-off for class C).
    Open {
        target: String,
        #[arg(long, default_value = "127.0.0.1:8790")]
        bind: SocketAddr,
        /// Write the sheet and print its path; do not serve.
        #[arg(long)]
        no_serve: bool,
    },
    /// Build (or rebuild) the evidence sheet; prints where it was written.
    Evidence { target: String },
    /// Record a decision on one extract cell (class C sign-off, class B spot check).
    Sign {
        target: String,
        /// Row index of the cell in the extract (the `#` column of the sheet).
        #[arg(long)]
        row: u32,
        /// accept | correct | reject
        #[arg(long)]
        decision: String,
        /// The text as printed (with `correct`).
        #[arg(long)]
        text: Option<String>,
        /// Extract the row belongs to (defaults to the target's extract).
        #[arg(long)]
        extract: Option<i64>,
        #[arg(long)]
        by: String,
    },
    /// Per-cell sign-off state of a class-C extract.
    Status { target: String },
}

fn extract_of(t: &Target, explicit: Option<i64>) -> Result<i64> {
    match (t, explicit) {
        (_, Some(e)) => Ok(e),
        (Target::Extract(e), None) => Ok(*e),
        (Target::Version(_), None) => anyhow::bail!("--extract is required for a version"),
    }
}

pub async fn run(g: &Global, cmd: ReviewCmd) -> Result<()> {
    let pool = pool(g, PoolRole::Ingest).await?;
    match cmd {
        ReviewCmd::List => print_json(&publish::pending_reviews(&pool).await?),
        ReviewCmd::Approve { dsv, by, note } => match Target::parse(&dsv)? {
            Target::Extract(id) => {
                let st = store(g, &find_registry(g.registry.as_deref())?);
                print_json(&review::approve_extract(&pool, &st, id, &by, note.as_deref()).await?)
            }
            Target::Version(dsv) => {
                review::check_reviewer(&by)?;
                print_json(&publish::approve(&pool, &dsv, &by, note.as_deref()).await?)
            }
        },
        ReviewCmd::Reject { dsv, by, note } => {
            match Target::parse(&dsv)? {
                Target::Extract(id) => review::reject_extract(&pool, id, &by, &note).await?,
                Target::Version(dsv) => {
                    review::check_reviewer(&by)?;
                    publish::reject(&pool, &dsv, &by, &note).await?;
                }
            }
            println!("{dsv} rejected");
            Ok(())
        }
        ReviewCmd::Evidence { target } => {
            let ctx = Ctx::new(g, pool, &target)?;
            print_json(&ctx.build().await?)
        }
        ReviewCmd::Open {
            target,
            bind,
            no_serve,
        } => {
            let ctx = Ctx::new(g, pool, &target)?;
            let built = ctx.build().await?;
            print_json(&built)?;
            if no_serve {
                return Ok(());
            }
            serve(Arc::new(ctx), bind).await
        }
        ReviewCmd::Sign {
            target,
            row,
            decision,
            text,
            extract,
            by,
        } => {
            let t = Target::parse(&target)?;
            let e = extract_of(&t, extract)?;
            review::sign(
                &pool,
                &t,
                e,
                row,
                Decision::parse(&decision)?,
                text.as_deref(),
                &by,
            )
            .await?;
            if let Target::Extract(id) = t {
                let st = store(g, &find_registry(g.registry.as_deref())?);
                print_json(&review::signoff_state(&pool, &st, id).await?)?;
            } else {
                println!("recorded {decision} on extract {e} row {row}");
            }
            Ok(())
        }
        ReviewCmd::Status { target } => {
            let t = Target::parse(&target)?;
            let st = store(g, &find_registry(g.registry.as_deref())?);
            print_json(&review::signoff_state(&pool, &st, extract_of(&t, None)?).await?)
        }
    }
}

/// Everything the evidence server needs.
struct Ctx {
    pool: PgPool,
    store: FsStore,
    registry: Registry,
    sandbox: Sandbox,
    target: Target,
    dir: PathBuf,
}

impl Ctx {
    fn new(g: &Global, pool: PgPool, target: &str) -> Result<Self> {
        let root = find_registry(g.registry.as_deref())?;
        let registry = Registry::load(&root)?;
        let store = store(g, &root);
        let target = Target::parse(target)?;
        let dir = store.root().join("evidence").join(target.subject());
        Ok(Self {
            pool,
            store,
            registry,
            sandbox: Sandbox::from_env()?,
            target,
            dir,
        })
    }

    async fn build(&self) -> Result<lanka_export::evidence::Built> {
        lanka_export::evidence::build(
            &self.pool,
            &self.store,
            &self.registry,
            &self.sandbox,
            &self.target,
        )
        .await
    }
}

/// Builds evidence sheets for versions a run left in review (best effort:
/// a failure here never fails the run).
pub async fn after_run(g: &Global, pool: &PgPool, dsvs: &[String]) {
    for dsv in dsvs {
        let r = async {
            let ctx = Ctx::new(g, pool.clone(), dsv)?;
            ctx.build().await
        }
        .await;
        match r {
            Ok(b) => eprintln!(
                "evidence sheet: {} (lanka review open {dsv})",
                b.index.display()
            ),
            Err(e) => eprintln!("warning: evidence sheet for {dsv} not built: {e:#}"),
        }
    }
}

async fn serve(ctx: Arc<Ctx>, bind: SocketAddr) -> Result<()> {
    ensure!(
        bind.ip().is_loopback(),
        "the evidence server binds loopback addresses only (got {bind})"
    );
    let app = axum::Router::new()
        .route("/", get(index))
        .route("/sign", post(sign))
        .route("/{file}", get(file))
        .with_state(ctx.clone());
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    eprintln!(
        "evidence for {} at http://{bind}/ (Ctrl-C to stop)",
        ctx.target
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn index(State(ctx): State<Arc<Ctx>>) -> Response {
    match tokio::fs::read(ctx.dir.join("index.html")).await {
        Ok(b) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], b).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

async fn file(State(ctx): State<Arc<Ctx>>, AxPath(name): AxPath<String>) -> Response {
    let safe = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !safe {
        return StatusCode::NOT_FOUND.into_response();
    }
    let ct = if name.ends_with(".png") {
        "image/png"
    } else if name.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(ctx.dir.join(&name)).await {
        Ok(b) => ([(header::CONTENT_TYPE, ct)], b).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct SignForm {
    extract: i64,
    row: u32,
    decision: String,
    #[serde(default)]
    text: String,
    by: String,
}

async fn sign(State(ctx): State<Arc<Ctx>>, Form(f): Form<SignForm>) -> Response {
    let r = async {
        let text = Some(f.text.trim()).filter(|t| !t.is_empty());
        review::sign(
            &ctx.pool,
            &ctx.target,
            f.extract,
            f.row,
            Decision::parse(&f.decision)?,
            text,
            &f.by,
        )
        .await?;
        ctx.build().await
    }
    .await;
    match r {
        Ok(_) => Redirect::to(&format!("/#c{}-{}", f.extract, f.row)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}
