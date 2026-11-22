//! RESOLVE and PLAN (11 §2, §3.2): ids → pinned versions, the target
//! support, an alignment method per variable, periods and dims made
//! explicit — and ALIGN: every variable onto one column of the support,
//! through the published frames (`lanka_core::frame`) and the enrichment
//! methods (`rollup`, `grid_zonal`, `features_zonal` of `lanka-enrich`;
//! `exposure_overlay` of `lanka-grid`).
//!
//! Reads only; never writes `data.*`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use lanka_core::frame::{Coded, FramePin, FrameQuery, frame, release_versions};
use lanka_core::{Basis, Catalog, Freq, Period};
use lanka_enrich::method::{
    Feature, GridInput, IndicatorMeta, Inputs, MemInputs, Method, MethodCtx, Row, Units,
};
use lanka_enrich::pg::{PgInputs, Pinned, load_indicators};
use lanka_stats::transform::Transform;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::frame::{AlignedFrame, AlignmentRecord, BASIS, Column, InputRef, Support};
use crate::method::{AnalysisMethod, ExposureFrame};
use crate::methods::Registry;
use crate::request::{Pin, Problem, Request, Support as SupportSpec, SupportRef, Variable};

/// A request error: HTTP 400 with hints (11 §11.1).
#[derive(Debug, Clone, PartialEq)]
pub struct BadRequest {
    pub message: String,
    pub hints: Value,
    /// JSON-pointer problems into the request (empty = the request as a whole).
    pub problems: Vec<Problem>,
}

impl std::fmt::Display for BadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BadRequest {}

pub fn bad(message: impl Into<String>, hints: Value) -> anyhow::Error {
    anyhow::Error::new(BadRequest {
        message: message.into(),
        hints,
        problems: Vec::new(),
    })
}

/// A request error pointing at one member of the request (`/variables/x/period`).
pub fn bad_at(path: impl Into<String>, message: impl Into<String>, hints: Value) -> anyhow::Error {
    let message = message.into();
    anyhow::Error::new(BadRequest {
        problems: vec![Problem {
            path: path.into(),
            message: message.clone(),
        }],
        message,
        hints,
    })
}

/// How a variable reaches the support.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Indicator {
        id: String,
        dataset_key: i32,
        indicator_key: i32,
        freq: Freq,
        /// Canonical `k=v;k=v` of the selected combination.
        dims: String,
        /// Level the values are read at.
        source_level: i16,
        period: NaiveDate,
        aggregation: String,
    },
    Grid {
        layer: String,
        layer_period: String,
        dataset_id: String,
        stat: String,
        cells: String,
        /// The layer's own indicator (period/frequency of the zonal rows).
        indicator: Option<String>,
        estimated: bool,
    },
    Features {
        dataset_id: String,
        measure: String,
        kinds: Vec<String>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedVar {
    pub role: String,
    pub var: Variable,
    pub source: Source,
    /// `identity | rollup | inherit | grid_zonal | features_zonal`.
    pub method: String,
    pub version: Version,
    pub input: InputRef,
}

impl ResolvedVar {
    #[must_use]
    pub fn method_ref(&self) -> String {
        match self.method.as_str() {
            "identity" => "identity".to_owned(),
            "inherit" => "inherit@1.0.0".to_owned(),
            m => format!("{m}@1.0.0"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Version {
    pub dataset_id: String,
    pub dataset_key: i32,
    pub version_id: i64,
    pub seq: i32,
    pub dsv: String,
}

#[derive(Clone)]
pub struct Resolved {
    /// The request with every default, "latest" and "auto" made explicit.
    pub request: Request,
    pub method: Arc<dyn AnalysisMethod>,
    pub data_version: String,
    pub level: i16,
    pub within: Option<String>,
    pub vars: Vec<ResolvedVar>,
    /// Other levels for the scale-sensitivity table (identity alignments).
    pub scale_levels: Vec<i16>,
}

impl Resolved {
    /// Serialisable form (golden fixtures): the method by id.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({"request": self.request, "method": self.method.info().id, "data_version": self.data_version,
               "level": self.level, "within": self.within, "vars": self.vars, "scale_levels": self.scale_levels})
    }

    pub fn from_json(v: &Value, methods: &Registry) -> Result<Self> {
        Ok(Self {
            request: serde_json::from_value(v["request"].clone())?,
            method: methods
                .get(v["method"].as_str().unwrap_or_default())
                .context("unknown method")?,
            data_version: v["data_version"].as_str().unwrap_or_default().to_owned(),
            level: i16::try_from(v["level"].as_i64().unwrap_or(0))?,
            within: v["within"].as_str().map(str::to_owned),
            vars: serde_json::from_value(v["vars"].clone())?,
            scale_levels: serde_json::from_value(v["scale_levels"].clone())?,
        })
    }
}

/// Shared read context of a run.
pub struct Env {
    pub pool: PgPool,
    pub catalog: Arc<Catalog>,
    pub store_root: Option<PathBuf>,
    pub methods: Registry,
    /// The host's memory-mapped grid layers, when it has any.
    pub mapped: Option<crate::engine::MappedGridFn>,
}

/// Recipe graph: output dataset → input datasets.
async fn recipe_graph(pool: &PgPool) -> Result<HashMap<String, Vec<String>>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT r.output_dataset, i.dataset_id FROM catalog.recipes r
         JOIN catalog.recipe_inputs i ON i.recipe_id = r.id ORDER BY 1, 2",
    )
    .fetch_all(pool)
    .await?;
    let mut g: HashMap<String, Vec<String>> = HashMap::new();
    for (o, i) in rows {
        g.entry(o).or_default().push(i);
    }
    Ok(g)
}

fn upstream(g: &HashMap<String, Vec<String>>, ds: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![ds.to_owned()];
    while let Some(d) = stack.pop() {
        if seen.insert(d.clone())
            && let Some(ins) = g.get(&d)
        {
            stack.extend(ins.iter().cloned());
        }
    }
    seen
}

fn people_unit(u: &str) -> bool {
    let u = u.to_lowercase();
    [
        "person",
        "people",
        "household",
        "dwelling",
        "building",
        "propert",
        "housing",
    ]
    .iter()
    .any(|w| u.contains(w))
}

/// Versions of every dataset under the pin.
///
/// `latest` is the release that is current in the database at the time of
/// the request — not the host's catalogue snapshot, which reloads
/// asynchronously after a release — so the fingerprint is always taken over
/// the input versions of the current release (11 §9).
async fn pinned_versions(env: &Env, pin: &Pin) -> Result<(String, HashMap<String, (i32, String)>)> {
    let release = match pin {
        Pin::Latest => {
            sqlx::query_scalar::<_, String>("SELECT data_version FROM data.current_release")
                .fetch_optional(&env.pool)
                .await?
                .ok_or_else(|| bad("no release has been published yet", json!({})))?
        }
        Pin::Release { release } => release.clone(),
    };
    let rows = release_versions(&env.pool, &release)
        .await?
        .ok_or_else(|| bad(format!("no release \"{release}\""), json!({})))?;
    Ok((
        release,
        rows.into_iter()
            .map(|r| (r.dataset_id, (r.seq, r.dsv)))
            .collect(),
    ))
}

