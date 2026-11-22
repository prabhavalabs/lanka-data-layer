//! `GET /app/v1/maps/{id}/package` (10 §6.1, §6.4): a Frictionless Data
//! Package (`datapackage.json`) listing, per layer, the download URLs of
//! the data behind it and its licence. Only ordinary `/v2` endpoints are
//! linked, so licence tiers apply as everywhere: a private, `link`-tier or
//! not-yet-served source appears in `ldl:metadata_only`, never as a path.

use std::collections::{BTreeMap, BTreeSet};

use lanka_core::Catalog;
use lanka_mapdoc::MapDocument;
use lanka_mapdoc::doc::Source;
use lanka_mapdoc::resolve::{Resolved, ResolvedSource, Shape};
use serde_json::{Value, json};

/// Layer ids per source id, in draw order.
fn layers_by_source(doc: &MapDocument) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for l in doc.layers_flat() {
        out.entry(l.source.clone()).or_default().push(l.id.clone());
    }
    out
}

/// A Frictionless resource name: `^[a-z0-9._-]+$`.
fn resource_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if n.is_empty() { "source".to_owned() } else { n }
}

fn with_format(url: &str, format: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}format={format}")
}

struct Licensing {
    licenses: Vec<Value>,
    sources: Vec<Value>,
    /// Why the data may not be linked for download, if so.
    blocked: Option<String>,
}

/// Licences and publishers behind `datasets`.
fn licensing(cat: &Catalog, datasets: &BTreeSet<String>) -> Licensing {
    let mut licenses = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut blocked = None;
    for d in datasets {
        let Some(ds) = cat.datasets.get(d) else {
            continue;
        };
        for sid in &ds.source_ids {
            let Some(src) = cat.sources.get(sid) else {
                continue;
            };
            let lic = cat.licences.get(&src.licence_id);
            let name = lic
                .and_then(|l| l.spdx.clone())
                .unwrap_or_else(|| src.licence_id.clone());
            let redistribution = lic.map_or("link", |l| l.redistribution.as_str());
            let status = lic.map_or("permission_unclear", |l| l.status.as_str());
            if matches!(redistribution, "link" | "internal") || status == "restricted" {
                blocked.get_or_insert_with(|| {
                    format!(
                        "the licence of {} permits linking to the source only ({redistribution}, {status})",
                        src.publisher
                    )
                });
            }
            let mut l = json!({ "name": name, "title": src.licence_id });
            if let Some(url) = lic.and_then(|l| l.url.clone()) {
                l["path"] = Value::String(url);
            }
            l["ldl:status"] = Value::String(status.to_owned());
            licenses.insert(name, l);
            sources.insert(
                src.id.clone(),
                json!({ "title": format!("{} — {}", src.publisher, src.title), "path": src.homepage }),
            );
        }
    }
    Licensing {
        licenses: licenses.into_values().collect(),
        sources: sources.into_values().collect(),
        blocked,
    }
}

/// The datasets a resolved source reads.
fn datasets_of(doc_src: &Source, r: &ResolvedSource, cat: &Catalog) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = r.versions.iter().map(|v| v.dataset.clone()).collect();
    if out.is_empty() {
        match doc_src {
            Source::Indicator(i) => {
                for id in i.indicator_ids() {
                    if let Some(ind) = cat.indicators.get(id) {
                        out.insert(ind.dataset_id.clone());
                    }
                }
            }
            Source::Boundaries(_) => out.extend(cat.boundary_dataset().map(str::to_owned)),
            _ => {}
        }
    }
    out
}

