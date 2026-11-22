//! L1 extracts (01-layers.md §L1, ADR 0003): what a parser read out of one
//! or more artifacts, before any mapping to indicators or p-codes.
//!
//! Three shapes, each persisted as one zstd Parquet file, content-addressed
//! in the store (`extract/ab/cd/<sha256>.parquet`) and indexed in
//! `ops.extracts`:
//!
//! | shape | columns |
//! |---|---|
//! | `cells` | `table_id, r, c, text, vtype, page, bbox[4], sheet, a1, row_header, col_header, artifact_idx, conf, reader` |
//! | `records` | `table_id, record_idx, path, fields map<string,string>, artifact_idx` |
//! | `features` | `source_key, kind, tags map<string,string>, geojson, artifact_idx` |
//!
//! Deviations from the doc's column lists, all additive: `cells.vtype`
//! (`s`tring / `n`umber, so a numeric cell and a string cell with the same
//! text stay distinguishable), `records.table_id` (one extract can hold an
//! API's metadata block and its data rows), and `features.geojson` in place
//! of `wkb` (the boundary dsv hashes the source GeoJSON text; WKB comes with
//! the `geo_file` driver, WP-04). Encoding is deterministic: the same rows
//! give the same bytes, so the same artifact + extractor version gives the
//! same extract hash and re-runs are no-ops at this layer.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use arrow_array::builder::{
    FixedSizeListBuilder, Float32Builder, Int32Builder, MapBuilder, StringBuilder, UInt32Builder,
};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float32Type, Int32Type, UInt32Type};
use arrow_array::{Array, ArrayRef, MapArray, RecordBatch};
use bytes::Bytes;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;

/// Rows per Parquet row group / record batch.
const BATCH: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Cells,
    Records,
    Features,
}

impl Shape {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cells => "cells",
            Self::Records => "records",
            Self::Features => "features",
        }
    }
}

/// Type of the source cell (`vtype`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellType {
    String,
    Number,
}

impl CellType {
    const fn code(self) -> &'static str {
        match self {
            Self::String => "s",
            Self::Number => "n",
        }
    }
}

/// One table cell: a token as printed plus where it was printed.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub table_id: String,
    /// 0-based row / column in the source table or sheet.
    pub r: u32,
    pub c: u32,
    /// Verbatim text.
    pub text: String,
    pub vtype: CellType,
    pub page: Option<i32>,
    pub bbox: Option<[f32; 4]>,
    pub sheet: Option<String>,
    pub a1: Option<String>,
    pub row_header: Option<String>,
    pub col_header: Option<String>,
    pub artifact_idx: u32,
    pub conf: Option<f32>,
    /// `calamine`, `pdftotext`, `ocr`, `llm_dual`, `manual`, …
    pub reader: String,
}

/// One record of a JSON/CSV/API source: selected fields as text (numbers
/// keep their exact serialised token; absent = JSON null or missing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub table_id: String,
    pub record_idx: u32,
    /// Location in the artifact (JSONPath for JSON, `row:n` for CSV).
    pub path: String,
    pub fields: BTreeMap<String, String>,
    pub artifact_idx: u32,
}

/// One feature: properties as text plus the geometry as printed (GeoJSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub source_key: String,
    pub kind: String,
    pub tags: BTreeMap<String, String>,
    pub geojson: String,
    pub artifact_idx: u32,
}

/// What `Connector::extract` returns, and what `standardise` reads back.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractOut {
    Cells(Vec<Cell>),
    Records(Vec<Record>),
    Features(Vec<Feature>),
}

impl ExtractOut {
    #[must_use]
    pub const fn shape(&self) -> Shape {
        match self {
            Self::Cells(_) => Shape::Cells,
            Self::Records(_) => Shape::Records,
            Self::Features(_) => Shape::Features,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Cells(v) => v.len(),
            Self::Records(v) => v.len(),
            Self::Features(v) => v.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        match self {
            Self::Cells(v) => v,
            _ => &[],
        }
    }

    #[must_use]
    pub fn records(&self) -> &[Record] {
        match self {
            Self::Records(v) => v,
            _ => &[],
        }
    }

    #[must_use]
    pub fn features(&self) -> &[Feature] {
        match self {
            Self::Features(v) => v,
            _ => &[],
        }
    }
}

/// The artifacts an extract read, by `artifact_idx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractArtifact {
    pub artifact_id: i64,
    pub key: String,
    pub url: String,
    pub sha256: [u8; 32],
}

/// A persisted extract handed to `standardise`.
#[derive(Debug, Clone)]
pub struct ExtractRef {
    pub id: i64,
    pub sha256: [u8; 32],
    pub extractor: String,
    pub extractor_ver: String,
    pub artifacts: Vec<ExtractArtifact>,
    pub data: Arc<ExtractOut>,
}

