//! Document tables (WP-08): `lanka template test | propose` and
//! `lanka extract propose` (class C, operator-run).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use lanka_extractd::Sandbox;
use lanka_ingest::template::{self, Value, failed_summary};

use crate::{Global, find_registry};

#[derive(Subcommand)]
pub enum TemplateCmd {
    /// Read a PDF through a template and print the table and its checks.
    Test {
        /// Template id (`registry/templates/<id>.yaml`).
        id: String,
        pdf: PathBuf,
        #[arg(long)]
        edition: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Propose a class-B template for a table in a PDF (reviewed as a
    /// registry PR; from then on extraction is deterministic).
    Propose(crate::propose::TemplateProposeArgs),
}

pub async fn template(g: &Global, cmd: TemplateCmd) -> Result<()> {
    match cmd {
        TemplateCmd::Test {
            id,
            pdf,
            edition,
            json,
        } => {
            let root = find_registry(g.registry.as_deref())?;
            let (tpl, _) = template::load(&root, &id)?;
            let bytes =
                std::fs::read(&pdf).with_context(|| format!("reading {}", pdf.display()))?;
            let sandbox = Sandbox::from_env()?;
            let words = sandbox.pdf_words(&bytes, 0, 0).await?;
            let table = tpl.read(&words, edition.as_deref())?;
            let (_, ed) = tpl.edition(edition.as_deref())?;
            let checks = table.check(ed);
            if json {
                let rows: Vec<serde_json::Value> = table
                    .rows
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "label": r.label,
                            "cells": r.cells.iter().map(|(k, c)| (k.clone(), serde_json::json!({
                                "text": c.text, "page": c.page, "bbox": c.bbox,
                            }))).collect::<serde_json::Map<_, _>>(),
                        })
                    })
                    .collect();
                crate::print_json(&serde_json::json!({
                    "template": tpl.id, "page": table.first_page,
                    "headers": table.headers.iter().map(|h| serde_json::json!({"key": h.key, "text": h.text, "bbox": h.bbox})).collect::<Vec<_>>(),
                    "rows": rows, "checks": checks,
                }))?;
            } else {
                println!("{} — page {}", tpl.id, table.first_page);
                for h in &table.headers {
                    println!("  column {:<14} {:?}", h.key, h.text);
                }
                for r in &table.rows {
                    let vals: Vec<String> = table
                        .headers
                        .iter()
                        .skip(1)
                        .map(|h| match r.cells.get(&h.key) {
                            Some(c) if matches!(c.value, Value::Number(_)) => c.text.clone(),
                            Some(c) => format!("«{}»", c.text),
                            None => "·".to_owned(),
                        })
                        .collect();
                    println!("  {:<32} {}", r.label, vals.join(" | "));
                }
                for c in &checks {
                    println!(
                        "  check {} {} ({} instance(s)){}",
                        if c.ok { "PASS" } else { "FAIL" },
                        c.check,
                        c.n,
                        if c.ok {
                            String::new()
                        } else {
                            format!(": {}", c.failures.join("; "))
                        }
                    );
                }
            }
            if let Some(s) = failed_summary(&checks) {
                anyhow::bail!("template checks failed: {s}");
            }
            Ok(())
        }
        TemplateCmd::Propose(args) => crate::propose::template_propose(g, args).await,
    }
}