/// Builds the package of `doc` (stored as `id`) from its resolution.
#[must_use]
pub fn package(
    id: &str,
    doc: &MapDocument,
    resolved: &Resolved,
    cat: &Catalog,
    base: &str,
) -> Value {
    let base = base.trim_end_matches('/');
    let by_source = layers_by_source(doc);
    let mut resources: Vec<Value> = Vec::new();
    let mut metadata_only: Vec<Value> = Vec::new();
    let mut used_names = BTreeSet::new();
    let mut unique = |n: String| {
        let mut name = n.clone();
        let mut k = 2;
        while !used_names.insert(name.clone()) {
            name = format!("{n}-{k}");
            k += 1;
        }
        name
    };
    for (sid, layers) in &by_source {
        let (Some(src), Some(r)) = (doc.sources.get(sid), resolved.sources.get(sid)) else {
            continue;
        };
        let datasets = datasets_of(src, r, cat);
        let lic = licensing(cat, &datasets);
        let only = |reason: String| {
            json!({
                "source": sid, "kind": src.kind(), "layers": layers,
                "datasets": datasets, "licenses": lic.licenses, "sources": lic.sources,
                "reason": reason,
            })
        };
        if r.access == "denied" {
            metadata_only.push(only("private workspace data".to_owned()));
            continue;
        }
        let urls: Vec<(String, u8)> = match &r.shape {
            Shape::Frame { frame, by_zoom, .. } => match by_zoom {
                Some(z) if !z.is_empty() => z.iter().map(|z| (z.frame.clone(), z.level)).collect(),
                _ => {
                    let level = match src {
                        Source::Indicator(i) => i.level.levels().first().copied().unwrap_or(0),
                        _ => 0,
                    };
                    vec![(frame.clone(), level)]
                }
            },
            Shape::Boundaries { .. } => {
                metadata_only.push(only(
                    "boundary geometry is served as vector tiles; GeoParquet downloads arrive with the exports (WP-09)"
                        .to_owned(),
                ));
                continue;
            }
            Shape::Inline => {
                metadata_only.push(only(
                    "inline annotations are part of the document itself".to_owned(),
                ));
                continue;
            }
            Shape::External { url } => {
                metadata_only.push(json!({
                    "source": sid, "kind": src.kind(), "layers": layers, "url": url,
                    "reason": "external data is loaded by the browser from its own host; check its terms",
                }));
                continue;
            }
            Shape::Unavailable => {
                metadata_only.push(only(
                    "not drawable on this deployment; see resolved.problems".to_owned(),
                ));
                continue;
            }
        };
        if let Some(reason) = lic.blocked.clone() {
            metadata_only.push(only(reason));
            continue;
        }
        let mut seen = BTreeSet::new();
        for (url, level) in urls {
            if !seen.insert(url.clone()) {
                continue;
            }
            let name = if seen.len() == 1
                && !matches!(&r.shape, Shape::Frame { by_zoom: Some(z), .. } if z.len() > 1)
            {
                unique(resource_name(sid))
            } else {
                unique(resource_name(&format!("{sid}-adm{level}")))
            };
            let mut res = json!({
                "name": name,
                "title": format!("Source {sid} at admin level {level}"),
                "path": format!("{base}{}", with_format(&url, "csv")),
                "format": "csv",
                "mediatype": "text/csv",
                "encoding": "utf-8",
                "licenses": lic.licenses,
                "sources": lic.sources,
                "ldl:source": sid,
                "ldl:kind": src.kind(),
                "ldl:layers": layers,
                "ldl:datasets": datasets,
                "ldl:alternates": {
                    "json": format!("{base}{url}"),
                    "arrow": format!("{base}{}", with_format(&url, "arrow")),
                },
            });
            if let Some(dsv) = &r.dsv {
                res["ldl:dsv"] = Value::String(dsv.clone());
            }
            if let Some(class) = &r.class {
                res["ldl:class"] = Value::String(class.clone());
            }
            resources.push(res);
        }
    }

    let mut pkg = json!({
        "profile": "data-package",
        "name": resource_name(id),
        "id": format!("{base}/m/{id}"),
        "homepage": format!("{base}/m/{id}"),
        "title": doc.title.en(),
        "resources": resources,
        "ldl:document": format!("{base}/app/v1/maps/{id}"),
        "ldl:data_version": resolved.data_version,
        "ldl:metadata_only": metadata_only,
        "ldl:attribution": resolved.attribution,
        "ldl:notices": resolved.notices,
    });
    if let Some(d) = &doc.description {
        pkg["description"] = Value::String(d.clone());
    }
    if let Some(l) = doc.meta.as_ref().and_then(|m| m.licence.clone()) {
        // the licence of the composition (the document), not of the data
        pkg["licenses"] = json!([{ "name": l, "title": "Map document" }]);
    }
    pkg
}
