//! Provenance of derived observations (04 §6, ADR 0005): derived rows have
//! no `data.obs_provenance`; their lineage is the `data.derivations` row of
//! the version that wrote them plus the method's `explain`, re-run over the
//! exact input versions the derivation pinned.

use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result};
use lanka_core::{Calendars, Freq};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::method::RowKey;
use crate::methods::Methods;
use crate::pg::{PgInputs, Pinned};
use crate::recipe::RecipeSpec;
use crate::runner;

/// Where explain reads the reference geography from.
#[derive(Debug, Clone)]
pub struct GeoContext {
    pub calendars: Calendars,
    pub reference_set: String,
    pub scheme: String,
    /// Blob store root (grid inputs of `grid_zonal` derivations).
    pub store_root: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivationRecord {
    pub id: i64,
    pub version_id: i64,
    pub dsv: String,
    pub recipe_id: String,
    pub fingerprint: String,
    pub outcome: String,
    pub methods: Value,
    pub inputs: Vec<Pinned>,
    pub tools: Value,
    pub git_rev: String,
    pub engine_version: String,
    pub report: Value,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip)]
    pub spec: Value,
}

type DerivRow = (
    i64,
    i64,
    String,
    String,
    Vec<u8>,
    String,
    Value,
    Value,
    Value,
    Value,
    String,
    String,
    Value,
    Option<chrono::DateTime<chrono::Utc>>,
);

/// The derivation that wrote dataset version `version_id` (the latest one
/// that produced new rows; a re-run with identical output reuses the
/// version and adds an `unchanged` record).
pub async fn derivation_of_version(
    pool: &PgPool,
    version_id: i64,
) -> Result<Option<DerivationRecord>> {
    let r: Option<DerivRow> = sqlx::query_as(
        "SELECT d.id, d.version_id, v.dsv, d.recipe_id, d.fingerprint, d.outcome, d.methods, d.params,
                d.inputs, d.tools, d.git_rev, d.engine_version, d.report, d.finished_at
         FROM data.derivations d JOIN data.dataset_versions v ON v.id = d.version_id
         WHERE d.version_id = $1 ORDER BY d.outcome = 'new' DESC, d.id DESC LIMIT 1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    r.map(|r| {
        Ok(DerivationRecord {
            id: r.0,
            version_id: r.1,
            dsv: r.2,
            recipe_id: r.3,
            fingerprint: hex::encode(r.4),
            outcome: r.5,
            methods: r.6,
            spec: r.7,
            inputs: serde_json::from_value(r.8).context("derivation inputs")?,
            tools: r.9,
            git_rev: r.10,
            engine_version: r.11,
            report: r.12,
            finished_at: r.13,
        })
    })
    .transpose()
}

/// Seed of a run: the first 8 bytes of the fingerprint.
#[must_use]
pub fn seed_of(fingerprint: &[u8]) -> u64 {
    let mut b = [0u8; 8];
    for (d, s) in b.iter_mut().zip(fingerprint) {
        *d = *s;
    }
    u64::from_be_bytes(b)
}

type ObsInfo = (
    i32,
    String,
    String,
    String,
    String,
    chrono::NaiveDate,
    String,
    i64,
);

/// Lineage of one observation: `None` when it is not derived. With
/// `depth > 1`, contributors that are themselves derived are explained too.
pub fn explain_obs<'a>(
    pool: &'a PgPool,
    geo: &'a GeoContext,
    methods: &'a Methods,
    obs_id: i64,
    depth: u8,
) -> Pin<Box<dyn Future<Output = Result<Option<Value>>> + Send + 'a>> {
    Box::pin(async move {
        let dataset_key = i32::try_from(obs_id >> 40).context("obs_id")?;
        let info: Option<ObsInfo> = sqlx::query_as(
            "SELECT o.dataset_key, d.id, i.id, u.code, o.freq::text, o.time_start, ds.dims, v.id
             FROM data.observations o
             JOIN catalog.datasets d ON d.key = o.dataset_key
             JOIN catalog.indicators i ON i.key = o.indicator_key
             JOIN geo.units u ON u.key = o.geo_key
             JOIN catalog.dim_sets ds ON ds.key = o.dims_key
             JOIN data.dataset_versions v ON v.dataset_id = d.id AND v.seq = o.vfrom
             WHERE o.dataset_key = $1 AND o.obs_id = $2",
        )
        .bind(dataset_key)
        .bind(obs_id)
        .fetch_optional(pool)
        .await?;
        let Some((_, dataset_id, indicator, geo_code, freq, start, dims, version_id)) = info else {
            return Ok(None);
        };
        let Some(d) = derivation_of_version(pool, version_id).await? else {
            return Ok(None);
        };
        let spec: RecipeSpec =
            serde_json::from_value(d.spec.clone()).context("derivation recipe snapshot")?;
        let inputs = PgInputs::new(
            pool.clone(),
            geo.calendars.clone(),
            &geo.reference_set,
            &geo.scheme,
            d.inputs.clone(),
        )
        .await?
        .with_store_root(geo.store_root.as_deref());
        let key = RowKey {
            indicator,
            geo: geo_code,
            freq: freq
                .chars()
                .next()
                .and_then(Freq::from_char)
                .context("freq")?,
            start,
            dims,
        };
        let fp = hex::decode(&d.fingerprint).unwrap_or_default();
        let explanation = runner::explain(&spec, methods, &inputs, seed_of(&fp), &key).await?;
        let mut contributors = serde_json::to_value(&explanation.contributors)?;
        if depth > 1
            && let Some(list) = contributors.as_array_mut()
        {
            for c in list.iter_mut().take(25) {
                if let Some(id) = c.get("obs_id").and_then(Value::as_i64)
                    && let Some(p) = explain_obs(pool, geo, methods, id, depth - 1).await?
                {
                    c["provenance"] = p;
                }
            }
        }
        Ok(Some(json!({
            "dataset_id": dataset_id,
            "derivation": {
                "recipe": d.recipe_id,
                "dsv": d.dsv,
                "version_id": d.version_id,
                "fingerprint": d.fingerprint,
                "methods": d.methods,
                "params": spec.steps.iter().map(|s| json!({"step": s.id, "method": s.method, "with": s.with})).collect::<Vec<_>>(),
                "inputs": d.inputs.iter().map(|p| json!({"port": p.port, "dataset": p.dataset_id, "dsv": p.dsv, "version_id": p.version_id})).collect::<Vec<_>>(),
                "tools": d.tools,
                "git_rev": d.git_rev,
                "engine_version": d.engine_version,
                "finished_at": d.finished_at,
            },
            "method": explanation.method,
            "formula": explanation.formula,
            "value": explanation.value,
            "contributors": contributors,
            "contributors_truncated": explanation.truncated,
        })))
    })
}
