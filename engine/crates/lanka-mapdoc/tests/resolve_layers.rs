//! WP-27: grid and features sources resolve to value tiles, columnar
//! points, vector tiles, GeoJSON or the dem tileset by the size rules of
//! 10 §3.3.

use lanka_core::Catalog;
use lanka_core::catalog::{CurrentVersion, DatasetInfo, IndicatorInfo};
use lanka_mapdoc::resolve::{
    FeatureCount, FieldInfo, GridLayerRef, ResolveContext, Shape, TilesetRef, feature_shape,
    resolve,
};
use lanka_mapdoc::validate;
use serde_json::json;

fn ds(id: &str, kind: &str, key: i32) -> DatasetInfo {
    DatasetInfo {
        id: id.into(),
        key,
        kind: kind.into(),
        title: id.into(),
        description: String::new(),
        method: String::new(),
        limits: Vec::new(),
        publication_class: "A".into(),
        time_from: None,
        time_to: None,
        source_ids: Vec::new(),
        indicator_ids: Vec::new(),
        current: Some(CurrentVersion {
            version_id: i64::from(key),
            seq: 1,
            dsv: format!("dsv_{key:016x}"),
            vintage: None,
            row_count: 1,
            published_at: None,
        }),
    }
}

fn catalog() -> Catalog {
    let mut c = Catalog {
        data_version: Some("20261122.3".into()),
        ..Catalog::default()
    };
    for d in [
        ds("pois", "features", 1),
        ds("roads", "features", 2),
        ds("jrc-river-flood-hazard", "indicators", 3),
        ds("copernicus-dem-terrain", "indicators", 4),
        ds("big-points", "features", 5),
        ds("small", "features", 6),
    ] {
        c.datasets.insert(d.id.clone(), d);
    }
    c.indicators.insert(
        "haz.flood_depth".into(),
        IndicatorInfo {
            id: "haz.flood_depth".into(),
            key: 1,
            dataset_id: "jrc-river-flood-hazard".into(),
            label: "Flood depth".into(),
            unit: Some("m".into()),
            unit_label: None,
            unit_mult: 0,
            dims: vec!["return_period".into()],
            geo_levels: vec!["raster".into()],
            period_type: "snapshot".into(),
            freq: "A".into(),
            measure_type: "mean".into(),
            aggregation: "none".into(),
            class: "third_party".into(),
            completes: None,
        },
    );
    c
}

fn grid(id: &str, rp: &str, dtype: &str, scale: f64) -> GridLayerRef {
    GridLayerRef {
        id: id.into(),
        period: "2024".into(),
        dataset_id: "jrc-river-flood-hazard".into(),
        indicator_id: Some("haz.flood_depth".into()),
        dims: format!("return_period={rp}"),
        sha16: "e3bd9c4226b91bc4".into(),
        dtype: dtype.into(),
        scale,
        offset: 0.0,
        nodata: Some(65_535.0),
        unit: "m".into(),
        overview: "max".into(),
        min: Some(0.1),
        max: Some(25.5),
        count: Some(455_225.0),
        sum: Some(997_340.9),
        classes: Vec::new(),
    }
}

fn ctx() -> ResolveContext {
    let mut c = ResolveContext {
        grids: vec![
            grid("haz.flood_depth.rp100", "rp100", "u16", 0.01),
            grid("haz.flood_depth.rp10", "rp10", "u16", 0.01),
        ],
        tilesets: vec![
            TilesetRef {
                id: "admin".into(),
                sha16: "2cf699ee9b7c2ac5".into(),
                minzoom: 0,
                maxzoom: 12,
                layers: vec![("adm4".into(), "cod-ab-lka-boundaries".into())],
                layer_minzoom: Default::default(),
            },
            TilesetRef {
                id: "osm-road-network".into(),
                sha16: "0000000000000001".into(),
                minzoom: 5,
                maxzoom: 14,
                layers: vec![("roads".into(), "roads".into())],
                layer_minzoom: Default::default(),
            },
            TilesetRef {
                id: "dem".into(),
                sha16: "0000000000000002".into(),
                minzoom: 5,
                maxzoom: 11,
                layers: vec![("dem".into(), "copernicus-dem-terrain".into())],
                layer_minzoom: Default::default(),
            },
            TilesetRef {
                id: "big-points".into(),
                sha16: "0000000000000003".into(),
                minzoom: 0,
                maxzoom: 14,
                layers: vec![("big-points".into(), "big-points".into())],
                layer_minzoom: Default::default(),
            },
        ],
        ..ResolveContext::default()
    };
    for (sid, n, pts) in [
        ("hospitals", 1_233, true),
        ("schools", 24_888, true),
        ("roads", 237_699, false),
        ("huge", 300_000, true),
        ("lines_no_tiles", 5_000, false),
    ] {
        c.features.insert(
            sid.into(),
            FeatureCount {
                count: n,
                points_only: pts,
            },
        );
    }
    c.fields.insert(
        "pois".into(),
        vec![FieldInfo {
            id: "operator_type".into(),
            label: "operator type".into(),
            ty: "category".into(),
            unit: None,
            measure_type: None,
        }],
    );
    c
}

