//! Resolution of a document against the catalogue (10 §2.3): the
//! `resolved` block a renderer uses and never edits — dataset versions
//! (pinned release, exact `{dsv}` or latest), data URLs, attribution,
//! notices and repair problems. Pure over a catalogue snapshot plus the
//! release/version rows `load_context` reads, so the same function serves
//! `lanka map resolve`, the app plane (WP-28) and tests.
//!
//! Data shapes (10 §3, WP-27): frames (`indicator` sources, `/v2/frame`),
//! boundaries, `values` (grid sources: `ldl-v16` value tiles), `dem` (the
//! raster-dem tileset for `grid: "dem"`), and for `features` sources the
//! shape chosen by the size rules of 10 §3.3 ([`feature_shape`]):
//! ≤ 2,000 features → `geojson`; points ≤ 250,000 → `points` (Arrow);
//! anything larger or with lines/polygons → `tiles` (PMTiles). `analysis`
//! sources resolve to `shape: "unavailable"` with `NOT_SERVED` until WP-29.

use std::collections::{BTreeMap, BTreeSet};

use lanka_core::Catalog;
use lanka_core::frame::{PinnedVersion, dsv_versions, release_versions};
use lanka_core::{CoreResult, Period};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::doc::{
    Extent, IndicatorSource, LevelRef, MapDocument, PeriodRef, Pin, Source, VersionRef,
};
use crate::refs::{host_of, is_workspace};
use crate::validate::ptr;

/// Release and version rows a resolution needs.
#[derive(Debug, Clone, Default)]
pub struct ResolveContext {
    /// The document pin's release and its versions (`None` = latest, or
    /// the release does not exist).
    pub release: Option<(String, Vec<PinnedVersion>)>,
    /// The pinned release was not found.
    pub release_missing: bool,
    /// Versions named by `{dsv}` sources.
    pub dsvs: Vec<PinnedVersion>,
    /// Served grid layers (`geo.grid_layers`, `serve = true`).
    pub grids: Vec<GridLayerRef>,
    /// Built tilesets (newest build of each).
    pub tilesets: Vec<TilesetRef>,
    /// Per `features` source id: the size of its filtered set.
    pub features: BTreeMap<String, FeatureCount>,
    /// Declared feature fields per dataset.
    pub fields: BTreeMap<String, Vec<FieldInfo>>,
}

/// A served grid layer-period, as the resolver needs it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayerRef {
    pub id: String,
    pub period: String,
    pub dataset_id: String,
    pub indicator_id: Option<String>,
    /// `k=v;…` (as `geo.grid_layers.dims`).
    pub dims: String,
    pub sha16: String,
    pub dtype: String,
    pub scale: f64,
    pub offset: f64,
    pub nodata: Option<f64>,
    pub unit: String,
    pub overview: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub count: Option<f64>,
    pub sum: Option<f64>,
    /// Class codes of categorical layers (`style.classes` keys).
    pub classes: Vec<String>,
}

/// A built tileset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TilesetRef {
    pub id: String,
    pub sha16: String,
    pub minzoom: u8,
    pub maxzoom: u8,
    /// `(layer id, dataset id)`.
    pub layers: Vec<(String, String)>,
    /// Lowest zoom with features, per layer id (absent: the tileset's).
    pub layer_minzoom: BTreeMap<String, u8>,
}

/// Size of a `features` source's filtered set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeatureCount {
    pub count: u64,
    /// Every feature is a (multi)point.
    pub points_only: bool,
}

/// A served feature field (ui-contract §4 `FieldInfo`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldInfo {
    pub id: String,
    pub label: String,
    /// number | category | string | date
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure_type: Option<String>,
}

/// Largest set served as GeoJSON (`/v2/features/{ds}`).
pub const GEOJSON_MAX: u64 = 2_000;
/// Largest point set served as columnar points.
pub const POINTS_MAX: u64 = 250_000;

/// The delivery shape of a features source by the size rules of 10 §3.3:
/// `geojson` (≤ 2,000), `points` (points only, ≤ 250,000), else `tiles`
/// (`None` when there is no tileset for it either).
#[must_use]
pub fn feature_shape(count: u64, points_only: bool, has_tiles: bool) -> Option<&'static str> {
    if count <= GEOJSON_MAX {
        Some("geojson")
    } else if points_only && count <= POINTS_MAX {
        Some("points")
    } else if has_tiles {
        Some("tiles")
    } else {
        None
    }
}

