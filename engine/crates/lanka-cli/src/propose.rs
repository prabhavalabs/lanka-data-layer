//! Operator-run LLM-assisted tools (03 §3.3): placeholder, filled below.

use anyhow::Result;
use clap::Args;

use crate::Global;

#[derive(Args)]
pub struct TemplateProposeArgs {
    pub pdf: std::path::PathBuf,
}

pub async fn template_propose(_g: &Global, _a: TemplateProposeArgs) -> Result<()> {
    anyhow::bail!("not implemented yet")
}
