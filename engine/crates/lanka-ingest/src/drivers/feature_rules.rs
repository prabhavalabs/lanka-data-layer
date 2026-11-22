//! Tag rules shared by the feature drivers (`osm_pbf`, `geo_file`): which
//! source features go into which dataset, with which `kind`, names, typed
//! attributes and flags. A source feature is a key, a tag map (OSM tags, or
//! GeoJSON properties as text) and a geometry.
//!
//! ```yaml
//! outputs:
//!   - dataset: osm-pois-amenities
//!     rules:                                  # first matching rule wins
//!       - match: { amenity: [hospital, clinic] }   # every key must match; "*" = any value
//!         not: { disused: ["*"] }                  # optional exclusions
//!         types: [node, way]                       # optional (OSM element types)
//!         geometry: point                          # point | line | area
//!         kind: "{amenity}"                        # template ({tag}) or literal
//!         attrs: { category: "=health", beds: beds }   # field: tag | "=const" | "{template}"
//!         flags: [UNOFFICIAL_GEOM]
//!         require_name: true
//!         name_contains: [zone, "industrial park"]  # case-insensitive, any
//!         drop_unnamed_below: 0.04                 # areas: v1's rough-size cut for unnamed ones
//! ```

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use lanka_core::Flags;
use serde::Deserialize;
use serde_json::Value;

use crate::geom::{Geometry, rough_ring_size};
use crate::registry::{FieldSpec, FieldType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeomMode {
    Point,
    Line,
    Area,
}

/// Values a tag must (or must not) have; `"*"` = any value.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Values {
    One(String),
    Many(Vec<String>),
}

impl Values {
    fn matches(&self, v: &str) -> bool {
        match self {
            Self::One(x) => x == "*" || x == v,
            Self::Many(xs) => xs.iter().any(|x| x == "*" || x == v),
        }
    }

    fn list(&self) -> Vec<&str> {
        match self {
            Self::One(x) => vec![x.as_str()],
            Self::Many(xs) => xs.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default, rename = "match")]
    pub when: BTreeMap<String, Values>,
    #[serde(default)]
    pub not: BTreeMap<String, Values>,
    /// Element types (`node`, `way`, `relation`; `feature` for files).
    #[serde(default)]
    pub types: Vec<String>,
    pub geometry: GeomMode,
    pub kind: String,
    #[serde(default)]
    pub attrs: BTreeMap<String, String>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub require_name: bool,
    #[serde(default)]
    pub name_contains: Vec<String>,
    #[serde(default)]
    pub drop_unnamed_below: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSpec {
    pub dataset: String,
    pub rules: Vec<Rule>,
    /// Name tags, most preferred first (default `name:en`, `name`).
    #[serde(default)]
    pub name_en: Vec<String>,
    #[serde(default)]
    pub name_si: Vec<String>,
    #[serde(default)]
    pub name_ta: Vec<String>,
}

/// A compiled rule: flags parsed once.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub rule: Rule,
    pub flags: Flags,
}

impl Rule {
    pub fn compile(self) -> Result<Compiled> {
        let flags = Flags::from_names(self.flags.iter().map(String::as_str))
            .map_err(|e| anyhow::anyhow!("rule flags: {e}"))?;
        if self.when.is_empty() {
            bail!("a rule needs at least one `match` condition");
        }
        Ok(Compiled { rule: self, flags })
    }
}

/// Tag lookup over a source feature.
pub trait Tags {
    fn tag(&self, k: &str) -> Option<&str>;
}

impl Tags for BTreeMap<String, String> {
    fn tag(&self, k: &str) -> Option<&str> {
        self.get(k).map(String::as_str)
    }
}

impl Compiled {
    /// The keys this rule reads in `match` (prefilter for extraction).
    pub fn match_keys(&self) -> impl Iterator<Item = &str> {
        self.rule.when.keys().map(String::as_str)
    }

    /// Tag conditions only (no geometry or name checks).
    pub fn matches_tags(&self, etype: &str, tags: &dyn Tags) -> bool {
        let r = &self.rule;
        if !r.types.is_empty() && !r.types.iter().any(|t| t == etype) {
            return false;
        }
        r.when
            .iter()
            .all(|(k, v)| tags.tag(k).is_some_and(|x| v.matches(x)))
            && !r
                .not
                .iter()
                .any(|(k, v)| tags.tag(k).is_some_and(|x| v.matches(x)))
    }

