//! Vector tilesets (05 §9, ADR 0014): Postgres → GeoJSON text sequences →
//! tippecanoe → PMTiles, content-named in the store
//! (`tiles/<tileset>/<sha16>.pmtiles`) and indexed in `geo.tilesets`.
//!
//! Tilesets:
//! - `admin` (built in): one layer per admin level `adm0…adm4` (zoom-gated:
//!   adm0–adm2 from z0, adm3 from z5, adm4 from z9 — at least one zoom
//!   below the `byZoom` switch of 10 §2 so a level is in the tiles before a
//!   map asks for it; adm3 also covers island views on narrow screens,
//!   whose fitted zoom is below 6), each feature
//!   carrying `pcode` (for MapLibre `promoteId`), `level` and the three
//!   names, plus a `centroids` point layer (point on surface) for labels and
//!   symbols (10 §2: the frame/value join is by p-code, never by name).
//! - one per `features` dataset that declares `tiles:` in the registry
//!   (datasets sharing `tiles.tileset` become layers of one tileset):
//!   the flat feature schema of `lanka_core::features::FeatureProps`:
//!   `key`, `kind`, `name`, the declared `tiles.attrs` at the top level,
//!   `flags` (comma-joined names, MVT has no arrays) when set; per-kind
//!   minimum zooms from `tiles.minzoom_by_kind`.
//!
//! A build is skipped when its input hash (spec, every input dataset
//! version, tippecanoe version, export format) equals the newest build's,
//! so `lanka tiles build` after a release rebuilds only changed tilesets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use futures::TryStreamExt;
use lanka_core::Profile;
use lanka_ingest::Registry;
use lanka_store::{BlobStore, Ns};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::io::AsyncWriteExt;

use crate::pmtiles::PmTiles;
use crate::tippecanoe;

/// Bumped when the exported properties or layout change.
/// v2: per-layer `minzoom`/`maxzoom` in the layer index, source attribution,
/// `flags` as names.
const EXPORT_FORMAT: &str = "lanka-tiles-v2";
pub const ADMIN: &str = "admin";
const ADMIN_MAXZOOM: u8 = 12;
/// Minimum zoom of each admin level's polygons and of its centroids. The
/// TileJSON's `vector_layers[].minzoom` states these (10 §3.1).
/// adm3 from z5: the island tile then holds every DS division (largest z5
/// tile 60 KB gzip, z6 81 KB; tippecanoe simplifies per zoom, nothing is
/// coalesced or dropped).
pub const ADMIN_ZOOMS: [(i16, u8, u8); 5] = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (3, 5, 5), (4, 9, 9)];

