//! Frame encodings besides JSON: Arrow IPC stream and CSV (10 §3.1).
//!
//! Arrow: one record batch; column `code` (Utf8), then per frame column
//! `<id>` (Float64 — the JSON numbers exactly), `<id>.lo` / `<id>.hi` when
//! the column has intervals, `<id>.basis` / `<id>.status` (Int8, 05 §2
//! codes) when they are not uniform, `<id>.flags` (Int32) when any value
//! carries a flag. Each value field's metadata `ldl:column` is the JSON
//! column minus its arrays; schema metadata carries `ldl:frame` (spec and
//! support) and `ldl:meta` (the envelope meta: release, versions, sources,
//! licences), so an Arrow consumer keeps attribution.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use arrow_array::{ArrayRef, Float64Array, Int8Array, Int32Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use lanka_core::Meta;
use lanka_core::frame::{Coded, Frame};
use serde_json::{Value, json};

pub const ARROW_STREAM: &str = "application/vnd.apache.arrow.stream";

fn column_meta(c: &lanka_core::frame::FrameColumn) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        for k in ["values", "lo", "hi", "flags"] {
            o.remove(k);
        }
        for k in ["basis", "status"] {
            if o.get(k).is_some_and(Value::is_array) {
                o.insert(k.to_owned(), json!({ "column": format!("{}.{k}", c.id) }));
            }
        }
    }
    v
}

fn i8s(codes: &[Option<i16>]) -> Int8Array {
    codes
        .iter()
        .map(|c| c.and_then(|x| i8::try_from(x).ok()))
        .collect()
}

