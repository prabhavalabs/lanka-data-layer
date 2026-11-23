//! `create_map_link`: layers → an `ldl-map/1` document → an immutable,
//! content-addressed snapshot (the app plane's own create path, with its
//! validation, content id, idempotence and rate limit) → `/m/{id}`.
//!
//! The only tool that writes; annotated `readOnlyHint: false,
//! idempotentHint: true` (the same layers give the same link).

use lanka_core::Catalog;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, Level, check_level, parse, search_hint};
use crate::McpContext;
use crate::out::{Next, Out, ToolError};

pub fn schema() -> Value {
    let layer = json!({
        "type": "object",
        "description": "One layer: an indicator by admin level (choropleth), the admin boundaries of a level (lines), or a features dataset (points, lines or polygons)",
        "properties": {
            "indicator": { "type": "string", "description": "Indicator id (with level)" },
            "level": { "type": ["integer", "string"], "description": "Admin level 0 … 4 (indicator or boundaries)" },
            "boundaries": { "type": ["integer", "string"], "description": "Draw the admin boundaries of this level" },
            "features": { "type": "string", "description": "Features dataset id" },
            "kind": { "type": "string", "description": "features: a feature kind to show, e.g. hospital" },
            "as": { "type": "string", "enum": ["point", "line", "polygon"], "description": "features: how to draw it (default point)" },
            "within": { "type": "string", "description": "features/boundaries: p-code to restrict to" },
            "period": { "type": "string", "description": "indicator: period (default latest)" },
            "dims": { "type": "object", "additionalProperties": { "type": "string" }, "description": "indicator: dimension filter, e.g. {\"sex\": \"total\"}" },
            "name": { "type": "string", "description": "Layer name in the legend" }
        }
    });
    super::schema(
        json!({
            "title": { "type": "string", "maxLength": 120, "description": "Map title" },
            "layers": { "type": "array", "items": layer, "minItems": 1, "maxItems": 12, "description": "Layers, bottom first" },
            "description": { "type": "string", "maxLength": 500 },
            "center": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[lon, lat]; default the island" },
            "zoom": { "type": "number", "minimum": 0, "maximum": 22 }
        }),
        &["title", "layers"],
    )
}

