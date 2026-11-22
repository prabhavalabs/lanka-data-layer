//! Publishing reference geography: admin units (`geo.units` +
//! `geo.unit_geoms`, PostGIS) and crosswalks.
//!
//! Admin units are versioned per boundary set: a load computes a multiset
//! hash over canonical unit rows (codes, names, parents and the sha256 of
//! each source geometry); identical input is a no-op, otherwise the set is
//! upserted in one transaction (COPY → temp table → set-based upserts; only
//! new or changed geometry is rebuilt) and a dataset version + release are
//! recorded so API responses can cite the exact boundary version they used.
//! Units that leave the reference set are retired (`valid_to`), never deleted.

use std::collections::HashMap;

use anyhow::{Result, bail, ensure};
use lanka_core::Profile;
use lanka_core::dsv::{MultisetHash, dsv};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::connector::{AdminUnitIn, BoundarySet, CrosswalkRow};
use crate::copy_bin::BinCopy;
use crate::publish::{
    Diff, PublishResult, boundary_schema, current_version, mark_approved, unit_row_bytes,
};

/// Ancestor codes (root → parent) for every unit, by walking parents.
fn ancestors(units: &[AdminUnitIn], max_depth: usize) -> Result<HashMap<&str, Vec<&str>>> {
    let by_code: HashMap<&str, &AdminUnitIn> =
        units.iter().map(|u| (u.pcode.as_str(), u)).collect();
    let mut out = HashMap::with_capacity(units.len());
    for u in units {
        let mut anc: Vec<&str> = Vec::new();
        let mut cur = u.parent_pcode.as_deref();
        while let Some(p) = cur {
            let Some(pu) = by_code.get(p) else {
                bail!("unit {} has unknown parent {p}", u.pcode)
            };
            ensure!(
                pu.level < u.level,
                "unit {} (level {}) has parent {p} at level {}",
                u.pcode,
                u.level,
                pu.level
            );
            anc.push(pu.pcode.as_str());
            ensure!(anc.len() <= max_depth, "parent cycle at {}", u.pcode);
            cur = pu.parent_pcode.as_deref();
        }
        anc.reverse();
        out.insert(u.pcode.as_str(), anc);
    }
    Ok(out)
}

