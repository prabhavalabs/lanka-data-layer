//! `lanka-extractd` — the document sandbox daemon.
//!
//! ```text
//! lanka-extractd serve --socket /run/lanka/extractd.sock [--concurrency 2]
//! lanka-extractd versions
//! lanka-extractd run '<op json>' <file>      # one op locally, output to stdout (debugging)
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lanka_extractd::{Limits, Op, Tools, ops, server};

#[derive(Parser)]
#[command(name = "lanka-extractd", version, about = "Sandboxed document tools for the Lanka Data Layer")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Listen on a Unix socket (inside the `tools` container).
    Serve {
        #[arg(long, env = "LANKA_EXTRACTD_SOCKET", default_value = "/run/lanka/extractd.sock")]
        socket: PathBuf,
        #[arg(long, env = "LANKA_EXTRACTD_CONCURRENCY", default_value_t = 2)]
        concurrency: usize,
    },
    /// Print the tool versions.
    Versions,
    /// Run one operation locally and write its output to stdout.
    Run { op: String, file: PathBuf },
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let tools = Tools::discover();
    let limits = Limits::from_env();
    match cli.cmd {
        Cmd::Serve {
            socket,
            concurrency,
        } => server::serve(&socket, tools, limits, concurrency).await,
        Cmd::Versions => {
            let v = ops::run(&Op::Versions, &[], &limits, &tools).await?;
            println!("{}", String::from_utf8_lossy(&v));
            Ok(())
        }
        Cmd::Run { op, file } => {
            let op: Op = serde_json::from_str(&op).context("op JSON")?;
            let input = std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
            let out = ops::run(&op, &input, &limits, &tools).await?;
            std::io::stdout().write_all(&out)?;
            Ok(())
        }
    }
}
