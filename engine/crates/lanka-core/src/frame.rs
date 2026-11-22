//! Frames (10 §3.1, ADR 0019): indicator values on one spatial support in
//! a column-aligned table — one code array, one value array per
//! (indicator, dims, period) — for MapLibre `feature-state` joins.
//!
//! A frame generalises `/v2/indicators/{id}/map` to several indicators and
//! periods. `codes` are every unit of the support (the reference boundary
//! set at `level`, under `parent`), sorted, so the arrays line up with the
//! boundary tileset whatever data exists; a unit without a value is `null`.
//!
//! Versions: a frame reads the current release, a pinned release, or exact
//! dataset versions (`dsv`), all on the range-versioned rows (05 §2): the
//! current release uses the partial `_map` index (`vto = open`), a pinned
//! one the version range `vfrom <= seq <= vto`. One indexed range read per
//! (indicator), covering all its periods and dims at once.

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::basis::{Basis, Status};
use crate::catalog::{Catalog, IndicatorInfo};
use crate::dims::Dims;
use crate::envelope::DatasetVersionRef;
use crate::error::{CoreError, CoreResult};
use crate::period::{Freq, Period};

pub const MAX_FRAME_INDICATORS: usize = 8;
pub const MAX_FRAME_COLUMNS: usize = 64;
pub const MAX_FRAME_PERIODS: usize = 64;
/// `codes × columns`, anonymous (10 §3.1; 5 M with a key, WP-31).
pub const MAX_FRAME_CELLS: usize = 500_000;

/// Which versions a frame reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePin {
    /// A release (`data_version`); `None` = the current release.
    pub release: Option<String>,
    /// Exact dataset versions; each overrides the release for its dataset.
    pub dsvs: Vec<String>,
}

impl FramePin {
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.release.is_none() && self.dsvs.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct FrameQuery {
    pub indicators: Vec<String>,
    pub level: i16,
    /// Geography scheme; `None` = the admin scheme.
    pub scheme: Option<String>,
    pub parent: Option<String>,
    /// Period codes; empty or `["latest"]` = the latest held at `level`.
    pub periods: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// `[<indicator>:]<dims>` entries separated by `|`; dims in the
    /// canonical (`k=v;k=v`) or query (`k:v,k:v`) form. Unscoped entries
    /// apply to every indicator that has those dimensions. Each entry
    /// selects the combinations containing its pairs; no entry = all.
    pub dims: Option<String>,
    pub pin: FramePin,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FrameSupport {
    /// `admin` (other schemes come with WP-10).
    pub kind: String,
    pub scheme: String,
    pub level: i16,
    pub parent: Option<String>,
    /// Boundary set whose units are the codes (the join target).
    pub boundary_set: Option<String>,
    /// Number of codes.
    pub n: usize,
    /// Observations at this level whose unit is not in the boundary set.
    pub unmatched: usize,
}

/// `basis` / `status` of a column: one name when uniform, else the int
/// codes of 05 §2 per code (`null` where there is no value).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub enum Coded {
    Uniform(String),
    Mixed(Vec<Option<i16>>),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, ToSchema)]