    /// Name conditions on the resolved English (or fallback) name.
    pub fn matches_name(&self, name: Option<&str>) -> bool {
        let r = &self.rule;
        if (r.require_name || !r.name_contains.is_empty()) && name.is_none() {
            return false;
        }
        if r.name_contains.is_empty() {
            return true;
        }
        let n = name.unwrap_or_default().to_lowercase();
        r.name_contains
            .iter()
            .any(|s| n.contains(&s.to_lowercase()))
    }

    /// `{tag}` substitution (missing tags render empty).
    pub fn render(template: &str, tags: &dyn Tags) -> String {
        if let Some(t) = template.strip_prefix('=') {
            return t.to_owned();
        }
        if !template.contains('{') {
            return template.to_owned();
        }
        let mut out = String::new();
        let mut rest = template;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let Some(j) = rest[i..].find('}') else {
                out.push_str(&rest[i..]);
                return out;
            };
            out.push_str(tags.tag(&rest[i + 1..i + j]).unwrap_or_default());
            rest = &rest[i + j + 1..];
        }
        out.push_str(rest);
        out
    }

    pub fn kind(&self, tags: &dyn Tags) -> String {
        Self::render(&self.rule.kind, tags)
    }

    /// The source text of every mapped attribute (`attr: tag`, `"=const"`,
    /// `"{template}"`).
    pub fn attr_texts(&self, tags: &dyn Tags) -> Vec<(String, Option<String>)> {
        self.rule
            .attrs
            .iter()
            .map(|(name, src)| {
                let v = if src.starts_with('=') || src.contains('{') {
                    Some(Self::render(src, tags)).filter(|s| !s.is_empty())
                } else {
                    tags.tag(src).map(str::to_owned)
                };
                (name.clone(), v)
            })
            .collect()
    }

    /// Converts a geometry for this rule's mode; `None` = the element has no
    /// such geometry (an open way under `area`, a node under `line`, …) or is
    /// dropped by `drop_unnamed_below`.
    pub fn shape(&self, g: &Geometry, named: bool) -> Option<Geometry> {
        let out = match (self.rule.geometry, g) {
            (GeomMode::Point, Geometry::Point(_)) => Some(g.clone()),
            (GeomMode::Point, Geometry::LineString(l)) if is_closed(l) => {
                Geometry::Polygon(vec![l.clone()])
                    .rep_point()
                    .map(Geometry::Point)
            }
            (
                GeomMode::Point,
                Geometry::Polygon(_) | Geometry::MultiPolygon(_) | Geometry::LineString(_),
            ) => g.rep_point().map(Geometry::Point),
            (GeomMode::Line, Geometry::LineString(l)) if l.len() >= 2 => Some(g.clone()),
            (GeomMode::Line, Geometry::MultiLineString(ls)) if !ls.is_empty() => Some(g.clone()),
            (GeomMode::Area, Geometry::LineString(l)) if is_closed(l) => {
                Some(Geometry::Polygon(vec![l.clone()]))
            }
            (GeomMode::Area, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) => Some(g.clone()),
            _ => None,
        }?;
        if let (Some(min), false) = (self.rule.drop_unnamed_below, named) {
            let outer = match &out {
                Geometry::Polygon(rs) => rs.first().map_or(0.0, |r| rough_ring_size(r)),
                Geometry::MultiPolygon(ps) => ps
                    .iter()
                    .filter_map(|p| p.first())
                    .map(|r| rough_ring_size(r))
                    .fold(0.0, f64::max),
                _ => f64::INFINITY,
            };
            if outer < min {
                return None;
            }
        }
        Some(out)
    }
}

fn is_closed(l: &[[f64; 2]]) -> bool {
    l.len() >= 4 && l.first() == l.last()
}

