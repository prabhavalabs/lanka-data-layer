//! `lanka tiles build | list` (WP-04): vector tilesets from the current
//! release (05 §9). Builds are skipped when their inputs are unchanged.

use anyhow::Result;
use clap::Subcommand;
use lanka_core::db::PoolRole;
use lanka_ingest::Registry;

use crate::{Global, find_registry, load_profile, pool, print_json, store};

#[derive(Subcommand)]
pub enum TilesCmd {
    /// Build (or skip when unchanged) the admin tileset and every features
    /// dataset's tileset declared with `tiles:` in the registry.
    Build {
        /// Only these tilesets (repeatable).
        #[arg(long = "tileset")]
        tilesets: Vec<String>,
        /// Rebuild even when the inputs are unchanged.
        #[arg(long)]
        force: bool,
    },
    /// The newest build of every tileset.
    List,
}

pub async fn run(g: &Global, cmd: TilesCmd) -> Result<()> {
    match cmd {
        TilesCmd::Build { tilesets, force } => {
            let dir = find_registry(g.registry.as_deref())?;
            let reg = Registry::load(&dir)?;
            let profile = load_profile(g, &dir)?;
            let pool = pool(g, PoolRole::Ingest).await?;
            let st = store(g, &dir);
            let built =
                lanka_export::tiles::build_all(&pool, &st, &reg, &profile, &tilesets, force)
                    .await?;
            print_json(&built)
        }
        TilesCmd::List => {
            let pool = pool(g, PoolRole::Api).await?;
            print_json(&lanka_core::features::tilesets(&pool).await?)
        }
    }
}