impl ExtractRef {
    /// The artifact a row came from.
    pub fn artifact(&self, idx: u32) -> Result<&ExtractArtifact> {
        self.artifacts
            .get(idx as usize)
            .with_context(|| format!("extract {} has no artifact {idx}", self.id))
    }
}

// ───────────────────────────── encoding ────────────────────────────────────

fn str_col<'a>(items: impl Iterator<Item = &'a str>) -> ArrayRef {
    let mut b = StringBuilder::new();
    for s in items {
        b.append_value(s);
    }
    Arc::new(b.finish())
}

fn opt_str_col<'a>(items: impl Iterator<Item = Option<&'a str>>) -> ArrayRef {
    let mut b = StringBuilder::new();
    for s in items {
        b.append_option(s);
    }
    Arc::new(b.finish())
}

fn u32_col(items: impl Iterator<Item = u32>) -> ArrayRef {
    let mut b = UInt32Builder::new();
    for v in items {
        b.append_value(v);
    }
    Arc::new(b.finish())
}

fn map_col<'a>(items: impl Iterator<Item = &'a BTreeMap<String, String>>) -> Result<ArrayRef> {
    let mut b = MapBuilder::new(None, StringBuilder::new(), StringBuilder::new());
    for m in items {
        for (k, v) in m {
            b.keys().append_value(k);
            b.values().append_value(v);
        }
        b.append(true)?;
    }
    Ok(Arc::new(b.finish()))
}

fn batch_cells(rows: &[Cell]) -> Result<RecordBatch> {
    let mut page = Int32Builder::new();
    let mut conf = Float32Builder::new();
    let mut bbox = FixedSizeListBuilder::new(Float32Builder::new(), 4);
    for r in rows {
        page.append_option(r.page);
        conf.append_option(r.conf);
        match r.bbox {
            Some(b) => {
                bbox.values().append_slice(&b);
                bbox.append(true);
            }
            None => {
                bbox.values().append_nulls(4);
                bbox.append(false);
            }
        }
    }
    Ok(RecordBatch::try_from_iter(vec![
        (
            "table_id",
            str_col(rows.iter().map(|r| r.table_id.as_str())),
        ),
        ("r", u32_col(rows.iter().map(|r| r.r))),
        ("c", u32_col(rows.iter().map(|r| r.c))),
        ("text", str_col(rows.iter().map(|r| r.text.as_str()))),
        ("vtype", str_col(rows.iter().map(|r| r.vtype.code()))),
        ("page", Arc::new(page.finish()) as ArrayRef),
        ("bbox", Arc::new(bbox.finish()) as ArrayRef),
        (
            "sheet",
            opt_str_col(rows.iter().map(|r| r.sheet.as_deref())),
        ),
        ("a1", opt_str_col(rows.iter().map(|r| r.a1.as_deref()))),
        (
            "row_header",
            opt_str_col(rows.iter().map(|r| r.row_header.as_deref())),
        ),
        (
            "col_header",
            opt_str_col(rows.iter().map(|r| r.col_header.as_deref())),
        ),
        ("artifact_idx", u32_col(rows.iter().map(|r| r.artifact_idx))),
        ("conf", Arc::new(conf.finish()) as ArrayRef),
        ("reader", str_col(rows.iter().map(|r| r.reader.as_str()))),
    ])?)
}

fn batch_records(rows: &[Record]) -> Result<RecordBatch> {
    Ok(RecordBatch::try_from_iter(vec![
        (
            "table_id",
            str_col(rows.iter().map(|r| r.table_id.as_str())),
        ),
        ("record_idx", u32_col(rows.iter().map(|r| r.record_idx))),
        ("path", str_col(rows.iter().map(|r| r.path.as_str()))),
        ("fields", map_col(rows.iter().map(|r| &r.fields))?),
        ("artifact_idx", u32_col(rows.iter().map(|r| r.artifact_idx))),
    ])?)
}

fn batch_features(rows: &[Feature]) -> Result<RecordBatch> {
    Ok(RecordBatch::try_from_iter(vec![
        (
            "source_key",
            str_col(rows.iter().map(|r| r.source_key.as_str())),
        ),
        ("kind", str_col(rows.iter().map(|r| r.kind.as_str()))),
        ("tags", map_col(rows.iter().map(|r| &r.tags))?),
        ("geojson", str_col(rows.iter().map(|r| r.geojson.as_str()))),
        ("artifact_idx", u32_col(rows.iter().map(|r| r.artifact_idx))),
    ])?)
}

