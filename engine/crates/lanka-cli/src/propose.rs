//! Operator-run proposal tools (03 §3.3). Never scheduled; the worker has
//! no model credentials.
//!
//! ```text
//! lanka extract propose <source> (--artifact <sha> | --file F --url U) --table <hint> [--page N] [--ocr]
//!                       [--readings a.json --readings b.json] [--max-usd 5]
//! lanka template propose <pdf> --id <source>/<table> --find-text <anchor> [--page N]
//! ```
//!
//! `extract propose` gets two independent readings — from the configured
//! model endpoint (`LANKA_LLM_ENDPOINT`, `LANKA_LLM_API_KEY`,
//! `LANKA_LLM_MODEL`, `LANKA_LLM_PROVIDER=openai|anthropic`) or, for dry
//! runs and audits, from recorded reading files (`--readings`) — anchors
//! them to the sandbox's words, writes a class-C `cells` extract and opens
//! its review with an evidence sheet.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use clap::Args;
use lanka_core::db::PoolRole;
use lanka_extractd::{Sandbox, Words};
use lanka_ingest::Registry;
use lanka_ingest::classc::{self, Reading};
use lanka_ingest::extract::ExtractArtifact;
use lanka_ingest::review::{self, ExtractMeta, Target};
use lanka_ingest::template::{self, Template, lines, norm};
use lanka_store::{BlobStore, Ns};
use sha2::{Digest, Sha256};

use crate::{Global, find_registry, pool, print_json, store};

#[derive(Args)]
pub struct ExtractProposeArgs {
    pub source: String,
    /// sha256 (or a prefix of ≥ 8 hex digits) of an archived artifact of the source.
    #[arg(long, conflicts_with = "file")]
    pub artifact: Option<String>,
    /// A local PDF to archive first (with its canonical `--url`).
    #[arg(long, requires = "url")]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub url: Option<String>,
    #[arg(long)]
    pub edition: Option<String>,
    /// What the table is ("Table 4.1a labour force participation"); also used to find the page.
    #[arg(long)]
    pub table: String,
    /// Table id in the extract (default: from the hint).
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub page: Option<u32>,
    /// OCR the page even if it has a text layer.
    #[arg(long)]
    pub ocr: bool,
    #[arg(long, default_value = "eng")]
    pub langs: String,
    /// Recorded readings (exactly two) instead of calling a model.
    #[arg(long)]
    pub readings: Vec<PathBuf>,
    /// Spend cap for the two model calls (USD).
    #[arg(long, default_value_t = 5.0)]
    pub max_usd: f64,
}

#[derive(Args)]
pub struct TemplateProposeArgs {
    pub pdf: PathBuf,
    /// Template id (`<source>/<table>`).
    #[arg(long)]
    pub id: String,
    /// Anchor text that identifies the table's page ("Table 4.1:").
    #[arg(long)]
    pub find_text: String,
    #[arg(long)]
    pub page: Option<u32>,
    #[arg(long)]
    pub edition: Option<String>,
}

fn table_id(hint: &str) -> String {
    let mut s: String = hint
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').chars().take(40).collect::<String>();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("t{s}")
    } else {
        s
    }
}

/// An archived artifact: (id, url, sha, path to the bytes).
async fn artifact(
    pool: &sqlx::PgPool,
    st: &lanka_store::FsStore,
    source: &str,
    a: &ExtractProposeArgs,
) -> Result<(i64, String, [u8; 32], Vec<u8>)> {
    if let Some(f) = &a.file {
        let bytes = std::fs::read(f).with_context(|| format!("reading {}", f.display()))?;
        let (blob, _) = st.put_cas(Ns::Raw, Bytes::from(bytes.clone())).await?;
        let url = a.url.clone().unwrap_or_default();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, media_type, edition, archive_path)
             VALUES ($1, $2, $3, now(), $4, 'application/pdf', $5, $6)
             ON CONFLICT (sha256) DO UPDATE SET edition = coalesce(ops.artifacts.edition, EXCLUDED.edition)
             RETURNING id",
        )
        .bind(blob.sha256.as_slice())
        .bind(source)
        .bind(&url)
        .bind(i64::try_from(blob.bytes)?)
        .bind(&a.edition)
        .bind(blob.rel_path())
        .fetch_one(pool)
        .await?;
        return Ok((id, url, blob.sha256, bytes));
    }
    let prefix = a
        .artifact
        .as_deref()
        .context("give --artifact <sha> or --file F --url U")?;
    ensure!(
        prefix.len() >= 8 && prefix.chars().all(|c| c.is_ascii_hexdigit()),
        "--artifact: at least 8 hex digits"
    );
    let rows: Vec<(i64, String, Vec<u8>, String)> = sqlx::query_as(
        "SELECT id, url, sha256, archive_path FROM ops.artifacts
         WHERE source_id = $1 AND encode(sha256, 'hex') LIKE $2 || '%'",
    )
    .bind(source)
    .bind(prefix.to_ascii_lowercase())
    .fetch_all(pool)
    .await?;
    ensure!(
        rows.len() == 1,
        "{} artifacts of {source} match {prefix}",
        rows.len()
    );
    let (id, url, sha, path) = rows.into_iter().next().context("artifact")?;
    let sha: [u8; 32] = sha.as_slice().try_into().context("sha")?;
    let key = path.strip_prefix("raw/").unwrap_or(&path).to_owned();
    let bytes = st
        .get(&lanka_store::BlobRef {
            ns: Ns::Raw,
            key,
            sha256: sha,
            bytes: 0,
        })
        .await?
        .to_vec();
    Ok((id, url, sha, bytes))
}