async fn version_of(
    env: &Env,
    versions: &HashMap<String, (i32, String)>,
    dataset: &str,
    data_version: &str,
) -> Result<Version> {
    let (seq, dsv) = versions.get(dataset).cloned().ok_or_else(|| {
        bad(
            format!("dataset \"{dataset}\" has no data in release {data_version}"),
            json!({"dataset": dataset}),
        )
    })?;
    let key = env.catalog.datasets.get(dataset).map_or(0, |d| d.key);
    let version_id: i64 = sqlx::query_scalar(
        "SELECT id FROM data.dataset_versions WHERE dataset_id = $1 AND seq = $2 ORDER BY id DESC LIMIT 1",
    )
    .bind(dataset)
    .bind(seq)
    .fetch_one(&env.pool)
    .await?;
    Ok(Version {
        dataset_id: dataset.to_owned(),
        dataset_key: key,
        version_id,
        seq,
        dsv,
    })
}

/// Levels where an indicator has rows in the pinned version.
async fn levels_of(pool: &PgPool, v: &Version, ind_key: i32) -> Result<Vec<i16>> {
    let mut out = Vec::new();
    for l in 0..=4_i16 {
        let has: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM data.observations WHERE dataset_key = $1 AND indicator_key = $2
                             AND geo_level = $3 AND vfrom <= $4 AND vto >= $4)",
        )
        .bind(v.dataset_key)
        .bind(ind_key)
        .bind(l)
        .bind(v.seq)
        .fetch_one(pool)
        .await?;
        if has {
            out.push(l);
        }
    }
    Ok(out)
}

/// Dims combinations (canonical text) of an indicator at a level/period.
async fn dims_at(
    env: &Env,
    v: &Version,
    ind_key: i32,
    level: i16,
    period: Option<NaiveDate>,
) -> Result<Vec<String>> {
    let keys: Vec<i32> = sqlx::query_scalar(
        "SELECT DISTINCT dims_key FROM data.observations WHERE dataset_key = $1 AND indicator_key = $2
         AND geo_level = $3 AND vfrom <= $4 AND vto >= $4 AND ($5::date IS NULL OR time_start = $5)",
    )
    .bind(v.dataset_key)
    .bind(ind_key)
    .bind(level)
    .bind(v.seq)
    .bind(period)
    .fetch_all(&env.pool)
    .await?;
    let mut d: Vec<String> = keys
        .into_iter()
        .filter_map(|k| env.catalog.dims.text(k).map(str::to_owned))
        .collect();
    d.sort();
    Ok(d)
}

