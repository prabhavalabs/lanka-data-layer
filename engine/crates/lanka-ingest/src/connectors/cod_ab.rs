//! OCHA COD-AB Sri Lanka administrative units, ADM0–ADM4.
//!
//! This first slice reads the v1 foundry's per-level layer exports
//! (`layers/country.geojson`, `layers/admin-adm{1..4}.geojson`: COD-AB
//! p-codes, trilingual names, parent p-codes) from `--from <dir>`. The files
//! are archived and hashed like any download, so provenance is identical
//! when the direct HDX connector replaces the local read.

use anyhow::{Context, Result, bail, ensure};
use futures::FutureExt;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::value::RawValue;

use lanka_core::AdminCodes;

use crate::connector::{AdminUnitIn, BoundarySet, Candidate, Connector, Ctx, Fetched, Output};

pub struct CodAb;

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
    features: Vec<Feature<'a>>,
}

#[derive(Deserialize)]
struct Feature<'a> {
    properties: Props,
    #[serde(borrow)]
    geometry: &'a RawValue,
}

#[derive(Deserialize)]
struct Props {
    pcode: Option<String>,
    name_en: Option<String>,
    #[serde(alias = "name")]
    name: Option<String>,
    name_si: Option<String>,
    name_ta: Option<String>,
    level: Option<i16>,
    parent_pcode: Option<String>,
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Parses one level file into units. Codes are checked against the country
/// profile's admin rules; the level-0 unit takes the profile's root code and
/// `country_name` when the file carries none.
pub fn parse_level(
    bytes: &[u8],
    key: &str,
    artifact: usize,
    admin: &AdminCodes,
    country_name: &str,
) -> Result<Vec<AdminUnitIn>> {
    let fc: Fc<'_> = serde_json::from_slice(bytes)
        .with_context(|| format!("{key}: not a GeoJSON FeatureCollection"))?;
    let expected: i16 = key[3..].parse()?;
    let mut out = Vec::with_capacity(fc.features.len());
    for f in fc.features {
        let p = f.properties;
        let level = p.level.unwrap_or(expected);
        ensure!(level == expected, "{key}: feature at level {level}");
        let (pcode, name_en, parent) = if level == 0 {
            (
                admin.root().to_owned(),
                clean(p.name_en)
                    .or(clean(p.name))
                    .unwrap_or_else(|| country_name.to_owned()),
                None,
            )
        } else {
            let pcode = clean(p.pcode).with_context(|| format!("{key}: feature without pcode"))?;
            // 65 COD-AB GN divisions carry no name in any script; the p-code
            // stands in as the label (reported as a warning by `parse`).
            let name = clean(p.name_en)
                .or(clean(p.name))
                .unwrap_or_else(|| pcode.clone());
            let parent = clean(p.parent_pcode)
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
            name_si: clean(p.name_si),
            name_ta: clean(p.name_ta),
            parent_pcode: parent,
            geometry: f.geometry.to_owned(),
            artifact,
        });
    }
    Ok(out)
}

impl Connector for CodAb {
    fn source_id(&self) -> &'static str {
        SOURCE
    }

    fn discover<'a>(&'a self, ctx: &'a Ctx<'a>) -> BoxFuture<'a, Result<Vec<Candidate>>> {
        async move {
            let Some(dir) = &ctx.opts.from_dir else {
                bail!(
                    "hdx-cod-ab-lka reads local layer exports for now: pass --from <dir> containing {}",
                    FILES.iter().map(|f| f.1).collect::<Vec<_>>().join(", ")
                );
            };
            Ok(FILES
                .iter()
                .map(|(k, f)| Candidate {
                    key: (*k).to_owned(),
                    url: format!("https://data.humdata.org/dataset/cod-ab-lka#{f}"),
                    local_path: Some(dir.join(f)),
                    media_type: Some("application/geo+json".to_owned()),
                    edition: Some("COD-AB LKA via v1 foundry layers".to_owned()),
                })
                .collect())
        }
        .boxed()
    }

    fn parse<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        fetched: &'a [Fetched],
    ) -> BoxFuture<'a, Result<Vec<Output>>> {
        async move {
            let mut units = Vec::new();
            for (i, f) in fetched.iter().enumerate() {
                units.extend(parse_level(
                    &f.bytes,
                    &f.key,
                    i,
                    &ctx.profile.admin,
                    &ctx.profile.spec.name.en,
                )?);
            }
            let unnamed: Vec<&str> =
                units.iter().filter(|u| u.name_en == u.pcode).map(|u| u.pcode.as_str()).collect();
            if !unnamed.is_empty() {
                ctx.run
                    .warn(
                        crate::run::Stage::Parse,
                        &format!("{} unit(s) have no name upstream; p-code used as label", unnamed.len()),
                        Some(serde_json::json!(unnamed)),
                    )
                    .await;
            }
            Ok(vec![Output::AdminUnits {
                dataset_id: DATASET.to_owned(),
                boundary_set: BoundarySet {
                    id: ctx.profile.reference_boundary_set().to_owned(),
                    title: "OCHA COD-AB Sri Lanka administrative boundaries (ADM0–ADM4)".to_owned(),
                    vintage: Some("COD-AB LKA (fetched 2026-08-12)".to_owned()),
                    notes: Some(
                        "ADM3–ADM4 geometry from COD-AB; ADM0–ADM2 geometry from geoBoundaries, p-codes from COD-AB."
                            .to_owned(),
                    ),
                },
                units,
            }])
        }
        .boxed()
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
