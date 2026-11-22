//! `geo_file` driver (03 §2.2): GeoJSON files → features.
//!
//! ```yaml
//! source: rda-gis
//! driver: geo_file
//! version: 1.0.0
//! discover:
//!   files:
//!     - { key: a-class, url: "https://…/a_class.geojson", file: a_class.geojson }
//! extract:
//!   key: road_id           # property holding the upstream id (default: the feature `id`)
//! standardise:
//!   outputs: [ {dataset, rules: [...]} ]   # feature_rules.rs; element type `feature`
//! ```
//!
//! Each file is one artifact (conditional GET and the artifact hash do the
//! change detection). `extract` writes one `features` extract row per
//! GeoJSON feature — properties as text (numbers keep their token), the
//! geometry exactly as printed — keyed `<file key>/<id>` (or `<file
//! key>/<index>` without an id). FeatureCollections and GeoJSON text
//! sequences (one feature per line) are both read. Shapefile, GeoPackage and
//! KML go through GDAL in the extraction sandbox (WP-08).

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::connector::{
    Candidate, Connector, DiscoverCtx, Discovery, ExtractCtx, Fetched, OutputHeader, OutputKind,
    OutputSink, StdCtx,
};
use crate::drivers::feature_rules::{Compiled, OutputSpec};
use crate::drivers::json_path;
use crate::drivers::osm_pbf::{Output, standardise_features};
use crate::extract::{ExtractOut, ExtractRef, Feature};
use crate::registry::{ConnectorSpec, Loaded};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub source: String,
    pub driver: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub discover: DiscoverSpec,
    #[serde(default)]
    pub extract: ExtractSpec,
    pub standardise: StandardiseSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSpec {
    pub files: Vec<FileSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub key: String,
    pub url: String,
    /// File name under `--from` (default: the last URL segment).
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub edition: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractSpec {
    /// Property with the upstream id (default: the GeoJSON feature `id`).
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardiseSpec {
    pub outputs: Vec<OutputSpec>,
    /// Edition of the outputs (`vintage`), when the files carry none.
    #[serde(default)]
    pub vintage: Option<String>,
}

pub struct GeoFile {
    spec: Spec,
    outputs: Vec<Output>,
    sha256: [u8; 32],
}

impl GeoFile {
    pub fn from_spec(l: &Loaded<ConnectorSpec>) -> Result<Self> {
        let spec: Spec = serde_json::from_value(l.json.clone())
            .with_context(|| format!("{}: invalid geo_file spec", l.path.display()))?;
        Self::new(spec, l.sha256)
    }

    pub fn new(spec: Spec, sha256: [u8; 32]) -> Result<Self> {
        ensure!(!spec.discover.files.is_empty(), "discover.files is empty");
        let outputs = spec
            .standardise
            .outputs
            .iter()
            .map(|o| -> Result<Output> {
                Ok(Output {
                    spec: o.clone(),
                    rules: o
                        .rules
                        .iter()
                        .cloned()
                        .map(|r| r.compile())
                        .collect::<Result<Vec<Compiled>>>()?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            spec,
            outputs,
            sha256,
        })
    }
}

#[derive(Deserialize)]
struct Fc<'a> {
    #[serde(borrow)]
    features: Vec<Feat<'a>>,
}

#[derive(Deserialize)]
struct Feat<'a> {
    #[serde(borrow, default)]
    id: Option<&'a RawValue>,
    #[serde(borrow, default)]
    properties: Option<&'a RawValue>,
    #[serde(borrow, default)]
    geometry: Option<&'a RawValue>,
}

fn text_of(v: &RawValue) -> Result<Option<String>> {
    let t = v.get().trim();
    if t.starts_with('{') || t.starts_with('[') {
        Ok(Some(t.to_owned()))
    } else {
        json_path::text(Some(v))
    }
}

/// Reads one GeoJSON file (FeatureCollection or text sequence).
pub fn extract_file(
    bytes: &[u8],
    file_key: &str,
    key_prop: Option<&str>,
    artifact_idx: u32,
) -> Result<Vec<Feature>> {
    let text = std::str::from_utf8(bytes).context("GeoJSON is not UTF-8")?;
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let feats: Vec<Feat<'_>> = if trimmed.starts_with('{')
        && trimmed.contains("\"FeatureCollection\"")
        && serde_json::from_str::<Fc<'_>>(trimmed).is_ok()
    {
        serde_json::from_str::<Fc<'_>>(trimmed)?.features
    } else {
        trimmed
            .lines()
            .map(|l| l.trim_matches(|c: char| c == '\u{1e}' || c.is_whitespace()))
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str::<Feat<'_>>(l).context("GeoJSON text sequence line"))
            .collect::<Result<_>>()?
    };
    let mut out = Vec::with_capacity(feats.len());
    for (i, f) in feats.into_iter().enumerate() {
        let Some(g) = f.geometry.filter(|g| g.get().trim() != "null") else {
            continue;
        };
        let mut tags = BTreeMap::new();
        if let Some(p) = f.properties.filter(|p| p.get().trim() != "null") {
            let props: BTreeMap<String, &RawValue> =
                serde_json::from_str(p.get()).with_context(|| {
                    format!("{file_key}: feature {i}: properties are not an object")
                })?;
            for (k, v) in props {
                if let Some(t) = text_of(v)? {
                    tags.insert(k, t);
                }
            }
        }
        let id = match key_prop {
            Some(k) => tags.get(k).cloned(),
            None => f.id.map(text_of).transpose()?.flatten(),
        };
        out.push(Feature {
            source_key: format!("{file_key}/{}", id.unwrap_or_else(|| i.to_string())),
            kind: "feature".to_owned(),
            tags,
            geojson: g.get().to_owned(),
            artifact_idx,
        });
    }
    Ok(out)
}

// async_trait marks the boxed futures #[must_use]; clippy then flags that.
#[allow(clippy::double_must_use)]
#[async_trait]
impl Connector for GeoFile {
    fn source_id(&self) -> &str {
        &self.spec.source
    }

    fn version(&self) -> &str {
        &self.spec.version
    }

    fn extractor(&self) -> &str {
        "geo_file"
    }

    fn spec_sha256(&self) -> Option<[u8; 32]> {
        Some(self.sha256)
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let mut candidates = Vec::new();
        for f in &self.spec.discover.files {
            let local = match &ctx.opts.from_dir {
                Some(dir) => {
                    let name = f
                        .file
                        .clone()
                        .or_else(|| f.url.rsplit('/').next().map(str::to_owned))
                        .context("file name")?;
                    let p = dir.join(&name);
                    ensure!(p.is_file(), "{} not found", p.display());
                    Some(p)
                }
                None => None,
            };
            candidates.push(Candidate {
                key: f.key.clone(),
                url: f.url.clone(),
                local_path: local,
                media_type: Some("application/geo+json".to_owned()),
                edition: f.edition.clone(),
                ..Candidate::default()
            });
        }
        Ok(Discovery {
            candidates,
            fingerprint: None,
        })
    }

    async fn extract(&self, _ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>> {
        let mut rows = Vec::new();
        for (i, f) in fetched.iter().enumerate() {
            rows.extend(extract_file(
                &f.read()?,
                &f.key,
                self.spec.extract.key.as_deref(),
                u32::try_from(i)?,
            )?);
        }
        Ok(vec![ExtractOut::Features(rows)])
    }

    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()> {
        let rows = extracts.iter().enumerate().flat_map(|(ei, e)| {
            e.data
                .features()
                .iter()
                .enumerate()
                .map(move |(ri, f)| (ei, u32::try_from(ri).unwrap_or(u32::MAX), f))
        });
        let (per, dropped) = standardise_features(
            &self.outputs,
            |ds| {
                ctx.registry
                    .dataset(ds)
                    .map(|d| d.fields.clone())
                    .unwrap_or_default()
            },
            rows,
        )?;
        if dropped > 0 {
            ctx.warn(format!(
                "{dropped} attribute value(s) did not fit their declared field type"
            ));
        }
        for (o, feats) in self.outputs.iter().zip(per) {
            sink.begin(OutputHeader {
                dataset_id: o.spec.dataset.clone(),
                kind: OutputKind::Features,
                vintage: self.spec.standardise.vintage.clone(),
            })?;
            for f in feats {
                sink.feature(f)?;
            }
            sink.end()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FC: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"road_id":"A001","class":"A","name":"Colombo - Kandy","km":115.5},
       "geometry":{"type":"LineString","coordinates":[[79.8612345678,6.93],[80.6,7.29]]}},
      {"type":"Feature","properties":{"road_id":"B123","class":"B","name":null},
       "geometry":{"type":"LineString","coordinates":[[80.1,7.0],[80.2,7.1]]}},
      {"type":"Feature","properties":{"road_id":"X"},"geometry":null}
    ]}"#;

    #[test]
    fn reads_collections_and_sequences() {
        let rows = extract_file(FC.as_bytes(), "a-class", Some("road_id"), 0).expect("extract");
        assert_eq!(rows.len(), 2, "features without geometry are skipped");
        assert_eq!(rows[0].source_key, "a-class/A001");
        assert_eq!(rows[0].tags["km"], "115.5");
        assert!(!rows[1].tags.contains_key("name"));
        assert!(
            rows[0].geojson.contains("79.8612345678"),
            "geometry verbatim in L1"
        );
        let seq = "{\"type\":\"Feature\",\"id\":7,\"properties\":{},\"geometry\":{\"type\":\"Point\",\"coordinates\":[80,7]}}\n\
                   \u{1e}{\"type\":\"Feature\",\"properties\":{},\"geometry\":{\"type\":\"Point\",\"coordinates\":[81,8]}}\n";
        let rows = extract_file(seq.as_bytes(), "pts", None, 1).expect("seq");
        assert_eq!(
            rows.iter()
                .map(|r| r.source_key.as_str())
                .collect::<Vec<_>>(),
            ["pts/7", "pts/1"]
        );
    }

    #[test]
    fn standardises_with_rules() {
        let spec: Spec = serde_yaml_ng::from_str(
            r#"
source: rda
driver: geo_file
version: 1.0.0
discover: { files: [ { key: a-class, url: "https://example.org/a.geojson" } ] }
extract: { key: road_id }
standardise:
  outputs:
    - dataset: roads
      rules:
        - { match: { class: [A, B] }, geometry: line, kind: "class_{class}", attrs: { ref: road_id } }
"#,
        )
        .expect("spec");
        let g = GeoFile::new(spec, [0; 32]).expect("compile");
        let rows = extract_file(FC.as_bytes(), "a-class", Some("road_id"), 0).expect("extract");
        let fields = |_: &str| {
            vec![crate::registry::FieldSpec {
                name: "ref".into(),
                ty: crate::registry::FieldType::String,
                label: None,
                unit: None,
                measure_type: None,
                values: Vec::new(),
            }]
        };
        let (per, _) = standardise_features(
            &g.outputs,
            fields,
            rows.iter().enumerate().map(|(i, f)| (0, i as u32, f)),
        )
        .expect("standardise");
        assert_eq!(per[0].len(), 2);
        assert_eq!(per[0][0].kind, "class_A");
        assert_eq!(per[0][0].attrs["ref"], "A001");
        assert!(
            per[0][0].geometry.contains("79.861235"),
            "6 decimals: {}",
            per[0][0].geometry
        );
    }
}