/// Canonical multiset of a unit list (same encoding as the hash backfill).
fn units_multiset(units: &[AdminUnitIn]) -> (MultisetHash, HashMap<&str, String>) {
    let mut geom_hash: HashMap<&str, String> = HashMap::with_capacity(units.len());
    let mut m = MultisetHash::empty();
    for u in units {
        let g = hex::encode(Sha256::digest(u.geometry.get().as_bytes()));
        m.add(
            &Sha256::digest(unit_row_bytes(
                &u.pcode,
                u.level,
                [
                    Some(u.name_en.as_str()),
                    u.name_si.as_deref(),
                    u.name_ta.as_deref(),
                ],
                u.parent_pcode.as_deref(),
                &g,
            ))
            .into(),
        );
        geom_hash.insert(u.pcode.as_str(), g);
    }
    (m, geom_hash)
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn publish_admin_units(
    pool: &PgPool,
    profile: &Profile,
    run_id: i64,
    dataset_id: &str,
    set: &BoundarySet,
    units: &[AdminUnitIn],
    artifact_ids: &[i64],
    extract_ids: &[i64],
) -> Result<PublishResult> {
    ensure!(!units.is_empty(), "no admin units parsed");
    let roots: Vec<&AdminUnitIn> = units.iter().filter(|u| u.level == 0).collect();
    ensure!(
        roots.len() == 1,
        "expected exactly one level-0 unit, found {}",
        roots.len()
    );
    ensure!(
        roots[0].pcode == profile.admin.root(),
        "level-0 unit is {:?}, the profile's root code is {:?}",
        roots[0].pcode,
        profile.admin.root()
    );
    for u in units {
        let p = profile.admin.parse(&u.pcode)?;
        ensure!(
            i16::from(p.level()) == u.level && p.as_str() == u.pcode,
            "unit {} is not a canonical level-{} p-code",
            u.pcode,
            u.level
        );
    }
    let anc = ancestors(units, usize::from(profile.admin.max_level()))?;
    let scheme = profile.admin_scheme();
    let (multiset, geom_hash) = units_multiset(units);
    let schema = boundary_schema(&set.id, scheme);
    let n = i64::try_from(units.len())?;
    let dsv = dsv(&schema, &multiset, u64::try_from(units.len())?);

    let mut tx = pool.begin().await?;
    let current = current_version(&mut tx, dataset_id).await?;
    if let Some((vid, cur, _)) = &current
        && *cur == dsv
    {
        let vid = *vid;
        tx.rollback().await?;
        return Ok(PublishResult {
            dataset_id: dataset_id.to_owned(),
            dsv,
            version_id: vid,
            outcome: "unchanged",
            data_version: None,
            rows: n,
            rows_written: 0,
            rows_hashed: u64::try_from(units.len())?,
            diff: None,
            note: "boundaries identical to the published version".to_owned(),
        });
    }

    sqlx::query(
        "INSERT INTO geo.boundary_sets (id, title, source_id, vintage, notes, scheme_id)
         VALUES ($1, $2, (SELECT source_id FROM ops.artifacts WHERE id = $3), $4, $5, $6)
         ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, vintage = EXCLUDED.vintage,
           notes = EXCLUDED.notes, source_id = EXCLUDED.source_id",
    )
    .bind(&set.id)
    .bind(&set.title)
    .bind(artifact_ids.first().copied())
    .bind(&set.vintage)
    .bind(&set.notes)
    .bind(scheme)
    .execute(&mut *tx)
    .await?;
    // The profile's reference set becomes the reference when none is set.
    sqlx::query(
        "UPDATE geo.boundary_sets SET is_reference = true WHERE id = $1 AND id = $2
           AND NOT EXISTS (SELECT 1 FROM geo.boundary_sets WHERE scheme_id = $3 AND is_reference)",
    )
    .bind(&set.id)
    .bind(profile.reference_boundary_set())
    .bind(scheme)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TEMP TABLE admin_stage (
            code text, level smallint, name_en text, name_si text, name_ta text, parent text,
            anc text, geom_sha256 text, artifact_id bigint, geojson text
         ) ON COMMIT DROP",
    )
    .execute(&mut *tx)
    .await?;
    {
        let mut copy = tx
            .copy_in_raw("COPY admin_stage FROM STDIN (FORMAT binary)")
            .await?;
        let mut b = BinCopy::new();
        for u in units {
            let a = anc
                .get(u.pcode.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default();
            b.row(10);
            b.text(Some(&u.pcode));
            b.i16(u.level);
            b.text(Some(&u.name_en));
            b.text(u.name_si.as_deref());
            b.text(u.name_ta.as_deref());
            b.text(u.parent_pcode.as_deref());
            b.text(Some(&a.join(",")));
            b.text(geom_hash.get(u.pcode.as_str()).map(String::as_str));
            b.opt_i64(artifact_ids.get(u.artifact).copied());
            b.text(Some(u.geometry.get()));
            if b.len() > 8 << 20 {
                copy.send(b.take()).await?;
            }
        }
        b.finish();
        copy.send(b.take()).await?;
        copy.finish().await?;
    }

    // Diff against what is stored for this boundary set, before overwriting.
    let (added, removed, changed, prev): (i64, i64, i64, i64) = sqlx::query_as(
        "WITH cur AS (
           SELECT u.code, u.name_en, u.name_si, u.name_ta, p.code AS parent, g.geom_sha256
           FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
           LEFT JOIN geo.units p ON p.key = u.parent_key
           WHERE g.boundary_set = $1)
         SELECT count(*) FILTER (WHERE c.code IS NULL),
                count(*) FILTER (WHERE s.code IS NULL),
                count(*) FILTER (WHERE s.code IS NOT NULL AND c.code IS NOT NULL AND (
                    c.name_en IS DISTINCT FROM s.name_en OR c.name_si IS DISTINCT FROM s.name_si
                    OR c.name_ta IS DISTINCT FROM s.name_ta OR c.parent IS DISTINCT FROM s.parent
                    OR c.geom_sha256 IS DISTINCT FROM decode(s.geom_sha256, 'hex'))),
                count(c.code)
         FROM admin_stage s FULL JOIN cur c ON c.code = s.code",
    )
    .bind(&set.id)
    .fetch_one(&mut *tx)
    .await?;

    // Units: insert new codes, refresh names/levels, un-retire returning ones.
    sqlx::query(
        "INSERT INTO geo.units (code, scheme_id, level, name_en, name_si, name_ta)
         SELECT code, $1, level, name_en, name_si, name_ta FROM admin_stage ORDER BY level, code
         ON CONFLICT (code) DO UPDATE SET level = EXCLUDED.level, name_en = EXCLUDED.name_en,
            name_si = EXCLUDED.name_si, name_ta = EXCLUDED.name_ta, valid_to = NULL, updated_at = now()
         WHERE (geo.units.level, geo.units.name_en, geo.units.name_si, geo.units.name_ta, geo.units.valid_to)
               IS DISTINCT FROM (EXCLUDED.level, EXCLUDED.name_en, EXCLUDED.name_si, EXCLUDED.name_ta, NULL::date)",
    )
    .bind(scheme)
    .execute(&mut *tx)
    .await?;
    // Parent and ancestors in one statement (units_parent_anc is per row).
    sqlx::query(
        "UPDATE geo.units u SET parent_key = x.parent_key, anc_keys = x.anc_keys, updated_at = now()
         FROM (SELECT s.code, p.key AS parent_key,
                      ARRAY(SELECT a.key FROM unnest(string_to_array(nullif(s.anc, ''), ',')) WITH ORDINALITY AS t(c, i)
                            JOIN geo.units a ON a.code = t.c ORDER BY t.i) AS anc_keys
               FROM admin_stage s LEFT JOIN geo.units p ON p.code = s.parent) x
         WHERE u.code = x.code
           AND (u.parent_key, u.anc_keys) IS DISTINCT FROM (x.parent_key, x.anc_keys)",
    )
    .execute(&mut *tx)
    .await?;
    // Geometry: only (re)build new or changed shapes — the PostGIS work
    // dominates the load, and most reloads change nothing.
    sqlx::query(
        "INSERT INTO geo.unit_geoms AS t (unit_key, boundary_set, geom, geom_simplified, area_km2, centroid,
            bbox, geom_sha256, artifact_id, updated_at)
         SELECT u.key, $1, g.geom, ST_Multi(ST_SimplifyPreserveTopology(g.geom, 0.0005)),
                ST_Area(g.geom::geography) / 1e6, ST_PointOnSurface(g.geom), Box2D(g.geom),
                decode(s.geom_sha256, 'hex'), s.artifact_id, now()
         FROM admin_stage s
         JOIN geo.units u ON u.code = s.code
         CROSS JOIN LATERAL (SELECT ST_SetSRID(ST_GeomFromGeoJSON(s.geojson), 4326) AS raw) r
         CROSS JOIN LATERAL (SELECT CASE WHEN ST_IsValid(r.raw) THEN ST_Multi(r.raw)
                                         ELSE ST_Multi(ST_CollectionExtract(ST_MakeValid(r.raw), 3)) END AS geom) g
         WHERE NOT EXISTS (SELECT 1 FROM geo.unit_geoms x WHERE x.unit_key = u.key AND x.boundary_set = $1
                             AND x.geom_sha256 = decode(s.geom_sha256, 'hex'))
         ON CONFLICT (unit_key, boundary_set) DO UPDATE SET geom = EXCLUDED.geom,
            geom_simplified = EXCLUDED.geom_simplified, area_km2 = EXCLUDED.area_km2,
            centroid = EXCLUDED.centroid, bbox = EXCLUDED.bbox, geom_sha256 = EXCLUDED.geom_sha256,
            artifact_id = EXCLUDED.artifact_id, updated_at = now()",
    )
    .bind(&set.id)
    .execute(&mut *tx)
    .await?;
    // Units no longer in this set: drop their geometry here; retire them
    // when this is the reference set (they stay resolvable, with valid_to).
    let gone: Vec<i32> = sqlx::query_scalar(
        "DELETE FROM geo.unit_geoms g USING geo.units u
         WHERE g.boundary_set = $1 AND u.key = g.unit_key
           AND NOT EXISTS (SELECT 1 FROM admin_stage s WHERE s.code = u.code)
         RETURNING g.unit_key",
    )
    .bind(&set.id)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE geo.units SET valid_to = current_date, updated_at = now()
         WHERE key = ANY($1) AND valid_to IS NULL
           AND EXISTS (SELECT 1 FROM geo.boundary_sets WHERE id = $2 AND is_reference)",
    )
    .bind(&gone)
    .bind(&set.id)
    .execute(&mut *tx)
    .await?;

    // One open version per dataset: a newer staging supersedes an older one.
    sqlx::query(
        "UPDATE data.dataset_versions SET status = 'rejected',
            review_note = 'superseded by a newer staging of ' || $2
         WHERE dataset_id = $1 AND dsv <> $2 AND status IN ('staged', 'in_review', 'approved')",
    )
    .bind(dataset_id)
    .bind(&dsv)
    .execute(&mut *tx)
    .await?;
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM data.dataset_versions WHERE dsv = $1")
            .bind(&dsv)
            .fetch_optional(&mut *tx)
            .await?;
    let version_id: i64 = match existing {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status,
                    publication_class, vintage, run_id, supersedes_id, diff, seq, multiset_hash, schema_hash,
                    boundary_set, git_rev)
                 VALUES ($1, $2, $3, $4, 'staged', 'A', $5, $6, $7, $8,
                         (SELECT coalesce(max(seq), 0) + 1 FROM data.dataset_versions WHERE dataset_id = $1),
                         $3, $9, $10, $11)
                 RETURNING id",
            )
            .bind(dataset_id)
            .bind(&dsv)
            .bind(multiset.0.as_slice())
            .bind(n)
            .bind(&set.vintage)
            .bind(run_id)
            .bind(current.as_ref().map(|c| c.0))
            .bind(serde_json::json!({ "rows_added": added, "rows_removed": removed, "rows_changed": changed,
                                      "rows_restated": 0, "prev_rows": prev }))
            .bind(schema.as_slice())
            .bind(&set.id)
            .bind(lanka_core::git_rev())
            .fetch_one(&mut *tx)
            .await?
        }
    };
    sqlx::query(
        "INSERT INTO data.dataset_version_artifacts (version_id, artifact_id) SELECT $1, unnest($2::bigint[])
         ON CONFLICT DO NOTHING",
    )
    .bind(version_id)
    .bind(artifact_ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO data.dataset_version_extracts (version_id, extract_id) SELECT $1, unnest($2::bigint[])
         ON CONFLICT DO NOTHING",
    )
    .bind(version_id)
    .bind(extract_ids)
    .execute(&mut *tx)
    .await?;
    let diff = current.as_ref().map(|_| Diff {
        rows_added: added,
        rows_removed: removed,
        rows_changed: changed,
        rows_restated: 0,
        prev_rows: prev,
    });
    // Boundary versions are class A: approved here, released by the next
    // train (geo.units / unit_geoms themselves are updated in place until
    // boundary vintages are release-pinned, WP-10).
    mark_approved(
        &mut tx,
        version_id,
        None,
        Some("auto-approved (boundaries)"),
    )
    .await?;
    tx.commit().await?;
    sqlx::query("ANALYZE geo.units").execute(pool).await?;
    sqlx::query("ANALYZE geo.unit_geoms").execute(pool).await?;
    Ok(PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome: "approved",
        data_version: None,
        rows: n,
        rows_written: added + changed,
        rows_hashed: u64::try_from(units.len())?,
        diff,
        note: format!("{n} units in boundary set {}", set.id),
    })
}

