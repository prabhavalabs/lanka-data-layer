//! What a document depends on (10 §5 `app.map_refs`): public datasets,
//! workspace datasets, analysis results and external hosts — for access
//! checks, takedown ("which maps use this dataset") and result pinning.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::doc::{MapDocument, Source};

/// Raw references, before indicator ids are mapped to their datasets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct References {
    /// Indicator ids (sources and analysis variables).
    pub indicators: BTreeSet<String>,
    /// Dataset ids named directly (`features`, grid layers).
    pub datasets: BTreeSet<String>,
    pub grids: BTreeSet<String>,
    /// `(scheme, level)` of boundary sources.
    pub boundaries: BTreeSet<(String, u8)>,
    /// `an_…` ids written into analyses at freeze.
    pub analysis_results: BTreeSet<String>,
    /// Hosts of `external` sources (fetched by the browser only).
    pub external_hosts: BTreeSet<String>,
}

/// One `app.map_refs` row: `ref_kind` ∈ dataset | ws_dataset | analysis | external_host.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct MapRef {
    pub ref_kind: &'static str,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// Workspace ids are `@workspace/id`.
#[must_use]
pub fn is_workspace(id: &str) -> bool {
    id.starts_with('@')
}

#[must_use]
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn walk_values(v: &Value, f: &mut impl FnMut(&str, &Value)) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                f(k, x);
                walk_values(x, f);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk_values(x, f)),
        _ => {}
    }
}

/// Collects every reference of a validated document.
#[must_use]
pub fn references(doc: &MapDocument) -> References {
    let mut r = References::default();
    for s in doc.sources.values() {
        match s {
            Source::Indicator(i) => r
                .indicators
                .extend(i.indicator_ids().into_iter().map(str::to_owned)),
            Source::Boundaries(b) => {
                r.boundaries.insert((b.scheme.clone(), b.level));
            }
            Source::Features(f) => {
                r.datasets.insert(f.dataset.clone());
            }
            Source::Grid(g) => {
                r.grids.insert(g.grid.clone());
            }
            Source::External(e) => r.external_hosts.extend(host_of(&e.url)),
            Source::Analysis(_) | Source::Inline(_) => {}
        }
    }
    for a in doc.analyses.values() {
        if let Some(id) = a.get("result").and_then(Value::as_str) {
            r.analysis_results.insert(id.to_owned());
        }
        walk_values(a, &mut |k, x| {
            if let Some(s) = x.as_str() {
                match k {
                    "indicator" => {
                        r.indicators.insert(s.to_owned());
                    }
                    "dataset" => {
                        r.datasets.insert(s.to_owned());
                    }
                    "grid" => {
                        r.grids.insert(s.to_owned());
                    }
                    _ => {}
                }
            }
        });
    }
    r
}

/// `app.map_refs` rows. `dataset_of` maps a public indicator or grid id to
/// its dataset (from the catalogue); unknown ids are kept as given so a
/// takedown of a dataset added later still finds the map.
#[must_use]
pub fn map_refs(r: &References, dataset_of: impl Fn(&str) -> Option<String>) -> Vec<MapRef> {
    let mut out = BTreeSet::new();
    let mut add = |id: &str, mapped: Option<String>| {
        if is_workspace(id) {
            // Workspace indicator → dataset mapping lives in the app plane (WP-32).
            out.insert(MapRef {
                ref_kind: "ws_dataset",
                reference: id.to_owned(),
            });
        } else {
            out.insert(MapRef {
                ref_kind: "dataset",
                reference: mapped.unwrap_or_else(|| id.to_owned()),
            });
        }
    };
    for i in r.indicators.iter().chain(&r.grids) {
        add(i, dataset_of(i));
    }
    for d in &r.datasets {
        add(d, None);
    }
    for a in &r.analysis_results {
        out.insert(MapRef {
            ref_kind: "analysis",
            reference: a.clone(),
        });
    }
    for h in &r.external_hosts {
        out.insert(MapRef {
            ref_kind: "external_host",
            reference: h.clone(),
        });
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(
            host_of("https://Tiles.Example.org:8443/a/b.pmtiles?x=1").as_deref(),
            Some("tiles.example.org")
        );
        assert_eq!(
            host_of("https://user@host.io/x").as_deref(),
            Some("host.io")
        );
        assert_eq!(host_of("nope"), None);
    }
}
