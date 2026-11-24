//! The listing response (13 §1.2 step 2): a React Server Components payload,
//! newline-delimited `<idx>:<json>` frames. The frame that carries the
//! listing is the JSON object with both `data` and `pagination`; every other
//! frame (the action reference, hints, text chunks) is ignored. Serde is
//! lenient: unknown fields are dropped, nulls and missing fields are allowed.

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// One gazette as the site lists it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RawRecord {
    pub id: String,
    #[serde(deserialize_with = "text_or_number")]
    pub gazette_no: Option<String>,
    #[serde(deserialize_with = "text_or_number")]
    pub gazette_sub_no: Option<String>,
    pub gazette_no_text: Option<String>,
    /// `2026-11-16T00:00:00.000Z`
    pub date: Option<String>,
    pub description_sinhala: Option<String>,
    pub description_tamil: Option<String>,
    pub description_english: Option<String>,
    pub keywords_sinhala: Option<Value>,
    pub keywords_tamil: Option<Value>,
    pub keywords_english: Option<Value>,
    pub updated_at: Option<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub contents: Vec<RawContent>,
}

/// One file of a gazette (one per language).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RawContent {
    /// `SINHALA` | `TAMIL` | `ENGLISH`
    pub language: Option<String>,
    pub original_file_name: Option<String>,
    /// The path the file proxy takes, `extra-gazette-content/G 48538 (E)_….pdf`.
    pub uploaded_file: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Pagination {
    pub total: Option<i64>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub total_pages: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListPage {
    pub records: Vec<RawRecord>,
    pub pagination: Pagination,
}

fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

fn text_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::String(s)) => Some(s),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

/// One row of an RSC payload.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    /// `<id>:<json>\n`
    Json(Value),
    /// `<id>:T<hex byte length>,<text>`: a long string sent on its own,
    /// referenced from JSON rows as `"$<id>"`. It may contain newlines and is
    /// not newline-terminated: the next row starts right after its last byte.
    Text(String),
}

/// Split an RSC payload into rows. Lines that are not `<id>:…` (an HTML
/// error page, "Server action not found.") are skipped.
fn rows(body: &[u8]) -> Vec<(String, Row)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if matches!(body[i], b'\n' | b'\r') {
            i += 1;
            continue;
        }
        let line_end = body[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(body.len(), |p| i + p);
        let id_len = body[i..line_end]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        let colon = i + id_len;
        if id_len == 0 || colon >= line_end || body[colon] != b':' {
            i = line_end + 1;
            continue;
        }
        let id = String::from_utf8_lossy(&body[i..colon]).into_owned();
        let start = colon + 1;
        if body.get(start) == Some(&b'T') {
            let hex_len = body[start + 1..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            let comma = start + 1 + hex_len;
            let len = std::str::from_utf8(&body[start + 1..comma])
                .ok()
                .and_then(|h| usize::from_str_radix(h, 16).ok());
            if let (Some(len), Some(b',')) = (len, body.get(comma)) {
                let from = comma + 1;
                let to = (from + len).min(body.len());
                out.push((
                    id,
                    Row::Text(String::from_utf8_lossy(&body[from..to]).into_owned()),
                ));
                i = to;
                continue;
            }
        }
        if let Ok(v) = serde_json::from_slice::<Value>(&body[start..line_end]) {
            out.push((id, Row::Json(v)));
        }
        i = line_end + 1;
    }
    out
}