/// Encodes an extract as Parquet (zstd), with `shape` and the caller's
/// metadata (`extractor`, `extractor_ver`, …) in the file footer.
pub fn to_parquet(out: &ExtractOut, meta: &[(&str, &str)]) -> Result<Vec<u8>> {
    let mut kv = vec![KeyValue::new(
        "lanka.shape".into(),
        out.shape().as_str().to_owned(),
    )];
    kv.extend(
        meta.iter()
            .map(|(k, v)| KeyValue::new(format!("lanka.{k}"), (*v).to_owned())),
    );
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
        .set_created_by("lanka-data-layer extract v1".to_owned())
        .set_key_value_metadata(Some(kv))
        .set_max_row_group_row_count(Some(BATCH))
        .build();
    let batches: Vec<RecordBatch> = match out {
        ExtractOut::Cells(v) => chunks(v).map(batch_cells).collect::<Result<_>>()?,
        ExtractOut::Records(v) => chunks(v).map(batch_records).collect::<Result<_>>()?,
        ExtractOut::Features(v) => chunks(v).map(batch_features).collect::<Result<_>>()?,
    };
    let schema = match batches.first() {
        Some(b) => b.schema(),
        // An empty extract still has a schema.
        None => match out {
            ExtractOut::Cells(_) => batch_cells(&[])?.schema(),
            ExtractOut::Records(_) => batch_records(&[])?.schema(),
            ExtractOut::Features(_) => batch_features(&[])?.schema(),
        },
    };
    let mut w = ArrowWriter::try_new(Vec::new(), schema, Some(props))?;
    for b in &batches {
        w.write(b)?;
    }
    Ok(w.into_inner()?)
}

fn chunks<T>(v: &[T]) -> impl Iterator<Item = &[T]> {
    // At least one (possibly empty) chunk so the schema is always written.
    let n = v.len().div_ceil(BATCH).max(1);
    (0..n).map(move |i| &v[(i * BATCH).min(v.len())..((i + 1) * BATCH).min(v.len())])
}

// ───────────────────────────── decoding ────────────────────────────────────

fn col<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a ArrayRef> {
    b.column_by_name(name)
        .with_context(|| format!("extract has no column {name:?}"))
}

fn strings(b: &RecordBatch, name: &str) -> Result<Vec<Option<String>>> {
    let a = col(b, name)?
        .as_string_opt::<i32>()
        .with_context(|| format!("{name} is not utf8"))?;
    Ok((0..a.len())
        .map(|i| a.is_valid(i).then(|| a.value(i).to_owned()))
        .collect())
}

fn req_strings(b: &RecordBatch, name: &str) -> Result<Vec<String>> {
    strings(b, name)?
        .into_iter()
        .map(|s| s.with_context(|| format!("null in {name}")))
        .collect()
}

fn u32s(b: &RecordBatch, name: &str) -> Result<Vec<u32>> {
    let a = col(b, name)?
        .as_primitive_opt::<UInt32Type>()
        .with_context(|| format!("{name} is not uint32"))?;
    Ok(a.values().to_vec())
}

fn maps(b: &RecordBatch, name: &str) -> Result<Vec<BTreeMap<String, String>>> {
    let a: &MapArray = col(b, name)?
        .as_map_opt()
        .with_context(|| format!("{name} is not a map"))?;
    let keys = a.keys().as_string::<i32>();
    let values = a.values().as_string::<i32>();
    let off = a.value_offsets();
    Ok((0..a.len())
        .map(|i| {
            (off[i] as usize..off[i + 1] as usize)
                .map(|j| (keys.value(j).to_owned(), values.value(j).to_owned()))
                .collect()
        })
        .collect())
}

