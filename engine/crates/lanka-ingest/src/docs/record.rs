//! The pure rules of the harvester: the normalised record and its hash, the
//! readable `doc_id`, what a sighting means for a stored document, when an
//! incremental walk stops and when a sweep may withdraw.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::rsc::RawRecord;

/// One file a document lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    /// `si` | `ta` | `en`
    pub lang: &'static str,
    pub path: String,
    pub name: Option<String>,
}

/// A listing record in the shape the tables store.
#[derive(Debug, Clone, PartialEq)]
pub struct DocRecord {
    pub upstream_id: String,
    pub number: Option<String>,
    pub sub_number: Option<String>,
    /// The readable id before collision handling: `gze-2515-04`.
    pub doc_id_base: String,
    pub issued_on: NaiveDate,
    pub title: Value,
    pub keywords: Option<Value>,
    pub upstream_updated_at: Option<DateTime<Utc>>,
    pub files: Vec<FileRef>,
    /// The normalised record that was hashed (stored as the revision).
    pub record: Value,
    /// sha256 of [`canonical_json`] of `record`.
    pub meta_hash: String,
}

#[must_use]
pub fn lang_code(upstream: &str) -> Option<&'static str> {
    match upstream.trim().to_ascii_uppercase().as_str() {
        "SINHALA" => Some("si"),
        "TAMIL" => Some("ta"),
        "ENGLISH" => Some("en"),
        _ => None,
    }
}

fn clean(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// JSON with object keys sorted, no whitespace: the same text however the
/// upstream ordered its keys.
#[must_use]
pub fn canonical_json(v: &Value) -> String {
    fn write(v: &Value, out: &mut String) {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&Value::String(k.clone()).to_string());
                    out.push(':');
                    write(&m[k], out);
                }
                out.push('}');
            }
            Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    write(v, &mut s);
    s
}

#[must_use]
pub fn hash_record(record: &Value) -> String {
    hex::encode(Sha256::digest(canonical_json(record).as_bytes()))
}

/// `gze` + `2515/04` → `gze-2515-04`.
#[must_use]
pub fn doc_id_base(prefix: &str, number_text: &str) -> String {
    let n: String = number_text
        .trim()
        .chars()
        .map(|c| if c == '/' { '-' } else { c })
        .collect::<String>()
        .to_lowercase();
    format!("{prefix}-{n}")
}

/// The first six letters or digits of an upstream id, lower-cased.
#[must_use]
pub fn id_suffix(upstream_id: &str) -> String {
    upstream_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(6)
        .collect::<String>()
        .to_lowercase()
}

/// The `doc_id` of a new document. `owner_of_base` is the upstream id of the
/// document that already holds `base` (if any): a different one is a
/// collision, resolved by appending the first six characters of this
/// document's upstream id.
#[must_use]
pub fn pick_doc_id(base: &str, upstream_id: &str, owner_of_base: Option<&str>) -> String {
    match owner_of_base {
        Some(o) if o != upstream_id => format!("{base}-{}", id_suffix(upstream_id)),
        _ => base.to_owned(),
    }
}