pub struct ColumnStats {
    /// Non-null values.
    pub n: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    /// p0, p5, p25, p50, p75, p95, p100 (linear interpolation, R type 7).
    pub q: Option<[f64; 7]>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FrameColumn {
    /// `c0`, `c1`, …: the feature-state key is `<sourceId>.<id>`.
    pub id: String,
    pub indicator: String,
    pub dataset: String,
    #[schema(value_type = Object)]
    pub dims: BTreeMap<String, String>,
    pub period: String,
    /// UCUM unit code.
    pub unit: Option<String>,
    /// Power of ten applied to values (SDMX `UNIT_MULT`).
    pub unit_mult: i16,
    /// official | third_party | derived | estimated
    pub class: String,
    /// Set when the indicator has no rows at this level and the column holds
    /// the roll-up that completes it (04 §8.1): its id. `dataset` is then the
    /// roll-up's dataset and `basis` says `computed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_from: Option<String>,
    pub values: Vec<Option<f64>>,
    /// Lower interval bounds; `null` when the column has none.
    pub lo: Option<Vec<Option<f64>>>,
    pub hi: Option<Vec<Option<f64>>>,
    pub basis: Option<Coded>,
    pub status: Option<Coded>,
    /// Flag bitmasks; `null` when no value carries a flag.
    pub flags: Option<Vec<i32>>,
    pub stats: ColumnStats,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Frame {
    pub support: FrameSupport,
    pub codes: Vec<String>,
    pub columns: Vec<FrameColumn>,
    /// Release the frame reads (`null` = exact versions only).
    #[serde(skip)]
    pub data_version: Option<String>,
    /// Dataset versions read (boundaries excluded).
    #[serde(skip)]
    pub versions: Vec<DatasetVersionRef>,
}

/// One dataset's version in a pin.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PinnedVersion {
    pub dataset_id: String,
    pub seq: i32,
    pub dsv: String,
    pub vintage: Option<String>,
    pub status: String,
}

/// Every dataset version of a release; `None` when the release is unknown.
pub async fn release_versions(
    pool: &PgPool,
    data_version: &str,
) -> CoreResult<Option<Vec<PinnedVersion>>> {
    let exists: Option<String> =
        sqlx::query_scalar("SELECT data_version FROM data.releases WHERE data_version = $1")
            .bind(data_version)
            .fetch_optional(pool)
            .await?;
    if exists.is_none() {
        return Ok(None);
    }
    Ok(Some(
        sqlx::query_as(
            "SELECT rd.dataset_id, v.seq, v.dsv, v.vintage, v.status
             FROM data.release_datasets rd JOIN data.dataset_versions v ON v.id = rd.version_id
             WHERE rd.data_version = $1 ORDER BY rd.dataset_id",
        )
        .bind(data_version)
        .fetch_all(pool)
        .await?,
    ))
}

/// Released dataset versions by dsv (several rows may share a dsv when a
/// dataset returned to earlier content; they hold the same rows, the
/// latest seq is used).
pub async fn dsv_versions(pool: &PgPool, dsvs: &[String]) -> CoreResult<Vec<PinnedVersion>> {
    Ok(sqlx::query_as(
        "SELECT DISTINCT ON (dsv) dataset_id, seq, dsv, vintage, status
         FROM data.dataset_versions
         WHERE dsv = ANY($1) AND status IN ('published', 'superseded', 'withdrawn')
         ORDER BY dsv, seq DESC",
    )
    .bind(dsvs)
    .fetch_all(pool)
    .await?)
}

/// `(seq, dsv, vintage, current?)` of every dataset under `pin`, plus the
/// release id the frame reports.
async fn pinned(
    pool: &PgPool,
    cat: &Catalog,
    pin: &FramePin,
) -> CoreResult<(
    HashMap<String, (i32, String, Option<String>, bool)>,
    Option<String>,
)> {
    let mut out: HashMap<String, (i32, String, Option<String>, bool)> = HashMap::new();
    let current = |id: &str| cat.datasets.get(id).and_then(|d| d.current.as_ref());
    let release = match &pin.release {
        None => {
            for (id, d) in &cat.datasets {
                if let Some(c) = &d.current {
                    out.insert(id.clone(), (c.seq, c.dsv.clone(), c.vintage.clone(), true));
                }
            }
            cat.data_version.clone()
        }
        Some(dv) => {
            let rows = release_versions(pool, dv)
                .await?
                .ok_or_else(|| CoreError::not_found(format!("no release \"{dv}\"")))?;
            for r in rows {
                let is_cur = current(&r.dataset_id).is_some_and(|c| c.seq == r.seq);
                out.insert(r.dataset_id, (r.seq, r.dsv, r.vintage, is_cur));
            }
            Some(dv.clone())
        }
    };
    if !pin.dsvs.is_empty() {
        let rows = dsv_versions(pool, &pin.dsvs).await?;
        for want in &pin.dsvs {
            let Some(r) = rows.iter().find(|r| &r.dsv == want) else {
                return Err(CoreError::not_found(format!(
                    "no released dataset version \"{want}\""
                )));
            };
            if r.status == "withdrawn" {
                return Err(CoreError::not_found(format!(
                    "dataset version \"{want}\" was withdrawn"
                )));
            }
            let is_cur = current(&r.dataset_id).is_some_and(|c| c.seq == r.seq);
            out.insert(
                r.dataset_id.clone(),
                (r.seq, r.dsv.clone(), r.vintage.clone(), is_cur),
            );
        }
    }
    Ok((out, release))
}

/// One `dims` entry: optional indicator scope + required pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DimsSel {
    indicator: Option<String>,
    pairs: Dims,
}