#[derive(Deserialize)]
struct LayerArg {
    indicator: Option<String>,
    level: Option<Level>,
    boundaries: Option<Level>,
    features: Option<String>,
    kind: Option<String>,
    r#as: Option<String>,
    within: Option<String>,
    period: Option<String>,
    dims: Option<serde_json::Map<String, Value>>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct MapArgs {
    title: String,
    layers: Vec<LayerArg>,
    description: Option<String>,
    center: Option<[f64; 2]>,
    zoom: Option<f64>,
}

/// Island view (the web app's default).
const CENTER: [f64; 2] = [80.72, 7.87];
const BOUNDS: [f64; 4] = [79.35, 5.75, 82.15, 10.0];
const ZOOM: f64 = 6.7;

fn base_layer(
    id: &str,
    kind: &str,
    name: &str,
    source: &str,
    opacity: f64,
    style: Value,
    legend: bool,
) -> Value {
    json!({
        "id": id, "type": kind, "name": name, "source": source, "visible": true,
        "opacity": opacity, "blend": "normal", "filter": null, "style": style,
        "legend": { "show": legend, "title": null },
        "interaction": { "hover": true, "select": true },
    })
}

pub async fn create_map_link(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    caller: &Caller,
) -> Result<Out, ToolError> {
    let a: MapArgs = parse("create_map_link", args)?;
    let Some(app) = ctx.app.clone() else {
        return Err(ToolError::unavailable(
            "maps are not hosted on this server (the app plane is off)",
        ));
    };
    let title = a.title.trim();
    if title.is_empty() {
        return Err(ToolError::bad("title must not be empty"));
    }
    if a.layers.is_empty() {
        return Err(ToolError::bad("give at least one layer"));
    }
    let mut sources = serde_json::Map::new();
    let mut layers: Vec<Value> = Vec::new();
    let mut datasets: Vec<String> = Vec::new();
    let mut described: Vec<String> = Vec::new();
    let mut legend_used = false;
    for (n, l) in a.layers.iter().enumerate() {
        let picked = usize::from(l.indicator.is_some())
            + usize::from(l.boundaries.is_some())
            + usize::from(l.features.is_some());
        if picked != 1 {
            return Err(ToolError::bad(format!(
                "layers[{n}]: give exactly one of `indicator` (with `level`), `boundaries` (a level) or `features` (a dataset id)"
            )));
        }
        let sid = format!("s{n}");
        let lid = format!("l{n}");
        if let Some(ind) = l.indicator.as_deref() {
            let ind = ind.trim();
            let Some(info) = cat.indicators.get(ind) else {
                return Err(ToolError::new(
                    "not_found",
                    format!("not_found: layers[{n}]: no indicator \"{ind}\""),
                )
                .next(search_hint(
                    &ind.rsplit('.').next().unwrap_or(ind).replace('_', " "),
                )));
            };
            let Some(level) = l.level else {
                return Err(ToolError::bad(format!(
                    "layers[{n}]: an indicator layer needs `level`"
                ))
                .hints(json!({ "levels": info.geo_levels })));
            };
            check_level(cat, level.0)?;
            let word = crate::out::level_word(i64::from(level.0));
            if !info.geo_levels.contains(&word) {
                return Err(ToolError::bad(format!(
                    "layers[{n}]: {ind} is not published at level {} ({word})",
                    level.0
                ))
                .hints(json!({ "available_levels": info.geo_levels })));
            }
            let mut src = json!({
                "kind": "indicator", "indicator": ind, "level": level.0,
                "period": l.period.as_deref().map_or_else(|| json!("latest"), |p| json!(p)),
            });
            if let Some(d) = &l.dims {
                src["dims"] = Value::Object(d.clone());
            }
            sources.insert(sid.clone(), src);
            let name = l.name.clone().unwrap_or_else(|| info.label.clone());
            layers.push(base_layer(
                &lid, "choropleth", &name, &sid, 0.9,
                json!({
                    "fill": { "field": "value", "scale": { "type": "quantile", "classes": 5, "ramp": "ldl.seq.violet" } },
                    "stroke": { "color": "ldl.tone.paper", "width": 0.5 },
                    "uncertainty": "off",
                }),
                !legend_used,
            ));
            legend_used = true;
            datasets.push(info.dataset_id.clone());
            described.push(format!("{name} ({ind}, level {})", level.0));
        } else if let Some(level) = l.boundaries {
            check_level(cat, level.0)?;
            let mut src = json!({ "kind": "boundaries", "scheme": cat.profile()?.admin_scheme(), "level": level.0 });
            if let Some(w) = l.within.as_deref().filter(|w| !w.trim().is_empty()) {
                src["parent"] = json!(w.trim());
            }
            sources.insert(sid.clone(), src);
            let name = l
                .name
                .clone()
                .unwrap_or_else(|| format!("Admin level {}", level.0));
            layers.push(base_layer(
                &lid,
                "line",
                &name,
                &sid,
                0.6,
                json!({ "stroke": { "color": "ldl.tone.ink", "width": 1.2 } }),
                false,
            ));
            datasets.extend(cat.boundary_dataset().map(str::to_owned));
            described.push(format!("{name} (boundaries, level {})", level.0));
        } else if let Some(ds) = l.features.as_deref() {
            let ds = ds.trim();
            let Some(info) = cat.datasets.get(ds) else {
                return Err(ToolError::new(
                    "not_found",
                    format!("not_found: layers[{n}]: no dataset \"{ds}\""),
                )
                .next(search_hint(ds)));
            };
            if info.kind != "features" {
                return Err(ToolError::bad(format!(
                    "layers[{n}]: dataset {ds} is of kind {:?}, not \"features\"",
                    info.kind
                )));
            }
            let mut src = json!({ "kind": "features", "dataset": ds });
            if let Some(k) = l.kind.as_deref().filter(|k| !k.trim().is_empty()) {
                src["featureKind"] = json!(k.trim());
            }
            if let Some(w) = l.within.as_deref().filter(|w| !w.trim().is_empty()) {
                src["within"] = json!(w.trim());
            }
            sources.insert(sid.clone(), src);
            let name = l.name.clone().unwrap_or_else(|| info.title.clone());
            let (kind, style) = match l.r#as.as_deref().unwrap_or("point") {
                "point" => (
                    "point",
                    json!({
                        "size": 3.5, "fill": "ldl.tone.ink",
                        "stroke": { "color": "ldl.tone.paper", "width": 1.0 },
                    }),
                ),
                "line" => (
                    "line",
                    json!({ "stroke": { "color": "ldl.tone.ink", "width": 1.2 } }),
                ),
                "polygon" => (
                    "polygon",
                    json!({
                        "fill": "ldl.tone.ink", "stroke": { "color": "ldl.tone.ink", "width": 0.8 },
                    }),
                ),
                other => {
                    return Err(ToolError::bad(format!("layers[{n}]: as {other:?}"))
                        .hints(json!({ "valid": ["point", "line", "polygon"] })));
                }
            };
            layers.push(base_layer(&lid, kind, &name, &sid, 0.85, style, false));
            datasets.push(ds.to_owned());
            described.push(format!("{name} (features {ds})"));
        }
    }
    let center = a.center.unwrap_or(CENTER);
    let mut view = json!({ "center": center, "zoom": a.zoom.unwrap_or(ZOOM) });
    if a.center.is_none() && a.zoom.is_none() {
        view["bounds"] = json!(BOUNDS);
    }
    let mut doc = json!({
        "spec": "ldl-map/1",
        "title": title,
        "pin": "latest",
        "view": view,
        "extent": null,
        "basemap": { "style": "ldl-auto", "labels": "off", "hillshade": { "on": false, "exaggeration": 1 } },
        "time": null,
        "sources": sources,
        "analyses": {},
        "layers": layers,
        "panels": [{ "id": "p_legend", "type": "legend" }],
        "meta": { "generator": "lanka-mcp" },
    });
    if let Some(d) = a.description.as_deref().filter(|d| !d.trim().is_empty()) {
        doc["description"] = json!(d.trim());
    }