/// Words of the table's page: text layer, or OCR when there is none.
async fn page_words(
    sandbox: &Sandbox,
    pdf: &[u8],
    a: &ExtractProposeArgs,
) -> Result<(u32, Words, &'static str)> {
    let text = sandbox.pdf_words(pdf, 0, 0).await?;
    let page = match a.page {
        Some(p) => p,
        None => {
            let want = norm(&a.table);
            text.pages
                .iter()
                .map(|p| p.page)
                .find(|p| {
                    let t: String = text.on_page(*p).map(|w| norm(&w.text)).collect();
                    !want.is_empty() && t.contains(&want)
                })
                .unwrap_or(1)
        }
    };
    let has_text = text.on_page(page).count() > 10;
    if has_text && !a.ocr {
        let words = Words {
            pages: text
                .pages
                .iter()
                .filter(|p| p.page == page)
                .copied()
                .collect(),
            words: text.words.into_iter().filter(|w| w.page == page).collect(),
        };
        return Ok((page, words, "text layer"));
    }
    let langs: Vec<&str> = a.langs.split('+').collect();
    Ok((page, sandbox.ocr(pdf, page, 300, &langs).await?, "ocr"))
}

/// Pluggable model endpoint (operator configuration only).
struct Llm {
    provider: String,
    endpoint: String,
    key: String,
    model: String,
}

impl Llm {
    fn from_env() -> Result<Self> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Ok(Self {
            provider: get("LANKA_LLM_PROVIDER").unwrap_or_else(|| "openai".into()),
            endpoint: get("LANKA_LLM_ENDPOINT")
                .context("LANKA_LLM_ENDPOINT is not set (or pass --readings for a dry run)")?,
            key: get("LANKA_LLM_API_KEY").unwrap_or_default(),
            model: get("LANKA_LLM_MODEL").context("LANKA_LLM_MODEL is not set")?,
        })
    }

    /// One call; (text, input tokens, output tokens).
    async fn call(&self, prompt: &str) -> Result<(String, u64, u64)> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        let (req, anthropic) = match self.provider.as_str() {
            "anthropic" => (
                http.post(format!(
                    "{}/v1/messages",
                    self.endpoint.trim_end_matches('/')
                ))
                .header("x-api-key", &self.key)
                .header("anthropic-version", "2023-06-01")
                .json(&serde_json::json!({
                    "model": self.model, "max_tokens": 8000, "temperature": 0,
                    "messages": [{ "role": "user", "content": prompt }],
                })),
                true,
            ),
            "openai" => (
                http.post(format!(
                    "{}/v1/chat/completions",
                    self.endpoint.trim_end_matches('/')
                ))
                .bearer_auth(&self.key)
                .json(&serde_json::json!({
                    "model": self.model, "max_tokens": 8000, "temperature": 0,
                    "response_format": { "type": "json_object" },
                    "messages": [{ "role": "user", "content": prompt }],
                })),
                false,
            ),
            p => bail!("LANKA_LLM_PROVIDER {p:?}: openai or anthropic"),
        };
        let v: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
        let (text, i, o) = if anthropic {
            (
                v["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                v["usage"]["input_tokens"].as_u64().unwrap_or(0),
                v["usage"]["output_tokens"].as_u64().unwrap_or(0),
            )
        } else {
            (
                v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            )
        };
        Ok((text, i, o))
    }
}