/// Parses `2026-11-16T00:00:00.000Z` (or a bare date) to its calendar date.
pub fn parse_date(s: &str) -> Result<NaiveDate> {
    let head = s.trim().get(..10).unwrap_or(s.trim());
    NaiveDate::parse_from_str(head, "%Y-%m-%d").with_context(|| format!("date {s:?}"))
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Normalises one listing record: trimmed text, a stable shape, the files
/// sorted by language, and its hash.
pub fn normalise(raw: &RawRecord, prefix: &str) -> Result<DocRecord> {
    if raw.id.trim().is_empty() {
        bail!("record without an id");
    }
    let issued_on = parse_date(raw.date.as_deref().context("record without a date")?)?;
    let text = clean(&raw.gazette_no_text);
    let (number, sub_number) = match &text {
        Some(t) => match t.split_once('/') {
            Some((n, s)) => (Some(n.trim().to_owned()), Some(s.trim().to_owned())),
            None => (Some(t.clone()), None),
        },
        None => (
            clean(&raw.gazette_no),
            clean(&raw.gazette_sub_no).map(|s| {
                if s.len() < 2 && s.bytes().all(|b| b.is_ascii_digit()) {
                    format!("0{s}")
                } else {
                    s
                }
            }),
        ),
    };
    let base_text = text
        .clone()
        .unwrap_or_else(|| match (&number, &sub_number) {
            (Some(n), Some(s)) => format!("{n}/{s}"),
            (Some(n), None) => n.clone(),
            _ => id_suffix(&raw.id),
        });
    let title = json!({
        "en": clean(&raw.description_english),
        "si": clean(&raw.description_sinhala),
        "ta": clean(&raw.description_tamil),
    });
    let kw = |v: &Option<Value>| match v {
        Some(Value::String(s)) if s.trim().is_empty() => Value::Null,
        Some(v) => v.clone(),
        None => Value::Null,
    };
    let keywords = json!({
        "en": kw(&raw.keywords_english),
        "si": kw(&raw.keywords_sinhala),
        "ta": kw(&raw.keywords_tamil),
    });
    let keywords = keywords
        .as_object()
        .is_some_and(|m| m.values().any(|v| !v.is_null()))
        .then_some(keywords);

    let mut files: Vec<FileRef> = Vec::new();
    for c in &raw.contents {
        let (Some(lang), Some(path)) = (
            c.language.as_deref().and_then(lang_code),
            clean(&c.uploaded_file),
        ) else {
            continue;
        };
        if files.iter().any(|f| f.lang == lang) {
            continue;
        }
        files.push(FileRef {
            lang,
            path,
            name: clean(&c.original_file_name),
        });
    }
    files.sort_by_key(|f| f.lang);

    let updated = clean(&raw.updated_at);
    let mut m = Map::new();
    m.insert("id".into(), json!(raw.id.trim()));
    m.insert("number".into(), json!(text));
    m.insert("gazetteNo".into(), json!(clean(&raw.gazette_no)));
    m.insert("gazetteSubNo".into(), json!(clean(&raw.gazette_sub_no)));
    m.insert("issuedOn".into(), json!(issued_on.to_string()));
    m.insert("title".into(), title.clone());
    m.insert("keywords".into(), keywords.clone().unwrap_or(Value::Null));
    m.insert("updatedAt".into(), json!(updated));
    m.insert(
        "files".into(),
        Value::Array(
            files
                .iter()
                .map(|f| json!({"lang": f.lang, "path": f.path, "name": f.name}))
                .collect(),
        ),
    );
    let record = Value::Object(m);
    let meta_hash = hash_record(&record);
    Ok(DocRecord {
        upstream_id: raw.id.trim().to_owned(),
        number,
        sub_number,
        doc_id_base: doc_id_base(prefix, &base_text),
        issued_on,
        title,
        keywords,
        upstream_updated_at: updated.as_deref().and_then(parse_ts),
        files,
        record,
        meta_hash,
    })
}

/// What one sighting of a record means for the stored document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    New,
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sighting {
    pub kind: Kind,
    /// The document had `withdrawn_at` set and is listed again: clear it.
    pub reappeared: bool,
}

/// `existing` is the stored `(meta_hash, withdrawn)` of the document with
/// this upstream id, if any.
#[must_use]
pub fn classify(existing: Option<(&str, bool)>, new_hash: &str) -> Sighting {
    match existing {
        None => Sighting {
            kind: Kind::New,
            reappeared: false,
        },
        Some((h, withdrawn)) => Sighting {
            kind: if h == new_hash {
                Kind::Unchanged
            } else {
                Kind::Changed
            },
            reappeared: withdrawn,
        },
    }
}

/// The incremental walk stops after a page when it is the last page, empty,
/// or a full page with no new and no changed record (everything newer was
/// already seen). `since_cut` is true when every record of the page is older
/// than `--since`.
#[must_use]
pub fn incremental_stop(
    page: i64,
    total_pages: Option<i64>,
    page_len: usize,
    new_or_changed: usize,
    since_cut: bool,
) -> bool {
    page_len == 0 || since_cut || total_pages.is_some_and(|t| page >= t) || new_or_changed == 0
}

/// A sweep withdraws documents it did not see only when it walked every page
/// and saw at least 90 % of what the upstream reports: a partial or empty
/// listing must never mark the archive withdrawn.
#[must_use]
pub fn may_withdraw(complete: bool, seen: i64, upstream_total: Option<i64>) -> bool {
    complete && seen > 0 && upstream_total.is_none_or(|t| seen * 10 >= t * 9)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::rsc::parse_listing;

    const P1: &str = include_str!("../../tests/fixtures/docs/p1.txt");

    fn fixture() -> Vec<DocRecord> {
        parse_listing(P1)
            .expect("parse")
            .expect("frame")
            .records
            .iter()
            .map(|r| normalise(r, "gze").expect("normalise"))
            .collect()
    }

    #[test]
    fn doc_ids_are_readable() {
        let d = fixture();
        assert_eq!(d[0].doc_id_base, "gze-2515-04");
        assert_eq!(d[2].doc_id_base, "gze-2515-01");
        assert_eq!(d[2].number.as_deref(), Some("2515"));
        assert_eq!(d[2].sub_number.as_deref(), Some("01"));
        assert_eq!(d[2].issued_on.to_string(), "2026-11-16");
        assert_eq!(
            d[2].files
                .iter()
                .map(|f| (f.lang, f.path.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("en", "extra-gazette-content/G 48538 (E)_1794986733.pdf"),
                ("si", "extra-gazette-content/G 48538 (S)_1794986718.pdf"),
                ("ta", "extra-gazette-content/G 48538 (T)_1794986725.pdf"),
            ]
        );
        assert_eq!(doc_id_base("gze", " 2515/04A "), "gze-2515-04a");
    }

    #[test]
    fn a_collision_appends_six_characters_of_the_upstream_id() {
        let base = "gze-2515-04";
        let a = "434c06b1-e28f-49d7-a9fd-8be524519bbe";
        let b = "68c50a90-7e06-46c5-9e4b-4c1ff74122fe";
        assert_eq!(pick_doc_id(base, a, None), base);
        assert_eq!(pick_doc_id(base, a, Some(a)), base);
        assert_eq!(pick_doc_id(base, b, Some(a)), "gze-2515-04-68c50a");
        assert_eq!(id_suffix("68C50A90-7e06"), "68c50a");
    }

    #[test]
    fn the_hash_ignores_key_order_and_whitespace() {
        let a = r#"{"id":"u","gazetteNo":1,"gazetteNoText":"1/02","date":"2020-01-02T00:00:00.000Z","descriptionEnglish":" Title ","updatedAt":"2020-01-03T00:00:00.000Z","contents":[{"language":"ENGLISH","uploadedFile":"x/e.pdf","originalFileName":"e.pdf"},{"language":"SINHALA","uploadedFile":"x/s.pdf","originalFileName":"s.pdf"}]}"#;
        let b = r#"{"contents":[{"originalFileName":"s.pdf","uploadedFile":"x/s.pdf","language":"SINHALA"},{"uploadedFile":"x/e.pdf","language":"ENGLISH","originalFileName":"e.pdf"}],"updatedAt":"2020-01-03T00:00:00.000Z","descriptionEnglish":"Title","date":"2020-01-02T00:00:00.000Z","gazetteNoText":"1/02","gazetteNo":1,"unknown":true,"id":"u"}"#;
        let ra: RawRecord = serde_json::from_str(a).expect("a");
        let rb: RawRecord = serde_json::from_str(b).expect("b");
        let (na, nb) = (
            normalise(&ra, "gze").expect("na"),
            normalise(&rb, "gze").expect("nb"),
        );
        assert_eq!(na.meta_hash, nb.meta_hash);
        assert_eq!(na.meta_hash.len(), 64);
        // a changed field changes the hash
        let mut rc = ra.clone();
        rc.updated_at = Some("2020-01-04T00:00:00.000Z".into());
        assert_ne!(normalise(&rc, "gze").expect("nc").meta_hash, na.meta_hash);
        // and canonical JSON sorts nested keys
        let x: Value =
            serde_json::from_str(r#"{"b":{"d":1,"c":2},"a":[{"z":1,"y":2}]}"#).expect("x");
        assert_eq!(
            canonical_json(&x),
            r#"{"a":[{"y":2,"z":1}],"b":{"c":2,"d":1}}"#
        );
    }

    #[test]
    fn records_without_an_id_or_date_are_refused() {
        let no_date: RawRecord = serde_json::from_str(r#"{"id":"u"}"#).expect("raw");
        assert!(normalise(&no_date, "gze").is_err());
        let no_id: RawRecord = serde_json::from_str(r#"{"date":"2020-01-01"}"#).expect("raw");
        assert!(normalise(&no_id, "gze").is_err());
    }

    #[test]
    fn the_incremental_walk_stops_on_a_quiet_page() {
        // a full page with news: go on
        assert!(!incremental_stop(1, Some(100), 200, 3, false));
        // a full page with nothing new or changed: stop
        assert!(incremental_stop(1, Some(100), 200, 0, false));
        // the last page, an empty page, a page entirely before --since
        assert!(incremental_stop(100, Some(100), 200, 5, false));
        assert!(incremental_stop(2, Some(100), 0, 0, false));
        assert!(incremental_stop(2, Some(100), 200, 5, true));
        // unknown page count: only news keeps it going
        assert!(!incremental_stop(7, None, 200, 1, false));
    }

    #[test]
    fn sightings_set_and_clear_withdrawal() {
        use Kind::*;
        assert_eq!(
            classify(None, "h"),
            Sighting {
                kind: New,
                reappeared: false
            }
        );
        assert_eq!(
            classify(Some(("h", false)), "h"),
            Sighting {
                kind: Unchanged,
                reappeared: false
            }
        );
        assert_eq!(
            classify(Some(("old", false)), "h"),
            Sighting {
                kind: Changed,
                reappeared: false
            }
        );
        // withdrawn and listed again: reappeared, whether or not it changed
        assert_eq!(
            classify(Some(("h", true)), "h"),
            Sighting {
                kind: Unchanged,
                reappeared: true
            }
        );
        assert_eq!(
            classify(Some(("old", true)), "h"),
            Sighting {
                kind: Changed,
                reappeared: true
            }
        );
    }

    #[test]
    fn a_sweep_withdraws_only_after_a_complete_walk() {
        assert!(may_withdraw(true, 39_000, Some(39_478)));
        assert!(!may_withdraw(false, 39_478, Some(39_478)));
        assert!(!may_withdraw(true, 0, Some(100)));
        // saw far less than the upstream reports: something is wrong, keep everything
        assert!(!may_withdraw(true, 1_000, Some(39_478)));
        assert!(may_withdraw(true, 5, None));
    }
}
