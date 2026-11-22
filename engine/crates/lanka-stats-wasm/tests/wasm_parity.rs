//! Native ≡ WebAssembly (11 §13 property): the WebAssembly build's outputs
//! on the reference fixtures (`fixtures/wasm_bits.json`, written by
//! `build.sh` → `js/parity.mjs` under node) must equal the native kernels'
//! to the last bit.

use lanka_stats_wasm::kernels;
use serde_json::Value;

fn fv(v: &Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().expect("number"))
        .collect()
}

fn bits(v: &[f64]) -> Vec<String> {
    v.iter().map(|x| format!("{:016x}", x.to_bits())).collect()
}

#[test]
fn native_and_wasm_agree_bit_for_bit() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../lanka-stats/tests/fixtures/reference.json"
    ))
    .expect("reference");
    let wasm: Value =
        serde_json::from_str(include_str!("fixtures/wasm_bits.json")).expect("wasm bits");
    let mut native: Vec<(String, Vec<f64>)> = Vec::new();
    let cv = fv(&reference["classify"]["values"]);
    for m in 0..6 {
        native.push((format!("classify/{m}"), kernels::classify(&cv, m, 5)));
    }
    for c in reference["correlate"].as_array().expect("correlate") {
        let name = c["name"].as_str().expect("name");
        let (x, y) = (fv(&c["x"]), fv(&c["y"]));
        native.push((format!("correlate/{name}"), kernels::correlate(&x, &y)));
        native.push((format!("preview/{name}"), kernels::preview(&x, &y, 0, 5)));
        native.push((
            format!("bivariate/{name}"),
            kernels::bivariate(&x, &y, 0, 3),
        ));
        native.push((format!("hexbin/{name}"), kernels::hexbin(&x, &y, 0.3)));
    }
    for r in reference["regress"].as_array().expect("regress") {
        let name = r["name"].as_str().expect("name");
        let y = fv(&r["y"]);
        let cols: Vec<Vec<f64>> = r["x"].as_array().expect("x").iter().map(fv).collect();
        let flat: Vec<f64> = cols.concat();
        native.push((
            format!("regress/{name}"),
            kernels::regress(&y, &flat, cols.len()),
        ));
    }
    native.push((
        "describe".to_owned(),
        kernels::describe(&fv(&reference["describe"]["values"])),
    ));
    let obj = wasm.as_object().expect("object");
    assert_eq!(obj.len(), native.len(), "case count");
    let mut values = 0;
    for (case, v) in &native {
        let w: Vec<String> = obj
            .get(case)
            .unwrap_or_else(|| panic!("{case} missing from wasm_bits.json"))
            .as_array()
            .expect("bits")
            .iter()
            .map(|s| s.as_str().expect("hex").to_owned())
            .collect();
        assert_eq!(bits(v), w, "{case}: native and WebAssembly differ");
        values += v.len();
    }
    assert!(values > 900, "{values} values compared");
}