#[derive(Debug, Clone, Serialize)]
pub enum LayerSource {
    Admin {
        level: i16,
        minzoom: u8,
    },
    AdminCentroids,
    Features {
        dataset_id: String,
        dataset_key: i32,
        seq: i32,
        minzoom: u8,
        minzoom_by_kind: BTreeMap<String, u8>,
        attrs: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerPlan {
    pub id: String,
    pub source: LayerSource,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: String,
    pub layers: Vec<LayerPlan>,
    pub minzoom: u8,
    pub maxzoom: u8,
    pub drop_densest: bool,
    /// (dataset id, version id, dsv) of every input.
    pub inputs: Vec<(String, i64, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Built {
    pub id: String,
    pub sha16: String,
    pub path: String,
    pub bytes: u64,
    /// false = skipped, identical inputs.
    pub built: bool,
    pub features: BTreeMap<String, u64>,
    pub export_ms: u128,
    pub tippecanoe_ms: u128,
    pub warnings: Vec<String>,
}

/// `(version id, seq, dsv)` of a dataset's released version.
async fn released(pool: &PgPool, dataset: &str) -> Result<Option<(i64, i32, String)>> {
    Ok(sqlx::query_as(
        "SELECT version_id, seq, dsv FROM data.current_versions WHERE dataset_id = $1",
    )
    .bind(dataset)
    .fetch_optional(pool)
    .await?)
}

/// Every tileset that can be built now (inputs released).
pub async fn plans(pool: &PgPool, registry: &Registry, profile: &Profile) -> Result<Vec<Plan>> {
    let mut out = Vec::new();
    let boundary = profile.boundary_dataset();
    if let Some((vid, _, dsv)) = released(pool, boundary).await? {
        let mut layers: Vec<LayerPlan> = ADMIN_ZOOMS
            .iter()
            .filter(|(l, _, _)| *l <= i16::from(profile.admin.max_level()))
            .map(|(level, minzoom, _)| LayerPlan {
                id: format!("adm{level}"),
                source: LayerSource::Admin {
                    level: *level,
                    minzoom: *minzoom,
                },
            })
            .collect();
        layers.push(LayerPlan {
            id: "centroids".to_owned(),
            source: LayerSource::AdminCentroids,
        });
        out.push(Plan {
            id: ADMIN.to_owned(),
            layers,
            minzoom: 0,
            maxzoom: ADMIN_MAXZOOM,
            drop_densest: false,
            inputs: vec![(boundary.to_owned(), vid, dsv)],
        });
    }
    let mut grouped: BTreeMap<String, Plan> = BTreeMap::new();
    for l in registry.datasets.values() {
        let d = &l.spec;
        let Some(t) = &d.tiles else { continue };
        if d.kind() != "features" {
            continue;
        }
        let Some((vid, seq, dsv)) = released(pool, &d.id).await? else {
            continue;
        };
        let key: i32 = sqlx::query_scalar("SELECT key FROM catalog.datasets WHERE id = $1")
            .bind(&d.id)
            .fetch_one(pool)
            .await?;
        let tid = t.tileset.clone().unwrap_or_else(|| d.id.clone());
        let p = grouped.entry(tid.clone()).or_insert_with(|| Plan {
            id: tid,
            layers: Vec::new(),
            minzoom: t.minzoom,
            maxzoom: t.maxzoom,
            drop_densest: false,
            inputs: Vec::new(),
        });
        p.minzoom = p.minzoom.min(t.minzoom);
        p.maxzoom = p.maxzoom.max(t.maxzoom);
        p.drop_densest |= t.drop_densest;
        p.inputs.push((d.id.clone(), vid, dsv));
        p.layers.push(LayerPlan {
            id: t.layer.clone().unwrap_or_else(|| d.id.clone()),
            source: LayerSource::Features {
                dataset_id: d.id.clone(),
                dataset_key: key,
                seq,
                minzoom: t.minzoom,
                minzoom_by_kind: t.minzoom_by_kind.clone(),
                attrs: t.attrs.clone(),
            },
        });
    }
    out.extend(grouped.into_values());
    Ok(out)
}

/// The lowest zoom at which a layer has any feature.
fn layer_minzoom(source: &LayerSource, tileset_min: u8) -> u8 {
    let z = match source {
        LayerSource::Admin { minzoom, .. } => *minzoom,
        LayerSource::AdminCentroids => ADMIN_ZOOMS
            .iter()
            .filter(|(l, _, _)| *l >= 1)
            .map(|(_, _, c)| *c)
            .min()
            .unwrap_or(0),
        LayerSource::Features {
            minzoom,
            minzoom_by_kind,
            ..
        } => minzoom_by_kind
            .values()
            .copied()
            .chain(std::iter::once(*minzoom))
            .min()
            .unwrap_or(*minzoom),
    };
    z.max(tileset_min)
}

fn input_hash(plan: &Plan, tool: &str) -> Result<[u8; 32]> {
    let mut h = Sha256::new();
    h.update(EXPORT_FORMAT.as_bytes());
    h.update([0x1f]);
    h.update(tool.as_bytes());
    h.update([0x1f]);
    h.update(serde_json::to_vec(plan)?);
    Ok(h.finalize().into())
}

/// Writes one layer as a GeoJSON text sequence; returns the feature count.
async fn export_layer(pool: &PgPool, profile: &Profile, l: &LayerPlan, path: &Path) -> Result<u64> {
    let set = profile.reference_boundary_set();
    let mut rows = match &l.source {
        LayerSource::Admin { level, minzoom } => sqlx::query_scalar::<_, String>(
            "SELECT json_build_object('type', 'Feature', 'tippecanoe', json_build_object('minzoom', $3::int),
                'properties', json_strip_nulls(json_build_object('pcode', u.code, 'level', u.level,
                    'name_en', u.name_en, 'name_si', u.name_si, 'name_ta', u.name_ta)),
                'geometry', ST_AsGeoJSON(g.geom, 6)::json)::text
             FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
             WHERE g.boundary_set = $1 AND u.level = $2 ORDER BY u.code",
        )
        .bind(set)
        .bind(*level)
        .bind(i32::from(*minzoom))
        .fetch(pool),
        LayerSource::AdminCentroids => sqlx::query_scalar::<_, String>(
            "SELECT json_build_object('type', 'Feature',
                'tippecanoe', json_build_object('minzoom', (($2::int[])[u.level + 1])),
                'properties', json_strip_nulls(json_build_object('pcode', u.code, 'level', u.level,
                    'name_en', u.name_en, 'name_si', u.name_si, 'name_ta', u.name_ta)),
                'geometry', ST_AsGeoJSON(g.centroid, 6)::json)::text
             FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
             WHERE g.boundary_set = $1 AND u.level >= 1 AND g.centroid IS NOT NULL ORDER BY u.level, u.code",
        )
        .bind(set)
        .bind(
            ADMIN_ZOOMS
                .iter()
                .map(|(_, _, c)| i32::from(*c))
                .collect::<Vec<i32>>(),
        )
        .fetch(pool),
        LayerSource::Features {
            dataset_key,
            seq,
            minzoom,
            minzoom_by_kind,
            attrs,
            ..
        } => sqlx::query_scalar::<_, String>(
            "SELECT json_build_object('type', 'Feature',
                'tippecanoe', json_build_object('minzoom', coalesce(($3::jsonb ->> f.kind)::int, $4::int)),
                'properties', jsonb_strip_nulls(
                    jsonb_build_object('key', f.feature_key, 'kind', f.kind, 'name', f.name_en,
                                       'flags', (SELECT string_agg(r.n, ',' ORDER BY r.b)
                                                 FROM unnest($6::int[], $7::text[]) AS r(b, n)
                                                 WHERE f.flags & (1 << r.b) <> 0))
                    || CASE WHEN 'name_si' = ANY($5) THEN jsonb_build_object('name_si', f.name_si) ELSE '{}' END
                    || CASE WHEN 'name_ta' = ANY($5) THEN jsonb_build_object('name_ta', f.name_ta) ELSE '{}' END
                    || coalesce((SELECT jsonb_object_agg(a.key, a.value) FROM jsonb_each(f.attrs) a
                                 WHERE a.key = ANY($5) AND NOT a.key = ANY($8)), '{}')),
                'geometry', ST_AsGeoJSON(f.geom, 6)::json)::text
             FROM geo.features f
             WHERE f.dataset_key = $1 AND f.vto = 2147483647 AND f.vfrom <= $2
             ORDER BY f.feature_id",
        )
        .bind(*dataset_key)
        .bind(*seq)
        .bind(serde_json::to_value(minzoom_by_kind)?)
        .bind(i32::from(*minzoom))
        .bind(attrs)
        .bind(
            lanka_core::flags::REGISTRY
                .iter()
                .map(|(b, _)| i32::from(*b))
                .collect::<Vec<i32>>(),
        )
        .bind(
            lanka_core::flags::REGISTRY
                .iter()
                .map(|(_, n)| (*n).to_owned())
                .collect::<Vec<String>>(),
        )
        .bind(
            lanka_core::features::CORE_PROPS
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<String>>(),
        )
        .fetch(pool),
    };
    let f = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("creating {}", path.display()))?;
    let mut w = tokio::io::BufWriter::with_capacity(1 << 20, f);
    let mut n = 0u64;
    while let Some(line) = rows.try_next().await? {
        w.write_all(line.as_bytes()).await?;
        w.write_all(b"\n").await?;
        n += 1;
    }
    w.flush().await?;
    Ok(n)
}

