//! In-memory snapshot of the catalog and the current release.
//!
//! The catalog is small (hundreds of rows), read on every request and only
//! changes on registry sync or release publish, so transports hold one
//! immutable snapshot behind an `Arc` and swap it on `NOTIFY lanka_release`.
//! It also carries the surrogate-key maps (dataset, indicator, dims set) and
//! the country profile, so queries bind integers and render codes without a
//! database round trip.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::envelope::{DatasetVersionRef, LicenceRef, Link, Meta, SourceRef};
use crate::error::{CoreError, CoreResult};
use crate::period::Freq;
use crate::profile::Profile;

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct LicenceInfo {
    pub id: String,
    pub status: String,
    pub url: Option<String>,
    pub attribution: Option<String>,
    pub spdx: Option<String>,
    /// bulk | api | link | internal
    pub redistribution: String,
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
    #[serde(skip)]
    pub key: i32,
    pub dataset_id: String,
    pub label: String,
    /// UCUM code (ISO 4217 for money), `catalog.units_of_measure`.
    pub unit: Option<String>,
    pub unit_label: Option<String>,
    /// Power of ten applied to values (SDMX `UNIT_MULT`).
    pub unit_mult: i16,
    pub dims: Vec<String>,
    pub geo_levels: Vec<String>,
    pub period_type: String,
    /// SDMX frequency code (A, F, H, S, Q, M, W, D, P).
    pub freq: String,
    pub measure_type: String,
    pub aggregation: String,
    /// official | third_party | derived | estimated
    pub class: String,
}

