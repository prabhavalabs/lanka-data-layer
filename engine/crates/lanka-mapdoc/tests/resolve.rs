//! Resolution against a synthetic catalogue: version pinning (latest,
//! release, `{dsv}`), frame URLs, attribution, notices and problems.

use lanka_core::Catalog;
use lanka_core::catalog::{CurrentVersion, DatasetInfo, IndicatorInfo, LicenceInfo, SourceInfo};
use lanka_core::frame::PinnedVersion;
use lanka_mapdoc::resolve::{ResolveContext, Shape, resolve};
use lanka_mapdoc::validate;
use serde_json::json;

fn catalog() -> Catalog {
    let mut c = Catalog {
        data_version: Some("20261122.2".into()),
        ..Catalog::default()
    };
    let ds = |id: &str, dsv: &str, seq: i32, sources: &[&str], inds: &[&str]| DatasetInfo {
        id: id.into(),
        key: seq,
        kind: "indicators".into(),
        title: id.into(),
        description: String::new(),
        method: String::new(),
        limits: Vec::new(),
        publication_class: "A".into(),
        time_from: None,
        time_to: None,
        source_ids: sources.iter().map(|s| (*s).to_owned()).collect(),
        indicator_ids: inds.iter().map(|s| (*s).to_owned()).collect(),
        current: Some(CurrentVersion {
            version_id: 1,
            seq,
            dsv: dsv.into(),
            vintage: None,
            row_count: 1,
            published_at: None,
        }),
    };
    for d in [
        ds("pop", "dsv_00000000000000b2", 2, &["stat"], &["pop.total"]),
        ds(
            "est",
            "dsv_00000000000000e1",
            1,
            &["model"],
            &["est.density"],
        ),
    ] {
        c.datasets.insert(d.id.clone(), d);
    }
    let ind = |id: &str, ds: &str, class: &str| IndicatorInfo {
        id: id.into(),
        key: 1,
        dataset_id: ds.into(),
        label: id.into(),
        unit: None,
        unit_label: None,
        unit_mult: 0,
        dims: vec!["sex".into()],
        geo_levels: vec!["national".into(), "adm2".into(), "adm3".into()],
        period_type: "year".into(),
        freq: "A".into(),
        measure_type: "count".into(),
        aggregation: "sum".into(),
        class: class.into(),
        completes: None,
    };
    c.indicators
        .insert("pop.total".into(), ind("pop.total", "pop", "official"));
    c.indicators
        .insert("est.density".into(), ind("est.density", "est", "estimated"));
    let src = |id: &str, lic: &str| SourceInfo {
        id: id.into(),
        publisher: format!("{id} office"),
        title: format!("{id} data"),
        homepage: "https://example.org".into(),
        licence_id: lic.into(),
        attribution: format!("{id} office"),
        cadence: "annual".into(),
        access_method: "api".into(),
        themes: Vec::new(),
        status: "active".into(),
    };
    c.sources.insert("stat".into(), src("stat", "unclear"));
    c.sources.insert("model".into(), src("model", "cc-by-4.0"));
    let lic = |id: &str, status: &str, spdx: Option<&str>| LicenceInfo {
        id: id.into(),
        status: status.into(),
        url: None,
        spdx: spdx.map(str::to_owned),
        redistribution: "bulk".into(),
    };
    c.licences
        .insert("unclear".into(), lic("unclear", "permission_unclear", None));
    c.licences.insert(
        "cc-by-4.0".into(),
        lic("cc-by-4.0", "attribution", Some("CC-BY-4.0")),
    );
    c
}

fn doc(pin: serde_json::Value) -> lanka_mapdoc::MapDocument {
    let v = json!({
        "spec": "ldl-map/1", "title": "t", "pin": pin,
        "view": {"center": [80, 7], "zoom": 7}, "basemap": {"style": "ldl-auto"},
        "time": {"cursor": "2020", "range": ["2012", "2024"], "freq": "A"},
        "sources": {
            "a": {"kind": "indicator", "indicator": "pop.total", "level": 2, "period": {"bind": "time"}, "dims": {"sex": "total"}},
            "b": {"kind": "indicator", "indicator": "est.density", "level": {"byZoom": [[0, 2], [9, 3]]}, "period": "latest", "version": {"dsv": "dsv_00000000000000e1"}},
            "c": {"kind": "indicator", "indicator": "pop.total", "level": 4, "period": "2024"},
            "d": {"kind": "indicator", "indicator": "@acme/re.price", "level": 2, "period": "2024"},
            "e": {"kind": "external", "type": "geojson", "url": "https://tiles.example.net/x.geojson", "attribution": "© Example"},
            "f": {"kind": "grid", "grid": "haz.flood_depth", "period": "latest"}
        },
        "layers": []
    });
    validate(&v).unwrap_or_else(|p| panic!("{p:#?}"))
}

