//! OCHA COD-AB Sri Lanka administrative units, ADM0–ADM4.
//!
//! This first slice reads the v1 foundry's per-level layer exports
//! (`layers/country.geojson`, `layers/admin-adm{1..4}.geojson`: COD-AB
//! p-codes, trilingual names, parent p-codes) from `--from <dir>`. The files
//! are archived and hashed like any download, so provenance is identical
//! when the direct HDX connector replaces the local read.
//!
//! `extract` writes a `features` extract (one row per GeoJSON feature: its
//! properties as text and its geometry exactly as printed); `standardise`
//! checks codes and levels against the country profile and emits the units.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::value::RawValue;

use lanka_core::AdminCodes;

use crate::connector::{
    AdminUnitIn, BoundarySet, Candidate, Connector, DiscoverCtx, Discovery, ExtractCtx, Fetched,
    OutputHeader, OutputKind, OutputSink, StdCtx,
};
use crate::drivers::json_path;
use crate::extract::{ExtractOut, ExtractRef, Feature};

pub struct CodAb;

/// Connector version (part of every extract and run record).
pub const VERSION: &str = "2.0.0";

const SOURCE: &str = "hdx-cod-ab-lka";
const DATASET: &str = "cod-ab-lka-boundaries";
const FILES: &[(&str, &str)] = &[
    ("adm0", "country.geojson"),
    ("adm1", "admin-adm1.geojson"),
    ("adm2", "admin-adm2.geojson"),
    ("adm3", "admin-adm3.geojson"),
    ("adm4", "admin-adm4.geojson"),
];

#[derive(Deserialize)]
struct Fc<'a> {
    #[serde(borrow)]
    features: Vec<Feat<'a>>,
}

#[derive(Deserialize)]
struct Feat<'a> {
    #[serde(borrow)]
    properties: Option<&'a RawValue>,
    #[serde(borrow)]
    geometry: &'a RawValue,
}

fn clean(s: Option<&String>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Reads one level file into feature rows: every scalar property as text
/// (numbers keep their token), the geometry verbatim.
pub fn extract_level(bytes: &[u8], key: &str, artifact_idx: u32) -> Result<Vec<Feature>> {
    let fc: Fc<'_> = serde_json::from_slice(bytes)
        .with_context(|| format!("{key}: not a GeoJSON FeatureCollection"))?;
    let mut out = Vec::with_capacity(fc.features.len());
    for f in fc.features {
        let mut tags = BTreeMap::new();
        if let Some(p) = f.properties.filter(|p| p.get().trim() != "null") {
            let props: BTreeMap<String, &RawValue> = serde_json::from_str(p.get())
                .with_context(|| format!("{key}: feature properties are not an object"))?;
            for (k, v) in props {
                let t = v.get().trim();
                let text = if t.starts_with('{') || t.starts_with('[') {
                    Some(t.to_owned())
                } else {
                    json_path::text(Some(v))?
                };
                if let Some(text) = text {
                    tags.insert(k, text);
                }
            }
        }
        out.push(Feature {
            source_key: tags.get("pcode").cloned().unwrap_or_default(),
            kind: key.to_owned(),
            tags,
            geojson: f.geometry.get().to_owned(),
            artifact_idx,
        });
    }
    Ok(out)
}

/// Maps the features of one level file to units. Codes are checked against
/// the country profile's admin rules; the level-0 unit takes the profile's
/// root code and `country_name` when the file carries none.
pub fn units_of_level<'a>(
    features: impl Iterator<Item = &'a Feature>,
    key: &str,
    admin: &AdminCodes,
    country_name: &str,
) -> Result<Vec<AdminUnitIn>> {
    let expected: i16 = key[3..].parse()?;
    let mut out = Vec::new();
    for f in features {
        let p = &f.tags;
        let level = match p.get("level") {
            Some(l) => l
                .parse::<i16>()
                .with_context(|| format!("{key}: level {l:?} is not an integer"))?,
            None => expected,
        };
        ensure!(level == expected, "{key}: feature at level {level}");
        let (pcode, name_en, parent) = if level == 0 {
            (
                admin.root().to_owned(),
                clean(p.get("name_en"))
                    .or(clean(p.get("name")))
                    .unwrap_or_else(|| country_name.to_owned()),
                None,
            )
        } else {
            let pcode =
                clean(p.get("pcode")).with_context(|| format!("{key}: feature without pcode"))?;
            // 65 COD-AB GN divisions carry no name in any script; the p-code
            // stands in as the label (reported as a warning by `standardise`).
            let name = clean(p.get("name_en"))
                .or(clean(p.get("name")))
                .unwrap_or_else(|| pcode.clone());
            let parent = clean(p.get("parent_pcode"))
                .with_context(|| format!("{key}: {pcode} has no parent_pcode"))?;
            (pcode, name, Some(parent))
        };
        let code = admin
            .parse(&pcode)
            .with_context(|| format!("{key}: bad p-code"))?;
        ensure!(
            i16::from(code.level()) == level,
            "{key}: {pcode} is a level-{} code in a level-{level} file",
            code.level()
        );
        out.push(AdminUnitIn {
            pcode,
            level,
            name_en,
            name_si: clean(p.get("name_si")),
            name_ta: clean(p.get("name_ta")),
            parent_pcode: parent,
            geometry: RawValue::from_string(f.geojson.clone())
                .with_context(|| format!("{key}: geometry is not JSON"))?,
            artifact: f.artifact_idx as usize,
        });
    }
    Ok(out)
}