impl IndicatorInfo {
    /// The indicator's frequency.
    pub fn frequency(&self) -> CoreResult<Freq> {
        self.freq
            .chars()
            .next()
            .and_then(Freq::from_char)
            .ok_or_else(|| {
                CoreError::bad_request(format!("indicator {} has no frequency", self.id))
            })
    }
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct CurrentVersion {
    #[serde(skip)]
    pub version_id: i64,
    /// Dataset-local sequence number of the version (`vfrom`/`vto` range unit).
    pub seq: i32,
    pub dsv: String,
    pub vintage: Option<String>,
    pub row_count: i64,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
struct DatasetRow {
    id: String,
    key: i32,
    kind: String,
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
    #[serde(skip)]
    pub key: i32,
    /// indicators | features | boundaries | grid | crosswalk | codelist
    pub kind: String,
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

/// Interned dimension combinations (`catalog.dim_sets`).
#[derive(Debug, Clone, Default)]
pub struct DimSets {
    by_key: HashMap<i32, String>,
    by_text: HashMap<String, i32>,
}

impl DimSets {
    #[must_use]
    pub fn text(&self, key: i32) -> Option<&str> {
        self.by_key.get(&key).map(String::as_str)
    }

    #[must_use]
    pub fn key(&self, canonical: &str) -> Option<i32> {
        self.by_text.get(canonical).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
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
    pub dataset_by_key: HashMap<i32, String>,
    pub indicator_by_key: HashMap<i32, String>,
    pub dims: DimSets,
    /// The synced country profile (`catalog.profile`); `None` before the first sync.
    pub profile: Option<Arc<Profile>>,
    /// Reference boundary set of the admin scheme.
    pub reference_set: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct CurrentVersionRow {
    dataset_id: String,
    version_id: i64,
    seq: i32,
    dsv: String,
    vintage: Option<String>,
    row_count: i64,
    published_at: Option<DateTime<Utc>>,
}

impl Catalog {
    /// Loads the catalog and current release in one short read transaction
    /// (REPEATABLE READ so the snapshot is consistent).
    pub async fn load(pool: &PgPool) -> CoreResult<Self> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;

        let profile: Option<(serde_json::Value, Vec<u8>)> =
            sqlx::query_as("SELECT spec, spec_sha256 FROM catalog.profile")
                .fetch_optional(&mut *tx)
                .await?;
        let profile = profile
            .map(|(spec, sha)| {
                let sha: [u8; 32] = sha.try_into().unwrap_or([0; 32]);
                Profile::from_json(spec, sha).map(Arc::new)
            })
            .transpose()
            .map_err(|e| CoreError::bad_request(format!("catalog.profile: {e}")))?;
        let reference_set: Option<String> = match &profile {
            Some(p) => {
                sqlx::query_scalar(
                    "SELECT id FROM geo.boundary_sets WHERE is_reference AND scheme_id = $1",
                )
                .bind(p.admin_scheme())
                .fetch_optional(&mut *tx)
                .await?
            }
            None => None,
        };
        let licences: Vec<LicenceInfo> = sqlx::query_as(
            "SELECT id, status::text AS status, url, attribution, spdx, redistribution FROM catalog.licences",
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
            "SELECT id, key, kind, title, description, method, limits,
                    publication_class::text AS publication_class, time_from, time_to
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
            "SELECT i.id, i.key, i.dataset_id, i.label, i.unit_code AS unit, u.label_en AS unit_label,
                    i.unit_mult, i.dims, i.geo_levels, i.period_type, i.freq::text AS freq,
                    i.measure_type, i.aggregation, i.class
             FROM catalog.indicators i LEFT JOIN catalog.units_of_measure u ON u.code = i.unit_code
             ORDER BY i.dataset_id, i.sort, i.id",
        )
        .fetch_all(&mut *tx)
        .await?;
        let dims: Vec<(i32, String)> = sqlx::query_as("SELECT key, dims FROM catalog.dim_sets")
            .fetch_all(&mut *tx)
            .await?;
        let data_version: Option<String> =
            sqlx::query_scalar("SELECT data_version FROM data.current_release")
                .fetch_optional(&mut *tx)
                .await?;
        let current: Vec<CurrentVersionRow> = sqlx::query_as(
            "SELECT dataset_id, version_id, seq, dsv, vintage, row_count, published_at
             FROM data.current_versions",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        let mut current: HashMap<String, CurrentVersionRow> = current
            .into_iter()
            .map(|r| (r.dataset_id.clone(), r))
            .collect();
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
        let dataset_by_key = rows.iter().map(|r| (r.key, r.id.clone())).collect();
        let datasets = rows
            .into_iter()
            .map(|r| {
                let cur = current.remove(&r.id).map(|c| CurrentVersion {
                    version_id: c.version_id,
                    seq: c.seq,
                    dsv: c.dsv,
                    vintage: c.vintage,
                    row_count: c.row_count,
                    published_at: c.published_at,
                });
                let info = DatasetInfo {
                    source_ids: source_ids.remove(&r.id).unwrap_or_default(),
                    indicator_ids: indicator_ids.remove(&r.id).unwrap_or_default(),
                    current: cur,
                    key: r.key,
                    kind: r.kind,
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

        let indicator_by_key = indicators.iter().map(|i| (i.key, i.id.clone())).collect();
        let dims = DimSets {
            by_text: dims.iter().map(|(k, d)| (d.clone(), *k)).collect(),
            by_key: dims.into_iter().collect(),
        };
        Ok(Self {
            data_version,
            datasets,
            indicators: indicators.into_iter().map(|i| (i.id.clone(), i)).collect(),
            sources: sources.into_iter().map(|s| (s.id.clone(), s)).collect(),
            licences: licences.into_iter().map(|l| (l.id.clone(), l)).collect(),
            dataset_order,
            dataset_by_key,
            indicator_by_key,
            dims,
            profile,
            reference_set,
        })
    }

    /// The country profile; requests needing codes fail cleanly before the
    /// first `lanka registry sync`.
    pub fn profile(&self) -> CoreResult<&Profile> {
        self.profile
            .as_deref()
            .ok_or_else(|| CoreError::not_found("country profile (run `lanka registry sync`)"))
    }

    /// The dataset whose boundaries every area response cites.
    #[must_use]
    pub fn boundary_dataset(&self) -> Option<&str> {
        self.profile.as_deref().map(Profile::boundary_dataset)
    }

    /// `(dataset_key, current seq)` of every dataset in the current release.
    #[must_use]
    pub fn current_seqs(&self) -> Vec<(i32, i32)> {
        let mut v: Vec<(i32, i32)> = self
            .datasets
            .values()
            .filter_map(|d| d.current.as_ref().map(|c| (d.key, c.seq)))
            .collect();
        v.sort_unstable();
        v
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
