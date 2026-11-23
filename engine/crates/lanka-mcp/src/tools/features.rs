//! `find_features`: features inside an admin unit or bounding box, or the
//! nearest to a point — `lanka_core::features`, as `/v2/features` and
//! `/v2/nearest`. Geometry is left out (representative points only): a
//! result is meant to be read; `/v2/features/{dataset}/{key}` has the shape.

use lanka_core::Catalog;
use lanka_core::features::{self, FeatureQuery, GeoFeature, NearestQuery};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, StringList, parse, schema, search_hint};
use crate::McpContext;
use crate::out::{Next, Out, ToolError, num};

const DEFAULT_LIMIT: i64 = 25;
const MAX_LIMIT: i64 = 100;

pub fn find_schema() -> Value {
    schema(
        json!({
            "dataset": { "type": "string", "description": "A features dataset id, e.g. osm-pois-amenities (search_catalog finds them: kind \"features\")" },
            "within": { "type": "string", "description": "p-code: features in this admin unit or below it" },
            "bbox": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "[west, south, east, north] in WGS84" },
            "near": {
                "type": "object",
                "description": "The nearest features to a point (at most 50, within radius_km ≤ 50)",
                "properties": {
                    "lat": { "type": "number" }, "lon": { "type": "number" },
                    "radius_km": { "type": "number", "minimum": 0.1, "maximum": 50 }
                },
                "required": ["lat", "lon"]
            },
            "kind": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Feature kinds, e.g. [\"hospital\", \"clinic\"]" },
            "q": { "type": "string", "description": "Name contains (case-insensitive); not with near" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 25" },
            "cursor": { "type": "integer", "description": "next_cursor of the previous page" }
        }),
        &["dataset"],
    )
}

#[derive(Deserialize)]
struct Near {
    lat: f64,
    lon: f64,
    radius_km: Option<f64>,
}

#[derive(Deserialize)]
struct FindArgs {
    dataset: String,
    within: Option<String>,
    bbox: Option<Vec<f64>>,
    near: Option<Near>,
    kind: Option<StringList>,
    q: Option<String>,
    limit: Option<i64>,
    cursor: Option<i64>,
}

fn row(f: &GeoFeature) -> Value {
    let p = &f.properties;
    let mut attrs = p.attrs.clone();
    // keep long free-text attributes short: the full record is one REST call away
    for v in attrs.values_mut() {
        if let Value::String(s) = v
            && s.chars().count() > 200
        {
            *s = format!("{}…", s.chars().take(200).collect::<String>());
        }
    }
    json!({
        "key": p.key, "kind": p.kind, "name": p.name, "name_si": p.name_si, "name_ta": p.name_ta,
        "unit": p.unit, "lon": p.rep[0], "lat": p.rep[1], "distance_m": f.distance_m,
        "flags": p.flags, "attributes": attrs,
    })
}

pub async fn find_features(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: FindArgs = parse("find_features", args)?;
    let dataset = a.dataset.trim().to_owned();
    let kinds: Vec<String> = a.kind.map(|k| k.0).unwrap_or_default();
    let limit = a.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let not_found = |e: lanka_core::CoreError| ToolError::from(e).next(search_hint(&dataset));

    let (features, next_cursor): (Vec<GeoFeature>, Option<i64>) = if let Some(n) = &a.near {
        if a.within.is_some() || a.bbox.is_some() || a.q.is_some() || a.cursor.is_some() {
            return Err(ToolError::bad(
                "`near` finds the nearest features to a point; do not combine it with within, bbox, q or cursor",
            ));
        }
        let hits = features::nearest(
            &ctx.pool,
            cat,
            &NearestQuery {
                lat: n.lat,
                lon: n.lon,
                datasets: vec![dataset.clone()],
                kinds: kinds.clone(),
                limit: Some(limit.min(features::MAX_NEAREST)),
                radius_km: n.radius_km,
            },
        )
        .await
        .map_err(not_found)?;
        (hits.into_iter().map(|h| h.feature).collect(), None)
    } else {
        let bbox = match a.bbox.as_deref() {
            None => None,
            Some(b) => Some(<[f64; 4]>::try_from(b).map_err(|_| {
                ToolError::bad("bbox must be four numbers: [west, south, east, north]")
            })?),
        };
        let fc = features::features(
            &ctx.pool,
            cat,
            &dataset,
            &FeatureQuery {
                bbox,
                within: a.within.clone().filter(|s| !s.trim().is_empty()),
                kinds: kinds.clone(),
                q: a.q.clone().filter(|s| !s.trim().is_empty()),
                cursor: a.cursor,
                limit: Some(limit),
            },
        )
        .await
        .map_err(not_found)?;
        (fc.features, fc.next_cursor)
    };

    let rows: Vec<Value> = features.iter().map(row).collect();
    let mut text = format!(
        "{} feature(s) of {}{}{}{}\n",
        rows.len(),
        dataset,
        a.within
            .as_deref()
            .map(|w| format!(" within {w}"))
            .unwrap_or_default(),
        a.near
            .as_ref()
            .map(|n| format!(" nearest to {}, {}", n.lat, n.lon))
            .unwrap_or_default(),
        if kinds.is_empty() {
            String::new()
        } else {
            format!(", kind {}", kinds.join("/"))
        },
    );
    for f in features.iter().take(40) {
        let p = &f.properties;
        text.push_str(&format!(
            "- {} [{}] {} — {} ({}, {}){}\n",
            p.key,
            p.kind,
            p.name.as_deref().unwrap_or("(unnamed)"),
            p.unit.as_deref().unwrap_or("no unit"),
            num(p.rep[1]),
            num(p.rep[0]),
            f.distance_m
                .map(|d| format!(", {} m away", num(d)))
                .unwrap_or_default(),
        ));
    }
    if rows.len() > 40 {
        text.push_str(&format!(
            "… {} more in the structured content\n",
            rows.len() - 40
        ));
    }
    if rows.is_empty() {
        text.push_str(
            "No features matched: widen `within`/`bbox`, drop `kind`, or raise radius_km.\n",
        );
    }
    text.push_str("Geometry is omitted: each feature has its representative point; GET /v2/features/{dataset}/{key} returns the shape.\n");

    let mut out = Out::new(
        json!({
            "dataset": dataset, "features": rows, "number_returned": features.len(),
            "next_cursor": next_cursor,
        }),
        text,
    )
    .datasets([dataset.clone()])
    .datasets(cat.boundary_dataset().map(str::to_owned));
    if let Some(c) = next_cursor {
        out = out.next(Next::new(
            "find_features",
            json!({ "dataset": dataset, "within": a.within, "bbox": a.bbox, "kind": kinds, "q": a.q, "limit": limit, "cursor": c }),
            "the next page",
        ));
    }
    if let Some(u) = features.iter().find_map(|f| f.properties.unit.clone()) {
        out = out.next(Next::new(
            "get_area_profile",
            json!({ "geo": u }),
            "the area of the first feature",
        ));
    }
    Ok(out)
}