/// Extract + standardise of one level file (tests).
pub fn parse_level(
    bytes: &[u8],
    key: &str,
    artifact: usize,
    admin: &AdminCodes,
    country_name: &str,
) -> Result<Vec<AdminUnitIn>> {
    let f = extract_level(bytes, key, u32::try_from(artifact)?)?;
    units_of_level(f.iter(), key, admin, country_name)
}

#[async_trait]
impl Connector for CodAb {
    fn source_id(&self) -> &str {
        SOURCE
    }

    fn version(&self) -> &str {
        VERSION
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let Some(dir) = &ctx.opts.from_dir else {
            bail!(
                "hdx-cod-ab-lka reads local layer exports for now: pass --from <dir> containing {}",
                FILES.iter().map(|f| f.1).collect::<Vec<_>>().join(", ")
            );
        };
        Ok(Discovery {
            candidates: FILES
                .iter()
                .map(|(k, f)| Candidate {
                    key: (*k).to_owned(),
                    url: format!("https://data.humdata.org/dataset/cod-ab-lka#{f}"),
                    local_path: Some(dir.join(f)),
                    media_type: Some("application/geo+json".to_owned()),
                    edition: Some("COD-AB LKA via v1 foundry layers".to_owned()),
                    ..Candidate::default()
                })
                .collect(),
            fingerprint: None,
        })
    }

    async fn extract(&self, _ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>> {
        let mut features = Vec::new();
        for (i, f) in fetched.iter().enumerate() {
            features.extend(extract_level(&f.read()?, &f.key, u32::try_from(i)?)?);
        }
        Ok(vec![ExtractOut::Features(features)])
    }

    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()> {
        let x = extracts.first().context("no extract")?;
        let features = x.data.features();
        let mut units = Vec::new();
        for (key, _) in FILES {
            units.extend(units_of_level(
                features.iter().filter(|f| f.kind == *key),
                key,
                &ctx.profile.admin,
                &ctx.profile.spec.name.en,
            )?);
        }
        // Parents follow from the p-codes (COD-AB codes nest by
        // construction); the layer files' parent column is only a cross-check.
        // The v1 export took district parents from geoBoundaries and put
        // Ampara (LK52) under Uva (LK8).
        let fixed = crate::geo_load::parents_from_pcodes(&ctx.profile.admin, &mut units)?;
        if !fixed.is_empty() {
            ctx.warn(format!(
                "{} parent(s) in the layer files disagree with the p-codes; the p-code parent is used: {}",
                fixed.len(),
                fixed.join("; ")
            ));
        }
        let unnamed = units.iter().filter(|u| u.name_en == u.pcode).count();
        if unnamed > 0 {
            ctx.warn(format!(
                "{unnamed} unit(s) have no name upstream; p-code used as label"
            ));
        }
        sink.begin(OutputHeader {
            dataset_id: DATASET.to_owned(),
            kind: OutputKind::AdminUnits(BoundarySet {
                id: ctx.profile.reference_boundary_set().to_owned(),
                title: "OCHA COD-AB Sri Lanka administrative boundaries (ADM0–ADM4)".to_owned(),
                vintage: Some("COD-AB LKA (fetched 2026-08-12)".to_owned()),
                notes: Some(
                    "ADM3–ADM4 geometry from COD-AB; ADM0–ADM2 geometry from geoBoundaries, p-codes from COD-AB."
                        .to_owned(),
                ),
            }),
            vintage: None,
        })?;
        for u in units {
            sink.unit(u)?;
        }
        sink.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admin() -> AdminCodes {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
        let p = lanka_core::Profile::discover(&root).expect("profile");
        lanka_core::Profile::load(p).expect("loads").admin
    }

    #[test]
    fn parses_levels() {
        let a = admin();
        let adm1 = br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"pcode":"LK4","name_en":"Northern Province","name_si":"x","name_ta":null,"level":1,"parent_pcode":"LK"},"geometry":{"type":"Polygon","coordinates":[[[79.9,8.9],[80.0,8.9],[80.0,9.0],[79.9,8.9]]]}}]}"#;
        let u = parse_level(adm1, "adm1", 2, &a, "Country").expect("parses");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].pcode, "LK4");
        assert_eq!(u[0].parent_pcode.as_deref(), Some("LK"));
        assert!(u[0].name_ta.is_none());
        assert!(u[0].geometry.get().starts_with("{\"type\":\"Polygon\""));
        let adm0 = br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"Sri Lanka","level":0},"geometry":{"type":"Point","coordinates":[80,7]}}]}"#;
        let u0 = parse_level(adm0, "adm0", 0, &a, "Country").expect("parses");
        assert_eq!(
            (u0[0].pcode.as_str(), u0[0].parent_pcode.is_none()),
            ("LK", true)
        );
        assert!(
            parse_level(adm1, "adm2", 0, &a, "Country").is_err(),
            "level mismatch is rejected"
        );
    }
}
