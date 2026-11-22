//! The tippecanoe operation (05 §9: "export → tippecanoe → PMTiles").
//!
//! One narrow, typed entry point: named GeoJSON-sequence layers in, one
//! `.pmtiles` out, a fixed set of flags. Inputs and output live in one work
//! directory and are passed by file name (tippecanoe runs in it), so the
//! command line recorded in the PMTiles metadata — and therefore the file's
//! hash — does not depend on where the work directory was. Nothing user-supplied reaches the
//! command line except file paths this crate created. WP-08 moves this call
//! into the extraction sandbox (`lanka-extractd`) unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

/// One `--named-layer`.
#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    pub file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Op {
    pub layers: Vec<Layer>,
    pub output: PathBuf,
    pub minzoom: u8,
    pub maxzoom: u8,
    /// Thin dense point/line layers at low zooms instead of failing on
    /// tile size (`--drop-densest-as-needed --extend-zooms-if-still-dropping`).
    pub drop_densest: bool,
    pub name: String,
    pub attribution: Option<String>,
}

/// The tippecanoe binary: `LANKA_TIPPECANOE`, else `tippecanoe` on PATH.
#[must_use]
pub fn binary() -> PathBuf {
    std::env::var_os("LANKA_TIPPECANOE")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from("tippecanoe"), PathBuf::from)
}

/// `tippecanoe v2.79.0` (it prints its version on stderr).
pub fn version() -> Result<String> {
    let out = Command::new(binary()).arg("--version").output().context(
        "tippecanoe is not installed (brew install tippecanoe, or set LANKA_TIPPECANOE)",
    )?;
    let v = String::from_utf8_lossy(if out.stdout.is_empty() {
        &out.stderr
    } else {
        &out.stdout
    })
    .trim()
    .to_owned();
    ensure!(!v.is_empty(), "tippecanoe printed no version");
    Ok(v)
}

fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn file_name(p: &Path) -> Result<String> {
    let n = p
        .file_name()
        .and_then(|n| n.to_str())
        .context("file name")?;
    ensure!(safe_file(n), "file name {n:?}");
    Ok(n.to_owned())
}

fn safe_file(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !s.starts_with('.')
}

/// Builds the arguments (pure; tested).
pub fn args(op: &Op) -> Result<Vec<String>> {
    ensure!(!op.layers.is_empty(), "no layers");
    ensure!(op.minzoom <= op.maxzoom && op.maxzoom <= 16, "zooms");
    ensure!(
        op.output.extension().is_some_and(|e| e == "pmtiles"),
        "output must be .pmtiles"
    );
    let mut a = vec![
        "--force".to_owned(),
        "--quiet".to_owned(),
        "--projection=EPSG:4326".to_owned(),
        format!("--minimum-zoom={}", op.minzoom),
        format!("--maximum-zoom={}", op.maxzoom),
        format!("--name={}", op.name),
        "--no-tile-stats".to_owned(),
        "--detect-shared-borders".to_owned(),
        format!("--output={}", file_name(&op.output)?),
    ];
    if let Some(at) = &op.attribution {
        a.push(format!("--attribution={at}"));
    }
    if op.drop_densest {
        a.push("--drop-densest-as-needed".to_owned());
        a.push("--extend-zooms-if-still-dropping".to_owned());
    } else {
        a.push("--coalesce-densest-as-needed".to_owned());
    }
    for l in &op.layers {
        if !safe_name(&l.name) {
            bail!("layer name {:?} is not [A-Za-z0-9_-]{{1,64}}", l.name);
        }
        ensure!(
            l.file.parent() == op.output.parent(),
            "layer files must sit beside the output"
        );
        let named = serde_json::json!({ "file": file_name(&l.file)?, "layer": l.name });
        a.push(format!("--named-layer={named}"));
    }
    Ok(a)
}

/// Runs tippecanoe; returns its warnings (stderr lines).
pub fn run(op: &Op) -> Result<Vec<String>> {
    let a = args(op)?;
    let dir = op.output.parent().context("output directory")?;
    let out = Command::new(binary())
        .args(&a)
        .current_dir(dir)
        .output()
        .context("running tippecanoe")?;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        bail!("tippecanoe failed ({}):\n{stderr}", out.status);
    }
    ensure!(
        Path::new(&op.output).is_file(),
        "tippecanoe wrote no {}",
        op.output.display()
    );
    Ok(stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_fixed_and_layer_names_checked() {
        let mut op = Op {
            layers: vec![Layer {
                name: "adm1".into(),
                file: "/tmp/a.geojsonl".into(),
            }],
            output: "/tmp/admin.pmtiles".into(),
            minzoom: 0,
            maxzoom: 12,
            drop_densest: false,
            name: "admin".into(),
            attribution: Some("OCHA".into()),
        };
        let a = args(&op).expect("args");
        assert!(a.contains(&"--maximum-zoom=12".to_owned()));
        assert!(a.iter().any(|x| x.starts_with("--named-layer={")));
        op.layers[0].name = "bad name; rm".into();
        assert!(args(&op).is_err());
        op.layers[0].name = "ok".into();
        op.output = "/tmp/x.mbtiles".into();
        assert!(args(&op).is_err());
    }
}
