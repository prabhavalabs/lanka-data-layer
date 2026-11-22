//! Rust types of `ldl-map/1` (`spec/map/1/map.schema.json`, 10 §2).
//!
//! Every object the schema lets carry `x-*` keys has an `ext` map that
//! collects them, so a parse → serialise round trip preserves extensions.
//! Free-form parts are `serde_json::Value`: CQL2-JSON filters (checked by
//! the validator), analysis requests (`ldl-analysis/1`, WP-29) and the
//! parameters of a scale that are a union of shapes (`clamp`, `mid`).
//!
//! `extent` and `time` are always written (`null` when absent); other
//! optional members are omitted when absent. Numbers are `f64`, so `9`
//! re-serialises as `9.0`; the canonical form (`canon`) writes both as `9`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `x-*` extension members of an object, preserved verbatim.
pub type Ext = BTreeMap<String, Value>;

/// The only major this crate reads.
pub const SPEC: &str = "ldl-map/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapDocument {
    pub spec: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: Text,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub pin: Pin,
    pub view: View,
    #[serde(default)]
    pub extent: Option<Extent>,
    pub basemap: Basemap,
    #[serde(default)]
    pub time: Option<TimeConfig>,
    pub sources: BTreeMap<String, Source>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub analyses: BTreeMap<String, Value>,
    pub layers: Vec<LayerNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<Panel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocMeta>,
    #[serde(flatten)]
    pub ext: Ext,
}

/// Plain text or per-language text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Text {
    Plain(String),
    Lang {
        en: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        si: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ta: Option<String>,
    },
}

