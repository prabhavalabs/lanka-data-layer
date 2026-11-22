//! TypeScript declarations generated from the map schema, published as
//! `@lanka-data-layer/map-spec` (`spec/map/1/index.d.ts`, `lanka map types`).
//!
//! A small JSON-Schema → TypeScript translation for the constructs the map
//! schema uses: `$ref`, `const`, `enum`, primitive and nullable types,
//! `anyOf`, `prefixItems` tuples, arrays, objects with optional members,
//! `additionalProperties` records, the `^x-` pattern (a template-literal
//! index signature) and `if/then` discriminators (unions of the `then`
//! references). Constraint-only parts (`required` alternatives, numeric
//! bounds, patterns) have no TypeScript form and are left to the validator.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::validate::SCHEMA_JSON;

fn type_name(def: &str) -> String {
    match def {
        "node" => "LayerNode".to_owned(),
        "group" => "LayerGroup".to_owned(),
        "cql2" => "Cql2".to_owned(),
        "bbox" => "BBox".to_owned(),
        "id" => "LocalId".to_owned(),
        _ => {
            let mut c = def.chars();
            c.next()
                .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        }
    }
}

fn lit(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "unknown".to_owned())
}

fn key(k: &str) -> String {
    let ident = k
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ident {
        k.to_owned()
    } else {
        lit(&Value::String(k.to_owned()))
    }
}

fn doc_comment(s: &Map<String, Value>, indent: &str) -> String {
    s.get("description")
        .and_then(Value::as_str)
        .map(|d| format!("{indent}/** {} */\n", d.replace("*/", "* /")))
        .unwrap_or_default()
}