/// Upserts crosswalk rows (name matches, boundary splits).
pub async fn upsert_crosswalks(
    pool: &PgPool,
    source_id: &str,
    rows: &[CrosswalkRow],
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }
    let col = |f: fn(&CrosswalkRow) -> String| rows.iter().map(f).collect::<Vec<String>>();
    let res = sqlx::query(
        "INSERT INTO geo.crosswalks (from_set, from_code, to_set, to_code, weight, weight_kind, method,
            source_id, updated_at)
         SELECT f, fc, t, tc, w, CASE WHEN w = 1 THEN 'identity' ELSE 'manual' END, m, $7, now()
         FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::float8[], $6::text[]) AS x(f, fc, t, tc, w, m)
         ON CONFLICT (from_set, from_code, to_set, to_code, weight_kind) DO UPDATE
           SET weight = EXCLUDED.weight, method = EXCLUDED.method, source_id = EXCLUDED.source_id, updated_at = now()",
    )
    .bind(col(|r| r.from_set.clone()))
    .bind(col(|r| r.from_code.clone()))
    .bind(col(|r| r.to_set.clone()))
    .bind(col(|r| r.to_code.clone()))
    .bind(rows.iter().map(|r| r.weight).collect::<Vec<f64>>())
    .bind(col(|r| r.method.to_owned()))
    .bind(source_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