fn doc() -> lanka_mapdoc::MapDocument {
    let v = json!({
        "spec": "ldl-map/1", "title": "t", "pin": "latest",
        "view": {"center": [80, 7], "zoom": 7}, "basemap": {"style": "ldl-auto"},
        "sources": {
            "flood": {"kind": "grid", "grid": "haz.flood_depth", "dims": {"return_period": "RP100"}, "period": "latest"},
            "flood10": {"kind": "grid", "grid": "haz.flood_depth.rp10", "period": "2024"},
            "flood_bad": {"kind": "grid", "grid": "haz.flood_depth", "dims": {"return_period": "RP75"}, "period": "latest"},
            "dem": {"kind": "grid", "grid": "dem", "period": "latest"},
            "hospitals": {"kind": "features", "dataset": "pois", "featureKind": "hospital"},
            "schools": {"kind": "features", "dataset": "pois", "fields": ["operator_type"]},
            "roads": {"kind": "features", "dataset": "roads"},
            "huge": {"kind": "features", "dataset": "big-points"},
            "lines_no_tiles": {"kind": "features", "dataset": "small"}
        },
        "layers": []
    });
    validate(&v).unwrap_or_else(|p| panic!("{p:#?}"))
}

#[test]
fn size_rules() {
    assert_eq!(feature_shape(0, false, false), Some("geojson"));
    assert_eq!(feature_shape(2_000, false, false), Some("geojson"));
    assert_eq!(feature_shape(2_001, true, false), Some("points"));
    assert_eq!(feature_shape(250_000, true, true), Some("points"));
    assert_eq!(feature_shape(250_001, true, true), Some("tiles"));
    assert_eq!(feature_shape(2_001, false, true), Some("tiles"));
    assert_eq!(feature_shape(2_001, false, false), None);
    assert_eq!(feature_shape(250_001, true, false), None);
}

#[test]
fn layers_resolve_to_their_shapes() {
    let r = resolve(&doc(), &catalog(), &ctx());
    let s = &r.sources;
    match &s["flood"].shape {
        Shape::Values {
            values,
            scale,
            nodata,
            max_error,
            layer,
            reducer,
            ..
        } => {
            assert_eq!(layer, "haz.flood_depth.rp100");
            assert!(values.starts_with(
                "/v2/grids/haz.flood_depth.rp100/values/{z}/{x}/{y}.png?period=2024&v=e3bd"
            ));
            assert!((scale - 0.01).abs() < 1e-12);
            assert_eq!(*nodata, 65_535);
            assert_eq!(*max_error, 0.0);
            assert_eq!(reducer, "max");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s["flood"].dsv.as_deref(), Some("dsv_0000000000000003"));
    assert!(
        matches!(&s["flood10"].shape, Shape::Values { layer, .. } if layer == "haz.flood_depth.rp10")
    );
    assert_eq!(s["flood_bad"].shape, Shape::Unavailable);
    assert!(
        matches!(&s["dem"].shape, Shape::Dem { encoding, maxzoom: 11, .. } if encoding == "terrarium")
    );
    assert!(
        matches!(&s["hospitals"].shape, Shape::Geojson { url, count: 1_233, .. } if url == "/v2/features/pois?kind=hospital&limit=2000&format=geojson")
    );
    match &s["schools"].shape {
        Shape::Points {
            points,
            count,
            fields,
        } => {
            assert_eq!(*count, 24_888);
            assert_eq!(
                points,
                "/v2/features/pois/points?fields=kind,name,operator_type&format=arrow"
            );
            assert_eq!(fields.len(), 1);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(&s["roads"].shape, Shape::Tiles { tiles, source_layers, maxzoom: 14, .. }
            if tiles == "/v2/tiles/osm-road-network/{z}/{x}/{y}.mvt" && source_layers == &vec!["roads".to_owned()])
    );
    assert!(matches!(
        &s["huge"].shape,
        Shape::Tiles { count: 300_000, .. }
    ));
    assert_eq!(s["lines_no_tiles"].shape, Shape::Unavailable);
    let codes: Vec<(&str, &str)> = r
        .problems
        .iter()
        .map(|p| (p.path.as_str(), p.code))
        .collect();
    assert!(
        codes.contains(&("/sources/flood_bad/grid", "NOT_FOUND")),
        "{codes:?}"
    );
    assert!(
        codes.contains(&("/sources/lines_no_tiles", "NOT_SERVED")),
        "{codes:?}"
    );
    assert_eq!(codes.len(), 2, "{codes:?}");
    // the serialised shape is what ui-contract §4 names
    let v = serde_json::to_value(&s["schools"]).expect("json");
    assert_eq!(v["shape"], "points");
    let v = serde_json::to_value(&s["flood"]).expect("json");
    assert_eq!(v["shape"], "values");
    assert_eq!(v["encoding"], "ldl-v16");
}