/// Decodes a Parquet extract written by [`to_parquet`].
pub fn from_parquet(bytes: Bytes) -> Result<ExtractOut> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(bytes)?;
    let shape = builder
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .and_then(|kv| kv.iter().find(|k| k.key == "lanka.shape"))
        .and_then(|k| k.value.clone())
        .context("extract has no lanka.shape")?;
    let reader = builder.with_batch_size(BATCH).build()?;
    let mut out = match shape.as_str() {
        "cells" => ExtractOut::Cells(Vec::new()),
        "records" => ExtractOut::Records(Vec::new()),
        "features" => ExtractOut::Features(Vec::new()),
        s => bail!("unknown extract shape {s:?}"),
    };
    for b in reader {
        let b = b?;
        match &mut out {
            ExtractOut::Cells(v) => {
                let page = col(&b, "page")?.as_primitive::<Int32Type>();
                let conf = col(&b, "conf")?.as_primitive::<Float32Type>();
                let bbox = col(&b, "bbox")?.as_fixed_size_list();
                let bvals = bbox.values().as_primitive::<Float32Type>();
                let (t, r, c, text, vt) = (
                    req_strings(&b, "table_id")?,
                    u32s(&b, "r")?,
                    u32s(&b, "c")?,
                    req_strings(&b, "text")?,
                    req_strings(&b, "vtype")?,
                );
                let (sheet, a1, rh, ch) = (
                    strings(&b, "sheet")?,
                    strings(&b, "a1")?,
                    strings(&b, "row_header")?,
                    strings(&b, "col_header")?,
                );
                let (ai, reader) = (u32s(&b, "artifact_idx")?, req_strings(&b, "reader")?);
                for i in 0..b.num_rows() {
                    v.push(Cell {
                        table_id: t[i].clone(),
                        r: r[i],
                        c: c[i],
                        text: text[i].clone(),
                        vtype: if vt[i] == "n" {
                            CellType::Number
                        } else {
                            CellType::String
                        },
                        page: page.is_valid(i).then(|| page.value(i)),
                        bbox: bbox.is_valid(i).then(|| {
                            let o = i * 4;
                            [
                                bvals.value(o),
                                bvals.value(o + 1),
                                bvals.value(o + 2),
                                bvals.value(o + 3),
                            ]
                        }),
                        sheet: sheet[i].clone(),
                        a1: a1[i].clone(),
                        row_header: rh[i].clone(),
                        col_header: ch[i].clone(),
                        artifact_idx: ai[i],
                        conf: conf.is_valid(i).then(|| conf.value(i)),
                        reader: reader[i].clone(),
                    });
                }
            }
            ExtractOut::Records(v) => {
                let (t, idx, path, fields, ai) = (
                    req_strings(&b, "table_id")?,
                    u32s(&b, "record_idx")?,
                    req_strings(&b, "path")?,
                    maps(&b, "fields")?,
                    u32s(&b, "artifact_idx")?,
                );
                v.extend(t.into_iter().zip(idx).zip(path).zip(fields).zip(ai).map(
                    |((((t, i), p), f), a)| Record {
                        table_id: t,
                        record_idx: i,
                        path: p,
                        fields: f,
                        artifact_idx: a,
                    },
                ));
            }
            ExtractOut::Features(v) => {
                let (k, kind, tags, g, ai) = (
                    req_strings(&b, "source_key")?,
                    req_strings(&b, "kind")?,
                    maps(&b, "tags")?,
                    req_strings(&b, "geojson")?,
                    u32s(&b, "artifact_idx")?,
                );
                v.extend(k.into_iter().zip(kind).zip(tags).zip(g).zip(ai).map(
                    |((((k, kind), tags), g), a)| Feature {
                        source_key: k,
                        kind,
                        tags,
                        geojson: g,
                        artifact_idx: a,
                    },
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(r: u32, text: &str, bbox: Option<[f32; 4]>) -> Cell {
        Cell {
            table_id: "A5".into(),
            r,
            c: 2,
            text: text.into(),
            vtype: CellType::Number,
            page: None,
            bbox,
            sheet: Some("A5".into()),
            a1: Some(format!("C{}", r + 1)),
            row_header: Some("Colombo".into()),
            col_header: None,
            artifact_idx: 1,
            conf: Some(1.0),
            reader: "calamine".into(),
        }
    }

    #[test]
    fn round_trips_every_shape_deterministically() {
        let cells = ExtractOut::Cells(vec![
            cell(7, "21781800", None),
            cell(8, "2.5", Some([1.0, 2.0, 3.0, 4.0])),
        ]);
        let recs = ExtractOut::Records(vec![Record {
            table_id: "data".into(),
            record_idx: 3,
            path: "$[1][3]".into(),
            fields: [("value".to_owned(), "2.5e7".to_owned())].into(),
            artifact_idx: 0,
        }]);
        let feats = ExtractOut::Features(vec![Feature {
            source_key: "LK1".into(),
            kind: "admin".into(),
            tags: BTreeMap::new(),
            geojson: r#"{"type":"Point","coordinates":[80,7]}"#.into(),
            artifact_idx: 2,
        }]);
        for e in [cells, recs, feats, ExtractOut::Records(Vec::new())] {
            let a = to_parquet(&e, &[("extractor", "t")]).expect("encode");
            let b = to_parquet(&e, &[("extractor", "t")]).expect("encode");
            assert_eq!(a, b, "encoding is deterministic");
            assert_eq!(from_parquet(Bytes::from(a)).expect("decode"), e);
        }
    }

    #[test]
    fn large_tables_span_row_groups() {
        let rows: Vec<Cell> = (0..(BATCH as u32 + 10))
            .map(|i| cell(i, &i.to_string(), None))
            .collect();
        let e = ExtractOut::Cells(rows);
        let back = from_parquet(Bytes::from(to_parquet(&e, &[]).expect("enc"))).expect("dec");
        assert_eq!(back, e);
    }
}
