//! Validate → stage → diff → gate → approve/review for `features` outputs
//! (points, lines, polygons) on the range-versioned model (05 §4, ADR 0004).
//!
//! Same contract as observations (`publish.rs`): a version is a
//! dataset-local seq; staging inserts only new or changed features
//! (`vfrom = seq`) and lists the rows they replace in `data.version_closes`;
//! the release train closes them (`vto = seq - 1`); rejecting deletes them.
//!
//! Identity is the upstream key (`feature_key`, e.g. `way/123`). Each row's
//! canonical bytes (key, kind, names, 6-dp GeoJSON, canonical attrs, flags,
//! entity) are hashed; the dsv is the multiset of those hashes, so an
//! unchanged delivery is recognised before anything is copied, and a
//! changed one is diffed by `(key, hash)` with only the keys COPY'd: the
//! full rows (geometry text) are sent for new and changed features only. A
//! daily OSM refresh therefore writes the few thousand features that changed.
//!
//! Derived per row at insert, outside the hash: the canonical grid `cell_id`
//! of the representative point, and `unit_key` — the deepest admin unit of
//! the reference boundary set containing it, else the nearest one
//! (`unit_match` 1 within 2 km, 2 beyond). When WP-05's lookup arrays exist
//! for the boundary set, a point in an *interior* cell (`land_mask` 1: no unit
//! boundary crosses it) takes the cell's unit from `gnd_idx` (05 §4); every
//! other point keeps the polygon path. Features whose representative
//! point lies outside the grid or the country (coverage = land buffered by
//! 2 km) are not the country's and are dropped with a warning.

use std::collections::{BTreeSet, HashSet};

use anyhow::{Context, Result, bail, ensure};
use lanka_core::dsv::{MultisetHash, canonical_json, dsv, schema_hash};
use lanka_core::{Flags, Profile};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool};

use crate::connector::FeatureIn;
use crate::copy_bin::BinCopy;
use crate::geom::Coverage;
use crate::publish::{
    Diff, MAX_CHANGED_SHARE, MAX_REMOVED_SHARE, OBS_KEY_SHIFT, PublishResult, current_full,
    discard_open, lock_dataset, mark_approved, open_review,
};
use crate::registry::{FieldType, Registry};

/// Distance within which a feature outside every unit snaps to the nearest
/// one (same as `/v2/reverse`).
pub const SNAP_M: f64 = 2000.0;
const COPY_CHUNK: usize = 4 << 20;

/// Canonical bytes of one feature (the dsv multiset and the diff hash).
#[must_use]
pub fn feature_row_bytes(f: &FeatureIn) -> Vec<u8> {
    let attrs = canonical_json(&Value::Object(f.attrs.clone()));
    let flags = f.flags.bits().to_string();
    let mut b = Vec::with_capacity(64 + f.geometry.len() + attrs.len());
    for (i, part) in [
        f.key.as_str(),
        f.kind.as_str(),
        f.name_en.as_deref().unwrap_or_default(),
        f.name_si.as_deref().unwrap_or_default(),
        f.name_ta.as_deref().unwrap_or_default(),
        f.geometry.as_str(),
        attrs.as_str(),
        flags.as_str(),
        f.entity_id.as_deref().unwrap_or_default(),
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            b.push(0x1f);
        }
        b.extend_from_slice(part.as_bytes());
    }
    b
}

#[must_use]
pub fn feature_hash(f: &FeatureIn) -> [u8; 32] {
    Sha256::digest(feature_row_bytes(f)).into()
}

/// Schema of a features dataset: its declared fields (name, type, unit).
#[must_use]
pub fn features_schema(registry: &Registry, dataset_id: &str) -> [u8; 32] {
    let mut lines = vec![format!("features\u{1f}{dataset_id}")];
    if let Some(d) = registry.dataset(dataset_id) {
        for f in &d.fields {
            lines.push(format!(
                "field\u{1f}{}\u{1f}{:?}\u{1f}{}",
                f.name,
                f.ty,
                f.unit.as_deref().unwrap_or_default()
            ));
        }
    }
    schema_hash(lines)
}