    let ip = caller
        .ip
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let (created, was_new) = app
        .create_snapshot(&doc, app.rate_identity_for(ip))
        .await
        .map_err(|f| {
            let mut e = ToolError::new(
                match f.kind {
                    "rate_limited" => "rate_limited",
                    "invalid_document" | "too_large" => "bad_request",
                    "withdrawn" => "not_found",
                    _ => "internal",
                },
                f.title.clone(),
            );
            if !f.problems.is_empty() {
                e.hints = json!({ "problems": f.problems });
            }
            e.retry_after_s = f.retry_after_s;
            e
        })?;
    let text = format!(
        "MAP LINK {}\n{}\nlayers: {}\n{} (the same layers always give this link; maps are immutable and public to anyone with the link)\nembed: {}\ndocument: {}\n",
        created.url,
        title,
        described.join("; "),
        if was_new {
            "created"
        } else {
            "already existed"
        },
        created.embed,
        created.document,
    );
    Ok(Out::new(
        json!({
            "id": created.id, "url": created.url, "embed": created.embed, "document": created.document,
            "created": was_new, "bytes": created.bytes, "layers": described,
        }),
        text,
    )
    .datasets(datasets)
    .next(Next::new("get_area_profile", json!({ "geo": "LK" }), "continue exploring")))
}