/// Reads what `resolve` needs from Postgres.
pub async fn load_context(pool: &PgPool, doc: &MapDocument) -> CoreResult<ResolveContext> {
    let mut ctx = ResolveContext::default();
    if let Some(r) = doc.pin.release() {
        match release_versions(pool, r).await? {
            Some(v) => ctx.release = Some((r.to_owned(), v)),
            None => ctx.release_missing = true,
        }
    }
    let wanted: Vec<String> = doc
        .sources
        .values()
        .filter_map(|s| match s {
            Source::Indicator(i) => i.version.as_ref(),
            Source::Boundaries(b) => b.version.as_ref(),
            Source::Features(f) => f.version.as_ref(),
            Source::Grid(g) => g.version.as_ref(),
            _ => None,
        })
        .filter_map(|v| match v {
            VersionRef::Dsv { dsv } => Some(dsv.clone()),
            VersionRef::Latest(_) => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !wanted.is_empty() {
        ctx.dsvs = dsv_versions(pool, &wanted).await?;
    }
    load_layers(pool, doc, &mut ctx).await?;
    Ok(ctx)
}

type GridRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    f64,
    f64,
    Option<f64>,
    String,
    String,
    Option<Value>,
    Option<Value>,
);

/// Grid layers, tilesets, feature counts and fields (WP-27).
async fn load_layers(pool: &PgPool, doc: &MapDocument, ctx: &mut ResolveContext) -> CoreResult<()> {
    let has = |k: &str| doc.sources.values().any(|s| s.kind() == k);
    if has("grid") {
        let rows: Vec<GridRow> = sqlx::query_as(
            "SELECT id, period, dataset_id, indicator_id, dims, left(encode(sha256, 'hex'), 16), dtype, scale,
                    \"offset\", nodata, unit, overview, stats, style
             FROM geo.grid_layers WHERE serve ORDER BY id, period",
        )
        .fetch_all(pool)
        .await?;
        ctx.grids = rows
            .into_iter()
            .map(|r| {
                let st = |k: &str| r.12.as_ref().and_then(|s| s.get(k)).and_then(Value::as_f64);
                GridLayerRef {
                    min: st("min"),
                    max: st("max"),
                    count: st("count"),
                    sum: st("sum"),
                    classes: r
                        .13
                        .as_ref()
                        .and_then(|s| s.get("classes"))
                        .and_then(Value::as_object)
                        .map(|o| o.keys().cloned().collect())
                        .unwrap_or_default(),
                    id: r.0,
                    period: r.1,
                    dataset_id: r.2,
                    indicator_id: r.3,
                    dims: r.4,
                    sha16: r.5,
                    dtype: r.6,
                    scale: r.7,
                    offset: r.8,
                    nodata: r.9,
                    unit: r.10,
                    overview: r.11,
                }
            })
            .collect();
    }
    if has("grid") || has("features") {
        let rows: Vec<(String, String, i16, i16, Value)> = sqlx::query_as(
            "SELECT id, sha16, minzoom, maxzoom, layers FROM geo.current_tilesets ORDER BY id",
        )
        .fetch_all(pool)
        .await?;
        ctx.tilesets = rows
            .into_iter()
            .map(|(id, sha16, minz, maxz, layers)| TilesetRef {
                id,
                sha16,
                minzoom: u8::try_from(minz).unwrap_or(0),
                maxzoom: u8::try_from(maxz).unwrap_or(0),
                layers: layers
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| {
                                Some((
                                    l.get("id")?.as_str()?.to_owned(),
                                    l.get("dataset")?.as_str()?.to_owned(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                layer_minzoom: layers
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| {
                                Some((
                                    l.get("id")?.as_str()?.to_owned(),
                                    u8::try_from(l.get("minzoom")?.as_u64()?).ok()?,
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
    }
    for (sid, s) in &doc.sources {
        let Source::Features(f) = s else { continue };
        if is_workspace(&f.dataset) {
            continue;
        }
        let within: Option<i32> = match f.within.as_deref() {
            Some(code) => {
                sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                    .bind(code)
                    .fetch_optional(pool)
                    .await?
            }
            None => None,
        };
        let kinds: Vec<String> = f
            .feature_kind
            .as_deref()
            .map(|k| k.split(',').map(|x| x.trim().to_owned()).collect())
            .unwrap_or_default();
        let row: Option<(i64, bool)> = sqlx::query_as(
            "SELECT count(*), coalesce(bool_and(GeometryType(f.geom) IN ('POINT', 'MULTIPOINT')), true)
             FROM geo.features f JOIN catalog.datasets d ON d.key = f.dataset_key
             JOIN data.current_versions c ON c.dataset_id = d.id
             LEFT JOIN geo.units u ON u.key = f.unit_key
             WHERE d.id = $1 AND f.vto = 2147483647 AND f.vfrom <= c.seq
               AND (cardinality($2::text[]) = 0 OR f.kind = ANY($2))
               AND ($3::int4 IS NULL OR f.unit_key = $3 OR $3 = ANY(u.anc_keys))",
        )
        .bind(&f.dataset)
        .bind(&kinds)
        .bind(within)
        .fetch_optional(pool)
        .await?;
        if let Some((n, points_only)) = row {
            ctx.features.insert(
                sid.clone(),
                FeatureCount {
                    count: u64::try_from(n).unwrap_or(0),
                    points_only,
                },
            );
        }
        if !ctx.fields.contains_key(&f.dataset) {
            let v: Option<Value> =
                sqlx::query_scalar("SELECT spec -> 'fields' FROM catalog.datasets WHERE id = $1")
                    .bind(&f.dataset)
                    .fetch_optional(pool)
                    .await?
                    .flatten();
            let fields = v
                .as_ref()
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            let name = x.get("name")?.as_str()?.to_owned();
                            let ty = match x.get("type").and_then(Value::as_str) {
                                Some("integer" | "number") => "number",
                                _ if x.get("enum").is_some() => "category",
                                _ => "string",
                            };
                            Some(FieldInfo {
                                label: name.replace('_', " "),
                                id: name,
                                ty: ty.to_owned(),
                                unit: x.get("unit").and_then(Value::as_str).map(str::to_owned),
                                measure_type: x
                                    .get("measure_type")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            ctx.fields.insert(f.dataset.clone(), fields);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VersionUsed {
    pub dataset: String,
    pub dsv: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoomFrame {
    /// From this zoom on (until the next entry).
    pub zoom: f64,
    pub level: u8,
    pub frame: String,
    pub source_layer: String,
    /// Lowest zoom at which the tileset holds `source_layer`.
    pub minzoom: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    Frame {
        frame: String,
        tileset: String,
        tiles: String,
        source_layer: String,
        join: String,
        centroids_layer: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        by_zoom: Option<Vec<ZoomFrame>>,
        /// TileJSON of the boundary tileset (its `vector_layers` state each
        /// layer's zoom range); over-zoom beyond `maxzoom`.
        tilejson: String,
        /// Lowest zoom at which the tileset holds `source_layer`.
        minzoom: u8,
        maxzoom: u8,
    },
    Boundaries {
        tiles: String,
        source_layer: String,
        tilejson: String,
        /// Lowest zoom at which the tileset holds `source_layer`.
        minzoom: u8,
        maxzoom: u8,
    },
    /// A grid as `ldl-v16` value tiles (10 §3.2).
    Values {
        values: String,
        encoding: String,
        scale: f64,
        offset: f64,
        nodata: u16,
        minzoom: u8,
        maxzoom: u8,
        /// Largest |decoded − source| at the native zoom (0 = exact).
        max_error: f64,
        unit: String,
        layer: String,
        period: String,
        /// Overview reducer below the native zoom.
        reducer: String,
        stats: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        categories: Option<BTreeMap<u16, String>>,
    },
    /// Columnar points (Arrow IPC; `&format=json` ≤ 2,000).
    Points {
        points: String,
        count: u64,
        fields: Vec<FieldInfo>,
    },
    /// Vector tiles of a features dataset.
    Tiles {
        tiles: String,
        tilejson: String,
        source_layers: Vec<String>,
        minzoom: u8,
        maxzoom: u8,
        count: u64,
        fields: Vec<FieldInfo>,
    },
    /// GeoJSON (≤ 2,000 features), a bare FeatureCollection.
    Geojson {
        url: String,
        count: u64,
        fields: Vec<FieldInfo>,
    },
    /// The raster-dem tileset (Terrarium PNG) for `hillshade` layers.
    Dem {
        tiles: String,
        tilejson: String,
        encoding: String,
        minzoom: u8,
        maxzoom: u8,
    },
    Inline,
    External {
        url: String,
    },
    /// Not drawable here: denied, not found, withdrawn or not served yet;
    /// the reason is in `problems`.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedSource {
    /// public | granted | denied
    pub access: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dsv: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<VersionUsed>,
    /// official | third_party | derived | estimated (the least official input).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<Vec<String>>,
    #[serde(flatten)]
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAnalysis {
    pub id: Option<String>,
    /// queued | running | ready | failed | blocked | unavailable
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Attribution {
    pub source_id: String,
    pub publisher: String,
    pub text: String,
    pub licence: String,
    pub licence_url: Option<String>,
    /// open | attribution | permission_unclear | restricted
    pub licence_status: String,
    /// bulk | api | link | internal
    pub redistribution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Notice {
    /// ESTIMATE | COMMUNITY_DATA | PRIVATE_DATA | SYNTHETIC | EXTERNAL | LICENCE_UNCLEAR
    pub code: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolveProblem {
    pub path: String,
    /// WITHDRAWN | NOT_FOUND | RENAMED | LEVEL_UNAVAILABLE | PERIOD_UNAVAILABLE | NOT_SERVED
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub data_version: Option<String>,
    pub sources: BTreeMap<String, ResolvedSource>,
    pub analyses: BTreeMap<String, ResolvedAnalysis>,
    pub attribution: Vec<Attribution>,
    pub notices: Vec<Notice>,
    pub problems: Vec<ResolveProblem>,
}

/// The document with its `resolved` block (what the renderer receives).
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedMap<'a> {
    #[serde(flatten)]
    pub doc: &'a MapDocument,
    pub resolved: Resolved,
}

/// Percent-encodes a query value (RFC 3986 unreserved plus `,` kept).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b',') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `level` → the indicator's `geo_levels` vocabulary.
fn level_name(level: u8) -> String {
    if level == 0 {
        "national".to_owned()
    } else {
        format!("adm{level}")
    }
}

fn admin_tiles() -> String {
    // The admin tileset route of 05 §9 (built by WP-04).
    "/v2/tiles/admin/{z}/{x}/{y}.mvt".to_owned()
}

/// Max zoom of the admin tileset (the WP-04 build: z0–12) unless the
/// context knows the built one.
/// Lowest zoom at which the admin tileset holds `layer` (e.g. `adm4` from z9).
fn admin_minzoom(ctx: &ResolveContext, layer: &str) -> u8 {
    ctx.tilesets
        .iter()
        .find(|t| t.id == "admin")
        .map_or(0, |t| {
            t.layer_minzoom.get(layer).copied().unwrap_or(t.minzoom)
        })
}

fn admin_maxzoom(ctx: &ResolveContext) -> u8 {
    ctx.tilesets
        .iter()
        .find(|t| t.id == "admin")
        .map_or(12, |t| t.maxzoom)
}

/// `k=v;…` as a sorted map with lower-cased values (dims codes are
/// lower-case in the registry; documents may write `RP100`).
fn dims_map(s: &str) -> BTreeMap<String, String> {
    s.split(';')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_lowercase()))
        .collect()
}

struct Builder<'a> {
    cat: &'a Catalog,
    ctx: &'a ResolveContext,
    doc: &'a MapDocument,
    problems: Vec<ResolveProblem>,
    notices: BTreeSet<Notice>,
    datasets: BTreeSet<String>,
    external: Vec<(String, String)>,
}

impl Builder<'_> {
    fn problem(&mut self, path: String, code: &'static str, hint: Option<Value>) {
        self.problems.push(ResolveProblem { path, code, hint });
    }

    /// The version of `dataset` a source reads: `(seq-free dsv, pinned?)`;
    /// `Err(())` after recording a problem.
    fn version_of(
        &mut self,
        at: &str,
        dataset: &str,
        v: Option<&VersionRef>,
    ) -> Result<(String, VersionMode), ()> {
        let current = self
            .cat
            .datasets
            .get(dataset)
            .and_then(|d| d.current.as_ref());
        match v {
            Some(VersionRef::Dsv { dsv }) => match self.ctx.dsvs.iter().find(|p| &p.dsv == dsv) {
                Some(p) if p.dataset_id != dataset => {
                    self.problem(format!("{at}/version"), "NOT_FOUND", Some(json!({ "reason": format!("{dsv} is a version of {}, not {dataset}", p.dataset_id) })));
                    Err(())
                }
                Some(p) if p.status == "withdrawn" => {
                    let latest = current.map(|c| c.dsv.clone());
                    self.problem(
                        format!("{at}/version"),
                        "WITHDRAWN",
                        Some(json!({ "successor": latest })),
                    );
                    Err(())
                }
                Some(p) => Ok((p.dsv.clone(), VersionMode::Dsv(p.dsv.clone()))),
                None => {
                    self.problem(
                        format!("{at}/version"),
                        "NOT_FOUND",
                        Some(json!({ "dsv": dsv, "latest": current.map(|c| c.dsv.clone()) })),
                    );
                    Err(())
                }
            },
            Some(VersionRef::Latest(_)) => current
                .map(|c| (c.dsv.clone(), VersionMode::Latest))
                .ok_or_else(|| {
                    self.problem(
                        at.to_owned(),
                        "NOT_FOUND",
                        Some(json!({ "reason": format!("{dataset} has no published version") })),
                    );
                }),
            None => match &self.ctx.release {
                Some((r, rows)) => match rows.iter().find(|p| p.dataset_id == dataset) {
                    Some(p) => Ok((p.dsv.clone(), VersionMode::Release(r.clone()))),
                    None => {
                        self.problem(at.to_owned(), "NOT_FOUND", Some(json!({ "reason": format!("{dataset} is not in release {r}"), "release": r })));
                        Err(())
                    }
                },
                None => current
                    .map(|c| (c.dsv.clone(), VersionMode::Latest))
                    .ok_or_else(|| {
                        self.problem(
                            at.to_owned(),
                            "NOT_FOUND",
                            Some(
                                json!({ "reason": format!("{dataset} has no published version") }),
                            ),
                        );
                    }),
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn indicator(&mut self, id: &str, s: &IndicatorSource) -> ResolvedSource {
        let at = format!("/sources/{}", ptr(id));
        let ids = s.indicator_ids();
        let unavailable = |class: Option<String>| ResolvedSource {
            access: "public",
            dsv: None,
            versions: Vec::new(),
            class,
            basis: None,
            shape: Shape::Unavailable,
        };
        if ids.iter().any(|i| is_workspace(i)) {
            self.notices.insert(Notice { code: "PRIVATE_DATA", text: "Some layers use private workspace data; they are shown only to members of that workspace.".to_owned() });
            return ResolvedSource {
                access: "denied",
                ..unavailable(None)
            };
        }
        let levels = s.level.levels();
        let mut versions: Vec<VersionUsed> = Vec::new();
        let mut modes: Vec<VersionMode> = Vec::new();
        let mut classes: Vec<String> = Vec::new();
        let mut ok = true;
        for (k, ind_id) in ids.iter().enumerate() {
            let iat = if s.indicators.is_some() {
                format!("{at}/indicators/{k}")
            } else {
                format!("{at}/indicator")
            };
            let Some(ind) = self.cat.indicators.get(*ind_id) else {
                self.problem(iat, "NOT_FOUND", Some(json!({ "indicator": ind_id })));
                ok = false;
                continue;
            };
            for l in &levels {
                if !ind.geo_levels.contains(&level_name(*l)) {
                    let avail: Vec<u8> = (0..=9)
                        .filter(|x| ind.geo_levels.contains(&level_name(*x)))
                        .collect();
                    self.problem(
                        format!("{at}/level"),
                        "LEVEL_UNAVAILABLE",
                        Some(json!({ "indicator": ind_id, "available_levels": avail })),
                    );
                    ok = false;
                }
            }
            if let Ok(freq) = ind.frequency()
                && let Ok(profile) = self.cat.profile()
            {
                let codes: Vec<&str> = match &s.period {
                    PeriodRef::Code(c) if c != "latest" => vec![c.as_str()],
                    PeriodRef::List(v) => v.iter().map(String::as_str).collect(),
                    PeriodRef::Range { from, to } => vec![from.as_str(), to.as_str()],
                    _ => Vec::new(),
                };
                for c in codes {
                    if Period::parse(freq, c, &profile.calendars).is_err() {
                        self.problem(
                            format!("{at}/period"),
                            "PERIOD_UNAVAILABLE",
                            Some(json!({ "indicator": ind_id, "period": c, "freq": ind.freq })),
                        );
                        ok = false;
                    }
                }
            }
            classes.push(ind.class.clone());
            match self.version_of(&at, &ind.dataset_id, s.version.as_ref()) {
                Ok((dsv, mode)) => {
                    if !versions.iter().any(|v| v.dataset == ind.dataset_id) {
                        versions.push(VersionUsed {
                            dataset: ind.dataset_id.clone(),
                            dsv,
                        });
                        self.datasets.insert(ind.dataset_id.clone());
                    }
                    modes.push(mode);
                }
                Err(()) => ok = false,
            }
        }
        let parent = s.parent.clone().or_else(|| match &self.doc.extent {
            Some(Extent::Geo { geo }) => Some(geo.clone()),
            _ => None,
        });
        if let (Some(p), Ok(profile)) = (&parent, self.cat.profile()) {
            match profile.admin.parse(p) {
                Ok(pc) if levels.iter().all(|l| pc.level() < *l) => {}
                Ok(_) => {
                    self.problem(
                        format!("{at}/parent"),
                        "LEVEL_UNAVAILABLE",
                        Some(json!({ "reason": format!("{p} is not above level {levels:?}") })),
                    );
                    ok = false;
                }
                Err(e) => {
                    self.problem(
                        format!("{at}/parent"),
                        "NOT_FOUND",
                        Some(json!({ "reason": e.to_string() })),
                    );
                    ok = false;
                }
            }
        }
        // dims: a key must be a dimension of one of the indicators, a code
        // one that is published (compared without regard to case).
        for (k, v) in s.dims.iter().flatten() {
            let known = ids.iter().any(|i| {
                self.cat
                    .indicators
                    .get(*i)
                    .is_some_and(|x| x.dims.iter().any(|d| d == k))
            });
            if !known {
                let valid: BTreeMap<String, Vec<String>> = ids
                    .iter()
                    .filter_map(|i| {
                        self.cat
                            .indicators
                            .get(*i)
                            .map(|x| (i.to_string(), x.dims.clone()))
                    })
                    .collect();
                self.problem(
                    format!("{at}/dims/{}", ptr(k)),
                    "NOT_FOUND",
                    Some(json!({ "reason": format!("{k} is not a dimension of {}", ids.join(", ")), "valid_dims": valid })),
                );
                ok = false;
                continue;
            }
            let codes = self.cat.dims.codes_of(k);
            if !codes.is_empty() && self.cat.dims.canonical_code(k, v).is_none() {
                self.problem(
                    format!("{at}/dims/{}", ptr(k)),
                    "NOT_FOUND",
                    Some(json!({ "reason": format!("no data for {k}={v}"), "valid": codes })),
                );
                ok = false;
            }
        }
        let class = least_official(&classes);
        if class.as_deref() == Some("estimated") {
            self.notices.insert(Notice { code: "ESTIMATE", text: "Some layers show modelled estimates, not official statistics; their legends carry the estimate badge.".to_owned() });
        }
        if !ok {
            return unavailable(class);
        }
        // One prefix for the whole source: a release when every dataset is
        // read at the document's release, else the current release, with
        // `dsv=` naming exact versions.
        let release = modes.iter().find_map(|m| match m {
            VersionMode::Release(r) => Some(r.clone()),
            _ => None,
        });
        let all_release = release.is_some()
            && modes
                .iter()
                .all(|m| matches!(m, VersionMode::Release(_) | VersionMode::Dsv(_)));
        let prefix = match (&release, all_release) {
            (Some(r), true) => format!("/v2/r/{r}/frame"),
            _ => "/v2/frame".to_owned(),
        };
        let mut dsvs: Vec<String> = modes
            .iter()
            .filter_map(|m| match m {
                VersionMode::Dsv(d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        if release.is_some() && !all_release {
            // Mixed pins: name every release-pinned version exactly.
            for (m, v) in modes.iter().zip(&versions) {
                if matches!(m, VersionMode::Release(_)) {
                    dsvs.push(v.dsv.clone());
                }
            }
        }
        dsvs.sort();
        dsvs.dedup();
        let url = |level: u8| {
            let mut q = vec![
                format!("indicators={}", enc(&ids.join(","))),
                format!("level={level}"),
            ];
            if let Some(p) = &parent {
                q.push(format!("parent={}", enc(p)));
            }
            match &s.period {
                PeriodRef::Code(c) if c == "latest" => {}
                PeriodRef::Code(c) => q.push(format!("periods={}", enc(c))),
                PeriodRef::List(v) => q.push(format!("periods={}", enc(&v.join(",")))),
                PeriodRef::Range { from, to } => {
                    q.push(format!("from={}", enc(from)));
                    q.push(format!("to={}", enc(to)));
                }
                PeriodRef::Bind { .. } => {
                    // The frame carries every period of the time range (10 §2.7).
                    if let Some(t) = &self.doc.time {
                        q.push(format!("from={}", enc(&t.range[0])));
                        q.push(format!("to={}", enc(&t.range[1])));
                    }
                }
            }
            if let Some(d) = s.dims.as_ref().filter(|d| !d.is_empty()) {
                // Codes in their stored spelling (`RP100` → `rp100`): the
                // frame compares codes exactly as published.
                let canon: Vec<String> = d
                    .iter()
                    .map(|(k, v)| {
                        format!("{k}={}", self.cat.dims.canonical_code(k, v).unwrap_or(v))
                    })
                    .collect();
                q.push(format!("dims={}", enc(&canon.join(";"))));
            }
            if !dsvs.is_empty() {
                q.push(format!("dsv={}", enc(&dsvs.join(","))));
            }
            format!("{prefix}?{}", q.join("&"))
        };
        let first = levels.first().copied().unwrap_or(0);
        let by_zoom = match &s.level {
            LevelRef::ByZoom { by_zoom } => Some(
                by_zoom
                    .iter()
                    .map(|(z, l)| ZoomFrame {
                        zoom: *z,
                        level: *l,
                        frame: url(*l),
                        source_layer: format!("adm{l}"),
                        minzoom: admin_minzoom(self.ctx, &format!("adm{l}")),
                    })
                    .collect(),
            ),
            LevelRef::Level(_) => None,
        };
        self.datasets
            .extend(self.cat.boundary_dataset().map(str::to_owned));
        ResolvedSource {
            access: "public",
            dsv: (versions.len() == 1).then(|| versions[0].dsv.clone()),
            versions,
            basis: (class.as_deref() == Some("estimated")).then(|| vec!["estimated".to_owned()]),
            class,
            shape: Shape::Frame {
                frame: url(first),
                tileset: "admin".to_owned(),
                tiles: admin_tiles(),
                source_layer: format!("adm{first}"),
                join: "pcode".to_owned(),
                centroids_layer: "centroids".to_owned(),
                by_zoom,
                tilejson: "/v2/tiles/admin.json".to_owned(),
                minzoom: admin_minzoom(self.ctx, &format!("adm{first}")),
                maxzoom: admin_maxzoom(self.ctx),
            },
        }
    }

    /// A grid source: `dem` → the raster-dem tileset; otherwise a served
    /// layer by id, or by indicator + dims, at the period asked.
    #[allow(clippy::too_many_lines)]
    fn grid(&mut self, id: &str, g: &crate::doc::GridSource) -> ResolvedSource {
        let at = format!("/sources/{}", ptr(id));
        let unavailable = ResolvedSource {
            access: "public",
            dsv: None,
            versions: Vec::new(),
            class: None,
            basis: None,
            shape: Shape::Unavailable,
        };
        if g.grid == "dem" {
            let Some(t) = self.ctx.tilesets.iter().find(|t| t.id == "dem") else {
                self.problem(
                    format!("{at}/grid"),
                    "NOT_SERVED",
                    Some(json!({ "reason": "the dem tileset is not built (lanka tiles build --tileset dem)" })),
                );
                return unavailable;
            };
            let ds = t
                .layers
                .first()
                .map_or_else(|| "copernicus-dem-terrain".to_owned(), |l| l.1.clone());
            let versions = match self.version_of(&at, &ds, g.version.as_ref()) {
                Ok((dsv, _)) => vec![VersionUsed {
                    dataset: ds.clone(),
                    dsv,
                }],
                Err(()) => Vec::new(),
            };
            self.datasets.insert(ds);
            return ResolvedSource {
                access: "public",
                dsv: versions.first().map(|v| v.dsv.clone()),
                versions,
                class: Some("third_party".to_owned()),
                basis: None,
                shape: Shape::Dem {
                    tiles: "/v2/tiles/dem/{z}/{x}/{y}.png".to_owned(),
                    tilejson: "/v2/tiles/dem.json".to_owned(),
                    encoding: "terrarium".to_owned(),
                    minzoom: t.minzoom,
                    maxzoom: t.maxzoom,
                },
            };
        }
        let want = g
            .dims
            .as_ref()
            .map(|d| {
                d.iter()
                    .map(|(k, v)| (k.clone(), v.to_lowercase()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let mut cands: Vec<&GridLayerRef> = self
            .ctx
            .grids
            .iter()
            .filter(|l| {
                l.id == g.grid
                    || (l.indicator_id.as_deref() == Some(g.grid.as_str())
                        && want
                            .iter()
                            .all(|(k, v)| dims_map(&l.dims).get(k) == Some(v)))
            })
            .collect();
        if cands.is_empty() {
            let known = self.cat.indicators.contains_key(&g.grid);
            let avail: Vec<String> = self
                .ctx
                .grids
                .iter()
                .filter(|l| l.indicator_id.as_deref() == Some(g.grid.as_str()))
                .map(|l| format!("{} ({})", l.id, l.dims))
                .collect();
            self.problem(
                format!("{at}/grid"),
                if known && avail.is_empty() {
                    "NOT_SERVED"
                } else {
                    "NOT_FOUND"
                },
                Some(json!({ "grid": g.grid, "dims": want, "available": avail })),
            );
            return unavailable;
        }
        // One layer: the id match, else a single dims match.
        let ids: BTreeSet<&str> = cands.iter().map(|l| l.id.as_str()).collect();
        if ids.len() > 1 {
            self.problem(
                format!("{at}/dims"),
                "NOT_FOUND",
                Some(json!({ "reason": "the dims match several layers", "layers": ids })),
            );
            return unavailable;
        }
        cands.sort_by(|a, b| a.period.cmp(&b.period));
        let l = match &g.period {
            PeriodRef::Code(c) if c != "latest" => match cands.iter().find(|l| &l.period == c) {
                Some(l) => *l,
                None => {
                    self.problem(
                        format!("{at}/period"),
                        "PERIOD_UNAVAILABLE",
                        Some(json!({ "period": c, "available": cands.iter().map(|l| &l.period).collect::<Vec<_>>() })),
                    );
                    return unavailable;
                }
            },
            // latest, bound to time, lists and ranges: the newest period
            // (value tiles carry one period; time stepping fetches per period).
            _ => cands[cands.len() - 1],
        };
        let versions = match self.version_of(&at, &l.dataset_id, g.version.as_ref()) {
            Ok((dsv, _)) => vec![VersionUsed {
                dataset: l.dataset_id.clone(),
                dsv,
            }],
            Err(()) => Vec::new(),
        };
        self.datasets.insert(l.dataset_id.clone());
        let class = l
            .indicator_id
            .as_ref()
            .and_then(|i| self.cat.indicators.get(i))
            .map(|i| i.class.clone())
            .or_else(|| Some("third_party".to_owned()));
        if class.as_deref() == Some("estimated") {
            self.notices.insert(Notice { code: "ESTIMATE", text: "Some layers show modelled estimates, not official statistics; their legends carry the estimate badge.".to_owned() });
        }
        let layer_enc = lanka_grid::LayerEncoding {
            dtype: lanka_grid::Dtype::parse(&l.dtype).unwrap_or(lanka_grid::Dtype::F32),
            scale: l.scale,
            offset: l.offset,
            nodata: l.nodata,
        };
        let q = lanka_grid::values::Quant::for_layer(&layer_enc, l.min, l.max);
        #[allow(clippy::cast_precision_loss)]
        let mean = match (l.sum, l.count) {
            (Some(s), Some(n)) if n > 0.0 => Some(s / n),
            _ => None,
        };
        let categories = (l.overview == "mode").then(|| {
            l.classes
                .iter()
                .filter_map(|c| c.parse::<f64>().ok().map(|v| (q.q(v), c.clone())))
                .collect()
        });
        ResolvedSource {
            access: "public",
            dsv: versions.first().map(|v| v.dsv.clone()),
            versions,
            basis: (class.as_deref() == Some("estimated")).then(|| vec!["estimated".to_owned()]),
            class,
            shape: Shape::Values {
                values: format!(
                    "/v2/grids/{}/values/{{z}}/{{x}}/{{y}}.png?period={}&v={}",
                    l.id,
                    enc(&l.period),
                    l.sha16
                ),
                encoding: lanka_grid::values::ENCODING.to_owned(),
                scale: q.scale,
                offset: q.offset,
                nodata: lanka_grid::values::NODATA_Q,
                minzoom: 5,
                maxzoom: 11,
                max_error: q.max_error,
                unit: l.unit.clone(),
                layer: l.id.clone(),
                period: l.period.clone(),
                reducer: l.overview.clone(),
                stats: json!({ "n": l.count, "min": l.min, "max": l.max, "mean": mean }),
                categories,
            },
        }
    }

    /// A features source by the size rules of 10 §3.3.
    fn features(&mut self, id: &str, f: &crate::doc::FeaturesSource) -> ResolvedSource {
        let at = format!("/sources/{}", ptr(id));
        let unavailable = ResolvedSource {
            access: "public",
            dsv: None,
            versions: Vec::new(),
            class: None,
            basis: None,
            shape: Shape::Unavailable,
        };
        let Some(ds) = self.cat.datasets.get(&f.dataset) else {
            self.problem(
                format!("{at}/dataset"),
                "NOT_FOUND",
                Some(json!({ "dataset": f.dataset })),
            );
            return unavailable;
        };
        if ds.kind != "features" {
            self.problem(
                format!("{at}/dataset"),
                "NOT_FOUND",
                Some(json!({ "reason": format!("{} is a {} dataset, not features", f.dataset, ds.kind) })),
            );
            return unavailable;
        }
        let Ok((dsv, _)) = self.version_of(&at, &f.dataset, f.version.as_ref()) else {
            return unavailable;
        };
        let n = self.ctx.features.get(id).copied().unwrap_or_default();
        let all_fields = self.ctx.fields.get(&f.dataset).cloned().unwrap_or_default();
        let fields: Vec<FieldInfo> = match &f.fields {
            Some(w) => all_fields
                .into_iter()
                .filter(|x| w.contains(&x.id))
                .collect(),
            None => all_fields,
        };
        let tileset = self
            .ctx
            .tilesets
            .iter()
            .find(|t| t.layers.iter().any(|(_, d)| d == &f.dataset));
        let mut q: Vec<String> = Vec::new();
        if let Some(k) = &f.feature_kind {
            q.push(format!("kind={}", enc(k)));
        }
        if let Some(w) = &f.within {
            q.push(format!("within={}", enc(w)));
        }
        let shape = match feature_shape(n.count, n.points_only, tileset.is_some()) {
            Some("geojson") => {
                let mut q = q.clone();
                q.push(format!("limit={GEOJSON_MAX}"));
                q.push("format=geojson".to_owned());
                Shape::Geojson {
                    url: format!("/v2/features/{}?{}", f.dataset, q.join("&")),
                    count: n.count,
                    fields,
                }
            }
            Some("points") => {
                let mut q = q.clone();
                // kind and name always; declared fields only when the source asks.
                let fields = if f.fields.is_some() {
                    fields
                } else {
                    Vec::new()
                };
                let mut names: Vec<String> = vec!["kind".to_owned(), "name".to_owned()];
                names.extend(fields.iter().map(|x| x.id.clone()));
                q.insert(0, format!("fields={}", enc(&names.join(","))));
                q.push("format=arrow".to_owned());
                Shape::Points {
                    points: format!("/v2/features/{}/points?{}", f.dataset, q.join("&")),
                    count: n.count,
                    fields,
                }
            }
            Some(_) => {
                let t = tileset.cloned().unwrap_or_default();
                Shape::Tiles {
                    tiles: format!("/v2/tiles/{}/{{z}}/{{x}}/{{y}}.mvt", t.id),
                    tilejson: format!("/v2/tiles/{}.json", t.id),
                    source_layers: t
                        .layers
                        .iter()
                        .filter(|(_, d)| d == &f.dataset)
                        .map(|(l, _)| l.clone())
                        .collect(),
                    minzoom: t.minzoom,
                    maxzoom: t.maxzoom,
                    count: n.count,
                    fields,
                }
            }
            None => {
                self.problem(
                    at,
                    "NOT_SERVED",
                    Some(json!({ "reason": format!("{} features with lines or polygons need a tileset; none is built", n.count) })),
                );
                return unavailable;
            }
        };
        self.datasets.insert(f.dataset.clone());
        let class = (ds.publication_class == "C").then(|| "third_party".to_owned());
        ResolvedSource {
            access: "public",
            dsv: Some(dsv.clone()),
            versions: vec![VersionUsed {
                dataset: f.dataset.clone(),
                dsv,
            }],
            class: class.or_else(|| Some("third_party".to_owned())),
            basis: None,
            shape,
        }
    }

    fn not_served(&mut self, at: String, what: &str, wp: &str) -> ResolvedSource {
        self.problem(
            at,
            "NOT_SERVED",
            Some(json!({ "reason": format!("{what} are served from {wp}") , "work_package": wp })),
        );
        ResolvedSource {
            access: "public",
            dsv: None,
            versions: Vec::new(),
            class: None,
            basis: None,
            shape: Shape::Unavailable,
        }
    }

    fn source(&mut self, id: &str, s: &Source) -> ResolvedSource {
        let at = format!("/sources/{}", ptr(id));
        let denied = || ResolvedSource {
            access: "denied",
            dsv: None,
            versions: Vec::new(),
            class: None,
            basis: None,
            shape: Shape::Unavailable,
        };
        let private_notice = Notice { code: "PRIVATE_DATA", text: "Some layers use private workspace data; they are shown only to members of that workspace.".to_owned() };
        match s {
            Source::Indicator(i) => self.indicator(id, i),
            Source::Boundaries(b) => {
                let Some(ds) = self.cat.boundary_dataset().map(str::to_owned) else {
                    self.problem(
                        at,
                        "NOT_FOUND",
                        Some(json!({ "reason": "no boundary dataset in the country profile" })),
                    );
                    return ResolvedSource {
                        shape: Shape::Unavailable,
                        ..denied()
                    };
                };
                let scheme_ok = self
                    .cat
                    .profile()
                    .is_ok_and(|p| p.admin_scheme() == b.scheme);
                if !scheme_ok {
                    return self.not_served(
                        format!("{at}/scheme"),
                        "boundaries of other schemes",
                        "WP-10",
                    );
                }
                match self.version_of(&at, &ds, b.version.as_ref()) {
                    Ok((dsv, _)) => {
                        self.datasets.insert(ds.clone());
                        ResolvedSource {
                            access: "public",
                            dsv: Some(dsv.clone()),
                            versions: vec![VersionUsed { dataset: ds, dsv }],
                            class: Some("official".to_owned()),
                            basis: None,
                            shape: Shape::Boundaries {
                                tiles: admin_tiles(),
                                source_layer: format!("adm{}", b.level),
                                tilejson: "/v2/tiles/admin.json".to_owned(),
                                minzoom: admin_minzoom(self.ctx, &format!("adm{}", b.level)),
                                maxzoom: admin_maxzoom(self.ctx),
                            },
                        }
                    }
                    Err(()) => ResolvedSource {
                        access: "public",
                        ..denied()
                    },
                }
            }
            Source::Features(f) if is_workspace(&f.dataset) => {
                self.notices.insert(private_notice);
                denied()
            }
            Source::Grid(g) if is_workspace(&g.grid) => {
                self.notices.insert(private_notice);
                denied()
            }
            Source::Features(f) => self.features(id, f),
            Source::Grid(g) => self.grid(id, g),
            Source::Analysis(_) => self.not_served(at, "analysis outputs", "WP-29"),
            Source::Inline(_) => ResolvedSource {
                access: "public",
                dsv: None,
                versions: Vec::new(),
                class: Some("user".to_owned()),
                basis: None,
                shape: Shape::Inline,
            },
            Source::External(e) => {
                let host = host_of(&e.url).unwrap_or_else(|| e.url.clone());
                self.notices.insert(Notice { code: "EXTERNAL", text: format!("Data from {host} is loaded by your browser directly; Lanka Data Layer does not check or cache it.") });
                self.external.push((host, e.attribution.clone()));
                ResolvedSource {
                    access: "public",
                    dsv: None,
                    versions: Vec::new(),
                    class: Some("third_party".to_owned()),
                    basis: None,
                    shape: Shape::External { url: e.url.clone() },
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VersionMode {
    Latest,
    Release(String),
    Dsv(String),
}

/// The least official class among inputs (estimated < derived < third_party < official).
fn least_official(classes: &[String]) -> Option<String> {
    let rank = |c: &str| match c {
        "estimated" => 0,
        "derived" => 1,
        "third_party" => 2,
        _ => 3,
    };
    classes.iter().min_by_key(|c| rank(c)).cloned()
}

/// Resolves a validated document.
#[must_use]
pub fn resolve(doc: &MapDocument, cat: &Catalog, ctx: &ResolveContext) -> Resolved {
    let mut b = Builder {
        cat,
        ctx,
        doc,
        problems: Vec::new(),
        notices: BTreeSet::new(),
        datasets: BTreeSet::new(),
        external: Vec::new(),
    };
    if ctx.release_missing
        && let Pin::Release { release } = &doc.pin
    {
        b.problem(
            "/pin".to_owned(),
            "NOT_FOUND",
            Some(json!({ "release": release, "fallback": "latest", "latest": cat.data_version })),
        );
    }
    let sources: BTreeMap<String, ResolvedSource> = doc
        .sources
        .iter()
        .map(|(id, s)| (id.clone(), b.source(id, s)))
        .collect();
    let mut analyses = BTreeMap::new();
    for (k, a) in &doc.analyses {
        analyses.insert(
            k.clone(),
            ResolvedAnalysis {
                id: a.get("result").and_then(Value::as_str).map(str::to_owned),
                state: "unavailable",
            },
        );
        b.problem(
            format!("/analyses/{}", ptr(k)),
            "NOT_SERVED",
            Some(json!({ "reason": "analyses are computed from WP-29", "work_package": "WP-29" })),
        );
    }

    // Attribution: every source behind every dataset read, then external hosts.
    let mut attribution: BTreeSet<Attribution> = BTreeSet::new();
    for d in &b.datasets {
        let Some(ds) = cat.datasets.get(d) else {
            continue;
        };
        for sid in &ds.source_ids {
            let Some(src) = cat.sources.get(sid) else {
                continue;
            };

            let lic = cat.licences.get(&src.licence_id);
            let licence = lic
                .and_then(|l| l.spdx.clone())
                .unwrap_or_else(|| src.licence_id.clone());
            let status = lic.map_or_else(|| "permission_unclear".to_owned(), |l| l.status.clone());
            if status == "permission_unclear" {
                b.notices.insert(Notice {
                    code: "LICENCE_UNCLEAR",
                    text: format!("{} has not stated a licence for this data; reuse beyond viewing may need its permission.", src.publisher),
                });
            }
            attribution.insert(Attribution {
                source_id: src.id.clone(),
                publisher: src.publisher.clone(),
                text: src.attribution.clone(),
                licence,
                licence_url: lic.and_then(|l| l.url.clone()),
                licence_status: status,
                redistribution: lic.map_or_else(|| "link".to_owned(), |l| l.redistribution.clone()),
            });
        }
    }
    let mut attribution: Vec<Attribution> = attribution.into_iter().collect();
    for (host, text) in std::mem::take(&mut b.external) {
        attribution.push(Attribution {
            source_id: format!("external:{host}"),
            publisher: host,
            text,
            licence: "unknown".to_owned(),
            licence_url: None,
            licence_status: "permission_unclear".to_owned(),
            redistribution: "link".to_owned(),
        });
    }
    let data_version = match &ctx.release {
        Some((r, _)) => Some(r.clone()),
        None => cat.data_version.clone(),
    };
    Resolved {
        data_version,
        sources,
        analyses,
        attribution,
        notices: b.notices.into_iter().collect(),
        problems: b.problems,
    }
}

/// Freezes the pin to the current release (10 §2.3). Class breaks and
/// analysis fingerprints are written by WP-29 (`lanka-stats`); until then a
/// frozen document keeps the breaks it carries. Returns `false` when there
/// is no release to pin to.
pub fn freeze(doc: &mut MapDocument, cat: &Catalog) -> bool {
    match &cat.data_version {
        Some(dv) => {
            doc.pin = Pin::Release {
                release: dv.clone(),
            };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(enc("sex=total;age=0-14"), "sex%3Dtotal%3Bage%3D0-14");
        assert_eq!(enc("a.b,c_d"), "a.b,c_d");
        assert_eq!(enc("Y65+"), "Y65%2B");
    }

    #[test]
    fn least_official_wins() {
        let c = |v: &[&str]| least_official(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(c(&["official", "estimated"]).as_deref(), Some("estimated"));
        assert_eq!(
            c(&["official", "third_party"]).as_deref(),
            Some("third_party")
        );
        assert_eq!(c(&[]), None);
    }
}