/// One features dataset to stage, as produced by `standardise`.
#[derive(Debug, Clone, Copy)]
pub struct FeatureStageInput<'a> {
    pub run_id: i64,
    pub dataset_id: &'a str,
    pub vintage: Option<&'a str>,
    pub rows: &'a [FeatureIn],
    pub artifact_ids: &'a [i64],
    pub extract_ids: &'a [i64],
    /// Country land area buffered by [`SNAP_M`]; `None` = grid bounds only.
    pub coverage: Option<&'a Coverage>,
    /// `gnd_idx`/`land_mask` of the boundary set (WP-05); `None` = polygons only.
    pub grid_units: Option<&'a GridUnits>,
}

/// Cell → unit for interior cells, from WP-05's `grid.gnd_idx` and
/// `grid.land_mask` of one boundary set.
pub struct GridUnits {
    pub boundary_set: String,
    gnd_idx: Vec<u16>,
    land_mask: Vec<u8>,
    /// `slot_unit[s]` = `geo.units.key` of slot `s` (0 unused).
    slot_unit: Vec<i32>,
}

impl std::fmt::Debug for GridUnits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridUnits")
            .field("boundary_set", &self.boundary_set)
            .field("cells", &self.gnd_idx.len())
            .field("slots", &self.slot_unit.len().saturating_sub(1))
            .finish()
    }
}

impl GridUnits {
    /// The unit of `cell` when the whole cell lies inside it.
    #[must_use]
    pub fn interior_unit(&self, cell: u32) -> Option<i32> {
        let i = cell as usize;
        if self.land_mask.get(i).copied() != Some(lanka_grid::lookup::MASK_INTERIOR) {
            return None;
        }
        let s = usize::from(*self.gnd_idx.get(i)?);
        (s > 0).then(|| self.slot_unit.get(s).copied()).flatten()
    }
}

/// Loads the lookup arrays of the profile's reference set when they were
/// built (`lanka grid lookups`, or the first raster run); never builds them.
pub async fn load_grid_units(
    pool: &PgPool,
    store: &dyn lanka_store::BlobStore,
    profile: &Profile,
) -> Result<Option<GridUnits>> {
    use lanka_grid::{GridArray, GridData};
    let set = profile.reference_boundary_set().to_owned();
    let paths: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, bin_path FROM geo.grid_layers WHERE id IN ('grid.gnd_idx', 'grid.land_mask') AND period = $1",
    )
    .bind(&set)
    .fetch_all(pool)
    .await?;
    let get = |id: &str| paths.iter().find(|p| p.0 == id).map(|p| p.1.clone());
    let (Some(g), Some(m)) = (get("grid.gnd_idx"), get("grid.land_mask")) else {
        return Ok(None);
    };
    let root = crate::grid::local_root(store)?;
    let cells = lanka_grid::cells(&profile.grid);
    let (gnd_idx, land_mask) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<u16>, Vec<u8>)> {
            let (ga, _) = GridArray::read_bin(&root.join(&g))?;
            let (ma, _) = GridArray::read_bin(&root.join(&m))?;
            let (GridData::U16(gi), GridData::U8(mk)) = (ga.data, ma.data) else {
                bail!("gnd_idx/land_mask have unexpected dtypes");
            };
            ensure!(
                gi.len() == cells && mk.len() == cells,
                "lookup arrays do not match the profile grid"
            );
            Ok((gi, mk))
        })
        .await??;
    let slots: Vec<(i32, i32)> = sqlx::query_as(
        "SELECT idx, unit_key FROM geo.grid_lookup_units WHERE layer_id = 'grid.gnd_idx' AND period = $1 ORDER BY idx",
    )
    .bind(&set)
    .fetch_all(pool)
    .await?;
    let n = usize::try_from(slots.last().map_or(0, |s| s.0))?;
    let mut slot_unit = vec![0; n + 1];
    for (i, k) in slots {
        slot_unit[usize::try_from(i)?] = k;
    }
    Ok(Some(GridUnits {
        boundary_set: set,
        gnd_idx,
        land_mask,
        slot_unit,
    }))
}

/// Rows kept after validation, with their hash and grid cell.
struct Kept<'a> {
    row: &'a FeatureIn,
    hash: [u8; 32],
    cell: u32,
}