pub fn arrow(f: &Frame, meta: &Meta) -> Result<Vec<u8>> {
    let mut fields: Vec<Field> = vec![Field::new("code", DataType::Utf8, false)];
    let mut arrays: Vec<ArrayRef> = vec![Arc::new(StringArray::from_iter_values(&f.codes))];
    for c in &f.columns {
        let md = HashMap::from([("ldl:column".to_owned(), column_meta(c).to_string())]);
        fields.push(Field::new(&c.id, DataType::Float64, true).with_metadata(md));
        arrays.push(Arc::new(Float64Array::from(c.values.clone())));
        for (suffix, v) in [("lo", &c.lo), ("hi", &c.hi)] {
            if let Some(v) = v {
                fields.push(Field::new(
                    format!("{}.{suffix}", c.id),
                    DataType::Float64,
                    true,
                ));
                arrays.push(Arc::new(Float64Array::from(v.clone())));
            }
        }
        for (suffix, v) in [("basis", &c.basis), ("status", &c.status)] {
            if let Some(Coded::Mixed(codes)) = v {
                fields.push(Field::new(
                    format!("{}.{suffix}", c.id),
                    DataType::Int8,
                    true,
                ));
                arrays.push(Arc::new(i8s(codes)));
            }
        }
        if let Some(fl) = &c.flags {
            fields.push(Field::new(
                format!("{}.flags", c.id),
                DataType::Int32,
                false,
            ));
            arrays.push(Arc::new(Int32Array::from(fl.clone())));
        }
    }
    let schema_md = HashMap::from([
        (
            "ldl:frame".to_owned(),
            json!({ "spec": "ldl-frame/1", "support": f.support }).to_string(),
        ),
        ("ldl:meta".to_owned(), serde_json::to_string(meta)?),
    ]);
    let schema = Arc::new(Schema::new(fields).with_metadata(schema_md));
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays)?;
    let mut buf = Vec::with_capacity(64 + f.codes.len() * (12 + 8 * f.columns.len()));
    {
        let mut w = StreamWriter::try_new(&mut buf, &schema)?;
        w.write(&batch)?;
        w.finish()?;
    }
    Ok(buf)
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// `code` then one column per frame column, headed
/// `<indicator>[<dims>]@<period>`; empty cell = no value.
#[must_use]
pub fn csv(f: &Frame) -> String {
    let mut out = String::from("code");
    for c in &f.columns {
        let dims: Vec<String> = c.dims.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let head = if dims.is_empty() {
            format!("{}@{}", c.indicator, c.period)
        } else {
            format!("{}[{}]@{}", c.indicator, dims.join(";"), c.period)
        };
        out.push(',');
        out.push_str(&csv_field(&head));
    }
    out.push('\n');
    for (i, code) in f.codes.iter().enumerate() {
        out.push_str(&csv_field(code));
        for c in &f.columns {
            out.push(',');
            if let Some(v) = c.values[i] {
                out.push_str(&v.to_string());
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;
    use lanka_core::frame::{ColumnStats, FrameColumn, FrameSupport, column_stats};

    fn sample() -> Frame {
        let values = vec![Some(2_375_415.0), None, Some(0.1 + 0.2), Some(-1.5e-9)];
        Frame {
            support: FrameSupport {
                kind: "admin".into(),
                scheme: "admin".into(),
                level: 2,
                parent: None,
                boundary_set: Some("cod-ab-lka-2022".into()),
                n: 4,
                unmatched: 0,
            },
            codes: vec!["LK11".into(), "LK12".into(), "LK13".into(), "LK21".into()],
            columns: vec![
                FrameColumn {
                    id: "c0".into(),
                    indicator: "census24.pop_total".into(),
                    dataset: "dcs-census-2024-population".into(),
                    dims: [("sex".to_owned(), "total".to_owned())].into(),
                    period: "2024".into(),
                    unit: Some("{person}".into()),
                    unit_mult: 0,
                    class: "official".into(),
                    completed_from: None,
                    stats: column_stats(&values),
                    values,
                    lo: None,
                    hi: None,
                    basis: Some(Coded::Uniform("reported".into())),
                    status: Some(Coded::Mixed(vec![Some(1), None, Some(2), Some(1)])),
                    flags: Some(vec![0, 0, 4, 0]),
                },
                FrameColumn {
                    id: "c1".into(),
                    indicator: "x.y".into(),
                    dataset: "d".into(),
                    dims: Default::default(),
                    period: "2012".into(),
                    unit: None,
                    unit_mult: 3,
                    class: "estimated".into(),
                    completed_from: None,
                    values: vec![Some(1.0), Some(2.0), None, Some(4.0)],
                    lo: Some(vec![Some(0.5), None, None, Some(3.0)]),
                    hi: Some(vec![Some(1.5), None, None, Some(5.0)]),
                    basis: Some(Coded::Uniform("estimated".into())),
                    status: Some(Coded::Uniform("normal".into())),
                    flags: None,
                    stats: ColumnStats::default(),
                },
            ],
            data_version: Some("20261122.1".into()),
            versions: Vec::new(),
        }
    }

    #[test]
    fn arrow_carries_the_json_values_exactly() {
        let f = sample();
        let bytes = arrow(&f, &Meta::bare(Some("20261122.1".into()))).unwrap_or_default();
        let mut r = arrow_ipc::reader::StreamReader::try_new(std::io::Cursor::new(bytes), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let batch = r
            .next()
            .and_then(Result::ok)
            .unwrap_or_else(|| panic!("one batch"));
        let schema = batch.schema();
        assert_eq!(batch.num_rows(), 4);
        let names: Vec<&str> = schema.fields().iter().map(|x| x.name().as_str()).collect();
        assert_eq!(
            names,
            [
                "code",
                "c0",
                "c0.status",
                "c0.flags",
                "c1",
                "c1.lo",
                "c1.hi"
            ]
        );
        for c in &f.columns {
            let idx = schema.index_of(&c.id).unwrap_or(usize::MAX);
            let col = batch
                .column(idx)
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap_or_else(|| panic!("f64"));
            let got: Vec<Option<f64>> = (0..col.len())
                .map(|i| col.is_valid(i).then(|| col.value(i)))
                .collect();
            // Bit-identical to the JSON numbers.
            assert_eq!(
                got.iter().map(|v| v.map(f64::to_bits)).collect::<Vec<_>>(),
                c.values
                    .iter()
                    .map(|v| v.map(f64::to_bits))
                    .collect::<Vec<_>>()
            );
            let md: Value = serde_json::from_str(&schema.field(idx).metadata()["ldl:column"])
                .unwrap_or_default();
            assert_eq!(md["indicator"], json!(c.indicator));
            assert!(md.get("values").is_none());
        }
        assert!(schema.metadata()["ldl:meta"].contains("20261122.1"));
        assert!(schema.metadata()["ldl:frame"].contains("cod-ab-lka-2022"));
    }

    #[test]
    fn csv_layout() {
        let s = csv(&sample());
        let mut lines = s.lines();
        assert_eq!(
            lines.next(),
            Some("code,census24.pop_total[sex=total]@2024,x.y@2012")
        );
        assert_eq!(lines.next(), Some("LK11,2375415,1"));
        assert_eq!(lines.next(), Some("LK12,,2"));
        assert_eq!(lines.next(), Some("LK13,0.30000000000000004,"));
    }
}
