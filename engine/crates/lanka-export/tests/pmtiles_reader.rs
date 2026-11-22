//! The PMTiles reader against tippecanoe output and the reference `pmtiles`
//! CLI (go-pmtiles): every tile of a small tileset, byte for byte.
//!
//! Gated on the tools being installed (`tippecanoe`, `pmtiles`); a real
//! archive can be checked too with `LANKA_TEST_PMTILES=/path/x.pmtiles`.

use std::path::Path;
use std::process::Command;

use lanka_export::pmtiles::PmTiles;
use lanka_export::tippecanoe::{self, Layer, Op};

fn have(bin: &str) -> bool {
    Command::new(bin).arg("--help").output().is_ok()
}

fn cli_tile(path: &Path, z: u8, x: u64, y: u64) -> Option<Vec<u8>> {
    let out = Command::new("pmtiles")
        .args([
            "tile",
            "-q",
            &path.to_string_lossy(),
            &z.to_string(),
            &x.to_string(),
            &y.to_string(),
        ])
        .output()
        .expect("pmtiles");
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

/// Compares every tile in the archive's bounds up to `max_z` (and a sample
/// above it) with the CLI.
fn compare(path: &Path, max_z: u8) -> usize {
    let pm = PmTiles::open(path).expect("open");
    let h = pm.header.clone();
    let mut checked = 0;
    for z in h.min_zoom..=h.max_zoom.min(max_z) {
        let n = 1u64 << z;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let col = |lon: f64| (((lon + 180.0) / 360.0 * n as f64).floor() as u64).min(n - 1);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let row = |lat: f64| {
            let r = lat.to_radians();
            (((1.0 - r.tan().asinh() / std::f64::consts::PI) / 2.0 * n as f64).floor() as u64)
                .min(n - 1)
        };
        let (x0, x1) = (col(h.bounds[0]), col(h.bounds[2]));
        let (y0, y1) = (row(h.bounds[3]), row(h.bounds[1]));
        for x in x0..=x1 {
            for y in y0..=y1 {
                let ours = pm.tile(z, x, y).expect("tile");
                let theirs = cli_tile(path, z, x, y);
                assert_eq!(ours, theirs, "tile {z}/{x}/{y}");
                checked += 1;
            }
        }
    }
    // Outside the world / zoom range.
    assert_eq!(pm.tile(h.max_zoom + 1, 0, 0).expect("tile"), None);
    assert_eq!(pm.tile(3, 100, 0).expect("tile"), None);
    checked
}

#[test]
fn reader_matches_go_pmtiles() {
    if !have("pmtiles") || !have("tippecanoe") {
        eprintln!("skipping: pmtiles / tippecanoe not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("lanka-pmtiles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    // A grid of points and lines over Sri Lanka.
    let mut s = String::new();
    for i in 0..400 {
        let lon = 79.7 + f64::from(i % 20) * 0.11;
        let lat = 6.0 + f64::from(i / 20) * 0.19;
        s.push_str(&format!(
            "{{\"type\":\"Feature\",\"properties\":{{\"key\":\"node/{i}\",\"kind\":\"k{}\"}},\"geometry\":{{\"type\":\"Point\",\"coordinates\":[{lon},{lat}]}}}}\n",
            i % 3
        ));
        s.push_str(&format!(
            "{{\"type\":\"Feature\",\"properties\":{{\"key\":\"way/{i}\"}},\"geometry\":{{\"type\":\"LineString\",\"coordinates\":[[{lon},{lat}],[{},{}]]}}}}\n",
            lon + 0.05,
            lat + 0.07
        ));
    }
    let input = dir.join("pts.geojsonl");
    std::fs::write(&input, s).expect("write");
    let out = dir.join("t.pmtiles");
    let op = Op {
        layers: vec![Layer {
            name: "pts".into(),
            file: input,
        }],
        output: out.clone(),
        minzoom: 0,
        maxzoom: 9,
        drop_densest: false,
        name: "t".into(),
        attribution: None,
    };
    tippecanoe::run(&op).expect("tippecanoe");
    let a = std::fs::read(&out).expect("read");
    // Same inputs, another directory → identical bytes (no paths inside).
    let dir2 = dir.join("again");
    std::fs::create_dir_all(&dir2).expect("dir");
    std::fs::copy(dir.join("pts.geojsonl"), dir2.join("pts.geojsonl")).expect("copy");
    let op2 = Op {
        layers: vec![Layer {
            name: "pts".into(),
            file: dir2.join("pts.geojsonl"),
        }],
        output: dir2.join("t.pmtiles"),
        ..op
    };
    tippecanoe::run(&op2).expect("tippecanoe");
    let b = std::fs::read(dir2.join("t.pmtiles")).expect("read");
    if a != b {
        let ma = PmTiles::open(&out).expect("a").metadata().expect("ma");
        let mb = PmTiles::open(&dir2.join("t.pmtiles"))
            .expect("b")
            .metadata()
            .expect("mb");
        eprintln!("A {ma}\nB {mb}");
    }
    assert_eq!(a, b, "deterministic build");

    let pm = PmTiles::open(&out).expect("open");
    let meta = pm.metadata().expect("metadata");
    assert_eq!(meta["vector_layers"][0]["id"], "pts");
    let n = compare(&out, 9);
    assert!(n > 20, "{n} tiles checked");
    if let Ok(p) = std::env::var("LANKA_TEST_PMTILES") {
        let n = compare(Path::new(&p), 10);
        eprintln!("{p}: {n} tiles identical to go-pmtiles");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