/// Structural validation (keys unique and non-empty, geometry class, typed
/// declared fields, flags, entity ids) and the coverage filter.
fn validate<'a>(
    registry: &Registry,
    profile: &Profile,
    input: &FeatureStageInput<'a>,
) -> Result<(Vec<Kept<'a>>, usize)> {
    let d = registry
        .dataset(input.dataset_id)
        .with_context(|| format!("dataset {:?} not in registry", input.dataset_id))?;
    ensure!(
        d.kind() == "features",
        "dataset {:?} is of kind {:?}, not features",
        input.dataset_id,
        d.kind()
    );
    let fields: std::collections::HashMap<&str, &crate::registry::FieldSpec> =
        d.fields.iter().map(|f| (f.name.as_str(), f)).collect();
    let mut seen: HashSet<&str> = HashSet::with_capacity(input.rows.len());
    let mut kept = Vec::with_capacity(input.rows.len());
    let mut outside = 0usize;
    let mut problems: Vec<String> = Vec::new();
    for r in input.rows {
        if problems.len() >= 20 {
            break;
        }
        if r.key.is_empty() || r.kind.is_empty() {
            problems.push(format!("feature with empty key or kind ({:?})", r.key));
            continue;
        }
        if !seen.insert(r.key.as_str()) {
            problems.push(format!("feature key {:?} delivered twice", r.key));
            continue;
        }
        if !r.geometry.starts_with("{\"type\":") {
            problems.push(format!("{}: geometry is not GeoJSON", r.key));
            continue;
        }
        if Flags::from_db(r.flags.to_db()).is_err() {
            problems.push(format!("{}: reserved flag bits", r.key));
            continue;
        }
        if let Some(e) = &r.entity_id
            && !(e.len() == 14 && e.starts_with("lke_"))
        {
            problems.push(format!("{}: entity id {e:?} is not lke_<10>", r.key));
            continue;
        }
        for (k, v) in &r.attrs {
            let Some(f) = fields.get(k.as_str()) else {
                problems.push(format!(
                    "{}: attribute {k:?} is not a declared field",
                    r.key
                ));
                continue;
            };
            let ok = match (f.ty, v) {
                (_, Value::Null) => true,
                (FieldType::String, Value::String(s)) => {
                    f.values.is_empty() || f.values.iter().any(|x| x == s)
                }
                (FieldType::Integer, Value::Number(n)) => n.is_i64(),
                (FieldType::Number, Value::Number(_)) => true,
                (FieldType::Boolean, Value::Bool(_)) => true,
                _ => false,
            };
            if !ok {
                problems.push(format!(
                    "{}: attribute {k:?} = {v} does not match its declared type {:?}",
                    r.key, f.ty
                ));
            }
        }
        let [lon, lat] = r.rep;
        let Some(cell) = profile.grid.cell_id(lat, lon) else {
            outside += 1;
            continue;
        };
        if let Some(c) = input.coverage
            && !c.contains(r.rep)
        {
            outside += 1;
            continue;
        }
        kept.push(Kept {
            row: r,
            hash: feature_hash(r),
            cell,
        });
    }
    if !problems.is_empty() {
        bail!(
            "{} feature(s) failed validation:\n  {}",
            problems.len(),
            problems.join("\n  ")
        );
    }
    Ok((kept, outside))
}