fn union(parts: Vec<String>) -> String {
    let mut seen: Vec<String> = Vec::new();
    for p in parts {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    if seen.is_empty() {
        "never".to_owned()
    } else {
        seen.join(" | ")
    }
}

/// `if/then` branches whose `then` is a `$ref`: the discriminated union.
fn branch_refs(s: &Map<String, Value>) -> Option<Vec<String>> {
    let all = s.get("allOf")?.as_array()?;
    all.iter()
        .map(|b| b.get("then")?.get("$ref")?.as_str().map(ref_name))
        .collect()
}

fn ref_name(r: &str) -> String {
    type_name(r.rsplit('/').next().unwrap_or(r))
}

fn ts(v: &Value, indent: &str) -> String {
    let Some(s) = v.as_object() else {
        return if v == &Value::Bool(false) {
            "never".to_owned()
        } else {
            "unknown".to_owned()
        };
    };
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        return ref_name(r);
    }
    if let Some(c) = s.get("const") {
        return lit(c);
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array) {
        return union(e.iter().map(lit).collect());
    }
    if let (Some(t), Some(e)) = (s.get("then"), s.get("else")) {
        return union(vec![ts(t, indent), ts(e, indent)]);
    }
    if let Some(refs) = branch_refs(s) {
        return union(refs);
    }
    if let Some(any) = s.get("anyOf").and_then(Value::as_array) {
        let typed: Vec<&Value> = any
            .iter()
            .filter(|a| {
                !(a.as_object()
                    .is_some_and(|o| o.len() == 1 && o.contains_key("required")))
            })
            .collect();
        if !typed.is_empty() && !s.contains_key("properties") {
            return union(typed.into_iter().map(|a| ts(a, indent)).collect());
        }
    }
    match s.get("type") {
        Some(Value::Array(ts_)) => union(
            ts_.iter()
                .map(|t| {
                    let mut o = s.clone();
                    o.insert("type".to_owned(), t.clone());
                    ts(&Value::Object(o), indent)
                })
                .collect(),
        ),
        Some(Value::String(t)) => match t.as_str() {
            "string" => "string".to_owned(),
            "number" | "integer" => "number".to_owned(),
            "boolean" => "boolean".to_owned(),
            "null" => "null".to_owned(),
            "array" => {
                if let Some(items) = s.get("prefixItems").and_then(Value::as_array) {
                    format!(
                        "[{}]",
                        items
                            .iter()
                            .map(|i| ts(i, indent))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    let item = s
                        .get("items")
                        .map_or_else(|| "unknown".to_owned(), |i| ts(i, indent));
                    if item.contains(' ') {
                        format!("({item})[]")
                    } else {
                        format!("{item}[]")
                    }
                }
            }
            "object" => object(s, indent),
            _ => "unknown".to_owned(),
        },
        _ => "unknown".to_owned(),
    }
}

fn object(s: &Map<String, Value>, indent: &str) -> String {
    let inner = format!("{indent}  ");
    let props = s.get("properties").and_then(Value::as_object);
    let required: Vec<&str> = s
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let x = s
        .get("patternProperties")
        .and_then(Value::as_object)
        .is_some_and(|p| p.contains_key("^x-"));
    let extra = s.get("additionalProperties").filter(|a| a.is_object());
    if props.is_none_or(Map::is_empty) && !x {
        return match extra {
            Some(a) => format!("Record<string, {}>", ts(a, indent)),
            None => "Record<string, unknown>".to_owned(),
        };
    }
    let mut out = String::from("{\n");
    for (k, v) in props.into_iter().flatten() {
        if let Some(o) = v.as_object() {
            out.push_str(&doc_comment(o, &inner));
        }
        let opt = if required.contains(&k.as_str()) {
            ""
        } else {
            "?"
        };
        let _ = writeln!(out, "{inner}{}{opt}: {}", key(k), ts(v, &inner));
    }
    let open = s
        .get("additionalProperties")
        .is_none_or(|a| a == &Value::Bool(true));
    if open {
        let _ = writeln!(out, "{inner}[k: string]: unknown");
    } else if x {
        let _ = writeln!(
            out,
            "{inner}/** Extensions: readers ignore them and preserve them on save. */"
        );
        let _ = writeln!(out, "{inner}[x: `x-${{string}}`]: unknown");
    }
    out.push_str(indent);
    out.push('}');
    out
}

/// The declarations, as committed in `spec/map/1/index.d.ts`.
#[must_use]
pub fn declarations() -> String {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).unwrap_or(Value::Null);
    let mut out = String::from(
        "// @lanka-data-layer/map-spec — TypeScript types of ldl-map/1.\n\
         // GENERATED from map.schema.json by `lanka map types`; do not edit.\n\
         // The schema is normative; the reference validator (`lanka map validate`)\n\
         // also checks limits and references these types cannot express.\n\n\
         export declare const MAP_SPEC: 'ldl-map/1'\n\n",
    );
    if let Some(root) = schema.as_object() {
        out.push_str(&doc_comment(root, ""));
        let _ = writeln!(out, "export type MapDocument = {}\n", object(root, ""));
    }
    if let Some(defs) = schema.get("$defs").and_then(Value::as_object) {
        for (name, d) in defs {
            if let Some(o) = d.as_object() {
                out.push_str(&doc_comment(o, ""));
            }
            let _ = writeln!(out, "export type {} = {}\n", type_name(name), ts(d, ""));
        }
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_declarations_are_current() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../spec/map/1/index.d.ts");
        let Ok(committed) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: {} not present", path.display());
            return;
        };
        assert!(
            committed == declarations(),
            "spec/map/1/index.d.ts is stale: run `lanka map types > spec/map/1/index.d.ts`"
        );
    }

    #[test]
    fn shapes() {
        let d = declarations();
        assert!(d.contains("export type MapDocument = {"));
        assert!(d.contains("export type Source = IndicatorSource | BoundariesSource | FeaturesSource | GridSource | AnalysisSource | InlineSource | ExternalSource"));
        assert!(d.contains("export type LayerNode = LayerGroup | Layer"));
        assert!(d.contains("[x: `x-${string}`]: unknown"));
    }
}
