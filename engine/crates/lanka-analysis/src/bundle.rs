//! `GET /v2/analyses/{id}/bundle` (11 §11.1): a zip with `result.json`,
//! every output as JSON and (frames) CSV, `README.md` (method, guards,
//! citation), `datapackage.json` and `prov.jsonld`. Entries are stored
//! (no compression) with a fixed timestamp, so a bundle is a pure function
//! of the result.

use serde_json::{Value, json};

/// CSV of a frame output (`{codes, columns}`), columns in `order`.
#[must_use]
pub fn frame_csv(data: &Value, order: &[String]) -> String {
    let codes = data["codes"].as_array().cloned().unwrap_or_default();
    let cols = &data["columns"];
    let mut names: Vec<String> = order
        .iter()
        .filter(|c| cols.get(c.as_str()).is_some())
        .cloned()
        .collect();
    if names.is_empty()
        && let Some(o) = cols.as_object()
    {
        names = o.keys().cloned().collect();
    }
    let esc = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    };
    let mut out = String::from("code");
    for n in &names {
        out.push(',');
        out.push_str(&esc(n));
    }
    out.push('\n');
    for (i, c) in codes.iter().enumerate() {
        out.push_str(&esc(c.as_str().unwrap_or_default()));
        for n in &names {
            out.push(',');
            match &cols[n.as_str()][i] {
                Value::Null => {}
                Value::String(s) => out.push_str(&esc(s)),
                v => out.push_str(&v.to_string()),
            }
        }
        out.push('\n');
    }
    out
}

/// README of a result: method, inputs, guards (above the numbers), citation.
#[must_use]
pub fn readme(doc: &Value) -> String {
    let mut s = format!(
        "# Analysis {}\n\n{} {} — state: {}\n\n",
        doc["id"].as_str().unwrap_or_default(),
        doc["method"]["id"].as_str().unwrap_or_default(),
        doc["method"]["version"].as_str().unwrap_or_default(),
        doc["state"].as_str().unwrap_or_default()
    );
    s.push_str("## Guards\n\n");
    for g in doc["guards"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "- **{}** ({}): {}\n",
            g["code"].as_str().unwrap_or_default(),
            g["severity"].as_str().unwrap_or_default(),
            g["message"].as_str().unwrap_or_default()
        ));
    }
    if let Some(sum) = doc["interpretation"]["summary"].as_str() {
        s.push_str(&format!("\n## Summary\n\n{sum}\n"));
    }
    s.push_str(
        "\n## Inputs\n\n| role | dataset | version | period | licence |\n|---|---|---|---|---|\n",
    );
    for i in doc["inputs"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            i["role"].as_str().unwrap_or_default(),
            i["dataset"].as_str().unwrap_or_default(),
            i["dsv"].as_str().unwrap_or_default(),
            i["period"].as_str().unwrap_or_default(),
            i["licence"].as_str().unwrap_or_default()
        ));
    }
    s.push_str("\n## Method\n\n");
    for c in doc["method"]["citations"].as_array().into_iter().flatten() {
        s.push_str(&format!("- {}\n", c.as_str().unwrap_or_default()));
    }
    s.push_str(&format!(
        "\n## Cite\n\n{}\n<{}>\n",
        doc["citation"]["text"].as_str().unwrap_or_default(),
        doc["citation"]["url"].as_str().unwrap_or_default()
    ));
    s
}

#[must_use]
pub fn datapackage(doc: &Value, files: &[(String, &'static str)]) -> Value {
    json!({
        "profile": "data-package",
        "name": doc["id"],
        "title": doc["citation"]["text"],
        "licenses": [{"name": doc["licence"]["id"]}],
        "sources": doc["attribution"],
        "resources": files.iter().map(|(p, fmt)| json!({"path": p, "format": fmt})).collect::<Vec<_>>(),
    })
}

#[must_use]
pub fn prov(doc: &Value) -> Value {
    let id = doc["id"].as_str().unwrap_or_default();
    json!({
        "@context": "https://www.w3.org/ns/prov.jsonld",
        "@graph": [
            {"@id": format!("analysis:{id}"), "@type": "prov:Entity", "prov:wasGeneratedBy": format!("run:{id}")},
            {"@id": format!("run:{id}"), "@type": "prov:Activity",
             "prov:used": doc["inputs"].as_array().into_iter().flatten()
                .map(|i| format!("dsv:{}", i["dsv"].as_str().unwrap_or_default())).collect::<Vec<_>>(),
             "prov:wasAssociatedWith": doc["provenance"]["engine"],
             "lanka:method": format!("{}@{}", doc["method"]["id"].as_str().unwrap_or_default(), doc["method"]["version"].as_str().unwrap_or_default()),
             "lanka:fingerprint": doc["provenance"]["fingerprint"]}
        ]
    })
}

/// A zip of stored entries (`name`, bytes), in the given order.
#[must_use]
pub fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    // DOS time 00:00, date 2026-01-01 (fixed: the bundle is reproducible)
    const TIME: u16 = 0;
    const DATE: u16 = ((2026 - 1980) << 9) | (1 << 5) | 1;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let crc = crc32fast::hash(data);
        let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let n = u16::try_from(name.len()).unwrap_or(u16::MAX);
        let header = |sig: u32, central: bool| -> Vec<u8> {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20_u16.to_le_bytes()); // made by
            }
            h.extend(20_u16.to_le_bytes()); // version needed
            h.extend(0x0800_u16.to_le_bytes()); // UTF-8 names
            h.extend(0_u16.to_le_bytes()); // stored
            h.extend(TIME.to_le_bytes());
            h.extend(DATE.to_le_bytes());
            h.extend(crc.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend(n.to_le_bytes());
            h.extend(0_u16.to_le_bytes()); // extra
            if central {
                h.extend(0_u16.to_le_bytes()); // comment
                h.extend(0_u16.to_le_bytes()); // disk
                h.extend(0_u16.to_le_bytes()); // internal attrs
                h.extend(0_u32.to_le_bytes()); // external attrs
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(data);
        central.extend(header(0x0201_4b50, true));
    }
    let cd_offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let cd_size = u32::try_from(central.len()).unwrap_or(u32::MAX);
    out.extend(central);
    let count = u16::try_from(entries.len()).unwrap_or(u16::MAX);
    out.extend(0x0605_4b50_u32.to_le_bytes());
    out.extend(0_u16.to_le_bytes());
    out.extend(0_u16.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(cd_size.to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0_u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_zip_shape() {
        let d = json!({"codes": ["A", "B"], "columns": {"x": [1.5, null], "g": ["in", "o,ut"]}});
        assert_eq!(
            frame_csv(&d, &["x".into(), "g".into()]),
            "code,x,g\nA,1.5,in\nB,,\"o,ut\"\n"
        );
        let z = zip(&[
            ("a.txt".into(), b"hello".to_vec()),
            ("b/c.json".into(), b"{}".to_vec()),
        ]);
        assert_eq!(&z[..4], &[0x50, 0x4b, 0x03, 0x04]);
        assert_eq!(&z[z.len() - 22..z.len() - 18], &[0x50, 0x4b, 0x05, 0x06]);
        assert_eq!(
            z,
            zip(&[
                ("a.txt".into(), b"hello".to_vec()),
                ("b/c.json".into(), b"{}".to_vec())
            ])
        );
    }
}