async fn copy_keys(conn: &mut PgConnection, kept: &[Kept<'_>]) -> Result<()> {
    sqlx::query(
        "CREATE TEMP TABLE feat_keys (rn int8, feature_key text, sha bytea) ON COMMIT DROP",
    )
    .execute(&mut *conn)
    .await?;
    let mut copy = conn
        .copy_in_raw("COPY feat_keys FROM STDIN (FORMAT binary)")
        .await?;
    let mut b = BinCopy::new();
    for (n, k) in kept.iter().enumerate() {
        b.row(3);
        b.i64(i64::try_from(n)?);
        b.text(Some(&k.row.key));
        b.bytea(&k.hash);
        if b.len() >= COPY_CHUNK {
            copy.send(b.take()).await?;
        }
    }
    b.finish();
    copy.send(b.take()).await?;
    copy.finish().await?;
    sqlx::query("CREATE INDEX ON feat_keys (feature_key)")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ANALYZE feat_keys").execute(&mut *conn).await?;
    Ok(())
}

/// Classifies staged keys against the rows open at `cur_seq`: `a`dded,
/// `c`hanged (hash differs) or `s`ame; lists removed rows and the rows this
/// version closes (`feat_closing`).
async fn classify(conn: &mut PgConnection, dataset_key: i32, cur_seq: i32) -> Result<Diff> {
    sqlx::query(
        "CREATE TEMP TABLE feat_diff ON COMMIT DROP AS
         SELECT s.rn, c.feature_id AS cur_id,
                CASE WHEN c.feature_id IS NULL THEN 'a' WHEN c.content_sha256 <> s.sha THEN 'c' ELSE 's' END AS change
         FROM feat_keys s
         LEFT JOIN geo.features c
           ON c.dataset_key = $1 AND c.vto = 2147483647 AND c.vfrom <= $2 AND c.feature_key = s.feature_key",
    )
    .bind(dataset_key)
    .bind(cur_seq)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "CREATE TEMP TABLE feat_closing ON COMMIT DROP AS
         SELECT cur_id AS feature_id FROM feat_diff WHERE change = 'c'
         UNION ALL
         SELECT f.feature_id FROM geo.features f
         WHERE f.dataset_key = $1 AND f.vto = 2147483647 AND f.vfrom <= $2
           AND NOT EXISTS (SELECT 1 FROM feat_keys s WHERE s.feature_key = f.feature_key)",
    )
    .bind(dataset_key)
    .bind(cur_seq)
    .execute(&mut *conn)
    .await?;
    let (added, changed, closing, prev): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE change = 'a'), count(*) FILTER (WHERE change = 'c'),
                (SELECT count(*) FROM feat_closing),
                (SELECT count(*) FROM geo.features
                  WHERE dataset_key = $1 AND vto = 2147483647 AND vfrom <= $2)
         FROM feat_diff",
    )
    .bind(dataset_key)
    .bind(cur_seq)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Diff {
        rows_added: added,
        rows_removed: closing - changed,
        rows_changed: changed,
        rows_restated: 0,
        prev_rows: prev,
    })
}