fn pinned(dsv: &str, seq: i32, ds: &str) -> PinnedVersion {
    PinnedVersion {
        dataset_id: ds.into(),
        seq,
        dsv: dsv.into(),
        vintage: None,
        status: "published".into(),
    }
}

#[test]
fn latest_document() {
    let cat = catalog();
    let ctx = ResolveContext {
        dsvs: vec![pinned("dsv_00000000000000e1", 1, "est")],
        ..ResolveContext::default()
    };
    let r = resolve(&doc(json!("latest")), &cat, &ctx);
    assert_eq!(r.data_version.as_deref(), Some("20261122.2"));
    let a = &r.sources["a"];
    assert_eq!(a.dsv.as_deref(), Some("dsv_00000000000000b2"));
    match &a.shape {
        Shape::Frame {
            frame,
            source_layer,
            ..
        } => {
            assert_eq!(
                frame,
                "/v2/frame?indicators=pop.total&level=2&from=2012&to=2024&dims=sex%3Dtotal"
            );
            assert_eq!(source_layer, "adm2");
        }
        s => panic!("{s:?}"),
    }
    match &r.sources["b"].shape {
        Shape::Frame {
            frame,
            by_zoom: Some(z),
            ..
        } => {
            assert!(
                frame.ends_with("level=2&dsv=dsv_00000000000000e1"),
                "{frame}"
            );
            assert_eq!(z.len(), 2);
            assert_eq!((z[1].level, z[1].source_layer.as_str()), (3, "adm3"));
        }
        s => panic!("{s:?}"),
    }
    assert_eq!(r.sources["b"].class.as_deref(), Some("estimated"));
    assert_eq!(r.sources["c"].shape, Shape::Unavailable);
    assert_eq!(r.sources["d"].access, "denied");
    assert!(matches!(r.sources["e"].shape, Shape::External { .. }));
    assert_eq!(r.sources["f"].shape, Shape::Unavailable);
    let codes: Vec<(&str, &str)> = r
        .problems
        .iter()
        .map(|p| (p.path.as_str(), p.code))
        .collect();
    assert!(
        codes.contains(&("/sources/c/level", "LEVEL_UNAVAILABLE")),
        "{codes:?}"
    );
    assert!(
        codes.contains(&("/sources/f/grid", "NOT_FOUND")),
        "{codes:?}"
    );
    let notices: Vec<&str> = r.notices.iter().map(|n| n.code).collect();
    for n in ["ESTIMATE", "EXTERNAL", "LICENCE_UNCLEAR", "PRIVATE_DATA"] {
        assert!(notices.contains(&n), "{n} in {notices:?}");
    }
    let attr: Vec<&str> = r.attribution.iter().map(|a| a.source_id.as_str()).collect();
    assert_eq!(attr, ["model", "stat", "external:tiles.example.net"]);
}

#[test]
fn release_pinned_document() {
    let cat = catalog();
    let ctx = ResolveContext {
        release: Some((
            "20261101.1".into(),
            vec![pinned("dsv_00000000000000b1", 1, "pop")],
        )),
        dsvs: vec![pinned("dsv_00000000000000e1", 1, "est")],
        release_missing: false,
        ..ResolveContext::default()
    };
    let r = resolve(&doc(json!({"release": "20261101.1"})), &cat, &ctx);
    assert_eq!(r.data_version.as_deref(), Some("20261101.1"));
    assert_eq!(r.sources["a"].dsv.as_deref(), Some("dsv_00000000000000b1"));
    match &r.sources["a"].shape {
        Shape::Frame { frame, .. } => {
            assert!(frame.starts_with("/v2/r/20261101.1/frame?"), "{frame}")
        }
        s => panic!("{s:?}"),
    }
    // est is not in the release but is pinned by dsv: the current-release route with dsv=.
    match &r.sources["b"].shape {
        Shape::Frame { frame, .. } => assert!(
            frame.starts_with("/v2/frame?") && frame.contains("dsv=dsv_00000000000000e1"),
            "{frame}"
        ),
        s => panic!("{s:?}"),
    }
    // An unknown release falls back to latest with a problem at /pin.
    let ctx = ResolveContext {
        release_missing: true,
        ..ResolveContext::default()
    };
    let r = resolve(&doc(json!({"release": "19990101.1"})), &cat, &ctx);
    assert!(
        r.problems
            .iter()
            .any(|p| p.path == "/pin" && p.code == "NOT_FOUND")
    );
    assert!(
        r.problems
            .iter()
            .any(|p| p.path == "/sources/b/version" && p.code == "NOT_FOUND"),
        "dsv not loaded"
    );
}
