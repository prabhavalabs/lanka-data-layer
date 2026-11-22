//! `lanka grid …` (WP-05): lookups, layer listing, v1 parity and the
//! reverse-geocoding grid-vs-polygon check.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use futures::StreamExt;
use lanka_core::{Catalog, Profile, query};
use lanka_grid::array::{GridArray, GridData};
use lanka_grid::lookup::{self, Cells};
use lanka_grid::polygon::MultiPolygon;
use lanka_ingest::Env;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

#[derive(Subcommand)]
pub enum GridCmd {
    /// Build `grid.gnd_idx` + `grid.land_mask` from the reference boundary set.
    Lookups,
    /// List the grid layers (`geo.grid_layers`).
    List,
    /// Compare the WorldPop 2025 layer and `gnd_idx` with the v1 `cells` /
    /// `cell_lookup` tables exported as CSV (`cells.csv`: cell_id,pop;
    /// `cell_lookup.csv`: cell_id,gnd_pcode).
    Parity {
        #[arg(long)]
        v1_dir: PathBuf,
        /// Also rebuild `gnd_idx` from the v1 raw GN GeoJSON (file order,
        /// v1 cell set) to check the algorithm itself.
        #[arg(long)]
        v1_geojson: Option<PathBuf>,
    },
    /// `/v2/reverse` grid path vs the PostGIS polygon path on random land points.
    ReverseCheck {
        #[arg(long, default_value_t = 100_000)]
        n: usize,
        #[arg(long, default_value_t = 20_261_122)]
        seed: u64,
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
    },
}

/// (id, period, dataset, dtype, serve, bytes, cog bytes, stats)
type LayerListRow = (
    String,
    String,
    String,
    String,
    bool,
    Option<i64>,
    Option<i64>,
    Option<serde_json::Value>,
);

/// splitmix64 — reproducible samples without a dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    #[allow(clippy::cast_precision_loss)]
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

async fn layer_path(pool: &PgPool, root: &Path, id: &str, period: Option<&str>) -> Result<PathBuf> {
    let p: Option<String> = sqlx::query_scalar(
        "SELECT bin_path FROM geo.grid_layers WHERE id = $1 AND ($2::text IS NULL OR period = $2)
         ORDER BY period DESC LIMIT 1",
    )
    .bind(id)
    .bind(period)
    .fetch_optional(pool)
    .await?;
    Ok(root.join(p.with_context(|| format!("grid layer {id} not found"))?))
}

fn read_csv(path: &Path) -> Result<Vec<(u32, String)>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (a, b) = l.split_once(',').context("csv line")?;
            Ok((a.parse()?, b.trim_matches('"').to_owned()))
        })
        .collect()
}

#[derive(Deserialize)]
struct Fc {
    features: Vec<Feature>,
}
#[derive(Deserialize)]
struct Feature {
    properties: Props,
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Props {
    adm4_pcode: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "type")]
enum Geometry {
    Polygon {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    MultiPolygon {
        coordinates: Vec<Vec<Vec<[f64; 2]>>>,
    },
}

pub async fn run(cmd: GridCmd, env: &Env<'_>, profile: &Profile) -> Result<()> {
    let root = lanka_ingest::grid::local_root(env.store)?;
    match cmd {
        GridCmd::Lookups => {
            let t = Instant::now();
            let b = lanka_ingest::grid::build_lookups(env).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"build": b, "total_ms": t.elapsed().as_millis()})
                )?
            );
        }
        GridCmd::List => {
            let rows: Vec<LayerListRow> =
                sqlx::query_as(
                    "SELECT id, period, dataset_id, dtype, serve, bytes, cog_bytes, stats FROM geo.grid_layers ORDER BY id, period",
                )
                .fetch_all(env.pool)
                .await?;
            for (id, period, ds, dtype, serve, b, cb, st) in rows {
                println!(
                    "{id:<24} {period:<12} {ds:<28} {dtype:<4} serve={serve:<5} bin={:>10} cog={:>9} {}",
                    b.unwrap_or(0),
                    cb.unwrap_or(0),
                    st.map(|s| s.to_string()).unwrap_or_default()
                );
            }
        }
        GridCmd::Parity { v1_dir, v1_geojson } => {
            parity(env, &root, profile, &v1_dir, v1_geojson.as_deref()).await?
        }
        GridCmd::ReverseCheck {
            n,
            seed,
            concurrency,
        } => reverse_check(env.pool, &root, profile, n, seed, concurrency).await?,
    }
    Ok(())
}

