//! Publishing reference geography: admin units (PostGIS) and crosswalks.
//!
//! Admin units are versioned per boundary set: a load computes a content
//! digest (codes, names, parents and the sha256 of each source geometry);
//! identical input is a no-op, otherwise the set is upserted in one
//! transaction (COPY → temp table → set-based INSERT … ON CONFLICT) and a
//! dataset version + release are recorded so API responses can cite the
//! exact boundary version they used.

use std::collections::HashMap;
use std::fmt::Write as _;

use anyhow::{Result, bail, ensure};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::connector::{AdminUnitIn, BoundarySet, CrosswalkRow, Fetched};
use crate::publish::{Diff, PublishResult, create_release, current_version, dsv_from_digest};

fn csv(out: &mut String, s: Option<&str>) {
    if let Some(v) = s {
        out.push('"');
        for c in v.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    }
}

/// Ancestor p-codes `[adm1, adm2, adm3]` for every unit, by walking parents.
fn ancestors(units: &[AdminUnitIn]) -> Result<HashMap<&str, [Option<&str>; 3]>> {
    let by_code: HashMap<&str, &AdminUnitIn> =
        units.iter().map(|u| (u.pcode.as_str(), u)).collect();
    let mut out = HashMap::with_capacity(units.len());
    for u in units {
        let mut anc: [Option<&str>; 3] = [None, None, None];
        let mut cur = u.parent_pcode.as_deref();
        let mut hops = 0;
        while let Some(p) = cur {
            let Some(pu) = by_code.get(p) else {
                bail!("unit {} has unknown parent {p}", u.pcode)
            };
            if (1..=3).contains(&pu.level) {
                anc[usize::try_from(pu.level - 1)?] = Some(pu.pcode.as_str());
            }
            ensure!(
                pu.level < u.level,
                "unit {} (level {}) has parent {p} at level {}",
                u.pcode,
                u.level,
                pu.level
            );
            cur = pu.parent_pcode.as_deref();
            hops += 1;
            ensure!(hops <= 4, "parent cycle at {}", u.pcode);
        }
        out.insert(u.pcode.as_str(), anc);
    }
    Ok(out)
}