/// Replace RSC string references inside a JSON row: `"$<id>"` → the text
/// row `<id>`, `"$$…"` → the literal `"$…"`, `"$undefined"` → null and
/// `"$D<iso>"` → the date string. Unknown references are left as they are.
fn resolve(v: &mut Value, texts: &std::collections::HashMap<String, String>) {
    match v {
        Value::String(s) if s.starts_with('$') => {
            if let Some(rest) = s.strip_prefix("$$") {
                *s = format!("${rest}");
            } else if s == "$undefined" {
                *v = Value::Null;
            } else if let Some(t) = texts.get(&s[1..]) {
                *s = t.clone();
            } else if let Some(d) = s.strip_prefix("$D") {
                *s = d.to_owned();
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| resolve(x, texts)),
        Value::Object(o) => o.values_mut().for_each(|x| resolve(x, texts)),
        _ => {}
    }
}

/// The listing of an RSC response body, or `None` when no frame has both
/// `data` and `pagination` (the signature of a rejected server action).
pub fn parse_listing(body: &str) -> Result<Option<ListPage>> {
    let rows = rows(body.as_bytes());
    let texts: std::collections::HashMap<String, String> = rows
        .iter()
        .filter_map(|(id, r)| match r {
            Row::Text(t) => Some((id.clone(), t.clone())),
            Row::Json(_) => None,
        })
        .collect();
    for (_, row) in rows {
        let Row::Json(Value::Object(mut obj)) = row else {
            continue;
        };
        if !(obj.get("data").is_some_and(Value::is_array) && obj.contains_key("pagination")) {
            continue;
        }
        let mut data = obj.remove("data").unwrap_or(Value::Null);
        resolve(&mut data, &texts);
        obj.insert("data".to_owned(), data);
        let pagination: Pagination =
            serde_json::from_value(obj.remove("pagination").unwrap_or(Value::Null))
                .context("listing pagination")?;
        let Some(Value::Array(rows)) = obj.remove("data") else {
            continue;
        };
        let mut records = Vec::with_capacity(rows.len());
        for (i, row) in rows.into_iter().enumerate() {
            records.push(
                serde_json::from_value::<RawRecord>(row)
                    .with_context(|| format!("listing record {i}"))?,
            );
        }
        return Ok(Some(ListPage {
            records,
            pagination,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P1: &str = include_str!("../../tests/fixtures/docs/p1.txt");

    #[test]
    fn the_listing_frame_is_found_and_typed() {
        let page = parse_listing(P1).expect("parse").expect("a data frame");
        assert_eq!(page.records.len(), 3);
        assert_eq!(page.pagination.total, Some(39478));
        assert_eq!(page.pagination.page, Some(1));
        assert_eq!(page.pagination.limit, Some(3));
        assert_eq!(page.pagination.total_pages, Some(13160));
        let r = &page.records[2];
        assert_eq!(r.id, "05f3cc95-76ca-46b8-bffe-bd5329b1202e");
        assert_eq!(r.gazette_no.as_deref(), Some("2515"));
        assert_eq!(r.gazette_sub_no.as_deref(), Some("1"));
        assert_eq!(r.gazette_no_text.as_deref(), Some("2515/01"));
        assert_eq!(r.date.as_deref(), Some("2026-11-16T00:00:00.000Z"));
        assert_eq!(
            r.description_english.as_deref(),
            Some("Sri Lanka Customs - Rates of Exchange")
        );
        assert_eq!(r.contents.len(), 3);
        assert_eq!(r.contents[2].language.as_deref(), Some("ENGLISH"));
        assert_eq!(
            r.contents[2].uploaded_file.as_deref(),
            Some("extra-gazette-content/G 48538 (E)_1794986733.pdf")
        );
        assert!(r.keywords_english.is_none());
        assert_eq!(page.records[0].contents.len(), 1);
    }

    #[test]
    fn rejected_actions_have_no_listing() {
        assert!(
            parse_listing("0:{\"a\":\"$@1\"}\n1:{\"error\":\"x\"}\n")
                .expect("parse")
                .is_none()
        );
        assert!(parse_listing("").expect("parse").is_none());
        assert!(
            parse_listing("Server action not found.")
                .expect("parse")
                .is_none()
        );
        // `data` without `pagination` is not the listing
        assert!(parse_listing("1:{\"data\":[]}\n").expect("parse").is_none());
    }

    #[test]
    fn unknown_fields_and_nulls_are_tolerated() {
        let body = r#"2:I["x"]
1:{"data":[{"id":"u1","gazetteNo":7,"gazetteSubNo":null,"extra":{"a":1},"contents":null,"date":null}],"pagination":{"total":1,"newField":true}}
"#;
        let page = parse_listing(body).expect("parse").expect("frame");
        assert_eq!(page.records[0].gazette_no.as_deref(), Some("7"));
        assert_eq!(page.records[0].gazette_sub_no, None);
        assert!(page.records[0].contents.is_empty());
        assert_eq!(page.pagination.total_pages, None);
    }

    /// Long strings arrive as text rows of an exact UTF-8 byte length, may
    /// hold newlines, run straight into the next row, and are referenced
    /// from the listing as `"$<id>"` (page 171 of the live site, 2026-11-24).
    #[test]
    fn text_rows_are_read_by_length_and_resolved() {
        let long =
            "Western Province - Appointed Hon. A.\nActing Minister w.e.f. 09.12.2013.\n\nශ්‍රී ලංකා";
        let body = format!(
            "0:{{\"a\":\"$@1\"}}\n2:T{:x},{long}3:T{:x},{long}1:{{\"data\":[{{\"id\":\"u1\",\"gazetteNo\":1848,\"descriptionSinhala\":\"$2\",\"descriptionEnglish\":\"$3\",\"descriptionTamil\":\"$$5 notes\",\"keywordsEnglish\":\"$undefined\",\"date\":\"$D2014-02-05T00:00:00.000Z\",\"contents\":[]}}],\"pagination\":{{\"total\":39478,\"page\":171}}}}\n",
            long.len(),
            long.len()
        );
        let page = parse_listing(&body).expect("parse").expect("a data frame");
        let r = &page.records[0];
        assert_eq!(r.description_sinhala.as_deref(), Some(long));
        assert_eq!(r.description_english.as_deref(), Some(long));
        assert_eq!(r.description_tamil.as_deref(), Some("$5 notes"));
        assert!(r.keywords_english.is_none());
        assert_eq!(r.date.as_deref(), Some("2014-02-05T00:00:00.000Z"));
        assert_eq!(page.pagination.page, Some(171));
    }

    #[test]
    fn a_truncated_text_row_does_not_panic() {
        assert!(parse_listing("2:T400,short").expect("parse").is_none());
        assert!(
            parse_listing("2:Tzz,x\n1:{\"data\":[],\"pagination\":{}}\n")
                .expect("parse")
                .is_some()
        );
    }
}