/// The tileset's attribution: each input dataset's sources, as
/// `<the source's attribution, else its publisher> (<licence>)`.
async fn attribution(pool: &PgPool, datasets: &[String]) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT string_agg(DISTINCT a.t, '; ' ORDER BY a.t) FROM (
            SELECT coalesce(nullif(btrim(s.spec #>> '{licence,attribution}'), ''), s.publisher)
                   || ' (' || coalesce(l.spdx, s.licence_id) || ')' AS t
            FROM catalog.dataset_sources ds JOIN catalog.sources s ON s.id = ds.source_id
            LEFT JOIN catalog.licences l ON l.id = s.licence_id
            WHERE ds.dataset_id = ANY($1)) a",
    )
    .bind(datasets)
    .fetch_one(pool)
    .await?)
}

/// Builds one tileset unless its inputs are unchanged (`force` rebuilds).
#[allow(clippy::too_many_lines)]
pub async fn build(
    pool: &PgPool,
    store: &dyn BlobStore,
    profile: &Profile,
    plan: &Plan,
    force: bool,
) -> Result<Built> {
    let tool = tippecanoe::version()?;
    let hash = input_hash(plan, &tool)?;
    let prev: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT sha16, path, bytes FROM geo.current_tilesets WHERE id = $1 AND input_hash = $2",
    )
    .bind(&plan.id)
    .bind(hash.as_slice())
    .fetch_optional(pool)
    .await?;
    if let Some((sha16, path, bytes)) = prev
        && !force
        && store
            .exists(Ns::Tile, path.trim_start_matches("tiles/"))
            .await?
    {
        return Ok(Built {
            id: plan.id.clone(),
            sha16,
            path,
            bytes: u64::try_from(bytes)?,
            built: false,
            features: BTreeMap::new(),
            export_ms: 0,
            tippecanoe_ms: 0,
            warnings: Vec::new(),
        });
    }
    let work: PathBuf = store.tmp_file(&format!("tiles-{}", plan.id))?;
    tokio::fs::create_dir_all(&work).await?;
    let t0 = Instant::now();
    let mut layers = Vec::new();
    let mut features = BTreeMap::new();
    for l in &plan.layers {
        let p = work.join(format!("{}.geojsonl", l.id));
        let n = export_layer(pool, profile, l, &p).await?;
        features.insert(l.id.clone(), n);
        layers.push(tippecanoe::Layer {
            name: l.id.clone(),
            file: p,
        });
    }
    let export_ms = t0.elapsed().as_millis();
    let datasets: Vec<String> = plan.inputs.iter().map(|i| i.0.clone()).collect();
    let out = work.join(format!("{}.pmtiles", plan.id));
    let op = tippecanoe::Op {
        layers,
        output: out.clone(),
        minzoom: plan.minzoom,
        maxzoom: plan.maxzoom,
        drop_densest: plan.drop_densest,
        name: plan.id.clone(),
        attribution: attribution(pool, &datasets).await?,
    };
    let t1 = Instant::now();
    let warnings = tokio::task::spawn_blocking(move || tippecanoe::run(&op)).await??;
    let tippecanoe_ms = t1.elapsed().as_millis();

    let (sha, bytes) = lanka_store::FsStore::hash_file(&out).await?;
    let sha16 = hex::encode(&sha[..8]);
    let key = format!("{}/{sha16}.pmtiles", plan.id);
    let blob = store.put_named(Ns::Tile, &key, &out).await?;
    let local = store
        .local_path(Ns::Tile, &key)
        .context("tiles need a local store")?;
    let pm = PmTiles::open(&local)?;
    let h = pm.header.clone();
    let meta = pm.metadata().unwrap_or_default();
    let vector_layers = meta.get("vector_layers").cloned().unwrap_or(json!([]));
    let layer_info: Vec<serde_json::Value> = plan
        .layers
        .iter()
        .map(|l| {
            let minzoom = layer_minzoom(&l.source, plan.minzoom);
            let fields = vector_layers
                .as_array()
                .and_then(|vs| vs.iter().find(|v| v["id"] == l.id.as_str()))
                .map(|v| v["fields"].clone())
                .unwrap_or(json!({}));
            let dataset = match &l.source {
                LayerSource::Features { dataset_id, .. } => json!(dataset_id),
                _ => json!(profile.boundary_dataset()),
            };
            json!({ "id": l.id, "dataset": dataset, "fields": fields,
                    "minzoom": minzoom, "maxzoom": plan.maxzoom,
                    "features": features.get(&l.id).copied().unwrap_or(0) })
        })
        .collect();
    ensure!(
        h.tile_type == 1,
        "tippecanoe wrote tile type {}",
        h.tile_type
    );
    let rel = blob.rel_path();
    sqlx::query(
        "INSERT INTO geo.tilesets (id, sha16, path, bytes, minzoom, maxzoom, bounds, layers, dataset_ids,
            version_ids, input_hash, tool, attribution, build_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (id, sha16) DO UPDATE SET built_at = now(), input_hash = EXCLUDED.input_hash,
            version_ids = EXCLUDED.version_ids, tool = EXCLUDED.tool, build_ms = EXCLUDED.build_ms",
    )
    .bind(&plan.id)
    .bind(&sha16)
    .bind(&rel)
    .bind(i64::try_from(bytes)?)
    .bind(i16::from(h.min_zoom))
    .bind(i16::from(h.max_zoom))
    .bind(h.bounds.to_vec())
    .bind(json!(layer_info))
    .bind(&datasets)
    .bind(plan.inputs.iter().map(|i| i.1).collect::<Vec<i64>>())
    .bind(hash.as_slice())
    .bind(&tool)
    .bind(meta.get("attribution").and_then(|a| a.as_str()))
    .bind(i64::try_from(export_ms + tippecanoe_ms)?)
    .execute(pool)
    .await?;
    let _ = tokio::fs::remove_dir_all(&work).await;
    Ok(Built {
        id: plan.id.clone(),
        sha16,
        path: rel,
        bytes,
        built: true,
        features,
        export_ms,
        tippecanoe_ms,
        warnings,
    })
}

