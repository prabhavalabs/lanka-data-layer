//! `lanka civic discover | extract | approve | reject | build | show` (13 §2.1):
//! ministries and the institutions in their charge, over time.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::Subcommand;
use lanka_core::civic;
use lanka_core::db::PoolRole;
use lanka_extractd::Sandbox;
use lanka_ingest::civic::build::{self, BuildReport};
use lanka_ingest::civic::discover::{self, Candidate};
use lanka_ingest::civic::entity::Aliases;
use lanka_ingest::civic::exceptions::Exceptions;
use lanka_ingest::civic::extract::{self, ExtractError, Extracted};
use lanka_ingest::civic::suggest;
use lanka_ingest::civic::validate::Check;

use crate::{Global, find_registry, pool, print_json, store};

#[derive(Subcommand)]
pub enum CivicCmd {
    /// List candidate instruments from document titles (subjects and
    /// functions of ministers, in the extraordinary gazettes), confirmed
    /// from the English text where the file is archived.
    Discover {
        #[arg(long)]
        json: bool,
    },
    /// Parse and validate an archived English gazette: a baseline is parsed
    /// into ministers and columns; an amendment is recorded as a timeline
    /// event with the baseline it amends.
    Extract {
        /// Document id, e.g. `gze-2412-08`.
        doc_id: Option<String>,
        /// Every discovered candidate whose English file is archived and
        /// confirmed (instruments already approved or rejected are kept).
        #[arg(long, conflicts_with = "doc_id")]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Approve an instrument that needs review (it is then served and built).
    Approve {
        doc_id: String,
        #[arg(long)]
        by: String,
    },
    /// Reject an instrument (it is neither served nor built).
    Reject {
        doc_id: String,
        #[arg(long)]
        by: String,
    },
    /// Rebuild `civic.assignments` and `civic.entities` from the validated
    /// and approved baselines (idempotent).
    Build {
        #[arg(long)]
        json: bool,
    },
    /// Alias review aids.
    #[command(subcommand)]
    Aliases(AliasCmd),
    /// Print the structure in force on a date.
    Show {
        /// `YYYY-MM-DD` (default: today).
        #[arg(long)]
        at: Option<NaiveDate>,
        /// List the institutions of every ministry.
        #[arg(long, short = 'v')]
        institutions: bool,
    },
}

#[derive(Subcommand)]
pub enum AliasCmd {
    /// Compare every pair of entity keys (trigram similarity of the de-slugged
    /// names) and write the candidates to `registry/civic/alias-suggestions.yaml`
    /// for a person to review. Nothing is applied automatically.
    Suggest {
        /// Smallest similarity to list (0..1).
        #[arg(long, default_value_t = 0.85)]
        min: f64,
    },
}

fn aliases(g: &Global) -> Result<Aliases> {
    let dir = find_registry(g.registry.as_deref())?;
    Aliases::load(&dir.join("civic").join("aliases.yaml"))
}

fn exceptions(g: &Global) -> Result<Exceptions> {
    let dir = find_registry(g.registry.as_deref())?;
    Exceptions::load(&dir.join("civic").join("exceptions.yaml"))
}

fn failed(checks: &[Check]) -> String {
    let f: Vec<&str> = checks
        .iter()
        .filter(|c| !c.ok)
        .map(|c| c.check.as_str())
        .collect();
    if f.is_empty() {
        "-".to_owned()
    } else {
        f.join(",")
    }
}

fn print_extracted(x: &Extracted) {
    println!(
        "{:<12} {:<9} {}  {:<12} failed: {:<28} portfolios {:>2}  institutions {:>3}  functions {:>3}  laws {:>3}{}",
        x.doc_id,
        x.kind,
        x.effective_from,
        x.status,
        failed(&x.checks),
        x.portfolios,
        x.institutions,
        x.functions,
        x.laws,
        x.amends
            .as_deref()
            .map(|a| format!("  amends {a}"))
            .unwrap_or_default()
    );
    for c in x.checks.iter().filter(|c| !c.ok) {
        println!("    FAIL {}: {}", c.check, c.detail);
    }
}

fn print_candidate(c: &Candidate) {
    let (kind, eff, conf) = match &c.confirmation {
        Some(k) => (
            k.kind.unwrap_or("?"),
            k.effective_from.to_string(),
            if k.confirmed {
                "Art.43/44".to_owned()
            } else {
                format!("art {:?}: not Cabinet", k.articles)
            },
        ),
        None => ("-", "-".to_owned(), "(file not archived)".to_owned()),
    };
    println!(
        "{:<12} {}  en {:<8}  {:<9} {:<10}  {:<12} {:<20} {}",
        c.doc_id,
        c.issued_on,
        c.en_status.as_deref().unwrap_or("-"),
        kind,
        eff,
        c.extracted.as_deref().unwrap_or("-"),
        conf,
        c.title.chars().take(70).collect::<String>()
    );
}

pub async fn run(g: &Global, cmd: CivicCmd) -> Result<()> {
    match cmd {
        CivicCmd::Discover { json } => {
            let dir = find_registry(g.registry.as_deref())?;
            let st = store(g, &dir);
            let pool = pool(g, PoolRole::Api).await?;
            let cands = discover::discover(&pool, &st).await?;
            if json {
                return print_json(&cands);
            }
            for c in &cands {
                print_candidate(c);
            }
            let ready = cands
                .iter()
                .filter(|c| c.confirmation.as_ref().is_some_and(|k| k.confirmed))
                .count();
            println!(
                "{} candidates, {ready} confirmed from the text, {} waiting for their English file",
                cands.len(),
                cands.iter().filter(|c| c.confirmation.is_none()).count()
            );
            Ok(())
        }
        CivicCmd::Extract { doc_id, all, json } => {
            let dir = find_registry(g.registry.as_deref())?;
            let st = store(g, &dir);
            let pool = pool(g, PoolRole::Ingest).await?;
            let al = aliases(g)?;
            let ex = exceptions(g)?;
            let sb = Sandbox::from_env()?;
            let ids: Vec<String> = match (doc_id, all) {
                (Some(d), _) => vec![d],
                (None, true) => {
                    let mut c: Vec<Candidate> = discover::discover(&pool, &st)
                        .await?
                        .into_iter()
                        .filter(|c| {
                            c.confirmation
                                .as_ref()
                                .is_some_and(|k| k.confirmed && k.kind.is_some())
                                && !matches!(c.extracted.as_deref(), Some("approved" | "rejected"))
                        })
                        .collect();
                    // baselines first (an amendment needs its baseline), then by date
                    c.sort_by_key(|c| {
                        (
                            c.confirmation.as_ref().and_then(|k| k.kind) != Some("baseline"),
                            c.issued_on,
                            c.doc_id.clone(),
                        )
                    });
                    c.into_iter().map(|c| c.doc_id).collect()
                }
                (None, false) => bail!("give a doc_id or --all"),
            };
            let mut out: Vec<Extracted> = Vec::new();
            let mut errors = 0;
            for id in &ids {
                match extract::extract(&pool, &st, &sb, &al, &ex, id).await {
                    Ok(x) => {
                        if !json {
                            print_extracted(&x);
                        }
                        out.push(x);
                    }
                    Err(e @ (ExtractError::NotReady(_) | ExtractError::NotInstrument(_)))
                        if ids.len() > 1 =>
                    {
                        eprintln!("skip {id}: {e}");
                    }
                    Err(e) => {
                        eprintln!("error {id}: {e}");
                        errors += 1;
                    }
                }
            }
            if json {
                print_json(&out)?;
            } else {
                let by = |s: &str| out.iter().filter(|x| x.status == s).count();
                println!(
                    "{} extracted: {} validated, {} needs_review",
                    out.len(),
                    by("validated"),
                    by("needs_review")
                );
            }
            if errors > 0 {
                bail!("{errors} instrument(s) failed");
            }
            Ok(())
        }
        CivicCmd::Approve { doc_id, by } => review(g, &doc_id, true, &by).await,
        CivicCmd::Reject { doc_id, by } => review(g, &doc_id, false, &by).await,
        CivicCmd::Build { json } => {
            let pool = pool(g, PoolRole::Ingest).await?;
            let al = aliases(g)?;
            let r: BuildReport = build::build(&pool, &al).await?;
            if json {
                return print_json(&r);
            }
            println!(
                "{} baselines and supplements make {} structures -> {} entities, {} assignments ({} in force); {} institutions listed under two ministers in one structure (first listing kept); {} aliases",
                r.instruments,
                r.steps,
                r.entities,
                r.assignments,
                r.open,
                r.shared_listings,
                r.aliases_loaded
            );
            Ok(())
        }
        CivicCmd::Aliases(AliasCmd::Suggest { min }) => {
            let dir = find_registry(g.registry.as_deref())?;
            let pool = pool(g, PoolRole::Api).await?;
            let al = aliases(g)?;
            let found = suggest::suggest(&pool, &al, min).await?;
            let path = dir.join("civic").join("alias-suggestions.yaml");
            std::fs::write(&path, suggest::render(&found, min))
                .with_context(|| format!("writing {}", path.display()))?;
            println!(
                "{} candidate pair(s) at similarity >= {min} written to {}",
                found.len(),
                path.display()
            );
            Ok(())
        }
        CivicCmd::Show { at, institutions } => {
            let pool = pool(g, PoolRole::Api).await?;
            let at = at.unwrap_or_else(|| chrono::Utc::now().date_naive());
            let s = civic::structure(&pool, at)
                .await
                .with_context(|| format!("structure on {at}"))?;
            println!(
                "structure on {at}: gazette {} ({}), in force from {}",
                s.instrument
                    .number
                    .as_deref()
                    .unwrap_or(&s.instrument.doc_id),
                s.instrument.doc_id,
                s.instrument.effective_from
            );
            for a in &s.amendments_since {
                println!(
                    "  amended since: {} from {}  {}",
                    a.doc_id,
                    a.effective_from,
                    a.title.as_deref().unwrap_or("")
                );
            }
            for p in &s.portfolios {
                println!(
                    "{:>2}. {:<70}  p{:<3} functions {:>2}  institutions {:>2}  laws {:>2}",
                    p.seq,
                    p.title,
                    p.page.unwrap_or(0),
                    p.functions.len(),
                    p.institutions.len(),
                    p.laws.len()
                );
                if institutions {
                    for i in &p.institutions {
                        println!(
                            "       {:>2}. {}  [{}]",
                            i.item_no,
                            i.text,
                            i.kind.as_deref().unwrap_or("?")
                        );
                    }
                }
            }
            let (f, i, l) = s.portfolios.iter().fold((0, 0, 0), |a, p| {
                (
                    a.0 + p.functions.len(),
                    a.1 + p.institutions.len(),
                    a.2 + p.laws.len(),
                )
            });
            println!(
                "{} ministers, {f} functions, {i} institutions, {l} laws",
                s.portfolios.len()
            );
            Ok(())
        }
    }
}

async fn review(g: &Global, doc_id: &str, approve: bool, by: &str) -> Result<()> {
    let pool = pool(g, PoolRole::Ingest).await?;
    extract::review(&pool, doc_id, approve, by).await?;
    println!(
        "{doc_id} {} by {by}; run `lanka civic build` to update the assignments",
        if approve { "approved" } else { "rejected" }
    );
    Ok(())
}