fn parse_dims_param(raw: Option<&str>, indicators: &[String]) -> CoreResult<Vec<DimsSel>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    raw.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let (scope, rest) = match entry.split_once(':') {
                Some((head, tail)) if indicators.iter().any(|i| i == head) => {
                    (Some(head.to_owned()), tail)
                }
                _ => (None, entry),
            };
            let pairs =
                Dims::parse(rest).map_err(|e| CoreError::bad_request(format!("dims: {e}")))?;
            Ok(DimsSel {
                indicator: scope,
                pairs,
            })
        })
        .collect()
}

fn dims_map(text: &str) -> BTreeMap<String, String> {
    text.split(';')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// R type 7 quantile of sorted values.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let h = (sorted.len() - 1) as f64 * p;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (h - h.floor()) * (sorted[hi] - sorted[lo])
}

/// Describes a value column (n, min, max, mean, 7 quantiles).
#[must_use]
pub fn column_stats(values: &[Option<f64>]) -> ColumnStats {
    let mut v: Vec<f64> = values
        .iter()
        .flatten()
        .copied()
        .filter(|x| x.is_finite())
        .collect();
    if v.is_empty() {
        return ColumnStats::default();
    }
    v.sort_by(f64::total_cmp);
    #[allow(clippy::cast_precision_loss)]
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    ColumnStats {
        n: v.len(),
        min: v.first().copied(),
        max: v.last().copied(),
        mean: Some(mean),
        q: Some([0.0, 0.05, 0.25, 0.5, 0.75, 0.95, 1.0].map(|p| quantile(&v, p))),
    }
}

fn coded(codes: &[Option<i16>], name: impl Fn(i16) -> &'static str) -> Option<Coded> {
    let mut present = codes.iter().flatten();
    let first = *present.next()?;
    if present.all(|c| *c == first) {
        Some(Coded::Uniform(name(first).to_owned()))
    } else {
        Some(Coded::Mixed(codes.to_vec()))
    }
}

type ObsCell = (
    i32,
    i32,
    NaiveDate,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    i16,
    i16,
    i32,
);

struct Plan<'c> {
    /// The requested indicator (column `indicator`, `unit`, `class`).
    shown: &'c IndicatorInfo,
    /// The indicator whose rows are read: `shown`, or the roll-up that
    /// completes it at this level.
    ind: &'c IndicatorInfo,
    dataset_key: i32,
    freq: Freq,
    seq: i32,
    current: bool,
    /// (dims key, canonical dims text)
    dims: Vec<(i32, String)>,
    periods: Vec<NaiveDate>,
}

/// The indicator whose rows serve `ind` at `level`: `ind` itself when it has
/// rows there under the pin, else the first roll-up that completes it
/// (`catalog.indicators.completes`, 04 §8.1) and has rows there, else `ind`.
/// `version_of(dataset)` gives `(seq, current?)` under the pin.
pub async fn source_at_level<'c>(
    pool: &PgPool,
    cat: &'c Catalog,
    ind: &'c IndicatorInfo,
    level: i16,
    version_of: impl Fn(&str) -> Option<(i32, bool)>,
) -> CoreResult<&'c IndicatorInfo> {
    let Some(fillers) = cat.completed_by.get(&ind.id) else {
        return Ok(ind);
    };
    let has = |i: &'c IndicatorInfo| {
        let v = version_of(&i.dataset_id);
        let ds = cat.datasets.get(&i.dataset_id).map(|d| d.key);
        async move {
            let (Some((seq, current)), Some(dk), Ok(freq)) = (v, ds, i.frequency()) else {
                return Ok::<bool, CoreError>(false);
            };
            let latest: Option<NaiveDate> = sqlx::query_scalar(latest_sql(current))
                .bind(dk)
                .bind(i.key)
                .bind(level)
                .bind(freq.as_db())
                .bind(seq)
                .fetch_one(pool)
                .await?;
            Ok(latest.is_some())
        }
    };
    if has(ind).await? {
        return Ok(ind);
    }
    for f in fillers {
        if let Some(fi) = cat.indicators.get(f)
            && has(fi).await?
        {
            return Ok(fi);
        }
    }
    Ok(ind)
}

