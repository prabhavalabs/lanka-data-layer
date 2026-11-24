//! `lanka docs harvest | status | search` (13 §1): the documents archive.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::{Subcommand, ValueEnum};
use lanka_core::db::PoolRole;
use lanka_ingest::docs::harvest::{self, Mode, Options};
use lanka_ingest::docs::mirror::{self, MirrorOptions};

use crate::{Global, find_registry, pool, print_json, store};

#[derive(Clone, Copy, ValueEnum)]
pub enum ModeArg {
    Incremental,
    Sweep,
    Files,
    Text,
    Gaps,
}

impl From<ModeArg> for Mode {
    fn from(m: ModeArg) -> Self {
        match m {
            ModeArg::Incremental => Self::Incremental,
            ModeArg::Sweep => Self::Sweep,
            ModeArg::Files => Self::Files,
            ModeArg::Text => Self::Text,
            ModeArg::Gaps => Self::Gaps,
        }
    }
}

#[derive(Subcommand)]
pub enum DocsCmd {
    /// List a collection from the publisher's site and archive its files.
    /// `incremental` walks from the newest page until nothing is new;
    /// `sweep` walks every page and withdraws what is no longer listed;
    /// `files` downloads pending and failed files; `text` reads the text of
    /// archived files that have none yet.
    Harvest {
        /// Collection id, e.g. `lk-gazette-extraordinary`.
        collection: String,
        #[arg(long, value_enum, default_value = "incremental")]
        mode: ModeArg,
        /// Stop after N documents (listing) or N files (files, text, tail).
        #[arg(long)]
        limit: Option<usize>,
        /// Ignore documents issued before this date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<NaiveDate>,
        /// Walk at most N listing pages.
        #[arg(long)]
        max_pages: Option<u32>,
        /// After a listing, do not download files (run `--mode files` later).
        #[arg(long)]
        skip_files: bool,
        /// Restrict the files tail to documents issued in this month
        /// (YYYY-MM), so a month can be completed on demand.
        #[arg(long, value_name = "YYYY-MM")]
        month: Option<String>,
    },
    /// Publish the archive to GitHub: one release per complete month (PDF tar,
    /// text tar.gz, manifest) and the metadata, month files and README
    /// statistics committed to the archive repository (13 §1.4). Only months
    /// whose manifest changed are published; a re-run with nothing new does
    /// nothing. Needs an authenticated `gh`.
    Mirror {
        /// Collection id, e.g. `lk-gazette-extraordinary`.
        collection: String,
        /// Local clone of the archive repository.
        #[arg(long)]
        repo_dir: std::path::PathBuf,
        /// Mirror this month (YYYY-MM); repeatable. Default: every complete
        /// month whose manifest changed, oldest first.
        #[arg(long, value_name = "YYYY-MM")]
        month: Vec<String>,
        /// Build and verify the packs in a temporary directory; do not
        /// upload, commit or push.
        #[arg(long)]
        dry_run: bool,
        /// Mirror at most N months in this run.
        #[arg(long)]
        max_months: Option<usize>,
        /// Testing only: mirror a month with files still pending.
        #[arg(long)]
        allow_incomplete: bool,
        /// GitHub repository, `owner/name`.
        #[arg(long, default_value = mirror::DEFAULT_REPO)]
        github_repo: String,
        /// The GitHub CLI binary.
        #[arg(long, env = "LANKA_GH_BIN", default_value = "gh")]
        gh_bin: String,
        /// Directory for the packs (default: the system temp directory).
        #[arg(long)]
        work_dir: Option<std::path::PathBuf>,
    },
    /// Counts by file and text status, the last harvests, withdrawn documents.
    Status {
        /// Collection id (default: every collection).
        collection: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Full-text search over the archived text (a quick check).
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: i64,
    },
}