async fn parity(
    env: &Env<'_>,
    root: &Path,
    profile: &Profile,
    dir: &Path,
    geojson: Option<&Path>,
) -> Result<()> {
    let grid = profile.grid;
    // ── WorldPop 2025 vs v1 cells ──
    let cells = read_csv(&dir.join("cells.csv"))?;
    let (pop, _) = GridArray::read_bin(
        &layer_path(env.pool, root, "worldpop.population", Some("2025")).await?,
    )?;
    let GridData::F32(pop) = pop.data else {
        bail!("worldpop layer is not f32")
    };
    let mut v1 = vec![None::<f64>; pop.len()];
    for (id, v) in &cells {
        v1[*id as usize] = Some(v.parse()?);
    }
    let (mut equal, mut missing, mut extra, mut positive) = (0usize, 0usize, 0usize, 0usize);
    let (mut sum_v1, mut sum_v2) = (0.0f64, 0.0f64);
    for (i, &v) in pop.iter().enumerate() {
        let pos = v.is_finite() && v > 0.0;
        positive += usize::from(pos);
        if pos {
            sum_v2 += f64::from(v);
        }
        match (v1[i], pos) {
            (Some(a), true) => {
                sum_v1 += a;
                #[allow(clippy::cast_possible_truncation)]
                if a as f32 == v {
                    equal += 1;
                }
            }
            (Some(_), false) => missing += 1,
            (None, true) => extra += 1,
            _ => {}
        }
    }
    let wp = json!({"v1_cells": cells.len(), "v2_positive_cells": positive, "equal_at_f32": equal,
                    "missing": missing, "extra": extra, "sum_v1": sum_v1, "sum_v2": sum_v2,
                    "pass": equal == cells.len() && positive == cells.len()});

    // ── gnd_idx vs v1 cell_lookup ──
    let lookup_rows = read_csv(&dir.join("cell_lookup.csv"))?;
    let lk = lanka_ingest::grid::load_lookups(env, root).await?;
    let codes: HashMap<i32, String> =
        sqlx::query_as::<_, (i32, String)>("SELECT key, code FROM geo.units")
            .fetch_all(env.pool)
            .await?
            .into_iter()
            .collect();
    let code_of = |slot: u16| -> Option<&str> {
        let k = *lk.slot_unit.get(slot as usize)?;
        codes.get(&k).map(String::as_str)
    };
    let (mut same, mut diff, mut none) = (0usize, 0usize, 0usize);
    let mut examples = Vec::new();
    for (cell, pcode) in &lookup_rows {
        match code_of(lk.gnd_idx[*cell as usize]).filter(|_| lk.gnd_idx[*cell as usize] != 0) {
            Some(c) if c == pcode => same += 1,
            Some(c) => {
                diff += 1;
                if examples.len() < 20 {
                    examples.push(json!({"cell": cell, "v1": pcode, "v2": c}));
                }
            }
            None => {
                none += 1;
                if examples.len() < 20 {
                    examples.push(json!({"cell": cell, "v1": pcode, "v2": null}));
                }
            }
        }
    }
    let assigned_v2 = lk.gnd_idx.iter().filter(|&&s| s != 0).count();
    let gnd = json!({"v1_rows": lookup_rows.len(), "equal": same, "different": diff, "unassigned_in_v2": none,
                     "v2_cells_assigned_all_grid": assigned_v2, "examples": examples,
                     "pass": same == lookup_rows.len()});

    // ── optional: the algorithm on v1's own inputs ──
    let algo = match geojson {
        None => serde_json::Value::Null,
        Some(p) => {
            let t = Instant::now();
            let text = std::fs::read(p).with_context(|| format!("reading {}", p.display()))?;
            let fc: Fc = serde_json::from_slice(&text)?;
            drop(text);
            let mut geoms = Vec::new();
            let mut pcodes = Vec::new();
            for f in fc.features {
                let Some(pc) = f.properties.adm4_pcode else {
                    continue;
                };
                geoms.push(MultiPolygon(match f.geometry {
                    Geometry::Polygon { coordinates } => vec![coordinates],
                    Geometry::MultiPolygon { coordinates } => coordinates,
                }));
                pcodes.push(pc);
            }
            let ids: Vec<u32> = cells.iter().map(|c| c.0).collect();
            let built = tokio::task::spawn_blocking(move || {
                lookup::build(&grid, &geoms, Cells::Only(&ids))
            })
            .await?;
            let (mut eq, mut ne) = (0usize, 0usize);
            let mut ex = Vec::new();
            let v1map: HashMap<u32, &str> =
                lookup_rows.iter().map(|(c, p)| (*c, p.as_str())).collect();
            for (cell, _) in &cells {
                let s = built.gnd_idx[*cell as usize];
                let mine = (s != 0).then(|| pcodes[s as usize - 1].as_str());
                if mine == v1map.get(cell).copied() {
                    eq += 1;
                } else {
                    ne += 1;
                    if ex.len() < 10 {
                        ex.push(json!({"cell": cell, "v1": v1map.get(cell), "rebuilt": mine}));
                    }
                }
            }
            json!({"features": pcodes.len(), "cells": cells.len(), "equal": eq, "different": ne,
                   "examples": ex, "report": built.report, "ms": t.elapsed().as_millis()})
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"worldpop_2025": wp, "gnd_idx": gnd, "gnd_idx_from_v1_geojson": algo})
        )?
    );
    Ok(())
}