/// COPYs the full rows of new and changed features and inserts them as
/// version `seq`, deriving geometry validity, cell and unit.
#[allow(clippy::too_many_arguments)]
async fn write_rows(
    conn: &mut PgConnection,
    profile: &Profile,
    boundary_set: &str,
    dataset_key: i32,
    version_id: i64,
    seq: i32,
    kept: &[Kept<'_>],
    artifact_ids: &[i64],
    extract_ids: &[i64],
    grid_units: Option<&GridUnits>,
) -> Result<i64> {
    let grid_units = grid_units.filter(|g| g.boundary_set == boundary_set);
    let rns: Vec<i64> =
        sqlx::query_scalar("SELECT rn FROM feat_diff WHERE change <> 's' ORDER BY rn")
            .fetch_all(&mut *conn)
            .await?;
    sqlx::query(
        "CREATE TEMP TABLE feat_full (
            rn int8, feature_key text, kind text, name_en text, name_si text, name_ta text,
            geojson text, lon float8, lat float8, cell_id int4, attrs jsonb, flags int4,
            entity_id text, sha bytea, artifact_id int8, extract_id int8, extract_row int4,
            grid_unit int4
         ) ON COMMIT DROP",
    )
    .execute(&mut *conn)
    .await?;
    {
        let mut copy = conn
            .copy_in_raw("COPY feat_full FROM STDIN (FORMAT binary)")
            .await?;
        let mut b = BinCopy::new();
        for rn in &rns {
            let k = kept
                .get(usize::try_from(*rn)?)
                .context("staged feature number out of range")?;
            let r = k.row;
            b.row(18);
            b.i64(*rn);
            b.text(Some(&r.key));
            b.text(Some(&r.kind));
            b.text(r.name_en.as_deref());
            b.text(r.name_si.as_deref());
            b.text(r.name_ta.as_deref());
            b.text(Some(&r.geometry));
            b.f64(Some(r.rep[0]));
            b.f64(Some(r.rep[1]));
            b.i32(i32::try_from(k.cell)?);
            b.jsonb(&canonical_json(&Value::Object(r.attrs.clone())));
            b.i32(r.flags.to_db());
            b.text(r.entity_id.as_deref());
            b.bytea(&k.hash);
            b.opt_i64(artifact_ids.get(r.artifact).copied());
            b.opt_i64(r.prov.and_then(|p| extract_ids.get(p.extract).copied()));
            b.opt_i32(r.prov.map(|p| i32::try_from(p.row).unwrap_or(i32::MAX)));
            b.opt_i32(grid_units.and_then(|g| g.interior_unit(k.cell)));
            if b.len() >= COPY_CHUNK {
                copy.send(b.take()).await?;
            }
        }
        b.finish();
        copy.send(b.take()).await?;
        copy.finish().await?;
    }
    let n_new = i64::try_from(rns.len())?;
    let next: i64 = sqlx::query_scalar(
        "UPDATE catalog.datasets SET obs_seq = obs_seq + $2 WHERE key = $1 RETURNING obs_seq - $2",
    )
    .bind(dataset_key)
    .bind(n_new)
    .fetch_one(&mut *conn)
    .await?;
    let base = i64::from(dataset_key) << OBS_KEY_SHIFT;
    // Geometry: as delivered when valid, else ST_MakeValid reduced to the
    // delivered dimension. Unit: the interior cell's unit from gnd_idx, else
    // the containing unit at the deepest level of the reference set, else
    // the nearest (KNN on the GiST index).
    let written = sqlx::query(
        "INSERT INTO geo.features (feature_id, dataset_key, feature_key, kind, vfrom, vto, name_en, name_si,
            name_ta, geom, rep, cell_id, unit_key, unit_match, attrs, flags, entity_id, content_sha256,
            artifact_id, extract_id, extract_row)
         SELECT $1::int8 + $2::int8 + row_number() OVER (ORDER BY s.rn), $3, s.feature_key, s.kind, $4,
                2147483647, s.name_en, s.name_si, s.name_ta, v.g, p.rep, s.cell_id,
                coalesce(s.grid_unit, c.unit_key, n.unit_key),
                CASE WHEN s.grid_unit IS NOT NULL OR c.unit_key IS NOT NULL THEN 0
                     WHEN n.unit_key IS NULL THEN NULL
                     WHEN n.dist <= $7 THEN 1 ELSE 2 END,
                s.attrs, s.flags, s.entity_id, s.sha, s.artifact_id, s.extract_id, s.extract_row
         FROM feat_full s
         CROSS JOIN LATERAL (SELECT ST_SetSRID(ST_GeomFromGeoJSON(s.geojson), 4326) AS raw,
                                    ST_SetSRID(ST_MakePoint(s.lon, s.lat), 4326) AS rep) p
         CROSS JOIN LATERAL (SELECT CASE WHEN ST_IsValid(p.raw) THEN p.raw
                                         ELSE coalesce(nullif(ST_CollectionExtract(ST_MakeValid(p.raw),
                                                  ST_Dimension(p.raw) + 1), ST_GeomFromText('GEOMETRYCOLLECTION EMPTY', 4326)),
                                                  p.raw) END AS g) v
         LEFT JOIN LATERAL (
           SELECT g.unit_key FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
           WHERE s.grid_unit IS NULL AND g.boundary_set = $5 AND u.level = $6
             AND ST_Intersects(g.geom, p.rep)
           ORDER BY g.unit_key LIMIT 1) c ON true
         LEFT JOIN LATERAL (
           SELECT k.unit_key, ST_Distance(k.geom::geography, p.rep::geography) AS dist
           FROM (SELECT g.unit_key, g.geom FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
                 WHERE s.grid_unit IS NULL AND c.unit_key IS NULL AND g.boundary_set = $5 AND u.level = $6
                 ORDER BY g.geom <-> p.rep LIMIT 1) k) n ON true
         ORDER BY s.rn",
    )
    .bind(base)
    .bind(next)
    .bind(dataset_key)
    .bind(seq)
    .bind(boundary_set)
    .bind(i16::from(profile.admin.max_level()))
    .bind(SNAP_M)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    sqlx::query(
        "INSERT INTO data.version_closes (version_id, obs_id) SELECT $1, feature_id FROM feat_closing",
    )
    .bind(version_id)
    .execute(&mut *conn)
    .await?;
    Ok(i64::try_from(written)?)
}

