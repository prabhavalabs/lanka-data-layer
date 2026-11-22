//! Validation of an `ldl-map/1` document: the JSON Schema, then the rules
//! the schema cannot express (10 §2.4 limits, references between sources,
//! layers, analyses and panels, layer type ↔ source kind, scales, filters,
//! ramps, sizes). Every problem carries a JSON pointer into the document.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::Serialize;
use serde_json::Value;

use crate::canon::jcs;
use crate::doc::{
    Binding, ColorChannel, Fill, LayerNode, MapDocument, NumberChannel, PeriodRef, RampRef, SPEC,
    Source, Style,
};
use crate::ramps::REGISTRY;

/// The schema, embedded (a byte-identical copy of `spec/map/1/map.schema.json`).
pub const SCHEMA_JSON: &str = include_str!("../schema/map.schema.json");

/// 10 §2.4 limits.
pub const MAX_LAYERS: usize = 32;
pub const MAX_GROUPS: usize = 8;
pub const MAX_GROUP_DEPTH: usize = 3;
pub const MAX_SOURCES: usize = 16;
pub const MAX_ANALYSES: usize = 6;
pub const MAX_DOC_BYTES: usize = 262_144;
pub const MAX_INLINE_BYTES: usize = 102_400;

#[allow(clippy::expect_used)]
static VALIDATOR: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).expect("embedded schema is JSON");
    jsonschema::draft202012::new(&schema).expect("embedded schema compiles")
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// JSON pointer (RFC 6901) to the offending member; `""` = the document.
    pub path: String,
    /// Stable code, e.g. `SCHEMA`, `UNKNOWN_SOURCE`, `TOO_MANY_LAYERS`.
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        write!(f, "{path}: {} ({})", self.message, self.code)
    }
}

fn p(path: impl Into<String>, code: &'static str, message: impl Into<String>) -> Problem {
    Problem {
        path: path.into(),
        code,
        message: message.into(),
    }
}

/// RFC 6901 escaping of one reference token.
#[must_use]
pub fn ptr(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Validates a parsed JSON value. On success returns the typed document;
/// otherwise every problem found, ordered by path.
pub fn validate(v: &Value) -> Result<MapDocument, Vec<Problem>> {
    let Some(obj) = v.as_object() else {
        return Err(vec![p("", "SCHEMA", "a map document is a JSON object")]);
    };
    match obj.get("spec").and_then(Value::as_str) {
        None => {
            return Err(vec![p(
                "/spec",
                "SCHEMA",
                "\"spec\" is required (\"ldl-map/1\")",
            )]);
        }
        Some(s) if s == SPEC => {}
        Some(s) if s.starts_with("ldl-map/") => {
            return Err(vec![p(
                "/spec",
                "UNSUPPORTED_SPEC",
                format!("{s} is not supported by this reader (only {SPEC})"),
            )]);
        }
        Some(s) => {
            return Err(vec![p(
                "/spec",
                "SCHEMA",
                format!("{s:?} is not an ldl-map spec"),
            )]);
        }
    }
    let mut problems: Vec<Problem> = schema_problems(v);
    if !problems.is_empty() {
        problems.sort_by(|a, b| a.path.cmp(&b.path));
        problems.dedup();
        return Err(problems);
    }
    let doc: MapDocument = match serde_json::from_value(v.clone()) {
        Ok(d) => d,
        Err(e) => return Err(vec![p("", "SCHEMA", format!("does not parse: {e}"))]),
    };
    let size = jcs(v).len();
    if size > MAX_DOC_BYTES {
        problems.push(p(
            "",
            "TOO_LARGE",
            format!("{size} bytes of canonical JSON (limit {MAX_DOC_BYTES})"),
        ));
    }
    semantic(&doc, v, &mut problems);
    if problems.is_empty() {
        Ok(doc)
    } else {
        problems.sort_by(|a, b| a.path.cmp(&b.path));
        problems.dedup();
        Err(problems)
    }
}

/// Parses and validates JSON text.
pub fn validate_str(s: &str) -> Result<MapDocument, Vec<Problem>> {
    match serde_json::from_str::<Value>(s) {
        Ok(v) => validate(&v),
        Err(e) => Err(vec![p("", "JSON", format!("not JSON: {e}"))]),
    }
}

/// Schema errors, keeping only the most specific ones: an error whose path
/// is a prefix of another error's path (the `if/then` or `anyOf` wrapper of
/// a nested failure) is dropped. `required` and `additionalProperties`
/// errors point at the member concerned.
fn schema_problems(v: &Value) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    for e in VALIDATOR.iter_errors(v) {
        let mut path = e.instance_path().to_string();
        let message = e.to_string();
        match e.kind() {
            jsonschema::error::ValidationErrorKind::Required { property } => {
                if let Some(name) = property.as_str() {
                    path = format!("{path}/{}", ptr(name));
                }
            }
            jsonschema::error::ValidationErrorKind::AdditionalProperties { unexpected } => {
                if let Some(name) = unexpected.first() {
                    path = format!("{path}/{}", ptr(name));
                }
            }
            _ => {}
        }
        out.push(p(path, "SCHEMA", message));
    }
    let paths: Vec<String> = out.iter().map(|x| x.path.clone()).collect();
    out.retain(|x| {
        let below = format!("{}/", x.path);
        !paths.iter().any(|q| q.starts_with(&below))
    });
    out
}

