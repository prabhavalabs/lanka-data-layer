//! WP-26 acceptance for the document: the 10 §7 example and the web-v2
//! starter maps validate; negative fixtures fail with a JSON-pointer path
//! and a stable code; `x-*` keys survive a round trip; the embedded schema
//! and ramps are the published files.

use std::path::{Path, PathBuf};

use lanka_mapdoc::canon::{jcs, snapshot_id};
use lanka_mapdoc::{validate, validate_str};
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load(rel: &str) -> Value {
    let text =
        std::fs::read_to_string(fixtures().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn example() -> Value {
    load("valid/flood-prices.json")
}

#[test]
fn spec_files_are_the_embedded_copies() {
    let spec = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../spec/map/1");
    for f in ["map.schema.json", "ramps.json"] {
        let Ok(published) = std::fs::read_to_string(spec.join(f)) else {
            eprintln!("skipping: spec/map/1/{f} not present (engine-only checkout)");
            continue;
        };
        let embedded =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("schema").join(f))
                .unwrap_or_default();
        assert!(
            published == embedded,
            "spec/map/1/{f} and crates/lanka-mapdoc/schema/{f} differ: copy the spec file over"
        );
    }
}

#[test]
fn the_worked_example_validates() {
    let doc = validate(&example()).unwrap_or_else(|p| panic!("{p:#?}"));
    assert_eq!(doc.layers_flat().len(), 6);
    assert_eq!(doc.sources.len(), 8);
}

#[test]
fn web_starter_maps_validate() {
    let mut n = 0;
    for e in std::fs::read_dir(fixtures().join("web"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        let v = load(&format!("web/{name}"));
        if let Err(p) = validate(&v) {
            panic!("{name}: {p:#?}");
        }
        n += 1;
    }
    assert!(n >= 4, "web fixtures present");
}

#[test]
fn x_keys_survive_a_round_trip() {
    let mut v = example();
    v["x-editor"] = json!({"grid": true, "zoomHistory": [9.2, 10]});
    v["basemap"]["x-coast"] = json!(true);
    v["sources"]["flood"]["x-cache"] = json!("warm");
    v["layers"][0]["x-notes"] = json!("multiply over the choropleth");
    v["layers"][0]["style"]["x-relief"] = json!(true);
    v["layers"][1]["children"][0]["style"]["stroke"]["x-casing"] = json!(true);
    v["layers"][1]["children"][0]["style"]["fill"]["bivariate"]["x"]["scale"]["x-sample"] =
        json!(5000);
    v["layers"][1]["x-group-colour"] = json!("#ff0000");
    v["panels"][1]["x-pinned"] = json!(1);
    v["meta"]["x-origin"] = json!("test");
    v["layers"][3]["type"] = json!("x-flow");
    let doc = validate(&v).unwrap_or_else(|p| panic!("{p:#?}"));
    let back = serde_json::to_value(&doc).unwrap_or_default();
    // `null` and absent are the same; the serialiser omits null members
    // (except `extent` and `time`, which it always writes).
    let mut want = v.clone();
    strip_nulls(&mut want, true);
    assert_eq!(
        jcs(&back),
        jcs(&want),
        "parse → serialise is the identity on canonical JSON"
    );
    assert_eq!(snapshot_id(&back), snapshot_id(&v));
    for ptr in [
        "/x-editor/zoomHistory/1",
        "/basemap/x-coast",
        "/sources/flood/x-cache",
        "/layers/0/x-notes",
        "/layers/0/style/x-relief",
        "/layers/1/children/0/style/stroke/x-casing",
        "/layers/1/children/0/style/fill/bivariate/x/scale/x-sample",
        "/layers/1/x-group-colour",
        "/panels/1/x-pinned",
        "/meta/x-origin",
    ] {
        assert!(back.pointer(ptr).is_some(), "{ptr} lost");
    }
    // A second pass is stable.
    let again = validate(&back)
        .map(|d| serde_json::to_value(d).unwrap_or_default())
        .unwrap_or_default();
    assert_eq!(jcs(&again), jcs(&back));
}

fn strip_nulls(v: &mut Value, top: bool) {
    match v {
        Value::Object(m) => {
            m.retain(|k, x| !x.is_null() || (top && (k == "extent" || k == "time")));
            m.values_mut().for_each(|x| strip_nulls(x, false));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| strip_nulls(x, false)),
        _ => {}
    }
}

/// Sets (`Some`) or removes (`None`) the member at a pointer.
fn patch(v: &mut Value, ptr: &str, new: Option<Value>) {
    let (parent, last) = ptr.rsplit_once('/').unwrap_or(("", ptr));
    let target = if parent.is_empty() {
        Some(&mut *v)
    } else {
        v.pointer_mut(parent)
    };
    let Some(t) = target else {
        panic!("no parent for {ptr}")
    };
    match (t, new) {
        (Value::Object(m), Some(x)) => {
            m.insert(last.replace("~1", "/").replace("~0", "~"), x);
        }
        (Value::Object(m), None) => {
            m.remove(last);
        }
        (Value::Array(a), Some(x)) => {
            if let Ok(i) = last.parse::<usize>() {
                if i < a.len() { a[i] = x } else { a.push(x) }
            }
        }
        (Value::Array(a), None) => {
            if let Ok(i) = last.parse::<usize>() {
                a.remove(i);
            }
        }
        _ => panic!("cannot patch {ptr}"),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn negative_fixtures_fail_with_a_pointer() {
    let layer = |i: usize| json!({"id": format!("l_extra{i}"), "type": "line", "name": "x", "source": "dsd", "visible": true, "opacity": 1, "style": {}});
    let many_layers: Vec<Value> = (0..33).map(layer).collect();
    let deep = json!({"id": "g1", "type": "group", "name": "a", "visible": true, "opacity": 1, "children": [
        {"id": "g2", "type": "group", "name": "b", "visible": true, "opacity": 1, "children": [
            {"id": "g3", "type": "group", "name": "c", "visible": true, "opacity": 1, "children": [
                {"id": "g4", "type": "group", "name": "d", "visible": true, "opacity": 1, "children": []}]}]}]});
    let groups: Vec<Value> = (0..9).map(|i| json!({"id": format!("g{i}"), "type": "group", "name": "g", "visible": true, "opacity": 1, "children": []})).collect();
    let sources: serde_json::Map<String, Value> = (0..17)
        .map(|i| {
            (
                format!("s{i}"),
                json!({"kind": "boundaries", "scheme": "admin", "level": 1}),
            )
        })
        .collect();
    let big = "x".repeat(3000);
    let huge_layers: Vec<Value> = (0..30).map(|i| json!({"id": format!("l_big{i}"), "type": "line", "name": big.repeat(0) + &"n".repeat(200), "source": "dsd", "visible": true, "opacity": 1, "style": {"x-pad": "y".repeat(9000)}})).collect();
    let inline = json!({"kind": "inline", "geojson": {"type": "FeatureCollection", "features": vec![json!({"type": "Feature", "properties": {"note": "z".repeat(1000)}, "geometry": null}); 110]}});

    // (description, pointer to change, new value (None = remove), expected path, expected code)
    let cases: Vec<(&str, &str, Option<Value>, &str, &str)> = vec![
        (
            "unknown major",
            "/spec",
            Some(json!("ldl-map/2")),
            "/spec",
            "UNSUPPORTED_SPEC",
        ),
        ("missing title", "/title", None, "/title", "SCHEMA"),
        (
            "unknown top-level key",
            "/colour",
            Some(json!("red")),
            "/colour",
            "SCHEMA",
        ),
        (
            "bad pin",
            "/pin",
            Some(json!({"release": "yesterday"})),
            "/pin",
            "SCHEMA",
        ),
        (
            "zoom out of range",
            "/view/zoom",
            Some(json!(30)),
            "/view/zoom",
            "SCHEMA",
        ),
        (
            "bad source kind",
            "/sources/dsd/kind",
            Some(json!("tiles")),
            "/sources/dsd/kind",
            "SCHEMA",
        ),
        (
            "indicator source without level",
            "/sources/exposure/level",
            None,
            "/sources/exposure/level",
            "SCHEMA",
        ),
        (
            "unknown member of a source",
            "/sources/flood/url",
            Some(json!("https://x")),
            "/sources/flood/url",
            "SCHEMA",
        ),
        (
            "bad opacity",
            "/layers/0/opacity",
            Some(json!(1.5)),
            "/layers/0/opacity",
            "SCHEMA",
        ),
        (
            "bad blend",
            "/layers/0/blend",
            Some(json!("soft-light")),
            "/layers/0/blend",
            "SCHEMA",
        ),
        (
            "bad colour",
            "/layers/3/style/stroke/color",
            Some(json!("red")),
            "/layers/3/style/stroke/color",
            "SCHEMA",
        ),
        (
            "classes out of range",
            "/layers/1/children/0/style/fill/bivariate/x/scale/classes",
            Some(json!(12)),
            "/layers/1/children/0/style/fill/bivariate/x/scale/classes",
            "SCHEMA",
        ),
        (
            "manual scale without breaks",
            "/layers/0/style/fill/scale/breaks",
            None,
            "/layers/0/style/fill/scale/breaks",
            "SCHEMA",
        ),
        (
            "layer on a missing source",
            "/layers/2/source",
            Some(json!("nope")),
            "/layers/2/source",
            "UNKNOWN_SOURCE",
        ),
        (
            "raster on boundaries",
            "/layers/3/type",
            Some(json!("raster")),
            "/layers/3/type",
            "INCOMPATIBLE_SOURCE",
        ),
        (
            "duplicate layer id",
            "/layers/4/id",
            Some(json!("l_flood")),
            "/layers/4/id",
            "DUPLICATE_ID",
        ),
        (
            "analysis source with no analysis",
            "/sources/rel_biv/ref",
            Some(json!("gone")),
            "/sources/rel_biv/ref",
            "UNKNOWN_ANALYSIS",
        ),
        (
            "analysis variable on a missing source",
            "/analyses/rel/variables/x/source",
            Some(json!("nowhere")),
            "/analyses/rel/variables/x/source",
            "UNKNOWN_SOURCE",
        ),
        (
            "panel on a missing analysis",
            "/panels/2/analysis",
            Some(json!("nope")),
            "/panels/2/analysis",
            "UNKNOWN_ANALYSIS",
        ),
        (
            "panel linked to a missing layer",
            "/panels/1/linked",
            Some(json!("l_none")),
            "/panels/1/linked",
            "UNKNOWN_LAYER",
        ),
        (
            "breaks not ascending",
            "/layers/0/style/fill/scale/breaks",
            Some(json!([0.5, 0.15, 1, 2])),
            "/layers/0/style/fill/scale/breaks",
            "BREAKS_ORDER",
        ),
        (
            "breaks do not match classes",
            "/layers/1/children/0/style/fill/bivariate/y/scale/breaks",
            Some(json!([1, 2, 3])),
            "/layers/1/children/0/style/fill/bivariate/y/scale/breaks",
            "BREAKS_COUNT",
        ),
        (
            "unknown ramp",
            "/layers/0/style/fill/scale/ramp",
            Some(json!("ldl.seq.rainbow")),
            "/layers/0/style/fill/scale/ramp",
            "UNKNOWN_RAMP",
        ),
        (
            "palette is not bivariate",
            "/layers/1/children/0/style/fill/bivariate/palette",
            Some(json!("ldl.seq.blue")),
            "/layers/1/children/0/style/fill/bivariate/palette",
            "RAMP_KIND",
        ),
        (
            "unknown tone",
            "/layers/3/style/stroke/color",
            Some(json!("ldl.tone.mauve")),
            "/layers/3/style/stroke/color",
            "UNKNOWN_TONE",
        ),
        (
            "filter arity",
            "/layers/1/children/1/filter",
            Some(json!({"op": "between", "args": [{"property": "p"}, 1]})),
            "/layers/1/children/1/filter/args",
            "CQL2_ARITY",
        ),
        (
            "unknown filter operator",
            "/layers/1/children/1/filter/op",
            Some(json!("near")),
            "/layers/1/children/1/filter/op",
            "SCHEMA",
        ),
        (
            "time cursor outside its range",
            "/time",
            Some(json!({"cursor": "2030", "range": ["2012", "2026"], "freq": "A"})),
            "/time/cursor",
            "TIME_RANGE",
        ),
        (
            "reversed period range",
            "/sources/exposure/period",
            Some(json!({"from": "2026", "to": "2012"})),
            "/sources/exposure/period",
            "ORDER",
        ),
        (
            "reversed zoom range",
            "/layers/4/zoom",
            Some(json!([12, 9])),
            "/layers/4/zoom",
            "ORDER",
        ),
        (
            "reversed bounds",
            "/view/bounds",
            Some(json!([80.25, 6.38, 79.80, 7.30])),
            "/view/bounds",
            "ORDER",
        ),
        (
            "width mapped to a string",
            "/layers/3/style/stroke/width",
            Some(
                json!({"field": "class", "scale": {"type": "categorical", "mapping": {"a": "#ff0000"}}}),
            ),
            "/layers/3/style/stroke/width/scale/mapping/a",
            "MAPPING_TYPE",
        ),
        (
            "too many layers",
            "/layers",
            Some(Value::Array(many_layers)),
            "/layers",
            "SCHEMA",
        ),
        (
            "groups too deep",
            "/layers/5",
            Some(deep),
            "/layers/5/children/0/children/0/children/0",
            "GROUP_DEPTH",
        ),
        (
            "too many groups",
            "/layers",
            Some(Value::Array(groups)),
            "/layers",
            "TOO_MANY_GROUPS",
        ),
        (
            "too many sources",
            "/sources",
            Some(Value::Object(sources)),
            "/sources",
            "SCHEMA",
        ),
        (
            "document over 256 KB",
            "/layers",
            Some(Value::Array(huge_layers)),
            "",
            "TOO_LARGE",
        ),
        (
            "inline GeoJSON over 100 KB",
            "/sources/notes",
            Some(inline),
            "/sources/notes/geojson",
            "INLINE_TOO_LARGE",
        ),
        (
            "external over http",
            "/sources/ext",
            Some(
                json!({"kind": "external", "type": "geojson", "url": "http://example.org/a.geojson", "attribution": "x"}),
            ),
            "/sources/ext/url",
            "SCHEMA",
        ),
    ];
    assert!(cases.len() >= 20);
    let mut failures = Vec::new();
    for (what, ptr, new, want_path, want_code) in cases {
        let mut v = example();
        patch(&mut v, ptr, new);
        match validate(&v) {
            Ok(_) => failures.push(format!("{what}: validated")),
            Err(problems) => {
                if !problems
                    .iter()
                    .any(|p| p.path == want_path && p.code == want_code)
                {
                    failures.push(format!(
                        "{what}: want {want_code} at {want_path:?}, got {problems:#?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn not_json_and_not_an_object() {
    assert_eq!(validate_str("{").err().map(|p| p[0].code), Some("JSON"));
    assert_eq!(
        validate(&json!([1])).err().map(|p| p[0].path.clone()),
        Some(String::new())
    );
    assert_eq!(
        validate(&json!({"title": "x"}))
            .err()
            .map(|p| p[0].path.clone()),
        Some("/spec".to_owned())
    );
}

#[test]
fn snapshot_id_is_stable_under_key_order_and_whitespace() {
    let a = example();
    let text = serde_json::to_string_pretty(&a).unwrap_or_default();
    let b: Value = serde_json::from_str(&text).unwrap_or_default();
    assert_eq!(snapshot_id(&a), snapshot_id(&b));
    assert!(snapshot_id(&a).starts_with("ms_") && snapshot_id(&a).len() == 19);
}