/// The country's land area (level-0 unit of the reference boundary set)
/// buffered by [`SNAP_M`], simplified for fast point tests. `None` before
/// boundaries are loaded.
pub async fn load_coverage(
    pool: &PgPool,
    profile: &Profile,
) -> Result<Option<std::sync::Arc<Coverage>>> {
    let g: Option<String> = sqlx::query_scalar(
        "SELECT ST_AsGeoJSON(ST_SimplifyPreserveTopology(
                  ST_Buffer(coalesce(g.geom_simplified, g.geom)::geography, $2)::geometry, 0.0005), 6)
         FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
         WHERE g.boundary_set = $1 AND u.level = 0 LIMIT 1",
    )
    .bind(profile.reference_boundary_set())
    .bind(SNAP_M)
    .fetch_optional(pool)
    .await?
    .flatten();
    g.map(|t| Coverage::from_geometry(&crate::geom::Geometry::parse(&t)?).map(std::sync::Arc::new))
        .transpose()
}

/// Closes the features a released version replaces (`release_one` in the
/// train; the observation closes are applied by the same statement shape).
pub async fn close_released(
    conn: &mut PgConnection,
    version_id: i64,
    dataset_key: i32,
    seq: i32,
) -> Result<u64> {
    Ok(sqlx::query(
        "UPDATE geo.features f SET vto = $3 - 1
         FROM data.version_closes c
         WHERE c.version_id = $1 AND f.dataset_key = $2 AND f.feature_id = c.obs_id
           AND f.vto = 2147483647",
    )
    .bind(version_id)
    .bind(dataset_key)
    .bind(seq)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// Validate → stage → diff → gate → publish one features dataset.
#[allow(clippy::too_many_lines)]
pub async fn publish_features(
    pool: &PgPool,
    registry: &Registry,
    profile: &Profile,
    input: FeatureStageInput<'_>,
) -> Result<PublishResult> {
    let dataset_id = input.dataset_id;
    let class = registry
        .dataset(dataset_id)
        .map(|d| d.publication_class.clone())
        .with_context(|| format!("dataset {dataset_id:?} not in registry"))?;
    let (kept, outside) = validate(registry, profile, &input)?;
    let n_rows = i64::try_from(kept.len())?;
    let n = u64::try_from(kept.len())?;
    let schema = features_schema(registry, dataset_id);
    let mut multiset = MultisetHash::empty();
    for k in &kept {
        multiset.add(&k.hash);
    }
    let dsv = dsv(&schema, &multiset, n);

    let mut tx = pool.begin().await?;
    let (dataset_key, boundary_set): (i32, Option<String>) =
        sqlx::query_as("SELECT key, boundary_set FROM catalog.datasets WHERE id = $1")
            .bind(dataset_id)
            .fetch_one(&mut *tx)
            .await?;
    lock_dataset(&mut tx, dataset_key).await?;
    sqlx::query("SELECT geo.ensure_feature_partition($1)")
        .bind(dataset_id)
        .execute(&mut *tx)
        .await?;
    let current = current_full(&mut tx, dataset_id).await?;
    let unchanged = |version_id, dsv: String, note: String| PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome: "unchanged",
        data_version: None,
        rows: n_rows,
        rows_written: 0,
        rows_hashed: n,
        diff: None,
        note,
    };
    // Identical content: nothing is copied or written.
    if let Some(c) = &current
        && c.dsv == dsv
    {
        let r = unchanged(
            c.version_id,
            dsv,
            "content identical to the published version".to_owned(),
        );
        tx.rollback().await?;
        return Ok(r);
    }
    // Open or withdrawn versions with this content block a re-stage; a
    // superseded one does not (A → B → A, migration 0008); a rejected one is
    // re-staged.
    let existing: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, status FROM data.dataset_versions WHERE dsv = $1 AND dataset_id = $2
         ORDER BY status IN ('staged', 'in_review', 'approved') DESC, status = 'withdrawn' DESC, seq DESC",
    )
    .bind(&dsv)
    .bind(dataset_id)
    .fetch_all(&mut *tx)
    .await?;
    let rejected: Vec<i64> = existing
        .iter()
        .filter(|(_, s)| s == "rejected")
        .map(|(id, _)| *id)
        .collect();
    if let Some((vid, status)) = existing.into_iter().find(|(_, s)| {
        matches!(
            s.as_str(),
            "staged" | "in_review" | "approved" | "withdrawn"
        )
    }) {
        tx.rollback().await?;
        let mut r = unchanged(
            vid,
            dsv,
            format!("identical content already exists as a {status} version"),
        );
        if status == "in_review" || status == "approved" {
            r.outcome = if status == "in_review" {
                "in_review"
            } else {
                "approved"
            };
        }
        return Ok(r);
    }
    let open: Vec<(i64, i32)> = sqlx::query_as(
        "SELECT id, seq FROM data.dataset_versions
         WHERE dataset_id = $1 AND status IN ('staged', 'in_review', 'approved')",
    )
    .bind(dataset_id)
    .fetch_all(&mut *tx)
    .await?;
    for (vid, seq) in open {
        discard_open(
            &mut tx,
            dataset_key,
            vid,
            seq,
            "rejected",
            None,
            &format!("superseded by a newer staging of {dsv}"),
        )
        .await?;
    }

    copy_keys(&mut tx, &kept).await?;
    let cur_seq = current.as_ref().map_or(0, |c| c.seq);
    let diff = classify(&mut tx, dataset_key, cur_seq).await?;

    let seq: i32 = sqlx::query_scalar(
        "SELECT coalesce(max(seq), 0) + 1 FROM data.dataset_versions WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_one(&mut *tx)
    .await?;
    if !rejected.is_empty() {
        sqlx::query(
            "UPDATE data.dataset_versions SET dsv = dsv || '~rejected' || id::text WHERE id = ANY($1)",
        )
        .bind(&rejected)
        .execute(&mut *tx)
        .await?;
    }
    let version_id: i64 = sqlx::query_scalar(
        "INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status,
            publication_class, vintage, run_id, supersedes_id, seq, multiset_hash, schema_hash, boundary_set,
            git_rev, reproducible)
         VALUES ($1, $2, $3, $4, 'staged', $5::catalog.publication_class, $6, $7, $8, $9, $3, $10, $11, $12, true)
         RETURNING id",
    )
    .bind(dataset_id)
    .bind(&dsv)
    .bind(multiset.0.as_slice())
    .bind(n_rows)
    .bind(&class)
    .bind(input.vintage)
    .bind(input.run_id)
    .bind(current.as_ref().map(|c| c.version_id))
    .bind(seq)
    .bind(schema.as_slice())
    .bind(&boundary_set)
    .bind(lanka_core::git_rev())
    .fetch_one(&mut *tx)
    .await?;
    let set = boundary_set
        .clone()
        .unwrap_or_else(|| profile.reference_boundary_set().to_owned());
    let written = write_rows(
        &mut tx,
        profile,
        &set,
        dataset_key,
        version_id,
        seq,
        &kept,
        input.artifact_ids,
        input.extract_ids,
        input.grid_units,
    )
    .await?;
    let unassigned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM geo.features WHERE dataset_key = $1 AND vfrom = $2 AND unit_key IS NULL",
    )
    .bind(dataset_key)
    .bind(seq)
    .fetch_one(&mut *tx)
    .await?;

    let used: BTreeSet<i64> = kept
        .iter()
        .filter_map(|k| input.artifact_ids.get(k.row.artifact).copied())
        .collect();
    sqlx::query(
        "INSERT INTO data.dataset_version_artifacts (version_id, artifact_id) SELECT $1, unnest($2::bigint[])",
    )
    .bind(version_id)
    .bind(used.into_iter().collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;
    let used_extracts: BTreeSet<i64> = kept
        .iter()
        .filter_map(|k| {
            k.row
                .prov
                .and_then(|p| input.extract_ids.get(p.extract).copied())
        })
        .collect();
    sqlx::query(
        "INSERT INTO data.dataset_version_extracts (version_id, extract_id) SELECT $1, unnest($2::bigint[])",
    )
    .bind(version_id)
    .bind(used_extracts.into_iter().collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;

    let has_prev = current.is_some() && diff.prev_rows > 0;
    #[allow(clippy::cast_precision_loss)]
    let gate_failure = has_prev
        .then(|| {
            let prev = diff.prev_rows.max(1) as f64;
            if diff.rows_changed as f64 / prev > MAX_CHANGED_SHARE {
                Some(format!(
                    "{} features changed (> {:.0}%)",
                    diff.rows_changed,
                    MAX_CHANGED_SHARE * 100.0
                ))
            } else if diff.rows_removed as f64 / prev > MAX_REMOVED_SHARE {
                Some(format!(
                    "{} features removed (> {:.0}%)",
                    diff.rows_removed,
                    MAX_REMOVED_SHARE * 100.0
                ))
            } else {
                None
            }
        })
        .flatten()
        .or_else(|| (unassigned > 0).then(|| format!("{unassigned} features have no admin unit")));
    let validation = json!({
        "rows": n_rows, "rows_written": written, "rows_hashed": n,
        "outside_coverage": outside, "unassigned": unassigned,
        "gate_failure": gate_failure,
    });
    let diff_out = has_prev.then_some(diff);
    sqlx::query("UPDATE data.dataset_versions SET validation = $2, diff = $3 WHERE id = $1")
        .bind(version_id)
        .bind(&validation)
        .bind(diff_out.as_ref().map(|d| json!(d)))
        .execute(&mut *tx)
        .await?;
    let (outcome, note) = if class == "A" && gate_failure.is_none() {
        mark_approved(&mut tx, version_id, None, Some("auto-approved (class A)")).await?;
        (
            "approved",
            "auto-approved (class A); released by the next train".to_owned(),
        )
    } else {
        sqlx::query("UPDATE data.dataset_versions SET status = 'in_review' WHERE id = $1")
            .bind(version_id)
            .execute(&mut *tx)
            .await?;
        let why = gate_failure.unwrap_or_else(|| format!("class {class} requires human review"));
        open_review(&mut tx, version_id, &why).await?;
        (
            "in_review",
            format!("{why}; approve with `lanka review approve {dsv} --by <name>`"),
        )
    };
    tx.commit().await?;
    // dataset_key is an integer from the catalogue, not user input.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ANALYZE geo.features_{dataset_key}"
    )))
    .execute(pool)
    .await?;
    let mut note = note;
    if outside > 0 {
        note.push_str(&format!(
            "; {outside} feature(s) outside the country were dropped"
        ));
    }
    Ok(PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome,
        data_version: None,
        rows: n_rows,
        rows_written: written,
        rows_hashed: n,
        diff: diff_out,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_units_answer_only_interior_cells() {
        use lanka_grid::lookup::{MASK_EDGE, MASK_INTERIOR, MASK_NEAR, MASK_NONE};
        let g = GridUnits {
            boundary_set: "s".into(),
            gnd_idx: vec![1, 1, 2, 0, 2],
            land_mask: vec![
                MASK_INTERIOR,
                MASK_EDGE,
                MASK_INTERIOR,
                MASK_NONE,
                MASK_NEAR,
            ],
            slot_unit: vec![0, 101, 202],
        };
        let got: Vec<Option<i32>> = (0..6).map(|c| g.interior_unit(c)).collect();
        assert_eq!(got, [Some(101), None, Some(202), None, None, None]);
    }

    fn f(key: &str, geom: &str) -> FeatureIn {
        FeatureIn {
            key: key.into(),
            kind: "hospital".into(),
            name_en: Some("General Hospital".into()),
            name_si: None,
            name_ta: None,
            geometry: geom.into(),
            rep: [79.86, 6.92],
            attrs: serde_json::Map::new(),
            flags: Flags::EMPTY,
            entity_id: None,
            artifact: 0,
            prov: None,
        }
    }

    #[test]
    fn row_hash_covers_content_not_provenance() {
        let a = f("node/1", r#"{"type":"Point","coordinates":[79.86,6.92]}"#);
        let mut b = a.clone();
        b.artifact = 3;
        b.rep = [80.0, 7.0];
        assert_eq!(
            feature_hash(&a),
            feature_hash(&b),
            "rep and artifact are derived/provenance"
        );
        b.name_si = Some("x".into());
        assert_ne!(feature_hash(&a), feature_hash(&b));
        let mut c = a.clone();
        c.attrs.insert("beds".into(), json!(12));
        let mut d = a.clone();
        d.attrs.insert("beds".into(), json!(13));
        assert_ne!(feature_hash(&c), feature_hash(&d));
        let mut e = a.clone();
        e.flags = Flags::UNOFFICIAL_GEOM;
        assert_ne!(feature_hash(&a), feature_hash(&e));
    }
}