/// Layer types and the source kinds they draw (10 §2.5). Analysis results
/// can be frames, grids or features, so they fit every data layer.
fn compatible(layer: &str, source: &Source) -> bool {
    if layer.starts_with("x-") {
        return true;
    }
    let kind = source.kind();
    let external = match source {
        Source::External(e) => Some(e.kind.as_str()),
        _ => None,
    };
    let vector_ext = matches!(external, Some("geojson" | "pmtiles"));
    match layer {
        "choropleth" | "bivariate" => matches!(kind, "indicator" | "analysis"),
        "symbol" => matches!(kind, "indicator" | "analysis" | "features") || vector_ext,
        "point" | "cluster" | "heatmap" | "bins" => {
            matches!(kind, "features" | "analysis" | "inline") || vector_ext
        }
        "line" | "polygon" => {
            matches!(kind, "features" | "boundaries" | "analysis" | "inline") || vector_ext
        }
        "raster" => matches!(kind, "grid" | "analysis") || external == Some("raster-xyz"),
        "hillshade" => kind == "grid",
        "label" => {
            matches!(
                kind,
                "indicator" | "boundaries" | "features" | "analysis" | "inline"
            ) || vector_ext
        }
        "annotation" => kind == "inline",
        _ => false,
    }
}

#[allow(clippy::too_many_lines)]
fn semantic(doc: &MapDocument, raw: &Value, out: &mut Vec<Problem>) {
    // Sources.
    for (id, s) in &doc.sources {
        let at = format!("/sources/{}", ptr(id));
        match s {
            Source::Indicator(i) => {
                if i.indicator.is_some() && i.indicators.is_some() {
                    out.push(p(
                        &at,
                        "AMBIGUOUS_INDICATOR",
                        "give `indicator` or `indicators`, not both",
                    ));
                }
                if let crate::doc::LevelRef::ByZoom { by_zoom } = &i.level
                    && by_zoom.windows(2).any(|w| w[1].0 <= w[0].0)
                {
                    out.push(p(
                        format!("{at}/level/byZoom"),
                        "ORDER",
                        "zooms must ascend",
                    ));
                }
                period_rules(doc, &i.period, &format!("{at}/period"), out);
            }
            Source::Grid(g) => period_rules(doc, &g.period, &format!("{at}/period"), out),
            Source::Analysis(a) => {
                if !doc.analyses.contains_key(&a.analysis) {
                    out.push(p(
                        format!("{at}/ref"),
                        "UNKNOWN_ANALYSIS",
                        format!("no analysis {:?} in `analyses`", a.analysis),
                    ));
                }
            }
            Source::Inline(_) => {
                let n = raw
                    .pointer(&format!("{at}/geojson"))
                    .map_or(0, |g| jcs(g).len());
                if n > MAX_INLINE_BYTES {
                    out.push(p(
                        format!("{at}/geojson"),
                        "INLINE_TOO_LARGE",
                        format!("{n} bytes (limit {MAX_INLINE_BYTES})"),
                    ));
                }
            }
            Source::Boundaries(_) | Source::External(_) | Source::Features(_) => {}
        }
    }

    // Layer tree.
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let (mut layers, mut groups) = (0usize, 0usize);
    walk_nodes(
        &doc.layers,
        "/layers",
        1,
        &mut |node, at, depth| match node {
            LayerNode::Group(g) => {
                groups += 1;
                if depth > MAX_GROUP_DEPTH {
                    out.push(p(
                        at,
                        "GROUP_DEPTH",
                        format!("groups nest to depth {MAX_GROUP_DEPTH}"),
                    ));
                }
                if let Some(first) = ids.insert(g.id.clone(), at.to_owned()) {
                    out.push(p(
                        format!("{at}/id"),
                        "DUPLICATE_ID",
                        format!("id {:?} is also used at {first}", g.id),
                    ));
                }
            }
            LayerNode::Layer(l) => {
                layers += 1;
                if let Some(first) = ids.insert(l.id.clone(), at.to_owned()) {
                    out.push(p(
                        format!("{at}/id"),
                        "DUPLICATE_ID",
                        format!("id {:?} is also used at {first}", l.id),
                    ));
                }
                match doc.sources.get(&l.source) {
                    None => out.push(p(
                        format!("{at}/source"),
                        "UNKNOWN_SOURCE",
                        format!("no source {:?}", l.source),
                    )),
                    Some(s) => {
                        if !compatible(&l.kind, s) {
                            out.push(p(
                                format!("{at}/type"),
                                "INCOMPATIBLE_SOURCE",
                                format!("a {} layer cannot draw a {} source", l.kind, s.kind()),
                            ));
                        }
                        if l.kind == "bivariate" {
                            if !matches!(l.style.fill, Some(Fill::Bivariate { .. })) {
                                out.push(p(
                                    format!("{at}/style/fill"),
                                    "BIVARIATE_FILL",
                                    "a bivariate layer needs `fill.bivariate`",
                                ));
                            }
                            if let Source::Indicator(i) = s
                                && i.indicator_ids().len() != 2
                            {
                                out.push(p(format!("{at}/source"), "BIVARIATE_SOURCE", "a bivariate layer needs an indicator source with two `indicators`"));
                            }
                        }
                    }
                }
                if let Some([a, b]) = l.zoom
                    && a > b
                {
                    out.push(p(
                        format!("{at}/zoom"),
                        "ORDER",
                        "zoom range [min, max] is reversed",
                    ));
                }
                if let Some(f) = &l.filter {
                    cql2(f, &format!("{at}/filter"), out);
                }
                if let Some(crate::doc::Extent::Bbox { bbox }) = &l.clip {
                    bbox_rule(bbox, &format!("{at}/clip/bbox"), out);
                }
                style_rules(&l.style, &format!("{at}/style"), out);
            }
        },
    );
    if layers > MAX_LAYERS {
        out.push(p(
            "/layers",
            "TOO_MANY_LAYERS",
            format!("{layers} layers (limit {MAX_LAYERS})"),
        ));
    }
    if groups > MAX_GROUPS {
        out.push(p(
            "/layers",
            "TOO_MANY_GROUPS",
            format!("{groups} groups (limit {MAX_GROUPS})"),
        ));
    }

    // Analyses: every `source` they name exists.
    for (id, a) in &doc.analyses {
        let mut refs = Vec::new();
        named_strings(a, "source", &format!("/analyses/{}", ptr(id)), &mut refs);
        for (at, s) in refs {
            if !doc.sources.contains_key(&s) {
                out.push(p(at, "UNKNOWN_SOURCE", format!("no source {s:?}")));
            }
        }
    }

    // Panels.
    let mut panel_ids = BTreeSet::new();
    for (i, panel) in doc.panels.iter().enumerate() {
        let at = format!("/panels/{i}");
        if !panel_ids.insert(panel.id().to_owned()) {
            out.push(p(
                format!("{at}/id"),
                "DUPLICATE_ID",
                format!("panel id {:?} repeats", panel.id()),
            ));
        }
        let (analysis, source, linked) = match panel {
            crate::doc::Panel::Chart(c) => (
                c.analysis.as_deref(),
                c.source.as_deref(),
                c.linked.as_deref(),
            ),
            crate::doc::Panel::Stats(s) => (Some(s.analysis.as_str()), None, None),
            _ => (None, None, None),
        };
        if let Some(a) = analysis
            && !doc.analyses.contains_key(a)
        {
            out.push(p(
                format!("{at}/analysis"),
                "UNKNOWN_ANALYSIS",
                format!("no analysis {a:?}"),
            ));
        }
        if let Some(s) = source
            && !doc.sources.contains_key(s)
        {
            out.push(p(
                format!("{at}/source"),
                "UNKNOWN_SOURCE",
                format!("no source {s:?}"),
            ));
        }
        if let Some(l) = linked
            && !ids.contains_key(l)
        {
            out.push(p(
                format!("{at}/linked"),
                "UNKNOWN_LAYER",
                format!("no layer {l:?}"),
            ));
        }
    }

    // View, extent, time.
    if let Some(b) = &doc.view.bounds {
        bbox_rule(b, "/view/bounds", out);
    }
    if let Some(crate::doc::Extent::Bbox { bbox }) = &doc.extent {
        bbox_rule(bbox, "/extent/bbox", out);
    }
    if let Some(t) = &doc.time {
        if t.range[0] > t.range[1] {
            out.push(p(
                "/time/range",
                "ORDER",
                "time range [from, to] is reversed",
            ));
        } else if t.cursor < t.range[0] || t.cursor > t.range[1] {
            out.push(p(
                "/time/cursor",
                "TIME_RANGE",
                "the cursor lies outside the time range",
            ));
        }
    }

    // Named tones anywhere in the layer tree must exist.
    let mut tones = Vec::new();
    prefixed_strings(
        raw.get("layers").unwrap_or(&Value::Null),
        "/layers",
        "ldl.tone.",
        &mut tones,
    );
    for (at, s) in tones {
        if !REGISTRY.tones.contains_key(&s["ldl.tone.".len()..]) {
            out.push(p(
                at,
                "UNKNOWN_TONE",
                format!(
                    "{s:?} is not in ramps.json tones ({})",
                    REGISTRY
                        .tones
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
}

/// A period bound to the time cursor in a document without `time` shows
/// the latest period (so a layer added from the catalogue works before the
/// map has a time slider); that is not an error.
fn period_rules(_doc: &MapDocument, period: &PeriodRef, at: &str, out: &mut Vec<Problem>) {
    if let PeriodRef::Range { from, to } = period
        && from > to
    {
        out.push(p(at, "ORDER", "period range {from, to} is reversed"));
    }
}

fn bbox_rule(b: &[f64; 4], at: &str, out: &mut Vec<Problem>) {
    if b[0] >= b[2] || b[1] >= b[3] {
        out.push(p(
            at,
            "ORDER",
            "bbox must be [west, south, east, north] with west < east and south < north",
        ));
    }
}

fn walk_nodes(
    nodes: &[LayerNode],
    at: &str,
    depth: usize,
    f: &mut impl FnMut(&LayerNode, &str, usize),
) {
    for (i, n) in nodes.iter().enumerate() {
        let here = format!("{at}/{i}");
        f(n, &here, depth);
        if let LayerNode::Group(g) = n {
            walk_nodes(&g.children, &format!("{here}/children"), depth + 1, f);
        }
    }
}

/// Every string value of a member named `key`, with its pointer.
fn named_strings(v: &Value, key: &str, at: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let here = format!("{at}/{}", ptr(k));
                match x {
                    Value::String(s) if k == key => out.push((here, s.clone())),
                    _ => named_strings(x, key, &here, out),
                }
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                named_strings(x, key, &format!("{at}/{i}"), out);
            }
        }
        _ => {}
    }
}

/// Every string value starting with `prefix`, with its pointer.
fn prefixed_strings(v: &Value, at: &str, prefix: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::String(s) if s.starts_with(prefix) => out.push((at.to_owned(), s.clone())),
        Value::Object(m) => {
            for (k, x) in m {
                prefixed_strings(x, &format!("{at}/{}", ptr(k)), prefix, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                prefixed_strings(x, &format!("{at}/{i}"), prefix, out);
            }
        }
        _ => {}
    }
}

/// Operator arity of the CQL2-JSON subset.
fn cql2(f: &Value, at: &str, out: &mut Vec<Problem>) {
    let Some(op) = f.get("op").and_then(Value::as_str) else {
        return;
    };
    let args = f
        .get("args")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let ok = match op {
        "and" | "or" => args.len() >= 2 && args.iter().all(|a| a.get("op").is_some()),
        "not" => args.len() == 1 && args[0].get("op").is_some(),
        "=" | "<>" | "<" | "<=" | ">" | ">=" => args.len() == 2,
        "in" => args.len() == 2 && args[1].is_array(),
        "between" => args.len() == 3 && args[1].is_number() && args[2].is_number(),
        "like" => args.len() == 2 && args[1].is_string(),
        "isNull" => args.len() == 1,
        _ => false,
    };
    if !ok {
        out.push(p(
            format!("{at}/args"),
            "CQL2_ARITY",
            format!("wrong arguments for {op:?}"),
        ));
    }
    if op == "between"
        && let (Some(a), Some(b)) = (
            args.get(1).and_then(Value::as_f64),
            args.get(2).and_then(Value::as_f64),
        )
        && a > b
    {
        out.push(p(
            format!("{at}/args"),
            "ORDER",
            "between: lower bound above upper bound",
        ));
    }
    if matches!(op, "and" | "or" | "not") {
        for (i, a) in args.iter().enumerate() {
            cql2(a, &format!("{at}/args/{i}"), out);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Channel {
    Color,
    Number,
}

fn style_rules(s: &Style, at: &str, out: &mut Vec<Problem>) {
    match &s.fill {
        Some(Fill::Field(b)) => binding_rules(b, Channel::Color, &format!("{at}/fill"), out),
        Some(Fill::Bivariate { bivariate, .. }) => {
            let here = format!("{at}/fill/bivariate");
            binding_rules(&bivariate.x, Channel::Color, &format!("{here}/x"), out);
            binding_rules(&bivariate.y, Channel::Color, &format!("{here}/y"), out);
            ramp_rule(
                &bivariate.palette,
                Some("biv"),
                &format!("{here}/palette"),
                out,
            );
        }
        _ => {}
    }
    if let Some(st) = &s.stroke {
        if let Some(ColorChannel::Field(b)) = &st.color {
            binding_rules(b, Channel::Color, &format!("{at}/stroke/color"), out);
        }
        if let Some(NumberChannel::Field(b)) = &st.width {
            binding_rules(b, Channel::Number, &format!("{at}/stroke/width"), out);
        }
    }
    if let Some(NumberChannel::Field(b)) = &s.size {
        binding_rules(b, Channel::Number, &format!("{at}/size"), out);
    }
    if let Some(NumberChannel::Field(b)) = &s.elevation {
        binding_rules(b, Channel::Number, &format!("{at}/elevation"), out);
    }
    if let Some(r) = &s.ramp {
        ramp_rule(r, None, &format!("{at}/ramp"), out);
    }
    if let Some(l) = &s.label
        && let Some([a, b]) = l.zoom
        && a > b
    {
        out.push(p(
            format!("{at}/label/zoom"),
            "ORDER",
            "zoom range [min, max] is reversed",
        ));
    }
    if let Some([Some(a), Some(b)]) = s.range
        && a > b
    {
        out.push(p(
            format!("{at}/range"),
            "ORDER",
            "range [lo, hi] is reversed",
        ));
    }
}

fn binding_rules(b: &Binding, channel: Channel, at: &str, out: &mut Vec<Problem>) {
    let sc = &b.scale;
    let here = format!("{at}/scale");
    let numeric_only = matches!(sc.kind.as_str(), "area" | "radius" | "graduated");
    if numeric_only && channel == Channel::Color {
        out.push(p(
            format!("{here}/type"),
            "SCALE_CHANNEL",
            format!("a {} scale sizes things; it cannot colour them", sc.kind),
        ));
    }
    if let Some(br) = &sc.breaks {
        if br.windows(2).any(|w| w[1] <= w[0]) {
            out.push(p(
                format!("{here}/breaks"),
                "BREAKS_ORDER",
                "breaks must strictly ascend",
            ));
        }
        if let Some(k) = sc.classes
            && sc.kind != "manual"
            && br.len() + 1 != usize::from(k)
        {
            out.push(p(
                format!("{here}/breaks"),
                "BREAKS_COUNT",
                format!("{k} classes need {} breaks, found {}", k - 1, br.len()),
            ));
        }
    }
    if let Some(r) = &sc.ramp {
        let want = match sc.kind.as_str() {
            "categorical" => Some("cat"),
            _ => None,
        };
        ramp_rule(r, want, &format!("{here}/ramp"), out);
    }
    if let Some(m) = &sc.mapping {
        for (code, v) in m {
            let ok = match channel {
                Channel::Number => v.is_number(),
                Channel::Color => v.is_string(),
            };
            if !ok {
                let want = if channel == Channel::Number {
                    "numbers"
                } else {
                    "colours"
                };
                out.push(p(
                    format!("{here}/mapping/{}", ptr(code)),
                    "MAPPING_TYPE",
                    format!("this channel maps codes to {want}"),
                ));
            }
        }
    }
    if let Some([a, b]) = sc.range
        && a > b
    {
        out.push(p(
            format!("{here}/range"),
            "ORDER",
            "range [lo, hi] is reversed",
        ));
    }
}

fn ramp_rule(r: &RampRef, kind: Option<&str>, at: &str, out: &mut Vec<Problem>) {
    let RampRef::Name(name) = r else { return };
    if name.starts_with("x-") {
        return;
    }
    match REGISTRY.ramps.get(name) {
        None => out.push(p(
            at,
            "UNKNOWN_RAMP",
            format!("{name:?} is not in ramps.json"),
        )),
        Some(ramp) => {
            if let Some(k) = kind
                && ramp.kind != k
            {
                out.push(p(
                    at,
                    "RAMP_KIND",
                    format!(
                        "{name:?} is a {} ramp; a {k} ramp is needed here",
                        ramp.kind
                    ),
                ));
            }
        }
    }
}