fn price(k: &str, default: f64) -> f64 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub async fn extract_propose(g: &Global, a: ExtractProposeArgs) -> Result<()> {
    let root = find_registry(g.registry.as_deref())?;
    let registry = Registry::load(&root)?;
    ensure!(
        registry.source(&a.source).is_some(),
        "unknown source {:?}",
        a.source
    );
    let pool = pool(g, PoolRole::Ingest).await?;
    let st = store(g, &root);
    let sandbox = Sandbox::from_env()?;
    let (artifact_id, url, sha, pdf) = artifact(&pool, &st, &a.source, &a).await?;
    let (page, words, via) = page_words(&sandbox, &pdf, &a).await?;
    let text = classc::page_text(&words, page);
    let prompts = [
        classc::prompt(0, &a.table, &text),
        classc::prompt(1, &a.table, &text),
    ];
    let mut ph = Sha256::new();
    for p in &prompts {
        ph.update(p.as_bytes());
    }
    let prompt_sha: [u8; 32] = ph.finalize().into();

    let (readings, model, spent): (Vec<Reading>, String, f64) = if a.readings.is_empty() {
        let llm = Llm::from_env()?;
        let (pin, pout) = (
            price("LANKA_LLM_USD_PER_MTOK_IN", 5.0),
            price("LANKA_LLM_USD_PER_MTOK_OUT", 25.0),
        );
        #[allow(clippy::cast_precision_loss)]
        let estimate: f64 = prompts
            .iter()
            .map(|p| (p.len() as f64 / 3.5) * pin / 1e6 + 8000.0 * pout / 1e6)
            .sum();
        ensure!(
            estimate <= a.max_usd,
            "estimated cost ${estimate:.2} exceeds --max-usd {:.2}",
            a.max_usd
        );
        let mut rs = Vec::new();
        let mut spent = 0.0;
        for p in &prompts {
            let (t, i, o) = llm.call(p).await?;
            #[allow(clippy::cast_precision_loss)]
            {
                spent += i as f64 * pin / 1e6 + o as f64 * pout / 1e6;
            }
            rs.push(Reading::parse(&t)?);
        }
        (rs, format!("{}:{}", llm.provider, llm.model), spent)
    } else {
        ensure!(a.readings.len() == 2, "--readings takes exactly two files");
        let mut rs = Vec::new();
        for f in &a.readings {
            rs.push(Reading::parse(
                &std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?,
            )?);
        }
        (rs, "replay:recorded-readings".to_owned(), 0.0)
    };
    let tid = a.id.clone().unwrap_or_else(|| table_id(&a.table));
    let combined = classc::combine(&tid, &readings[0], &readings[1], &words, 0)?;
    ensure!(
        combined.cells.iter().any(|c| c.r > 0 && c.c > 0),
        "no proposed cell could be anchored to the page ({} dropped)",
        combined.dropped.len()
    );
    let extract_id = review::persist_cells(
        &pool,
        &st,
        &a.source,
        classc::READER,
        env!("CARGO_PKG_VERSION"),
        &ExtractMeta {
            template_sha256: None,
            model: Some(&model),
            prompt_sha256: Some(prompt_sha),
            parent_id: None,
        },
        combined.cells.clone(),
        &[ExtractArtifact {
            artifact_id,
            key: "document".into(),
            url: url.clone(),
            sha256: sha,
        }],
    )
    .await?;
    let target = Target::Extract(extract_id);
    review::open_review(
        &pool,
        &target,
        &format!("class C reading ({via}, {model}) needs per-cell sign-off"),
    )
    .await?;
    let evidence = lanka_export::evidence::build(&pool, &st, &registry, &sandbox, &target).await?;
    let state = review::signoff_state(&pool, &st, extract_id).await?;
    print_json(&serde_json::json!({
        "extract": format!("x{extract_id}"),
        "table": tid, "page": page, "words_from": via, "model": model, "spent_usd": spent,
        "readings": readings.iter().map(|r| serde_json::json!({"cells": r.cells.len(), "discarded": r.discarded})).collect::<Vec<_>>(),
        "anchoring": combined,
        "signoff_required": state.required.len(),
        "evidence": evidence.index,
        "next": format!("lanka review open x{extract_id}  (sign each required cell), then lanka review approve x{extract_id} --by NAME"),
    }))
}