fn latest_sql(current: bool) -> &'static str {
    if current {
        "SELECT max(time_start) FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND vto = 2147483647 AND vfrom <= $5"
    } else {
        "SELECT max(time_start) FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND vfrom <= $5 AND vto >= $5"
    }
}

fn periods_sql(current: bool) -> &'static str {
    if current {
        "SELECT DISTINCT time_start FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND vto = 2147483647 AND vfrom <= $5
           AND ($6::date IS NULL OR time_start >= $6) AND ($7::date IS NULL OR time_start <= $7)
         ORDER BY 1"
    } else {
        "SELECT DISTINCT time_start FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND vfrom <= $5 AND vto >= $5
           AND ($6::date IS NULL OR time_start >= $6) AND ($7::date IS NULL OR time_start <= $7)
         ORDER BY 1"
    }
}

fn dims_sql(current: bool) -> &'static str {
    if current {
        "SELECT DISTINCT dims_key FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND time_start = ANY($6) AND vto = 2147483647 AND vfrom <= $5"
    } else {
        "SELECT DISTINCT dims_key FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND time_start = ANY($6) AND vfrom <= $5 AND vto >= $5"
    }
}

fn rows_sql(current: bool) -> &'static str {
    if current {
        "SELECT geo_key, dims_key, time_start, value, lo, hi, basis, status, flags
         FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND time_start = ANY($6) AND dims_key = ANY($7) AND vto = 2147483647 AND vfrom <= $5"
    } else {
        "SELECT geo_key, dims_key, time_start, value, lo, hi, basis, status, flags
         FROM data.observations
         WHERE dataset_key = $1 AND indicator_key = $2 AND geo_level = $3 AND freq = $4
           AND time_start = ANY($6) AND dims_key = ANY($7) AND vfrom <= $5 AND vto >= $5"
    }
}