pub async fn publish_admin_units(
    pool: &PgPool,
    run_id: i64,
    dataset_id: &str,
    set: &BoundarySet,
    units: &[AdminUnitIn],
    fetched: &[Fetched],
) -> Result<PublishResult> {
    ensure!(!units.is_empty(), "no admin units parsed");
    let roots = units.iter().filter(|u| u.level == 0).count();
    ensure!(roots == 1, "expected exactly one ADM0 unit, found {roots}");
    let anc = ancestors(units)?;

    // Content digest over sorted units.
    let mut order: Vec<&AdminUnitIn> = units.iter().collect();
    order.sort_by(|a, b| a.pcode.cmp(&b.pcode));
    let mut h = Sha256::new();
    let mut geom_hash: HashMap<&str, String> = HashMap::with_capacity(units.len());
    for u in &order {
        let g = hex::encode(Sha256::digest(u.geometry.get().as_bytes()));
        h.update(
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{g}\n",
                u.pcode,
                u.level,
                u.name_en,
                u.name_si.as_deref().unwrap_or_default(),
                u.name_ta.as_deref().unwrap_or_default(),
                u.parent_pcode.as_deref().unwrap_or_default()
            )
            .as_bytes(),
        );
        geom_hash.insert(u.pcode.as_str(), g);
    }
    let digest: [u8; 32] = h.finalize().into();
    let dsv = dsv_from_digest(&digest);
    let n = i64::try_from(units.len())?;

    let mut tx = pool.begin().await?;
    let current = current_version(&mut tx, dataset_id).await?;
    if current.as_ref().is_some_and(|(_, d)| *d == dsv) {
        tx.rollback().await?;
        return Ok(PublishResult {
            dataset_id: dataset_id.to_owned(),
            dsv,
            version_id: current.map_or(0, |c| c.0),
            outcome: "unchanged",
            data_version: None,
            rows: n,
            diff: None,
            note: "boundaries identical to the published version".to_owned(),
        });
    }

    sqlx::query(
        "INSERT INTO geo.boundary_sets (id, title, source_id, vintage, notes)
         VALUES ($1, $2, (SELECT source_id FROM ops.artifacts WHERE id = $3), $4, $5)
         ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, vintage = EXCLUDED.vintage, notes = EXCLUDED.notes",
    )
    .bind(&set.id)
    .bind(&set.title)
    .bind(fetched.first().map(|f| f.artifact_id))
    .bind(&set.vintage)
    .bind(&set.notes)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE TEMP TABLE admin_stage (
            pcode text, level smallint, name_en text, name_si text, name_ta text, parent_pcode text,
            adm1 text, adm2 text, adm3 text, geom_sha256 text, artifact_id bigint, geojson text
         ) ON COMMIT DROP",
    )
    .execute(&mut *tx)
    .await?;
    {
        let mut copy = tx
            .copy_in_raw("COPY admin_stage FROM STDIN (FORMAT csv)")
            .await?;
        let mut buf = String::with_capacity(8 << 20);
        for u in units {
            let a = anc.get(u.pcode.as_str()).copied().unwrap_or_default();
            csv(&mut buf, Some(&u.pcode));
            let _ = write!(buf, ",{},", u.level);
            csv(&mut buf, Some(&u.name_en));
            buf.push(',');
            csv(&mut buf, u.name_si.as_deref());
            buf.push(',');
            csv(&mut buf, u.name_ta.as_deref());
            buf.push(',');
            csv(&mut buf, u.parent_pcode.as_deref());
            for x in a {
                buf.push(',');
                csv(&mut buf, x);
            }
            buf.push(',');
            csv(
                &mut buf,
                geom_hash.get(u.pcode.as_str()).map(String::as_str),
            );
            buf.push(',');
            if let Some(f) = fetched.get(u.artifact) {
                let _ = write!(buf, "{}", f.artifact_id);
            }
            buf.push(',');
            csv(&mut buf, Some(u.geometry.get()));
            buf.push('\n');
            if buf.len() > (8 << 20) - (1 << 16) {
                copy.send(buf.as_bytes()).await?;
                buf.clear();
            }
        }
        if !buf.is_empty() {
            copy.send(buf.as_bytes()).await?;
        }
        copy.finish().await?;
    }

    // Diff against what is stored for this boundary set, before overwriting.
    let (added, removed, changed, prev): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE u.pcode IS NULL),
                count(*) FILTER (WHERE s.pcode IS NULL),
                count(*) FILTER (WHERE s.pcode IS NOT NULL AND u.pcode IS NOT NULL AND (
                    u.name_en IS DISTINCT FROM s.name_en OR u.name_si IS DISTINCT FROM s.name_si
                    OR u.name_ta IS DISTINCT FROM s.name_ta OR u.parent_pcode IS DISTINCT FROM s.parent_pcode
                    OR u.geom_sha256 IS DISTINCT FROM decode(s.geom_sha256, 'hex'))),
                count(u.pcode)
         FROM admin_stage s
         FULL JOIN (SELECT * FROM geo.admin_units WHERE boundary_set = $1) u ON u.pcode = s.pcode",
    )
    .bind(&set.id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO geo.admin_units AS t (pcode, level, name_en, name_si, name_ta, parent_pcode, adm1_pcode,
            adm2_pcode, adm3_pcode, boundary_set, area_km2, centroid, bbox, geom, geom_simplified, geom_sha256,
            artifact_id, updated_at)
         SELECT s.pcode, s.level, s.name_en, s.name_si, s.name_ta, s.parent_pcode, s.adm1, s.adm2, s.adm3, $1,
                ST_Area(g.geom::geography) / 1e6, ST_PointOnSurface(g.geom), Box2D(g.geom), g.geom,
                ST_Multi(ST_SimplifyPreserveTopology(g.geom, 0.0005)), decode(s.geom_sha256, 'hex'),
                s.artifact_id, now()
         FROM admin_stage s
         CROSS JOIN LATERAL (SELECT ST_SetSRID(ST_GeomFromGeoJSON(s.geojson), 4326) AS raw) r
         CROSS JOIN LATERAL (SELECT CASE WHEN ST_IsValid(r.raw) THEN ST_Multi(r.raw)
                                         ELSE ST_Multi(ST_CollectionExtract(ST_MakeValid(r.raw), 3)) END AS geom) g
         -- Only (re)build geometry for new or changed units: the PostGIS work
         -- dominates the load, and most reloads change nothing.
         WHERE NOT EXISTS (SELECT 1 FROM geo.admin_units x WHERE x.pcode = s.pcode
                 AND x.geom_sha256 = decode(s.geom_sha256, 'hex') AND x.name_en = s.name_en
                 AND x.name_si IS NOT DISTINCT FROM s.name_si AND x.name_ta IS NOT DISTINCT FROM s.name_ta
                 AND x.parent_pcode IS NOT DISTINCT FROM s.parent_pcode AND x.boundary_set = $1)
         ON CONFLICT (pcode) DO UPDATE SET level = EXCLUDED.level, name_en = EXCLUDED.name_en,
            name_si = EXCLUDED.name_si, name_ta = EXCLUDED.name_ta, parent_pcode = EXCLUDED.parent_pcode,
            adm1_pcode = EXCLUDED.adm1_pcode, adm2_pcode = EXCLUDED.adm2_pcode, adm3_pcode = EXCLUDED.adm3_pcode,
            boundary_set = EXCLUDED.boundary_set, area_km2 = EXCLUDED.area_km2, centroid = EXCLUDED.centroid,
            bbox = EXCLUDED.bbox, geom = EXCLUDED.geom, geom_simplified = EXCLUDED.geom_simplified,
            geom_sha256 = EXCLUDED.geom_sha256, artifact_id = EXCLUDED.artifact_id, updated_at = now()"

    )
    .bind(&set.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM geo.admin_units u WHERE u.boundary_set = $1
         AND NOT EXISTS (SELECT 1 FROM admin_stage s WHERE s.pcode = u.pcode)",
    )
    .bind(&set.id)
    .execute(&mut *tx)
    .await?;

    let version_id: i64 = sqlx::query_scalar(
        "INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status, publication_class,
            vintage, run_id, supersedes_id, diff)
         VALUES ($1, $2, $3, $4, 'staged', 'A', $5, $6, $7, $8)
         ON CONFLICT (dsv) DO UPDATE SET run_id = EXCLUDED.run_id RETURNING id",
    )
    .bind(dataset_id)
    .bind(&dsv)
    .bind(digest.as_slice())
    .bind(n)
    .bind(&set.vintage)
    .bind(run_id)
    .bind(current.as_ref().map(|c| c.0))
    .bind(serde_json::json!({ "rows_added": added, "rows_removed": removed, "rows_changed": changed, "prev_rows": prev }))
    .fetch_one(&mut *tx)
    .await?;
    let ids: Vec<i64> = fetched.iter().map(|f| f.artifact_id).collect();
    sqlx::query(
        "INSERT INTO data.dataset_version_artifacts (version_id, artifact_id) SELECT $1, unnest($2::bigint[])
         ON CONFLICT DO NOTHING",
    )
    .bind(version_id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    let diff = current.as_ref().map(|_| Diff {
        rows_added: added,
        rows_removed: removed,
        rows_changed: changed,
        prev_rows: prev,
    });
    let dv = create_release(
        &mut tx,
        dataset_id,
        version_id,
        current.as_ref().map(|c| c.0),
        diff.as_ref(),
        &format!("{dataset_id} → {dsv} (boundary set {})", set.id),
        Some(run_id),
    )
    .await?;
    tx.commit().await?;
    sqlx::query("ANALYZE geo.admin_units").execute(pool).await?;
    Ok(PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome: "published",
        data_version: Some(dv),
        rows: n,
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
        "INSERT INTO geo.crosswalks (from_set, from_code, to_set, to_code, weight, method, source_id, updated_at)
         SELECT f, fc, t, tc, w, m, $7, now()
         FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::float8[], $6::text[]) AS x(f, fc, t, tc, w, m)
         ON CONFLICT (from_set, from_code, to_set, to_code) DO UPDATE
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
