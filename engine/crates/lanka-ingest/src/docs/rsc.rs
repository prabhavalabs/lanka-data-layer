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

/// The listing of an RSC response body, or `None` when no frame has both
/// `data` and `pagination` (the signature of a rejected server action).
pub fn parse_listing(body: &str) -> Result<Option<ListPage>> {
    for line in body.lines() {
        let Some((idx, json)) = line.split_once(':') else {
            continue;
        };
        if idx.is_empty() || !idx.bytes().all(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        if !json.starts_with('{') {
            continue;
        }
        let Ok(Value::Object(mut obj)) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        if !(obj.get("data").is_some_and(Value::is_array) && obj.contains_key("pagination")) {
            continue;
        }
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
}