fn dims_text(m: &BTreeMap<String, String>) -> String {
    m.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn dims_map(t: &str) -> BTreeMap<String, String> {
    t.split(';')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Periods (start dates, ascending) with data for an indicator/dims at a level.
async fn periods_of(
    pool: &PgPool,
    v: &Version,
    ind_key: i32,
    level: i16,
    dims_key: Option<i32>,
) -> Result<Vec<NaiveDate>> {
    Ok(sqlx::query_scalar(
        "SELECT DISTINCT time_start FROM data.observations WHERE dataset_key = $1 AND indicator_key = $2
         AND geo_level = $3 AND vfrom <= $4 AND vto >= $4 AND ($5::int IS NULL OR dims_key = $5)
         ORDER BY time_start",
    )
    .bind(v.dataset_key)
    .bind(ind_key)
    .bind(level)
    .bind(v.seq)
    .bind(dims_key)
    .fetch_all(pool)
    .await?)
}

/// JSON pointer of a role's variable in the request.
fn var_path(role: &str) -> String {
    role.strip_prefix("control:").map_or_else(
        || format!("/variables/{role}"),
        |i| format!("/variables/controls/{i}"),
    )
}

async fn latest_period(
    pool: &PgPool,
    v: &Version,
    ind_key: i32,
    level: i16,
    dims_key: Option<i32>,
) -> Result<Option<NaiveDate>> {
    Ok(sqlx::query_scalar(
        "SELECT max(time_start) FROM data.observations WHERE dataset_key = $1 AND indicator_key = $2
         AND geo_level = $3 AND vfrom <= $4 AND vto >= $4 AND ($5::int IS NULL OR dims_key = $5)",
    )
    .bind(v.dataset_key)
    .bind(ind_key)
    .bind(level)
    .bind(v.seq)
    .bind(dims_key)
    .fetch_one(pool)
    .await?)
}

fn input_ref(
    cat: &Catalog,
    graph: &HashMap<String, Vec<String>>,
    role: &str,
    dataset: &str,
    dsv: &str,
) -> InputRef {
    let ds = cat.datasets.get(dataset);
    let sources = ds.map(|d| d.source_ids.clone()).unwrap_or_default();
    let lic = sources
        .iter()
        .filter_map(|s| cat.sources.get(s))
        .filter_map(|s| cat.licences.get(&s.licence_id))
        .max_by_key(|l| redistribution_rank(&l.redistribution));
    let limits = ds
        .map(|d| d.limits.join(" ").to_lowercase())
        .unwrap_or_default();
    InputRef {
        role: role.to_owned(),
        dataset: dataset.to_owned(),
        dsv: dsv.to_owned(),
        licence: lic.map_or_else(|| "unknown".to_owned(), |l| l.id.clone()),
        redistribution: lic.map_or_else(|| "link".to_owned(), |l| l.redistribution.clone()),
        synthetic: dataset.starts_with("@demo/") || limits.contains("synthetic"),
        community: dataset.starts_with('@') && !dataset.starts_with("@demo/"),
        upstream: upstream(graph, dataset),
        sources,
        ..InputRef::default()
    }
}

/// bulk < api < link < internal.
#[must_use]
pub fn redistribution_rank(r: &str) -> u8 {
    match r {
        "bulk" => 0,
        "api" => 1,
        "link" => 2,
        _ => 3,
    }
}

fn parse_transform(v: &Variable) -> Result<Transform> {
    match v.transform.as_deref() {
        None => Ok(Transform::Identity),
        Some(t) => {
            Transform::parse(t).ok_or_else(|| bad(format!("unknown transform {t:?}"), json!({})))
        }
    }
}

/// Resolves and plans a request.
#[allow(clippy::too_many_lines)]
pub async fn resolve(env: &Env, req: &Request) -> Result<Resolved> {
    let cat = &env.catalog;
    // dims codes in their stored spelling (`RP100` → `rp100`), so the
    // request, its fingerprint and the alignment agree with the frame.
    let mut canon = req.clone();
    for v in canon.variables.all_mut() {
        for (k, c) in &mut v.dims {
            if let Some(s) = cat.dims.canonical_code(k, c) {
                *c = s.to_owned();
            }
        }
    }
    let req = &canon;
    let profile = cat.profile().map_err(|e| anyhow::anyhow!("{e}"))?;
    let method = env.methods.get(&req.method).ok_or_else(|| {
        bad(
            format!(
                "method \"{}\" is not available (WP-29 implements tier 1)",
                req.method
            ),
            json!({"available": env.methods.iter().map(|m| m.info().id).collect::<Vec<_>>()}),
        )
    })?;
    let info = method.info();
    for r in info.roles.iter().filter(|r| r.required) {
        let present = match r.name {
            "x" => req.variables.x.is_some(),
            "y" => req.variables.y.is_some(),
            "group" => req.variables.group.is_some(),
            "weight" => req.variables.weight.is_some(),
            _ => true,
        };
        if !present {
            return Err(bad(
                format!("{} needs variables.{}", info.id, r.name),
                json!({"roles": info.roles.iter().map(|r| json!({"name": r.name, "required": r.required})).collect::<Vec<_>>()}),
            ));
        }
    }
    let params = method.defaults(&req.params);
    if let Ok(schema) = serde_json::from_str::<Value>(info.params)
        && let Ok(v) = jsonschema::validator_for(&schema)
    {
        let problems: Vec<Value> = v
            .iter_errors(&params)
            .map(|e| json!({"path": format!("/params{}", e.instance_path()), "message": e.to_string()}))
            .collect();
        if !problems.is_empty() {
            return Err(bad(
                format!("invalid params for {}", info.id),
                json!({"problems": problems}),
            ));
        }
    }
    let (data_version, versions) = pinned_versions(env, &req.pin).await?;
    let graph = recipe_graph(&env.pool).await?;
    let max_level = i16::from(profile.admin.max_level());
    let (want_level, within) = match &req.support {
        SupportRef::Auto => (None, None),
        SupportRef::Explicit(SupportSpec::Admin {
            level,
            scheme,
            within,
        }) => {
            if let Some(s) = scheme
                && s != profile.admin_scheme()
            {
                return Err(bad(
                    format!("scheme \"{s}\": WP-29 analyses run on the admin scheme"),
                    json!({}),
                ));
            }
            if *level > max_level {
                return Err(bad(
                    format!("level must be 0..{max_level}"),
                    json!({"available_levels": (0..=max_level).collect::<Vec<_>>()}),
                ));
            }
            (Some(*level), within.clone())
        }
        SupportRef::Explicit(SupportSpec::Grid { .. } | SupportSpec::Points { .. }) => {
            return Err(bad(
                "grid and point supports arrive with WP-27/WP-33; use an admin support",
                json!({"supports": ["admin"]}),
            ));
        }
    };
    if let Some(w) = &within {
        profile
            .admin
            .parse(w)
            .map_err(|e| bad(format!("within: {e}"), json!({})))?;
    }

    // first pass: what each variable is and where it has data
    struct Pre {
        role: String,
        var: Variable,
        version: Version,
        kind: PreKind,
        input: InputRef,
    }
    #[allow(clippy::large_enum_variant)]
    enum PreKind {
        Ind {
            info: lanka_core::catalog::IndicatorInfo,
            levels: Vec<i16>,
        },
        Grid {
            layer: String,
            period: String,
            zonal: String,
            indicator: Option<String>,
        },
        Feat {
            dataset: String,
        },
    }
    let mut pre: Vec<Pre> = Vec::new();
    for (role, v) in req.variables.roles() {
        parse_transform(v)?;
        if v.source.is_some() {
            return Err(bad(
                format!(
                    "variables.{role}: `source` keys are resolved inside a map document; give an indicator, grid or dataset"
                ),
                json!({}),
            ));
        }
        if let Some(id) = &v.indicator {
            let ind = cat.indicators.get(id).cloned().ok_or_else(|| {
                let close: Vec<&String> = cat
                    .indicators
                    .keys()
                    .filter(|k| k.contains(id.split('.').next_back().unwrap_or(id)))
                    .take(8)
                    .collect();
                bad(
                    format!("unknown indicator \"{id}\""),
                    json!({"similar": close}),
                )
            })?;
            let version = version_of(env, &versions, &ind.dataset_id, &data_version).await?;
            let levels = levels_of(&env.pool, &version, ind.key).await?;
            let mut input = input_ref(cat, &graph, &role, &ind.dataset_id, &version.dsv);
            input.indicator = Some(id.clone());
            input.time_invariant = ind.time_invariant;
            input.class = ind.class.clone();
            input.measure_type = ind.measure_type.clone();
            input.unit = ind.unit.clone();
            input.label = v
                .label
                .as_ref()
                .and_then(|l| {
                    l.as_str()
                        .map(str::to_owned)
                        .or_else(|| l["en"].as_str().map(str::to_owned))
                })
                .unwrap_or_else(|| ind.label.to_lowercase());
            input.about_people = ind.unit.as_deref().is_some_and(people_unit)
                || v.dims
                    .get("source_indicator")
                    .and_then(|s| cat.indicators.get(s))
                    .and_then(|s| s.unit.as_deref())
                    .is_some_and(people_unit);
            pre.push(Pre {
                role,
                var: v.clone(),
                version,
                kind: PreKind::Ind { info: ind, levels },
                input,
            });
        } else if let Some(layer) = &v.grid {
            let rows: Vec<(String, String, Option<String>, String, String)> = sqlx::query_as(
                "SELECT period, dataset_id, indicator_id, zonal, encode(sha256, 'hex') FROM geo.grid_layers
                 WHERE id = $1 ORDER BY period",
            )
            .bind(layer)
            .fetch_all(&env.pool)
            .await?;
            let pick = match v.period.as_deref().filter(|p| *p != "latest") {
                Some(p) => rows.iter().find(|r| r.0 == p),
                None => rows.last(),
            }
            .cloned()
            .ok_or_else(|| {
                let member = if rows.is_empty() { "grid" } else { "period" };
                bad_at(
                    format!("{}/{member}", var_path(&role)),
                    format!("unknown grid layer \"{layer}\" (period {:?})", v.period),
                    json!({"periods": rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>()}),
                )
            })?;
            let version = version_of(env, &versions, &pick.1, &data_version).await?;
            let mut input = input_ref(cat, &graph, &role, &pick.1, &version.dsv);
            input.grid = Some(layer.clone());
            input.grid_sha256 = Some(pick.4.clone());
            let ind = pick.2.as_ref().and_then(|i| cat.indicators.get(i));
            // a layer without an indicator is time-invariant when its
            // dataset's indicators all are (the JRC flood fractions)
            input.time_invariant = ind.map_or_else(
                || {
                    cat.datasets.get(&pick.1).is_some_and(|d| {
                        !d.indicator_ids.is_empty()
                            && d.indicator_ids
                                .iter()
                                .all(|i| cat.indicators.get(i).is_some_and(|i| i.time_invariant))
                    })
                },
                |i| i.time_invariant,
            );
            input.class = ind.map_or_else(|| "third_party".to_owned(), |i| i.class.clone());
            input.measure_type = if pick.3 == "sum" {
                "count".to_owned()
            } else {
                "mean".to_owned()
            };
            input.unit = ind.and_then(|i| i.unit.clone());
            input.label = v
                .label
                .as_ref()
                .and_then(|l| l.as_str().map(str::to_owned))
                .unwrap_or_else(|| layer.clone());
            input.about_people = input.unit.as_deref().is_some_and(people_unit);
            pre.push(Pre {
                role,
                var: v.clone(),
                version,
                kind: PreKind::Grid {
                    layer: layer.clone(),
                    period: pick.0,
                    zonal: pick.3,
                    indicator: pick.2,
                },
                input,
            });
        } else if let Some(ds) = &v.dataset {
            let d = cat
                .datasets
                .get(ds)
                .ok_or_else(|| bad(format!("unknown dataset \"{ds}\""), json!({})))?;
            if d.kind != "features" {
                return Err(bad(
                    format!(
                        "dataset \"{ds}\" is {}, not features; name an indicator",
                        d.kind
                    ),
                    json!({}),
                ));
            }
            if v.field.is_some() {
                return Err(bad(
                    "feature fields as point samples arrive with point support (WP-27/WP-33); WP-29 counts features per unit",
                    json!({}),
                ));
            }
            let version = version_of(env, &versions, ds, &data_version).await?;
            let mut input = input_ref(cat, &graph, &role, ds, &version.dsv);
            input.class = "third_party".to_owned();
            input.measure_type = "count".to_owned();
            input.unit = Some("{count}".to_owned());
            input.label = v
                .label
                .as_ref()
                .and_then(|l| l.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{ds} features"));
            pre.push(Pre {
                role,
                var: v.clone(),
                version,
                kind: PreKind::Feat {
                    dataset: ds.clone(),
                },
                input,
            });
        } else {
            return Err(bad(
                format!("variables.{role}: give an indicator, grid or dataset"),
                json!({}),
            ));
        }
    }

    // target level: explicit, or the coarsest native support (rule 1)
    let level = match want_level {
        Some(l) => l,
        None => pre
            .iter()
            .filter_map(|p| match &p.kind {
                PreKind::Ind { levels, .. } => levels.iter().max().copied(),
                _ => None,
            })
            .min()
            .unwrap_or(2),
    };
    if let Some(w) = &within
        && let Ok(pc) = profile.admin.parse(w)
        && i16::from(pc.level()) >= level
    {
        return Err(bad(
            format!("within {w} must be above level {level}"),
            json!({}),
        ));
    }

    // second pass: alignment, dims, period per variable
    let mut vars = Vec::new();
    let mut resolved_req = req.clone();
    resolved_req.pin = Pin::Release {
        release: data_version.clone(),
    };
    resolved_req.support = SupportRef::Explicit(SupportSpec::Admin {
        level,
        scheme: None,
        within: within.clone(),
    });
    for p in pre {
        let align_req = p
            .var
            .align
            .as_ref()
            .and_then(|a| a["method"].as_str())
            .map(str::to_owned);
        let (source, method, var) = match p.kind {
            PreKind::Ind { info, levels } => {
                let freq = info.frequency().map_err(|e| anyhow::anyhow!("{e}"))?;
                let finer = levels.iter().copied().filter(|l| *l > level).min();
                let coarser = levels.iter().copied().filter(|l| *l < level).max();
                let (method, source_level): (&'static str, i16) = match align_req.as_deref() {
                    None | Some("identity") if levels.contains(&level) => ("identity", level),
                    None | Some("rollup") if finer.is_some() && info.aggregation != "none" => {
                        ("rollup", finer.unwrap_or(level))
                    }
                    Some("inherit") if coarser.is_some() => ("inherit", coarser.unwrap_or(level)),
                    Some(
                        m @ ("dasymetric" | "crosswalk_rebase" | "spatial_interp"
                        | "point_aggregate" | "sample"),
                    ) => {
                        return Err(bad(
                            format!(
                                "alignment {m} arrives with a later package (WP-10/WP-17/WP-33)"
                            ),
                            json!({"available_levels": levels}),
                        ));
                    }
                    _ => {
                        let why = if levels.iter().all(|l| *l < level) {
                            format!(
                                "\"{}\" is published only at coarser levels; analyse at level {} or ask for align.method \"inherit\" (explicit disaggregation)",
                                info.id,
                                levels.iter().max().copied().unwrap_or(0)
                            )
                        } else {
                            format!(
                                "\"{}\" is not published at level {level} and its aggregation rule ({}) does not allow a roll-up",
                                info.id, info.aggregation
                            )
                        };
                        return Err(bad(
                            why,
                            json!({"indicator": info.id, "available_levels": levels}),
                        ));
                    }
                };
                // dims: the requested pairs must select one combination
                let want = dims_text(&p.var.dims);
                let combos = dims_at(env, &p.version, info.key, source_level, None).await?;
                let matching: Vec<&String> = combos
                    .iter()
                    .filter(|c| {
                        let m = dims_map(c);
                        p.var.dims.iter().all(|(k, v)| m.get(k) == Some(v))
                    })
                    .collect();
                let dims = match matching.as_slice() {
                    [one] => (*one).clone(),
                    [] => {
                        return Err(bad(
                            format!(
                                "\"{}\": no data for dims {want:?} at level {source_level}",
                                info.id
                            ),
                            json!({"dims": combos}),
                        ));
                    }
                    many => {
                        if let Some(exact) = many.iter().find(|c| **c == &want) {
                            (*exact).clone()
                        } else {
                            return Err(bad(
                                format!(
                                    "\"{}\" has {} dimension combinations; choose one with dims",
                                    info.id,
                                    many.len()
                                ),
                                json!({"dims": many}),
                            ));
                        }
                    }
                };
                let dims_key = env.catalog.dims.key(&dims);
                let period = match p.var.period.as_deref().filter(|s| *s != "latest") {
                    Some(s) => {
                        let at = format!("{}/period", var_path(&p.role));
                        let start = Period::parse(freq, s, &profile.calendars)
                            .map_err(|e| {
                                bad_at(
                                    at.clone(),
                                    format!("period {s:?} for {}: {e}", info.id),
                                    json!({}),
                                )
                            })?
                            .start;
                        // an explicit period must have data: a request whose
                        // inputs resolve to nothing is a 400, never a frame error
                        let have =
                            periods_of(&env.pool, &p.version, info.key, source_level, dims_key)
                                .await?;
                        if !have.contains(&start) {
                            let available: Vec<String> = have
                                .iter()
                                .rev()
                                .map(|d| Period { freq, start: *d }.render(&profile.calendars))
                                .collect();
                            return Err(bad_at(
                                at,
                                format!(
                                    "\"{}\" has no data for period {s} at level {source_level}{}",
                                    info.id,
                                    if available.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" (available: {})", available.join(", "))
                                    }
                                ),
                                json!({"indicator": info.id, "dims": dims_map(&dims),
                                       "level": source_level, "available_periods": available}),
                            ));
                        }
                        start
                    }
                    None => latest_period(&env.pool, &p.version, info.key, source_level, dims_key)
                        .await?
                        .ok_or_else(|| {
                            bad(
                                format!("\"{}\" has no data at level {source_level}", info.id),
                                json!({}),
                            )
                        })?,
                };
                let code = Period {
                    freq,
                    start: period,
                }
                .render(&profile.calendars);
                let mut var = p.var.clone();
                var.dims = dims_map(&dims);
                var.period = Some(code.clone());
                var.transform = Some(parse_transform(&p.var)?.as_str().to_owned());
                var.align = Some(match method {
                    "rollup" => {
                        json!({"method": "rollup", "from_level": source_level, "rule": info.aggregation})
                    }
                    "inherit" => json!({"method": "inherit", "from_level": source_level}),
                    _ => json!({"method": "identity"}),
                });
                let mut input = p.input;
                input.period = code;
                (
                    Source::Indicator {
                        id: info.id.clone(),
                        dataset_key: p.version.dataset_key,
                        indicator_key: info.key,
                        freq,
                        dims,
                        source_level,
                        period,
                        aggregation: info.aggregation.clone(),
                    },
                    method,
                    (var, input),
                )
            }
            PreKind::Grid {
                layer,
                period,
                zonal,
                indicator,
            } => {
                let a = p.var.align.clone().unwrap_or(Value::Null);
                if let Some(m) = a["method"].as_str()
                    && m != "grid_zonal"
                {
                    return Err(bad(
                        format!("a grid variable aligns by grid_zonal, not {m}"),
                        json!({}),
                    ));
                }
                let stat = a["stat"].as_str().map_or_else(
                    || {
                        if zonal == "sum" {
                            "sum".to_owned()
                        } else {
                            "mean".to_owned()
                        }
                    },
                    str::to_owned,
                );
                if !matches!(stat.as_str(), "sum" | "mean" | "max") {
                    return Err(bad(
                        format!("grid_zonal stat {stat:?}: WP-29 supports sum, mean, max"),
                        json!({}),
                    ));
                }
                let cells = a["cells"]
                    .as_str()
                    .unwrap_or(if stat == "sum" { "all" } else { "inside" })
                    .to_owned();
                let mut var = p.var.clone();
                var.period = Some(period.clone());
                var.transform = Some(parse_transform(&p.var)?.as_str().to_owned());
                var.align = Some(json!({"method": "grid_zonal", "stat": stat, "cells": cells}));
                let mut input = p.input;
                input.period = period.clone();
                if stat != "sum" {
                    input.measure_type = "mean".to_owned();
                }
                let estimated = input.class == "estimated" || layer.starts_with("worldpop.");
                (
                    Source::Grid {
                        layer,
                        layer_period: period,
                        dataset_id: p.version.dataset_id.clone(),
                        stat,
                        cells,
                        indicator,
                        estimated,
                    },
                    "grid_zonal",
                    (var, input),
                )
            }
            PreKind::Feat { dataset } => {
                let a = p.var.align.clone().unwrap_or(Value::Null);
                let kinds: Vec<String> = a["kinds"]
                    .as_array()
                    .map(|k| {
                        k.iter()
                            .filter_map(|s| s.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut var = p.var.clone();
                var.transform = Some(parse_transform(&p.var)?.as_str().to_owned());
                var.align =
                    Some(json!({"method": "features_zonal", "stat": "count", "kinds": kinds}));
                let mut input = p.input;
                input.period = p.version.dsv.clone();
                (
                    Source::Features {
                        dataset_id: dataset,
                        measure: "count".to_owned(),
                        kinds,
                    },
                    "features_zonal",
                    (var, input),
                )
            }
        };
        let (var, input) = var;
        if let Some(slot) = resolved_req.variables.get_mut(&p.role) {
            *slot = var.clone();
        }
        vars.push(ResolvedVar {
            role: p.role,
            var,
            source,
            method: method.to_owned(),
            version: p.version,
            input,
        });
    }

    // scale sensitivity (relate): adjacent levels where x and y are identity-alignable
    let mut scale_levels = Vec::new();
    let mut params = params;
    if info.id == "relate" {
        let explicit: Option<Vec<i16>> = params["scale_sensitivity"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_i64().and_then(|l| i16::try_from(l).ok()))
                .collect()
        });
        let candidates = explicit.unwrap_or_else(|| vec![level - 1, level + 1]);
        for l in candidates {
            if l == level || !(0..=max_level).contains(&l) {
                continue;
            }
            if let Some(w) = &within
                && let Ok(pc) = profile.admin.parse(w)
                && i16::from(pc.level()) >= l
            {
                continue;
            }
            let mut ok = true;
            for v in vars.iter().filter(|v| v.role == "x" || v.role == "y") {
                let Source::Indicator {
                    indicator_key,
                    dims,
                    period,
                    ..
                } = &v.source
                else {
                    ok = false;
                    break;
                };
                let d = dims_at(env, &v.version, *indicator_key, l, Some(*period)).await?;
                if !d.contains(dims) {
                    ok = false;
                    break;
                }
            }
            if ok {
                scale_levels.push(l);
            }
        }
        scale_levels.sort_unstable();
        params["scale_sensitivity"] = json!(scale_levels);
    }
    resolved_req.params = params;
    let mut ov = resolved_req.overrides.clone();
    ov.sort();
    ov.dedup();
    resolved_req.overrides = ov;
    Ok(Resolved {
        request: resolved_req,
        method,
        data_version,
        level,
        within,
        vars,
        scale_levels,
    })
}

// ───────────────────────────── align ───────────────────────────────────────

/// `Inputs` over `PgInputs` plus synthetic indicator metadata (the
/// enrichment methods need an indicator for the period of their rows).
struct AnalysisInputs {
    pg: PgInputs,
    extra: HashMap<String, IndicatorMeta>,
}

#[async_trait]
impl Inputs for AnalysisInputs {
    async fn obs(&self, port: &str) -> Result<Arc<Vec<Row>>> {
        self.pg.obs(port).await
    }
    async fn features(&self, port: &str) -> Result<Arc<Vec<Feature>>> {
        self.pg.features(port).await
    }
    async fn units(&self) -> Result<Arc<Units>> {
        self.pg.units().await
    }
    async fn grid(&self, port: &str) -> Result<Arc<GridInput>> {
        self.pg.grid(port).await
    }
    async fn grid_layer(
        &self,
        port: &str,
        layer: &str,
        period: Option<&str>,
    ) -> Result<Arc<GridInput>> {
        self.pg.grid_layer(port, layer, period).await
    }
    fn indicator(&self, id: &str) -> Option<IndicatorMeta> {
        self.extra
            .get(id)
            .cloned()
            .or_else(|| self.pg.indicator(id))
    }
    fn calendars(&self) -> &lanka_core::Calendars {
        self.pg.calendars()
    }
}

const SYNTH_IND: &str = "analysis.aligned";

async fn pg_inputs(env: &Env, pinned: Vec<Pinned>) -> Result<AnalysisInputs> {
    let profile = env.catalog.profile().map_err(|e| anyhow::anyhow!("{e}"))?;
    let pg = PgInputs::new(
        env.pool.clone(),
        profile.calendars.clone(),
        profile.reference_boundary_set(),
        profile.admin_scheme(),
        pinned,
    )
    .await?
    .with_store_root(env.store_root.as_deref());
    let mut extra = HashMap::new();
    extra.insert(
        SYNTH_IND.to_owned(),
        IndicatorMeta {
            id: SYNTH_IND.to_owned(),
            dataset_id: "analysis".to_owned(),
            freq: Freq::P,
            measure_type: "count".to_owned(),
            aggregation: "sum".to_owned(),
            class: "derived".to_owned(),
            numerator: None,
            denominator: None,
            weight: None,
            ci_level: None,
            dims: Vec::new(),
        },
    );
    Ok(AnalysisInputs { pg, extra })
}

fn pinned(port: &str, v: &Version, kind: &str) -> Pinned {
    Pinned {
        port: port.to_owned(),
        dataset_id: v.dataset_id.clone(),
        dataset_key: v.dataset_key,
        version_id: v.version_id,
        seq: v.seq,
        dsv: v.dsv.clone(),
        kind: kind.to_owned(),
        indicators: Vec::new(),
    }
}

/// The support: units of `level` under `within`, sorted, with centroids.
async fn support(env: &Env, units: &Units, level: i16, within: Option<&str>) -> Result<Support> {
    let profile = env.catalog.profile().map_err(|e| anyhow::anyhow!("{e}"))?;
    let set = profile.reference_boundary_set().to_owned();
    let wl = match within {
        Some(w) => Some((
            w,
            i16::from(
                profile
                    .admin
                    .parse(w)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .level(),
            ),
        )),
        None => None,
    };
    let mut codes: Vec<String> = units
        .at_level(level)
        .filter(|u| {
            wl.is_none_or(|(w, l)| units.ancestor_at(&u.code, l).is_some_and(|a| a.code == w))
        })
        .map(|u| u.code.clone())
        .collect();
    codes.sort();
    let pts: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT u.code, ST_X(g.centroid), ST_Y(g.centroid) FROM geo.units u
         JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $1 WHERE u.level = $2",
    )
    .bind(&set)
    .bind(level)
    .fetch_all(&env.pool)
    .await?;
    let by: HashMap<String, (f64, f64)> = pts
        .into_iter()
        .map(|(c, x, y)| (c, (x.unwrap_or(f64::NAN), y.unwrap_or(f64::NAN))))
        .collect();
    let lon = codes
        .iter()
        .map(|c| by.get(c).map_or(f64::NAN, |p| p.0))
        .collect();
    let lat = codes
        .iter()
        .map(|c| by.get(c).map_or(f64::NAN, |p| p.1))
        .collect();
    let parents = codes
        .iter()
        .map(|c| {
            if level > 0 {
                units.ancestor_at(c, level - 1).map(|u| u.code.clone())
            } else {
                None
            }
        })
        .collect();
    Ok(Support {
        kind: "admin".to_owned(),
        scheme: profile.admin_scheme().to_owned(),
        level,
        within: within.map(str::to_owned),
        boundary_set: set,
        codes,
        lon,
        lat,
        parents,
    })
}

fn basis_code(name: &str) -> u8 {
    u8::try_from(BASIS.iter().position(|b| *b == name).unwrap_or(1)).unwrap_or(1)
}

fn coded_at(c: Option<&Coded>, i: usize) -> Option<String> {
    match c? {
        Coded::Uniform(s) => Some(s.clone()),
        Coded::Mixed(v) => v.get(i).copied().flatten().map(|code| {
            Basis::from_code(code).map_or_else(|| code.to_string(), |b| b.as_str().to_owned())
        }),
    }
}

fn status_at(c: Option<&Coded>, i: usize) -> Option<String> {
    match c? {
        Coded::Uniform(s) => Some(s.clone()),
        Coded::Mixed(v) => v.get(i).copied().flatten().map(|code| {
            lanka_core::Status::from_code(code)
                .map_or_else(|| code.to_string(), |s| s.as_str().to_owned())
        }),
    }
}

/// Per code: (value, lo, hi, basis code, suppressed) of an indicator at a level.
type Cells = HashMap<String, (f64, f64, f64, u8, bool)>;

async fn read_frame(
    env: &Env,
    data_version: &str,
    id: &str,
    level: i16,
    dims: &str,
    period: &str,
) -> Result<Cells> {
    let q = FrameQuery {
        indicators: vec![id.to_owned()],
        level,
        scheme: None,
        parent: None,
        periods: vec![period.to_owned()],
        from: None,
        to: None,
        dims: Some(if dims.is_empty() {
            String::new()
        } else {
            format!("{id}:{dims}")
        }),
        pin: FramePin {
            release: Some(data_version.to_owned()),
            dsvs: Vec::new(),
        },
    };
    let fr = frame(&env.pool, &env.catalog, &q)
        .await
        .map_err(|e| anyhow::anyhow!("frame {id}: {e}"))?;
    let want = dims_map(dims);
    let col = fr
        .columns
        .iter()
        .find(|c| c.dims == want && c.period == period)
        .ok_or_else(|| {
            bad(
                format!("\"{id}\" has no data for dims {dims:?} period {period} at level {level}"),
                json!({"indicator": id, "level": level, "period": period}),
            )
        })?;
    let mut out = HashMap::new();
    for (i, code) in fr.codes.iter().enumerate() {
        let v = col.values[i];
        let st = status_at(col.status.as_ref(), i);
        let suppressed = st.as_deref() == Some("suppressed");
        if v.is_none() && !suppressed {
            continue;
        }
        let b = coded_at(col.basis.as_ref(), i).map_or(1, |s| basis_code(&s));
        out.insert(
            code.clone(),
            (
                v.unwrap_or(f64::NAN),
                col.lo.as_ref().and_then(|l| l[i]).unwrap_or(f64::NAN),
                col.hi.as_ref().and_then(|h| h[i]).unwrap_or(f64::NAN),
                b,
                suppressed,
            ),
        );
    }
    Ok(out)
}

/// One variable on the support.
#[allow(clippy::too_many_lines)]
async fn align_var(
    env: &Env,
    r: &Resolved,
    v: &ResolvedVar,
    sup: &Support,
    units: &Units,
    term: &str,
) -> Result<Column> {
    let level = sup.level;
    let n = sup.codes.len();
    let mut values = vec![f64::NAN; n];
    let mut lo = vec![f64::NAN; n];
    let mut hi = vec![f64::NAN; n];
    let mut basis = vec![u8::MAX; n];
    let mut suppressed = vec![false; n];
    let mut params = json!({});
    let profile = env.catalog.profile().map_err(|e| anyhow::anyhow!("{e}"))?;
    let from: String;
    match &v.source {
        Source::Indicator {
            id,
            dims,
            source_level,
            period,
            freq,
            ..
        } => {
            let code = Period {
                freq: *freq,
                start: *period,
            }
            .render(&profile.calendars);
            from = format!("admin:{source_level}");
            match v.method.as_str() {
                "identity" => {
                    let cells = read_frame(env, &r.data_version, id, level, dims, &code).await?;
                    for (i, c) in sup.codes.iter().enumerate() {
                        if let Some(x) = cells.get(c) {
                            (values[i], lo[i], hi[i], basis[i], suppressed[i]) = *x;
                        }
                    }
                }
                "inherit" => {
                    let cells =
                        read_frame(env, &r.data_version, id, *source_level, dims, &code).await?;
                    for (i, c) in sup.codes.iter().enumerate() {
                        if let Some(p) = units.ancestor_at(c, *source_level)
                            && let Some(x) = cells.get(&p.code)
                        {
                            (values[i], lo[i], hi[i], basis[i], suppressed[i]) = *x;
                            basis[i] = basis[i].max(2);
                        }
                    }
                    params = json!({"from_level": source_level});
                }
                "rollup" => {
                    let cells =
                        read_frame(env, &r.data_version, id, *source_level, dims, &code).await?;
                    let rows: Vec<Row> = cells
                        .iter()
                        .map(|(g, (val, l, h, b, s))| Row {
                            indicator: id.clone(),
                            geo: g.clone(),
                            level: *source_level,
                            freq: *freq,
                            start: *period,
                            dims: dims.clone(),
                            value: if *s {
                                None
                            } else {
                                val.is_finite().then_some(*val)
                            },
                            lo: l.is_finite().then_some(*l),
                            hi: h.is_finite().then_some(*h),
                            basis: Basis::from_code(i16::from(*b)).unwrap_or(Basis::Computed),
                            status: if *s {
                                lanka_core::Status::Suppressed
                            } else {
                                lanka_core::Status::Normal
                            },
                            flags: lanka_core::Flags::EMPTY,
                            obs_id: None,
                        })
                        .collect();
                    let mut rows = rows;
                    rows.sort_by(|a, b| a.geo.cmp(&b.geo));
                    let mut mem = MemInputs {
                        units: Arc::new(units.clone()),
                        indicators: load_indicators(&env.pool).await?,
                        calendars: profile.calendars.clone(),
                        ..MemInputs::default()
                    };
                    mem.obs.insert("input".to_owned(), Arc::new(rows));
                    let levels: Vec<i16> = (level..*source_level).collect();
                    let p = json!({"input": "input", "levels": levels});
                    let ctx = MethodCtx {
                        inputs: &mem,
                        rng_seed: 0,
                        as_of: *period,
                        changed_keys: None,
                    };
                    let mut sink = lanka_enrich::method::CollectSink::default();
                    lanka_enrich::methods::rollup::Rollup
                        .run(&ctx, &p, &mut sink)
                        .await?;
                    let by: HashMap<&str, &Row> = sink
                        .rows
                        .iter()
                        .filter(|o| o.row.level == level)
                        .map(|o| (o.row.geo.as_str(), &o.row))
                        .collect();
                    for (i, c) in sup.codes.iter().enumerate() {
                        if let Some(row) = by.get(c.as_str()) {
                            values[i] = row.value.unwrap_or(f64::NAN);
                            lo[i] = row.lo.unwrap_or(f64::NAN);
                            hi[i] = row.hi.unwrap_or(f64::NAN);
                            basis[i] = basis_code(row.basis.as_str()).max(1);
                            suppressed[i] = row.status == lanka_core::Status::Suppressed;
                        }
                    }
                    params = json!({"from_level": source_level, "rule": v.source_aggregation()});
                }
                m => anyhow::bail!("alignment {m} for an indicator"),
            }
        }
        Source::Grid {
            layer,
            layer_period,
            stat,
            cells,
            estimated,
            ..
        } => {
            from = "grid:1".to_owned();
            let inputs = pg_inputs(env, vec![pinned("grid", &v.version, "grid")]).await?;
            let p = json!({"grid": "grid", "layer": layer, "layer_period": layer_period, "indicator": SYNTH_IND,
                           "period": "2000", "stat": stat, "cells": cells, "levels": [level],
                           "basis": if *estimated { "estimated" } else { "computed" }});
            let rows =
                run_enrich(&inputs, &lanka_enrich::methods::grid_zonal::GridZonal, &p).await?;
            for (i, c) in sup.codes.iter().enumerate() {
                if let Some(row) = rows.get(c) {
                    values[i] = row.value.unwrap_or(f64::NAN);
                    basis[i] = if *estimated { 2 } else { 1 };
                }
            }
            params = json!({"layer": layer, "period": layer_period, "stat": stat, "cells": cells});
        }
        Source::Features { measure, kinds, .. } => {
            from = format!("features:{}", v.version.dataset_id);
            let inputs = pg_inputs(env, vec![pinned("features", &v.version, "features")]).await?;
            let mut p = json!({"features": "features", "measure": measure, "indicator": SYNTH_IND,
                               "period": "2000", "levels": [level], "zeros": true});
            if !kinds.is_empty() {
                p["kinds"] = json!(kinds);
            }
            let rows = run_enrich(
                &inputs,
                &lanka_enrich::methods::features_zonal::FeaturesZonal,
                &p,
            )
            .await?;
            for (i, c) in sup.codes.iter().enumerate() {
                if let Some(row) = rows.get(c) {
                    values[i] = row.value.unwrap_or(f64::NAN);
                    basis[i] = 1;
                }
            }
            params = json!({"measure": measure, "kinds": kinds});
        }
    }
    let have = values.iter().filter(|x| x.is_finite()).count();
    #[allow(clippy::cast_precision_loss)]
    let coverage = if n == 0 { 0.0 } else { have as f64 / n as f64 };
    let mut dropped = BTreeMap::new();
    if n > have {
        dropped.insert("units_without_value".to_owned(), (n - have) as u64);
    }
    let any_interval = lo.iter().any(|x| x.is_finite());
    Ok(Column {
        role: v.role.clone(),
        term: term.to_owned(),
        input: v.input.clone(),
        transform: Transform::parse(v.var.transform.as_deref().unwrap_or("identity"))
            .unwrap_or_default(),
        values,
        lo: any_interval.then_some(lo),
        hi: any_interval.then_some(hi),
        basis,
        suppressed,
        alignment: AlignmentRecord {
            role: v.role.clone(),
            from,
            to: sup.label(),
            method: v.method_ref(),
            params,
            coverage,
            quality: (v.method == "inherit").then(|| "D".to_owned()),
            dropped,
            period: v.input.period.clone(),
        },
    })
}

impl ResolvedVar {
    fn source_aggregation(&self) -> String {
        match &self.source {
            Source::Indicator { aggregation, .. } => aggregation.clone(),
            _ => String::new(),
        }
    }
}

async fn run_enrich(
    inputs: &AnalysisInputs,
    m: &dyn Method,
    p: &Value,
) -> Result<HashMap<String, Row>> {
    let ctx = MethodCtx {
        inputs,
        rng_seed: 0,
        as_of: NaiveDate::from_ymd_opt(2000, 1, 1).context("date")?,
        changed_keys: None,
    };
    let mut sink = lanka_enrich::method::CollectSink::default();
    m.run(&ctx, p, &mut sink).await?;
    Ok(sink
        .rows
        .into_iter()
        .map(|o| (o.row.geo.clone(), o.row))
        .collect())
}

/// Term names: x, y, c1, c2, …, weight, group.
fn term_of(role: &str) -> String {
    role.strip_prefix("control:")
        .and_then(|i| i.parse::<usize>().ok())
        .map_or_else(|| role.to_owned(), |i| format!("c{}", i + 1))
}

/// ALIGN: the frame on the target support, the frames at the
/// scale-sensitivity levels, and the exposure overlay when the method needs it.
pub async fn align(
    env: &Env,
    r: &Resolved,
) -> Result<(AlignedFrame, Vec<AlignedFrame>, Option<ExposureFrame>)> {
    let base = pg_inputs(env, Vec::new()).await?;
    let units = base.units().await?;
    let sup = support(env, &units, r.level, r.within.as_deref()).await?;
    if sup.codes.is_empty() {
        return Err(bad(
            format!("no units at level {} under {:?}", r.level, r.within),
            json!({}),
        ));
    }
    let mut cols = Vec::new();
    for v in &r.vars {
        cols.push(align_var(env, r, v, &sup, &units, &term_of(&v.role)).await?);
    }
    let main = AlignedFrame {
        support: sup,
        columns: cols,
    };
    let mut scale = Vec::new();
    for &l in &r.scale_levels {
        let s = support(env, &units, l, r.within.as_deref()).await?;
        let mut cols = Vec::new();
        for v in r.vars.iter().filter(|v| v.role == "x" || v.role == "y") {
            let mut v2 = v.clone();
            v2.method = "identity".to_owned();
            cols.push(align_var(env, r, &v2, &s, &units, &term_of(&v.role)).await?);
        }
        scale.push(AlignedFrame {
            support: s,
            columns: cols,
        });
    }
    let exposure = if r.method.info().id == "exposure" {
        Some(exposure_frame(env, r, &main, &units).await?)
    } else {
        None
    };
    Ok((main, scale, exposure))
}

/// A mapped layer read as the store path reads it (physical value rounded
/// to `f32`), so a result does not depend on where the cells came from.
struct MappedF32(Arc<lanka_grid::MappedGrid>);

impl lanka_grid::CellValues for MappedF32 {
    fn len(&self) -> usize {
        lanka_grid::CellValues::len(&*self.0)
    }
    fn value(&self, i: usize) -> Option<f64> {
        #[allow(clippy::cast_possible_truncation)]
        let v = self.0.value(i)? as f32;
        v.is_finite().then_some(f64::from(v))
    }
}

/// The cells of an `exposure` layer: the host's memory-mapped copy when it
/// holds the layer for the pinned dataset, else the store file.
async fn exposure_cells(
    env: &Env,
    inputs: &AnalysisInputs,
    port: &str,
    layer: &str,
    period: &str,
    version: &Version,
    cells: usize,
) -> Result<Arc<dyn lanka_grid::CellValues + Send + Sync>> {
    if let Some(m) = env.mapped.as_ref().and_then(|f| f(layer, period))
        && m.dataset_id == version.dataset_id
        && lanka_grid::CellValues::len(&*m.grid) == cells
    {
        return Ok(Arc::new(MappedF32(m.grid)));
    }
    let g = inputs.grid_layer(port, layer, Some(period)).await?;
    Ok(Arc::new(StoreF32(g.values.clone())))
}

/// Cells read from the store (`NaN` = nodata).
struct StoreF32(Arc<[f32]>);

impl lanka_grid::CellValues for StoreF32 {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn value(&self, i: usize) -> Option<f64> {
        self.0.value(i)
    }
}

async fn exposure_frame(
    env: &Env,
    r: &Resolved,
    f: &AlignedFrame,
    units: &Units,
) -> Result<ExposureFrame> {
    let hz = r.vars.iter().find(|v| v.role == "x").context("x")?;
    let ex = r
        .vars
        .iter()
        .find(|v| v.role == "weight")
        .context("weight")?;
    let (
        Source::Grid {
            layer: hl,
            layer_period: hp,
            ..
        },
        Source::Grid {
            layer: el,
            layer_period: ep,
            ..
        },
    ) = (&hz.source, &ex.source)
    else {
        return Err(bad(
            "exposure needs a hazard grid as x and an exposed grid as weight",
            json!({}),
        ));
    };
    let threshold = r.request.params["threshold"]
        .as_f64()
        .context("threshold")?;
    let inputs = pg_inputs(
        env,
        vec![
            pinned("hazard", &hz.version, "grid"),
            pinned("exposed", &ex.version, "grid"),
        ],
    )
    .await?;
    let lk = inputs.pg.lookup().await?;
    let h = exposure_cells(
        env,
        &inputs,
        "hazard",
        hl,
        hp,
        &hz.version,
        lk.unit_idx.len(),
    )
    .await?;
    let e = exposure_cells(
        env,
        &inputs,
        "exposed",
        el,
        ep,
        &ex.version,
        lk.unit_idx.len(),
    )
    .await?;
    let n_units = lk.units.len();
    let slots: Vec<u16> = lk
        .unit_idx
        .iter()
        .map(|&u| {
            if u == u32::MAX {
                0
            } else {
                u16::try_from(u + 1).unwrap_or(0)
            }
        })
        .collect();
    let (cols, area) = (lk.cols, lk.row_area.clone());
    let accs = tokio::task::spawn_blocking(move || {
        lanka_grid::exposure::exposure_overlay(&slots, &*h, threshold, &*e, cols, &area, n_units)
    })
    .await?;
    let level = f.support.level;
    let parents: Vec<Vec<String>> = std::iter::once(Vec::new())
        .chain(lk.units.iter().map(|c| {
            units
                .ancestor_at(c, level)
                .map(|u| vec![u.code.clone()])
                .unwrap_or_default()
        }))
        .collect();
    let up: HashMap<String, lanka_grid::exposure::ExposureAcc> =
        lanka_grid::exposure::rollup(&accs, &parents)
            .into_iter()
            .collect();
    let pick = |g: &dyn Fn(&lanka_grid::exposure::ExposureAcc) -> f64| -> Vec<f64> {
        f.support
            .codes
            .iter()
            .map(|c| up.get(c).map_or(f64::NAN, g))
            .collect()
    };
    Ok(ExposureFrame {
        hazard: hl.clone(),
        exposed: el.clone(),
        threshold,
        total: pick(&|a| a.total),
        exposed_in_mask: pick(&|a| a.exposed),
        area_km2: pick(&|a| a.area / 1e6),
        masked_area_km2: pick(&|a| a.masked_area / 1e6),
        cells: f
            .support
            .codes
            .iter()
            .map(|c| up.get(c).map_or(0, |a| a.cells))
            .collect(),
    })
}

#[cfg(test)]
mod mapped_tests {
    use super::{MappedF32, StoreF32};
    use lanka_core::grid::Grid;
    use lanka_grid::{CellValues, Dtype, GridArray, LayerEncoding, MappedGrid};
    use std::sync::Arc;

    /// `exposure` must give the same cells whether a layer comes from the
    /// server's mapped set or from the store file.
    #[test]
    fn mapped_cells_equal_store_cells() {
        let g = Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 6,
            cols: 9,
        };
        #[allow(clippy::cast_precision_loss)]
        let vals: Vec<f32> = (0..54)
            .map(|i| {
                if i % 7 == 3 {
                    f32::NAN
                } else {
                    i as f32 * 0.013
                }
            })
            .collect();
        let dir = std::env::temp_dir().join(format!("lanka-analysis-mm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        for (k, enc) in [
            LayerEncoding {
                dtype: Dtype::U16,
                scale: 0.01,
                offset: 0.0,
                nodata: Some(65_535.0),
            },
            LayerEncoding {
                dtype: Dtype::F32,
                scale: 1.0,
                offset: 0.0,
                nodata: None,
            },
        ]
        .into_iter()
        .enumerate()
        {
            let a = GridArray::quantise(g, enc, &vals).expect("quantise");
            let p = dir.join(format!("l{k}.bin"));
            a.write_bin(&p, serde_json::Value::Null).expect("write");
            let (read, _) = GridArray::read_bin(&p).expect("read");
            #[allow(clippy::cast_possible_truncation)]
            let store: Arc<[f32]> = (0..read.len())
                .map(|i| read.value(i).map_or(f32::NAN, |v| v as f32))
                .collect();
            let store = StoreF32(store);
            let mapped = MappedF32(Arc::new(MappedGrid::open(&p).expect("map")));
            assert_eq!(mapped.len(), store.len());
            for i in 0..store.len() {
                assert_eq!(
                    mapped.value(i).map(f64::to_bits),
                    store.value(i).map(f64::to_bits),
                    "{enc:?} cell {i}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