async fn reverse_check(
    pool: &PgPool,
    root: &Path,
    profile: &Profile,
    n: usize,
    seed: u64,
    conc: usize,
) -> Result<()> {
    let grid = profile.grid;
    let gs = lanka_server::grids::GridSet::load(pool, root, grid).await?;
    let cat = Catalog::load(pool).await?;
    let (gnd, _) = GridArray::read_bin(&layer_path(pool, root, "grid.gnd_idx", None).await?)?;
    let GridData::U16(gnd) = gnd.data else {
        bail!("gnd_idx is not u16")
    };
    let mut rng = Rng(seed);
    let mut pts = Vec::with_capacity(n);
    while pts.len() < n {
        let lat = grid.lat_min() + rng.unit() * (grid.lat_max - grid.lat_min());
        let lon = grid.lon_min + rng.unit() * (grid.lon_max() - grid.lon_min);
        if let Some(c) = grid.cell_id(lat, lon)
            && gnd[c as usize] != 0
        {
            pts.push((lat, lon));
        }
    }
    let t = Instant::now();
    let grid_answers: Vec<Option<query::ReverseResult>> =
        pts.iter().map(|&(la, lo)| gs.reverse(la, lo)).collect();
    let grid_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let poly: Vec<std::result::Result<query::ReverseResult, String>> =
        futures::stream::iter(pts.iter().copied())
            .map(|(la, lo)| {
                let (pool, cat) = (pool, &cat);
                async move {
                    query::reverse(pool, cat, la, lo)
                        .await
                        .map_err(|e| e.to_string())
                }
            })
            .buffered(conc.max(1))
            .collect()
            .await;
    let poly_ms = t.elapsed().as_secs_f64() * 1e3;
    let (mut by_grid, mut fallback, mut agree, mut disagree) = (0usize, 0usize, 0usize, 0usize);
    let mut examples = Vec::new();
    let key = |r: &query::ReverseResult| {
        let mut v: Vec<String> = r.ancestors.iter().map(|a| a.pcode.clone()).collect();
        v.push(r.unit.pcode.clone());
        (v, r.match_kind.clone())
    };
    for ((g, p), &(la, lo)) in grid_answers.iter().zip(&poly).zip(&pts) {
        match g {
            None => fallback += 1,
            Some(g) => {
                by_grid += 1;
                match p {
                    Ok(p) if key(p) == key(g) => agree += 1,
                    other => {
                        disagree += 1;
                        if examples.len() < 20 {
                            examples.push(json!({"lat": la, "lon": lo, "grid": g.unit.pcode,
                                "polygon": other.as_ref().map(|p| p.unit.pcode.clone()).map_err(Clone::clone)}));
                        }
                    }
                }
            }
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let per = |ms: f64, k: usize| if k == 0 { 0.0 } else { ms * 1e3 / k as f64 };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "points": n, "seed": seed, "answered_by_grid": by_grid, "polygon_fallback": fallback,
            "grid_equals_polygon": agree, "grid_differs": disagree, "examples": examples,
            "grid_us_per_point": per(grid_ms, n), "polygon_us_per_point_at_concurrency": per(poly_ms, n),
            "concurrency": conc, "pass": disagree == 0,
        }))?
    );
    Ok(())
}