pub async fn run(g: &Global, cmd: DocsCmd) -> Result<()> {
    match cmd {
        DocsCmd::Harvest {
            collection,
            mode,
            limit,
            since,
            max_pages,
            skip_files,
            month,
        } => {
            let dir = find_registry(g.registry.as_deref())?;
            let st = store(g, &dir);
            let pool = pool(g, PoolRole::Ingest).await?;
            let report = harvest::run(
                &pool,
                &st,
                &collection,
                &Options {
                    mode: Some(mode.into()),
                    limit,
                    since,
                    max_pages,
                    skip_files,
                    month,
                },
            )
            .await
            .with_context(|| format!("docs harvest {collection}"))?;
            print_json(&report)
        }
        DocsCmd::Mirror {
            collection,
            repo_dir,
            month,
            dry_run,
            max_months,
            allow_incomplete,
            github_repo,
            gh_bin,
            work_dir,
        } => {
            let dir = find_registry(g.registry.as_deref())?;
            let st = store(g, &dir);
            let pool = pool(g, PoolRole::Api).await?;
            let mut opts = MirrorOptions::new(repo_dir);
            opts.months = month;
            opts.dry_run = dry_run;
            opts.max_months = max_months;
            opts.allow_incomplete = allow_incomplete;
            opts.github_repo = github_repo;
            opts.gh_bin = gh_bin;
            opts.work_dir = work_dir;
            let report = mirror::run(&pool, &st, &collection, &opts)
                .await
                .with_context(|| format!("docs mirror {collection}"))?;
            println!("{}", report.message);
            print_json(&report)
        }
        DocsCmd::Status { collection, json } => {
            let pool = pool(g, PoolRole::Api).await?;
            let ids = match collection {
                Some(c) => vec![c],
                None => harvest::collections(&pool).await?,
            };
            let mut all = Vec::new();
            for id in &ids {
                all.push(harvest::status(&pool, id).await?);
            }
            if json {
                return print_json(&all);
            }
            for s in &all {
                print_status(s);
            }
            Ok(())
        }
        DocsCmd::Search { query, limit } => {
            let pool = pool(g, PoolRole::Api).await?;
            let hits = harvest::search(&pool, &query, limit).await?;
            if hits.is_empty() {
                println!("no match");
            }
            for h in hits {
                println!(
                    "{:<18} {}  {}  {:.4}  {}",
                    h.doc_id,
                    h.lang,
                    h.issued_on,
                    h.rank,
                    h.title.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
    }
}

fn fmt_counts(v: &[(String, i64)]) -> String {
    if v.is_empty() {
        return "-".to_owned();
    }
    v.iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect::<Vec<_>>()
        .join("  ")
}

fn print_status(s: &harvest::Status) {
    println!("{}", s.collection);
    println!(
        "  documents  {}  (withdrawn {}, issued {} .. {})",
        s.documents,
        s.withdrawn,
        s.oldest.map_or("-".to_owned(), |d| d.to_string()),
        s.newest.map_or("-".to_owned(), |d| d.to_string()),
    );
    println!("  files      {}", fmt_counts(&s.files_by_status));
    println!(
        "  text       {}  (searchable {})",
        fmt_counts(&s.text_by_status),
        s.searchable
    );
    println!("  harvests (newest first)");
    for h in &s.harvests {
        println!(
            "    #{:<4} {:<11} {}  {}  listed {} new {} changed {} withdrawn {} files ok {} failed {}{}",
            h.id,
            h.mode,
            h.started_at.format("%Y-%m-%d %H:%M"),
            h.finished_at
                .map_or("running/aborted".to_owned(), |f| format!(
                    "{}s",
                    (f - h.started_at).num_seconds()
                )),
            h.listed,
            h.new,
            h.changed,
            h.withdrawn,
            h.files_ok,
            h.files_failed,
            h.error
                .as_deref()
                .map(|e| format!("  ERROR {e}"))
                .unwrap_or_default()
        );
    }
}