/// First present tag of a preference list.
pub fn first_tag(tags: &dyn Tags, keys: &[String], default: &[&str]) -> Option<String> {
    let pick = |k: &str| {
        tags.tag(k)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    if keys.is_empty() {
        default.iter().find_map(|k| pick(k))
    } else {
        keys.iter().find_map(|k| pick(k))
    }
}

/// Text → typed JSON value of a declared field. `Ok(None)` = not
/// representable (e.g. `beds=12;30`): the attribute is left out.
pub fn typed(field: &FieldSpec, text: &str) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    match field.ty {
        FieldType::String => (field.values.is_empty() || field.values.iter().any(|v| v == t))
            .then(|| Value::String(t.to_owned())),
        FieldType::Integer => t.parse::<i64>().ok().map(Value::from),
        FieldType::Number => t
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .and_then(|v| serde_json::Number::from_f64(v).map(Value::Number)),
        FieldType::Boolean => match t.to_ascii_lowercase().as_str() {
            "yes" | "true" | "1" => Some(Value::Bool(true)),
            "no" | "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
    }
}

/// Typed attributes of one feature from the rule's mapping; counts values
/// that did not fit their field's type in `dropped`.
pub fn attrs_of(
    rule: &Compiled,
    tags: &dyn Tags,
    fields: &BTreeMap<&str, &FieldSpec>,
    dropped: &mut usize,
) -> Result<serde_json::Map<String, Value>> {
    let mut m = serde_json::Map::new();
    for (name, text) in rule.attr_texts(tags) {
        let f = fields.get(name.as_str()).with_context(|| {
            format!("attribute {name:?} is not a declared field of the dataset")
        })?;
        if let Some(t) = text {
            match typed(f, &t) {
                Some(v) => {
                    m.insert(name, v);
                }
                None => *dropped += 1,
            }
        }
    }
    Ok(m)
}

/// Every `match` value list of every rule (for documentation/tests).
#[must_use]
pub fn describe(rule: &Compiled) -> String {
    rule.rule
        .when
        .iter()
        .map(|(k, v)| format!("{k}={}", v.list().join("|")))
        .collect::<Vec<_>>()
        .join(" & ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    fn rule(yaml: &str) -> Compiled {
        serde_yaml_ng::from_str::<Rule>(yaml)
            .expect("rule")
            .compile()
            .expect("compile")
    }

    #[test]
    fn matching_templates_and_types() {
        let r = rule(
            "match: {highway: [construction], construction: motorway}\ngeometry: line\nkind: motorway\n\
             attrs: {status: '=under_construction', ref: ref, label: '{ref} {name}'}\ntypes: [way]\n",
        );
        let t = tags(&[
            ("highway", "construction"),
            ("construction", "motorway"),
            ("ref", "E01"),
        ]);
        assert!(r.matches_tags("way", &t));
        assert!(!r.matches_tags("node", &t));
        assert!(!r.matches_tags("way", &tags(&[("highway", "construction")])));
        let a = r.attr_texts(&t);
        assert_eq!(
            a,
            vec![
                ("label".into(), Some("E01 ".into())),
                ("ref".into(), Some("E01".into())),
                ("status".into(), Some("under_construction".into())),
            ]
        );
        let any = rule(
            "match: {tourism: '*'}\nnot: {tourism: [hotel]}\ngeometry: point\nkind: '{tourism}'\n",
        );
        assert!(any.matches_tags("node", &tags(&[("tourism", "museum")])));
        assert!(!any.matches_tags("node", &tags(&[("tourism", "hotel")])));
        assert_eq!(any.kind(&tags(&[("tourism", "museum")])), "museum");
    }

    #[test]
    fn shapes_and_v1_size_cut() {
        // rough size 0.02 (< 0.04)
        let ring = vec![
            [0.0, 0.0],
            [0.005, 0.0],
            [0.005, 0.005],
            [0.0, 0.005],
            [0.0, 0.0],
        ];
        let closed = Geometry::LineString(ring.clone());
        let open = Geometry::LineString(ring[..3].to_vec());
        let area = rule(
            "match: {natural: water}\ngeometry: area\nkind: water_body\ndrop_unnamed_below: 0.04\n",
        );
        assert!(area.shape(&closed, true).is_some());
        assert!(
            area.shape(&closed, false).is_none(),
            "0.04 rough size cut for unnamed"
        );
        assert!(area.shape(&open, true).is_none());
        let pt = rule("match: {amenity: school}\ngeometry: point\nkind: school\n");
        match pt.shape(&closed, false) {
            Some(Geometry::Point(p)) => assert!(p[0] > 0.0 && p[0] < 0.005),
            other => panic!("{other:?}"),
        }
        let line = rule("match: {highway: primary}\ngeometry: line\nkind: primary\n");
        assert!(line.shape(&Geometry::Point([0.0, 0.0]), false).is_none());
    }

    #[test]
    fn typed_fields() {
        let f = |ty: FieldType, values: &[&str]| FieldSpec {
            name: "x".into(),
            ty,
            label: None,
            unit: None,
            measure_type: None,
            values: values.iter().map(|s| (*s).to_owned()).collect(),
        };
        assert_eq!(
            typed(&f(FieldType::Integer, &[]), " 120 "),
            Some(Value::from(120))
        );
        assert_eq!(typed(&f(FieldType::Integer, &[]), "12;30"), None);
        assert_eq!(
            typed(&f(FieldType::Boolean, &[]), "yes"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            typed(&f(FieldType::String, &["open", "proposed"]), "open"),
            Some(Value::from("open"))
        );
        assert_eq!(typed(&f(FieldType::String, &["open"]), "closed"), None);
    }
}