/// Builds a frame. Errors: 400 for invalid parameters or limits, 404 for
/// unknown indicators, parents, releases or versions.
#[allow(clippy::too_many_lines)]
pub async fn frame(pool: &PgPool, cat: &Catalog, q: &FrameQuery) -> CoreResult<Frame> {
    let profile = cat.profile()?;
    if q.indicators.is_empty() {
        return Err(CoreError::bad_request(
            "indicators is required (comma-separated indicator ids)",
        ));
    }
    if q.indicators.len() > MAX_FRAME_INDICATORS {
        return Err(CoreError::bad_request(format!(
            "at most {MAX_FRAME_INDICATORS} indicators per frame"
        )));
    }
    let scheme = q
        .scheme
        .clone()
        .unwrap_or_else(|| profile.admin_scheme().to_owned());
    if scheme != profile.admin_scheme() {
        return Err(CoreError::bad_request(format!(
            "scheme \"{scheme}\": frames are served on the admin scheme \"{}\" (other schemes with WP-10)",
            profile.admin_scheme()
        )));
    }
    let max_level = i16::from(profile.admin.max_level());
    if !(0..=max_level).contains(&q.level) {
        return Err(CoreError::bad_request(format!(
            "level must be 0..{max_level}"
        )));
    }
    let parent: Option<(i32, String)> = match q.parent.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(p) => {
            let pc = profile
                .admin
                .parse(p)
                .map_err(|e| CoreError::bad_request(e.to_string()))?;
            if i16::from(pc.level()) >= q.level {
                return Err(CoreError::bad_request(format!(
                    "parent {pc} (level {}) must be above level {}",
                    pc.level(),
                    q.level
                )));
            }
            let k: Option<i32> = sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                .bind(pc.as_str())
                .fetch_optional(pool)
                .await?;
            Some((
                k.ok_or_else(|| CoreError::not_found(format!("no admin unit \"{pc}\"")))?,
                pc.as_str().to_owned(),
            ))
        }
    };
    let (versions, data_version) = pinned(pool, cat, &q.pin).await?;
    let selectors = parse_dims_param(q.dims.as_deref(), &q.indicators)?;
    let cal = &profile.calendars;
    let latest_only = q.periods.is_empty() || q.periods.iter().all(|p| p == "latest");
    if !latest_only && (q.from.is_some() || q.to.is_some()) {
        return Err(CoreError::bad_request("give periods or from/to, not both"));
    }

    // Plan every indicator: version, periods, dims combinations.
    let mut plans: Vec<Plan<'_>> = Vec::with_capacity(q.indicators.len());
    for id in &q.indicators {
        if q.indicators.iter().filter(|x| *x == id).count() > 1 {
            return Err(CoreError::bad_request(format!(
                "indicator \"{id}\" is listed twice"
            )));
        }
        let shown = cat
            .indicators
            .get(id)
            .ok_or_else(|| CoreError::not_found(format!("no indicator \"{id}\"")))?;
        let ind = source_at_level(pool, cat, shown, q.level, |d| {
            versions.get(d).map(|v| (v.0, v.3))
        })
        .await?;
        let ds = cat
            .datasets
            .get(&ind.dataset_id)
            .ok_or_else(|| CoreError::not_found(format!("no dataset for \"{id}\"")))?;
        let (seq, _, _, current) = versions.get(&ind.dataset_id).cloned().ok_or_else(|| {
            CoreError::not_found(match &q.pin.release {
                Some(r) => format!("\"{id}\" has no data in release {r}"),
                None => format!("indicator \"{id}\" has no published data yet"),
            })
        })?;
        let freq = ind.frequency()?;
        let periods: Vec<NaiveDate> = if latest_only && q.from.is_none() && q.to.is_none() {
            let latest: Option<NaiveDate> = sqlx::query_scalar(latest_sql(current))
                .bind(ds.key)
                .bind(ind.key)
                .bind(q.level)
                .bind(freq.as_db())
                .bind(seq)
                .fetch_one(pool)
                .await?;
            vec![latest.ok_or_else(|| {
                CoreError::not_found(format!("\"{id}\" has no data at level {}", q.level))
            })?]
        } else if latest_only {
            let bound = |p: Option<&String>| -> CoreResult<Option<NaiveDate>> {
                p.map(|s| {
                    Period::parse(freq, s, cal)
                        .or_else(|_| Period::infer(s, cal))
                        .map(|p| p.start)
                })
                .transpose()
                .map_err(|e| CoreError::bad_request(e.to_string()))
            };
            let (from, to) = (bound(q.from.as_ref())?, bound(q.to.as_ref())?);
            sqlx::query_scalar(periods_sql(current))
                .bind(ds.key)
                .bind(ind.key)
                .bind(q.level)
                .bind(freq.as_db())
                .bind(seq)
                .bind(from)
                .bind(to)
                .fetch_all(pool)
                .await?
        } else {
            let mut v = q
                .periods
                .iter()
                .map(|p| {
                    Period::parse(freq, p, cal).map(|p| p.start).map_err(|e| {
                        CoreError::bad_request(format!("period \"{p}\" for {id}: {e}"))
                    })
                })
                .collect::<CoreResult<Vec<_>>>()?;
            v.sort_unstable();
            v.dedup();
            v
        };
        if periods.len() > MAX_FRAME_PERIODS {
            return Err(CoreError::bad_request(format!(
                "{} periods for {id} (limit {MAX_FRAME_PERIODS}); narrow from/to",
                periods.len()
            )));
        }
        let keys: Vec<i32> = if periods.is_empty() {
            Vec::new()
        } else {
            sqlx::query_scalar(dims_sql(current))
                .bind(ds.key)
                .bind(ind.key)
                .bind(q.level)
                .bind(freq.as_db())
                .bind(seq)
                .bind(&periods)
                .fetch_all(pool)
                .await?
        };
        let mut available: Vec<(i32, String)> = keys
            .into_iter()
            .map(|k| (k, cat.dims.text(k).unwrap_or_default().to_owned()))
            .collect();
        available.sort_by(|a, b| a.1.cmp(&b.1));
        let dim_ids: Vec<&str> = ind.dims.iter().map(String::as_str).collect();
        let mine: Vec<&DimsSel> = selectors
            .iter()
            .filter(|s| match &s.indicator {
                Some(i) => i == id,
                None => s.pairs.iter().all(|(k, _)| dim_ids.contains(&k)),
            })
            .collect();
        let dims: Vec<(i32, String)> = if mine.is_empty() {
            available.clone()
        } else {
            let picked: Vec<(i32, String)> = available
                .iter()
                .filter(|(_, text)| {
                    let have = dims_map(text);
                    mine.iter().any(|s| {
                        s.pairs
                            .iter()
                            .all(|(k, v)| have.get(k).map(String::as_str) == Some(v))
                    })
                })
                .cloned()
                .collect();
            if picked.is_empty() && !available.is_empty() {
                return Err(CoreError::bad_request(format!(
                    "dims for {id} match nothing; valid values: {}",
                    available
                        .iter()
                        .map(|a| a.1.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                )));
            }
            picked
        };
        plans.push(Plan {
            shown,
            ind,
            dataset_key: ds.key,
            freq,
            seq,
            current,
            dims,
            periods,
        });
    }
    let n_columns: usize = plans.iter().map(|p| p.dims.len() * p.periods.len()).sum();
    if n_columns > MAX_FRAME_COLUMNS {
        return Err(CoreError::bad_request(format!(
            "{n_columns} columns (limit {MAX_FRAME_COLUMNS}); select dims or fewer periods"
        )));
    }

    // The support: every unit of the reference boundary set at the level.
    let units: Vec<(i32, String)> = sqlx::query_as(
        "SELECT u.key, u.code FROM geo.units u
         WHERE u.scheme_id = $1 AND u.level = $2
           AND ($3::int4 IS NULL OR u.anc_keys @> ARRAY[$3::int4])
           AND ($4::text IS NULL OR EXISTS (SELECT 1 FROM geo.unit_geoms g
                                            WHERE g.unit_key = u.key AND g.boundary_set = $4))
         ORDER BY u.code",
    )
    .bind(&scheme)
    .bind(q.level)
    .bind(parent.as_ref().map(|p| p.0))
    .bind(cat.reference_set.as_deref())
    .fetch_all(pool)
    .await?;
    let n = units.len();
    if n * n_columns > MAX_FRAME_CELLS {
        return Err(CoreError::bad_request(format!(
            "{n} codes × {n_columns} columns = {} cells (limit {MAX_FRAME_CELLS})",
            n * n_columns
        )));
    }
    let index: HashMap<i32, usize> = units.iter().enumerate().map(|(i, u)| (u.0, i)).collect();

    let mut columns: Vec<FrameColumn> = Vec::with_capacity(n_columns);
    let mut unmatched = 0usize;
    let mut used: Vec<String> = Vec::new();
    for plan in &plans {
        if !used.contains(&plan.ind.dataset_id) {
            used.push(plan.ind.dataset_id.clone());
        }
        let base = columns.len();
        let mut col_of: HashMap<(i32, NaiveDate), usize> = HashMap::new();
        for (dk, text) in &plan.dims {
            for t in &plan.periods {
                col_of.insert((*dk, *t), columns.len());
                columns.push(FrameColumn {
                    id: format!("c{}", columns.len()),
                    indicator: plan.shown.id.clone(),
                    dataset: plan.ind.dataset_id.clone(),
                    dims: dims_map(text),
                    period: Period {
                        freq: plan.freq,
                        start: *t,
                    }
                    .render(cal),
                    unit: plan.shown.unit.clone(),
                    unit_mult: plan.shown.unit_mult,
                    class: plan.shown.class.clone(),
                    completed_from: (plan.ind.id != plan.shown.id).then(|| plan.ind.id.clone()),
                    values: vec![None; n],
                    lo: Some(vec![None; n]),
                    hi: Some(vec![None; n]),
                    basis: None,
                    status: None,
                    flags: Some(vec![0; n]),
                    stats: ColumnStats::default(),
                });
            }
        }
        if plan.dims.is_empty() || plan.periods.is_empty() {
            continue;
        }
        let dims_keys: Vec<i32> = plan.dims.iter().map(|d| d.0).collect();
        let rows: Vec<ObsCell> = sqlx::query_as(rows_sql(plan.current))
            .bind(plan.dataset_key)
            .bind(plan.ind.key)
            .bind(q.level)
            .bind(plan.freq.as_db())
            .bind(plan.seq)
            .bind(&plan.periods)
            .bind(&dims_keys)
            .fetch_all(pool)
            .await?;
        let width = columns.len() - base;
        let mut basis: Vec<Vec<Option<i16>>> = vec![vec![None; n]; width];
        let mut status: Vec<Vec<Option<i16>>> = vec![vec![None; n]; width];
        for (geo, dk, t, v, lo, hi, b, s, f) in rows {
            let Some(&c) = col_of.get(&(dk, t)) else {
                continue;
            };
            let Some(&i) = index.get(&geo) else {
                if parent.is_none() {
                    unmatched += 1;
                }
                continue;
            };
            let col = &mut columns[c];
            col.values[i] = v;
            if let Some(x) = col.lo.as_mut() {
                x[i] = lo;
            }
            if let Some(x) = col.hi.as_mut() {
                x[i] = hi;
            }
            if let Some(x) = col.flags.as_mut() {
                x[i] = f;
            }
            basis[c - base][i] = Some(b);
            status[c - base][i] = Some(s);
        }
        for (k, col) in columns[base..].iter_mut().enumerate() {
            col.basis = coded(&basis[k], |c| {
                Basis::from_code(c).unwrap_or(Basis::Reported).as_str()
            });
            col.status = coded(&status[k], |c| {
                Status::from_code(c).unwrap_or(Status::Normal).as_str()
            });
            if col
                .lo
                .as_ref()
                .is_some_and(|x| x.iter().all(Option::is_none))
            {
                col.lo = None;
            }
            if col
                .hi
                .as_ref()
                .is_some_and(|x| x.iter().all(Option::is_none))
            {
                col.hi = None;
            }
            if col
                .flags
                .as_ref()
                .is_some_and(|x| x.iter().all(|f| *f == 0))
            {
                col.flags = None;
            }
            col.stats = column_stats(&col.values);
        }
    }
    let versions_out = used
        .iter()
        .filter_map(|d| {
            versions
                .get(d)
                .map(|(_, dsv, vintage, _)| DatasetVersionRef {
                    dataset: d.clone(),
                    dsv: dsv.clone(),
                    vintage: vintage.clone(),
                })
        })
        .collect();
    Ok(Frame {
        support: FrameSupport {
            kind: "admin".to_owned(),
            scheme,
            level: q.level,
            parent: parent.map(|p| p.1),
            boundary_set: cat.reference_set.clone(),
            n,
            unmatched,
        },
        codes: units.into_iter().map(|u| u.1).collect(),
        columns,
        data_version,
        versions: versions_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_use_type_7_quantiles() {
        let s = column_stats(&[
            Some(1.0),
            None,
            Some(2.0),
            Some(3.0),
            Some(4.0),
            Some(f64::NAN),
        ]);
        assert_eq!(s.n, 4);
        assert_eq!((s.min, s.max, s.mean), (Some(1.0), Some(4.0), Some(2.5)));
        let q = s.q.unwrap_or_default();
        assert_eq!(q[0], 1.0);
        assert_eq!(q[3], 2.5);
        assert_eq!(q[6], 4.0);
        assert!((q[1] - 1.15).abs() < 1e-12);
        assert_eq!(column_stats(&[None]), ColumnStats::default());
    }

    #[test]
    fn dims_entries_scope_by_indicator() {
        let inds = vec![
            "census24.pop_total".to_owned(),
            "census24.sex_ratio".to_owned(),
        ];
        let s = parse_dims_param(Some("census24.pop_total:sex=total|sex:female"), &inds)
            .unwrap_or_default();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].indicator.as_deref(), Some("census24.pop_total"));
        assert_eq!(s[0].pairs.canonical(), "sex=total");
        assert_eq!(s[1].indicator, None);
        assert_eq!(s[1].pairs.canonical(), "sex=female");
        assert!(parse_dims_param(Some("sex=total=x"), &inds).is_err());
    }

    #[test]
    fn coded_is_scalar_when_uniform() {
        let name = |c: i16| if c == 1 { "reported" } else { "estimated" };
        assert_eq!(
            coded(&[Some(1), None, Some(1)], name),
            Some(Coded::Uniform("reported".into()))
        );
        assert_eq!(
            coded(&[Some(1), Some(3)], name),
            Some(Coded::Mixed(vec![Some(1), Some(3)]))
        );
        assert_eq!(coded(&[None], name), None);
    }
}
