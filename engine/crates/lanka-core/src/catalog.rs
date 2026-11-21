//! In-memory snapshot of the catalog and the current release.
//!
//! The catalog is small (hundreds of rows), read on every request and only
//! changes on registry sync or release publish, so transports hold one
//! immutable snapshot behind an `Arc` and swap it on `NOTIFY lanka_release`.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::envelope::{DatasetVersionRef, LicenceRef, Link, Meta, SourceRef};
use crate::error::CoreResult;

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct LicenceInfo {
    pub id: String,
    pub status: String,
    pub url: Option<String>,
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct SourceInfo {
    pub id: String,
    pub publisher: String,
    pub title: String,
    pub homepage: String,
    pub licence_id: String,
    pub cadence: String,
    pub access_method: String,
    pub themes: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct IndicatorInfo {
    pub id: String,
    pub dataset_id: String,
    pub label: String,
    pub unit: String,
    pub dims: Vec<String>,
    pub geo_levels: Vec<String>,
    pub period_type: String,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct CurrentVersion {
    #[serde(skip)]
    pub version_id: i64,
    pub dsv: String,
    pub vintage: Option<String>,
    pub row_count: i64,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
struct DatasetRow {
    id: String,
    title: String,
    description: String,
    method: String,
    limits: Vec<String>,
    publication_class: String,
    time_from: Option<String>,
    time_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DatasetInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub method: String,
    pub limits: Vec<String>,
    pub publication_class: String,
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub source_ids: Vec<String>,
    pub indicator_ids: Vec<String>,
    pub current: Option<CurrentVersion>,
}

/// Immutable snapshot; cheap to share, rebuilt on change.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub data_version: Option<String>,
    pub datasets: HashMap<String, DatasetInfo>,
    pub indicators: HashMap<String, IndicatorInfo>,
    pub sources: HashMap<String, SourceInfo>,
    pub licences: HashMap<String, LicenceInfo>,
    /// Datasets in stable (id) order, for listings.
    pub dataset_order: Vec<String>,
}

impl Catalog {
    /// Loads the catalog and current release in one short read transaction
    /// (REPEATABLE READ so the snapshot is consistent).
    pub async fn load(pool: &PgPool) -> CoreResult<Self> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;

        let licences: Vec<LicenceInfo> = sqlx::query_as(
            "SELECT id, status::text AS status, url, attribution FROM catalog.licences",
        )
        .fetch_all(&mut *tx)
        .await?;
        let sources: Vec<SourceInfo> = sqlx::query_as(
            "SELECT id, publisher, title, homepage, licence_id, cadence, access_method, themes, status
             FROM catalog.sources",
        )
        .fetch_all(&mut *tx)
        .await?;
        let rows: Vec<DatasetRow> = sqlx::query_as(
            "SELECT id, title, description, method, limits, publication_class::text AS publication_class,
                    time_from, time_to
             FROM catalog.datasets ORDER BY id",
        )
        .fetch_all(&mut *tx)
        .await?;
        let links: Vec<(String, String)> = sqlx::query_as(
            "SELECT dataset_id, source_id FROM catalog.dataset_sources ORDER BY 1, 2",
        )
        .fetch_all(&mut *tx)
        .await?;
        let indicators: Vec<IndicatorInfo> = sqlx::query_as(
            "SELECT id, dataset_id, label, unit, dims, geo_levels, period_type
             FROM catalog.indicators ORDER BY dataset_id, sort, id",
        )
        .fetch_all(&mut *tx)
        .await?;
        let data_version: Option<String> =
            sqlx::query_scalar("SELECT data_version FROM data.current_release")
                .fetch_optional(&mut *tx)
                .await?;
        let current: Vec<(String, CurrentVersionRow)> = sqlx::query_as::<_, CurrentVersionRow>(
            "SELECT dataset_id, version_id, dsv, vintage, row_count, published_at FROM data.current_versions",
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.dataset_id.clone(), r))
        .collect();
        tx.commit().await?;

        let mut current: HashMap<String, CurrentVersionRow> = current.into_iter().collect();
        let mut source_ids: HashMap<String, Vec<String>> = HashMap::new();
        for (d, s) in links {
            source_ids.entry(d).or_default().push(s);
        }
        let mut indicator_ids: HashMap<String, Vec<String>> = HashMap::new();
        for i in &indicators {
            indicator_ids
                .entry(i.dataset_id.clone())
                .or_default()
                .push(i.id.clone());
        }

        let dataset_order = rows.iter().map(|r| r.id.clone()).collect();
        let datasets = rows
            .into_iter()
            .map(|r| {
                let cur = current.remove(&r.id).map(|c| CurrentVersion {
                    version_id: c.version_id,
                    dsv: c.dsv,
                    vintage: c.vintage,
                    row_count: c.row_count,
                    published_at: c.published_at,
                });
                let info = DatasetInfo {
                    source_ids: source_ids.remove(&r.id).unwrap_or_default(),
                    indicator_ids: indicator_ids.remove(&r.id).unwrap_or_default(),
                    current: cur,
                    id: r.id,
                    title: r.title,
                    description: r.description,
                    method: r.method,
                    limits: r.limits,
                    publication_class: r.publication_class,
                    time_from: r.time_from,
                    time_to: r.time_to,
                };
                (info.id.clone(), info)
            })
            .collect();

        Ok(Self {
            data_version,
            datasets,
            indicators: indicators.into_iter().map(|i| (i.id.clone(), i)).collect(),
            sources: sources.into_iter().map(|s| (s.id.clone(), s)).collect(),
            licences: licences.into_iter().map(|l| (l.id.clone(), l)).collect(),
            dataset_order,
        })
    }

    /// Version ids of every dataset in the current release.
    #[must_use]
    pub fn current_version_ids(&self) -> Vec<i64> {
        self.datasets
            .values()
            .filter_map(|d| d.current.as_ref().map(|c| c.version_id))
            .collect()
    }

    /// Builds response meta (versions, sources, licences) for the datasets touched.
    pub fn meta_for<'a>(
        &self,
        dataset_ids: impl IntoIterator<Item = &'a str>,
        next: Vec<Link>,
    ) -> Meta {
        let mut meta = Meta::bare(self.data_version.clone());
        let ds: BTreeSet<&str> = dataset_ids.into_iter().collect();
        let mut src: BTreeSet<&str> = BTreeSet::new();
        for id in &ds {
            if let Some(d) = self.datasets.get(*id) {
                if let Some(c) = &d.current {
                    meta.dataset_versions.push(DatasetVersionRef {
                        dataset: d.id.clone(),
                        dsv: c.dsv.clone(),
                        vintage: c.vintage.clone(),
                    });
                }
                src.extend(d.source_ids.iter().map(String::as_str));
            }
        }
        let mut lic: BTreeSet<&str> = BTreeSet::new();
        for id in src {
            if let Some(s) = self.sources.get(id) {
                meta.source.push(SourceRef {
                    id: s.id.clone(),
                    publisher: s.publisher.clone(),
                    title: s.title.clone(),
                    url: s.homepage.clone(),
                });
                lic.insert(s.licence_id.as_str());
            }
        }
        for id in lic {
            if let Some(l) = self.licences.get(id) {
                meta.licences.push(LicenceRef {
                    id: l.id.clone(),
                    status: l.status.clone(),
                    url: l.url.clone(),
                    attribution: l.attribution.clone(),
                });
            }
        }
        meta.next = next;
        meta
    }
}

#[derive(Debug, Clone, FromRow)]
struct CurrentVersionRow {
    dataset_id: String,
    version_id: i64,
    dsv: String,
    vintage: Option<String>,
    row_count: i64,
    published_at: Option<DateTime<Utc>>,
}