/// Builds every plannable tileset (or the named ones).
pub async fn build_all(
    pool: &PgPool,
    store: &dyn BlobStore,
    registry: &Registry,
    profile: &Profile,
    only: &[String],
    force: bool,
) -> Result<Vec<Built>> {
    let plans = plans(pool, registry, profile).await?;
    // The raster-dem tileset (WP-27) is built from the elevation grid layer,
    // when there is one.
    let has_dem: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM geo.grid_layers WHERE id = $1)")
            .bind(crate::dem::ELEVATION_LAYER)
            .fetch_one(pool)
            .await?;
    for o in only {
        if o == crate::dem::DEM && has_dem {
            continue;
        }
        if !plans.iter().any(|p| &p.id == o) {
            bail!(
                "no tileset {o:?} can be built (known: {})",
                plans
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    let mut out = Vec::new();
    for p in plans
        .iter()
        .filter(|p| only.is_empty() || only.contains(&p.id))
    {
        let b = build(pool, store, profile, p, force)
            .await
            .with_context(|| format!("tileset {}", p.id))?;
        tracing::info!(tileset = %b.id, sha16 = %b.sha16, bytes = b.bytes, built = b.built, "tileset");
        out.push(b);
    }
    if has_dem && (only.is_empty() || only.iter().any(|o| o == crate::dem::DEM)) {
        let b = crate::dem::build(pool, store, force)
            .await
            .context("tileset dem")?;
        tracing::info!(tileset = %b.id, sha16 = %b.sha16, bytes = b.bytes, built = b.built, "tileset");
        out.push(b);
    }
    Ok(out)
}