/// Heuristic class-B template proposal: data lines are lines with ≥ 3
/// numbers; columns are clusters of their right edges; headers are the
/// words above each column. The proposal is printed as YAML and read back
/// through the template engine; it is reviewed as a registry PR.
pub async fn template_propose(g: &Global, a: TemplateProposeArgs) -> Result<()> {
    let _ = find_registry(g.registry.as_deref());
    let pdf = std::fs::read(&a.pdf).with_context(|| format!("reading {}", a.pdf.display()))?;
    let sandbox = Sandbox::from_env()?;
    let words = sandbox.pdf_words(&pdf, 0, 0).await?;
    let want: Vec<String> = a.find_text.split_whitespace().map(norm).collect();
    let page = match a.page {
        Some(p) => p,
        None => words
            .pages
            .iter()
            .map(|p| p.page)
            .find(|p| {
                let ws: Vec<&lanka_extractd::Word> = words.on_page(*p).collect();
                lines(&ws).iter().any(|l| {
                    let t: Vec<String> = l.words.iter().map(|w| norm(&w.text)).collect();
                    t.windows(want.len()).any(|w| w == want.as_slice())
                })
            })
            .context("--find-text not found on any page")?,
    };
    let ws: Vec<&lanka_extractd::Word> = words.on_page(page).collect();
    let ls = lines(&ws);
    let numspec = template::NumberSpec::default();
    let is_num = |t: &str| matches!(numspec.parse(t), template::Value::Number(_));
    let data: Vec<usize> = (0..ls.len())
        .filter(|i| ls[*i].words.iter().filter(|w| is_num(&w.text)).count() >= 3)
        .collect();
    let first = *data
        .first()
        .context("no line with 3+ numbers on the page")?;
    // Column anchors: right edges of numbers (skipping each line's first
    // token, usually the row label), clustered within 6 pt.
    let mut edges: Vec<f32> = data
        .iter()
        .flat_map(|i| {
            ls[*i]
                .words
                .iter()
                .skip(1)
                .filter(|w| is_num(&w.text))
                .map(|w| w.bbox[2])
        })
        .collect();
    edges.sort_by(f32::total_cmp);
    let mut cols: Vec<Vec<f32>> = Vec::new();
    for e in edges {
        match cols.last_mut() {
            Some(c) if e - c[c.len() - 1] <= 6.0 => c.push(e),
            _ => cols.push(vec![e]),
        }
    }
    cols.retain(|c| c.len() * 2 >= data.len());
    ensure!(!cols.is_empty(), "no stable numeric columns");
    // Header words: non-numeric words above the first data line, nearest to each column.
    let above: Vec<&lanka_extractd::Word> = ls[..first]
        .iter()
        .rev()
        .take(6)
        .flat_map(|l| l.words.iter().copied())
        .filter(|w| !is_num(&w.text))
        .collect();
    let mut yaml = format!(
        "# Proposed by `lanka template propose` from {} page {page}: review every column before committing.\nid: {}\neditions:\n  - match: {{ edition: \"{}\" }}\n    locate:\n      page: {{ find_text: {:?} }}\n    columns:\n",
        a.pdf
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        a.id,
        a.edition.as_deref().unwrap_or("2024..2030"),
        a.find_text
    );
    for (k, c) in cols.iter().enumerate() {
        let x = c[c.len() / 2];
        let h = above
            .iter()
            .min_by(|p, q| (p.bbox[2] - x).abs().total_cmp(&(q.bbox[2] - x).abs()))
            .map_or_else(|| format!("COLUMN {}", k + 1), |w| w.text.clone());
        yaml.push_str(&format!(
            "      - {{ key: col{}, header: {:?} }}\n",
            k + 1,
            h
        ));
    }
    yaml.push_str("    checks:\n      - { row_count: { min: 1 } }\n");
    println!("{yaml}");
    match Template::from_yaml(&yaml)
        .and_then(|t| t.read(&words, a.edition.as_deref().or(Some("2024"))))
    {
        Ok(t) => eprintln!(
            "read back: {} row(s) × {} column(s) on page {}",
            t.rows.len(),
            t.headers.len() - 1,
            t.first_page
        ),
        Err(e) => eprintln!(
            "warning: the proposal does not read back yet ({e:#}): adjust headers/under/nth by hand"
        ),
    }
    Ok(())
}
