//! Canonical JSON (RFC 8785, JCS) and the document content hash (10 §5).
//!
//! JCS: object members sorted by their UTF-16 code units, no whitespace,
//! strings escaped as ECMAScript `JSON.stringify` does, numbers written as
//! ECMAScript `Number.prototype.toString` (shortest round-trip digits,
//! exponent form outside `1e-7 < |x| < 1e21`). Every number is treated as
//! an IEEE-754 double, as JCS requires.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// RFC 8785 canonical form of `v`.
#[must_use]
pub fn jcs(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&es_number(n.as_f64().unwrap_or(f64::NAN))),
        Value::String(s) => write_str(s, out),
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
        Value::Object(m) => {
            let mut keys: Vec<(&String, Vec<u16>)> =
                m.keys().map(|k| (k, k.encode_utf16().collect())).collect();
            keys.sort_by(|a, b| a.1.cmp(&b.1));
            out.push('{');
            for (i, (k, _)) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write(&m[*k], out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript `Number.prototype.toString(10)` of a finite double.
#[must_use]
pub fn es_number(x: f64) -> String {
    if !x.is_finite() {
        return "null".to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    // Rust's `{:e}` prints the shortest round-trip digits: "d.ddde±x".
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let n = exp + 1;
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if k <= n && n <= 21 {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(n - k).unwrap_or(0))
        )
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(usize::try_from(n).unwrap_or(0));
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(usize::try_from(-n).unwrap_or(0)))
    } else {
        let e = n - 1;
        let es = if e < 0 {
            format!("-{}", -e)
        } else {
            format!("+{e}")
        };
        if k == 1 {
            format!("{digits}e{es}")
        } else {
            format!("{}.{}e{es}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

fn strip_nulls(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            m.values_mut().for_each(strip_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

/// The document without what is not content: `id`, `meta.updated_at` and
/// `null` members (a `null` member and an absent one mean the same).
#[must_use]
pub fn content_value(doc: &Value) -> Value {
    let mut v = doc.clone();
    strip_nulls(&mut v);
    if let Some(o) = v.as_object_mut() {
        o.remove("id");
        if let Some(m) = o.get_mut("meta").and_then(Value::as_object_mut) {
            m.remove("updated_at");
        }
    }
    v
}

/// sha256 of the canonical content (10 §5).
#[must_use]
pub fn content_sha256(doc: &Value) -> [u8; 32] {
    Sha256::digest(jcs(&content_value(doc)).as_bytes()).into()
}

/// Snapshot id: `ms_` + the first 16 hex of the content hash.
#[must_use]
pub fn snapshot_id(doc: &Value) -> String {
    format!("ms_{}", &hex::encode(content_sha256(doc))[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_follow_ecmascript() {
        for (x, s) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (9.0, "9"),
            (0.85, "0.85"),
            (-1.5, "-1.5"),
            (2024.0, "2024"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (123_456_789_012.0, "123456789012"),
            (5e-324, "5e-324"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157e+308"),
            (0.1 + 0.2, "0.30000000000000004"),
        ] {
            assert_eq!(es_number(x), s, "{x}");
        }
    }

    #[test]
    fn rfc8785_ordering_and_escapes() {
        // RFC 8785 §3.2.3 sorting example (keys by UTF-16 code units).
        let v = json!({"\u{20ac}": "Euro Sign", "\r": "Carriage Return", "\u{fb33}": "Hebrew Letter Dalet With Dagesh",
                       "1": "One", "\u{1f600}": "Emoji: Grinning Face", "\u{0080}": "Control", "\u{00f6}": "Latin Small Letter O With Diaeresis"});
        let keys: Vec<String> = match serde_json::from_str::<Value>(&jcs(&v)) {
            Ok(Value::Object(m)) => m.keys().cloned().collect(),
            _ => Vec::new(),
        };
        assert!(!keys.is_empty());
        let s = jcs(&v);
        let order = [
            "\\r",
            "1",
            "\u{0080}",
            "\u{00f6}",
            "\u{20ac}",
            "\u{1f600}",
            "\u{fb33}",
        ];
        let pos: Vec<usize> = order
            .iter()
            .map(|k| s.find(k).unwrap_or(usize::MAX))
            .collect();
        assert!(pos.windows(2).all(|w| w[0] < w[1]), "{s}");
        assert_eq!(
            jcs(&json!({"b": [1, 2.5, "x\u{1}\"y"], "a": null})),
            r#"{"a":null,"b":[1,2.5,"x\u0001\"y"]}"#
        );
    }

    #[test]
    fn hash_ignores_id_and_updated_at() {
        let a = json!({"spec": "ldl-map/1", "id": "m_aaaaaaaa", "meta": {"updated_at": "2026-01-01T00:00:00Z", "author": "x"}});
        let b = json!({"spec": "ldl-map/1", "meta": {"author": "x", "updated_at": "2027-01-01T00:00:00Z"}});
        assert_eq!(snapshot_id(&a), snapshot_id(&b));
        let c = json!({"spec": "ldl-map/1", "meta": {"author": "y"}});
        assert_ne!(snapshot_id(&a), snapshot_id(&c));
    }
}