impl Text {
    /// The English (or only) text.
    #[must_use]
    pub fn en(&self) -> &str {
        match self {
            Self::Plain(s) | Self::Lang { en: s, .. } => s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LatestTag {
    #[serde(rename = "latest")]
    Latest,
}

/// Which release every public source resolves against (10 §2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pin {
    Latest(LatestTag),
    Release { release: String },
}

impl Pin {
    #[must_use]
    pub fn release(&self) -> Option<&str> {
        match self {
            Self::Latest(_) => None,
            Self::Release { release } => Some(release),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub center: [f64; 2],
    pub zoom: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[f64; 4]>,
    #[serde(flatten)]
    pub ext: Ext,
}

/// A spatial scope: a coded unit or a bbox (`extent`, `clip`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Extent {
    Geo { geo: String },
    Bbox { bbox: [f64; 4] },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Basemap {
    pub style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hillshade: Option<Hillshade>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hillshade {
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exaggeration: Option<f64>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeConfig {
    pub cursor: String,
    pub range: [String; 2],
    pub freq: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback: Option<Playback>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playback {
    pub fps: f64,
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub looping: Option<bool>,
    #[serde(flatten)]
    pub ext: Ext,
}

// ───────────────────────────── sources ─────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    Indicator(IndicatorSource),
    Boundaries(BoundariesSource),
    Features(FeaturesSource),
    Grid(GridSource),
    Analysis(AnalysisSource),
    Inline(InlineSource),
    External(ExternalSource),
}

impl Source {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Indicator(_) => "indicator",
            Self::Boundaries(_) => "boundaries",
            Self::Features(_) => "features",
            Self::Grid(_) => "grid",
            Self::Analysis(_) => "analysis",
            Self::Inline(_) => "inline",
            Self::External(_) => "external",
        }
    }
}

/// `"latest"` or `{dsv}`; absent = inherit the document pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VersionRef {
    Latest(LatestTag),
    Dsv { dsv: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PeriodRef {
    /// `"latest"` or one period code.
    Code(String),
    List(Vec<String>),
    Range {
        from: String,
        to: String,
    },
    Bind {
        bind: String,
        #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
        matching: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LevelRef {
    Level(u8),
    ByZoom {
        #[serde(rename = "byZoom")]
        by_zoom: Vec<(f64, u8)>,
    },
}

impl LevelRef {
    /// Every level the reference can draw, in zoom order.
    #[must_use]
    pub fn levels(&self) -> Vec<u8> {
        match self {
            Self::Level(l) => vec![*l],
            Self::ByZoom { by_zoom } => by_zoom.iter().map(|z| z.1).collect(),
        }
    }
}

pub type Dims = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndicatorSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicators: Option<Vec<String>>,
    pub level: LevelRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub period: PeriodRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dims: Option<Dims>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionRef>,
    #[serde(flatten)]
    pub ext: Ext,
}

impl IndicatorSource {
    /// `indicators[]`, else `[indicator]`.
    #[must_use]
    pub fn indicator_ids(&self) -> Vec<&str> {
        match (&self.indicators, &self.indicator) {
            (Some(v), _) => v.iter().map(String::as_str).collect(),
            (None, Some(i)) => vec![i.as_str()],
            (None, None) => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundariesSource {
    pub scheme: String,
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionRef>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturesSource {
    pub dataset: String,
    #[serde(
        rename = "featureKind",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub feature_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    /// Binds a date field to the document time cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<FeatureTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionRef>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureTime {
    pub field: String,
    #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
    pub matching: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridSource {
    pub grid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dims: Option<Dims>,
    pub period: PeriodRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionRef>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisSource {
    #[serde(rename = "ref")]
    pub analysis: String,
    pub output: String,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineSource {
    pub geojson: Value,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalSource {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub attribution: String,
    #[serde(flatten)]
    pub ext: Ext,
}

// ───────────────────────────── layers ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupTag {
    #[serde(rename = "group")]
    Group,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayerNode {
    Group(Group),
    Layer(Box<Layer>),
}

impl LayerNode {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Group(g) => &g.id,
            Self::Layer(l) => &l.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    #[serde(rename = "type")]
    pub tag: GroupTag,
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    pub children: Vec<LayerNode>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    /// A layer type of 10 §2.5, or `x-…` (rendered as a placeholder row).
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub source: String,
    pub visible: bool,
    /// 0..1; absent = 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<[f64; 2]>,
    /// CQL2-JSON subset; `null` = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Extent>,
    pub style: Style,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<Legend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction: Option<Interaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Legend {
    pub show: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<Text>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interaction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<Vec<String>>,
    #[serde(flatten)]
    pub ext: Ext,
}

// ───────────────────────────── style ───────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RampRef {
    Name(String),
    Colors(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaks: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<RampRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodata: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clamp: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid: Option<Value>,
    /// categorical: code → colour (colour channels) or number (numeric channels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<BTreeMap<String, Value>>,
    /// Output range in px for numeric channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 2]>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub field: String,
    pub scale: Scale,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColorChannel {
    Const(String),
    Field(Box<Binding>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NumberChannel {
    Const(f64),
    Field(Box<Binding>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bivariate {
    pub x: Binding,
    pub y: Binding,
    pub palette: RampRef,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Fill {
    Const(String),
    Field(Box<Binding>),
    Bivariate {
        bivariate: Box<Bivariate>,
        #[serde(flatten)]
        ext: Ext,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<NumberChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelStyle {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<NumberChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<LabelStyle>,
    /// raster: mask outside `[lo, hi]` (`None` = open end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[Option<f64>; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resample: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<RampRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<NumberChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exaggeration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<String>,
    #[serde(flatten)]
    pub ext: Ext,
}

// ───────────────────────────── panels, meta ────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Panel {
    Legend(LegendPanel),
    Chart(ChartPanel),
    Stats(StatsPanel),
    Text(TextPanel),
}

impl Panel {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Legend(p) => &p.id,
            Self::Chart(p) => &p.id,
            Self::Stats(p) => &p.id,
            Self::Text(p) => &p.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegendPanel {
    pub id: String,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartPanel {
    pub id: String,
    pub chart: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsPanel {
    pub id: String,
    pub analysis: String,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPanel {
    pub id: String,
    pub text: String,
    #[serde(flatten)]
    pub ext: Ext,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(flatten)]
    pub ext: Ext,
}

impl MapDocument {
    /// Every layer (not group), depth-first in document (draw) order.
    #[must_use]
    pub fn layers_flat(&self) -> Vec<&Layer> {
        fn walk<'a>(nodes: &'a [LayerNode], out: &mut Vec<&'a Layer>) {
            for n in nodes {
                match n {
                    LayerNode::Group(g) => walk(&g.children, out),
                    LayerNode::Layer(l) => out.push(l),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.layers, &mut out);
        out
    }
}
