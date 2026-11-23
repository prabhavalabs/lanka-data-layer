//! `resolve_place` and `get_area_profile`.

use lanka_core::Catalog;
use lanka_core::places::{self, PlaceQuery};
use lanka_core::query::{self, AreaIndicator};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, Level, StringList, check_level, parse, schema};
use crate::McpContext;
use crate::out::{
    MAX_ROWS, Next, Out, ToolError, cap_rows, is_estimate_basis, is_estimate_indicator, level_word,
    num, value_text,
};

fn level_label(cat: &Catalog, level: i16) -> String {
    let names = cat
        .profile
        .as_deref()
        .and_then(|p| {
            p.spec
                .admin
                .levels
                .get(usize::try_from(level).unwrap_or(usize::MAX))
        })
        .map(|l| l.name.en.clone());
    names.unwrap_or_else(|| level_word(i64::from(level)))
}

// ───────────────────────────── resolve_place ───────────────────────────────

pub fn resolve_schema() -> Value {
    schema(
        json!({
            "query": { "type": "string", "description": "A place name in English, Sinhala or Tamil (\"Kandy\", \"මහනුවර\"), or a p-code. Use this OR lat/lon." },
            "lat": { "type": "number", "minimum": -90, "maximum": 90, "description": "Latitude (WGS84); with lon" },
            "lon": { "type": "number", "minimum": -180, "maximum": 180, "description": "Longitude (WGS84); with lat" },
            "level": { "type": ["integer", "string"], "description": "Name search only: restrict to one admin level (0 national … 4 GN division)" },
            "parent": { "type": "string", "description": "Name search only: restrict to units below this p-code" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 25, "description": "Candidates (default 8)" }
        }),
        &[],
    )
}

#[derive(Deserialize)]
struct ResolveArgs {
    query: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    level: Option<Level>,
    parent: Option<String>,
    limit: Option<i64>,
}

fn unit_json(u: &query::AreaRef) -> Value {
    json!({ "pcode": u.pcode, "level": u.level, "name_en": u.name_en, "name_si": u.name_si, "name_ta": u.name_ta })
}

pub async fn resolve_place(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: ResolveArgs = parse("resolve_place", args)?;
    let named = a.query.as_deref().is_some_and(|q| !q.trim().is_empty());
    let pointed = a.lat.is_some() || a.lon.is_some();
    if named == pointed {
        return Err(ToolError::bad(
            "give either `query` (a place name or p-code) or both `lat` and `lon`",
        ));
    }
    let mut candidates: Vec<Value> = Vec::new();
    let mut text = String::new();
    let top: Option<String>;
    if pointed {
        let (Some(lat), Some(lon)) = (a.lat, a.lon) else {
            return Err(ToolError::bad("lat and lon are both required"));
        };
        if !(lat.is_finite() && lon.is_finite()) {
            return Err(ToolError::bad(
                "lat and lon must be finite numbers (decimal degrees)",
            ));
        }
        let hit = match ctx.reverse.as_ref().and_then(|f| f(lat, lon)) {
            Some(r) => r,
            None => query::reverse(&ctx.pool, cat, lat, lon).await?,
        };
        text.push_str(&format!(
            "POINT {lat}, {lon} → {} ({}), {}\n",
            hit.unit.name_en,
            hit.unit.pcode,
            level_label(cat, hit.unit.level)
        ));
        text.push_str(&format!(
            "chain: {}\nmatch: {}{}; grid cell {}\n",
            chain(&hit.ancestors, Some(&hit.unit)),
            hit.match_kind,
            hit.distance_m
                .map(|d| format!(" ({} m from the unit)", num(d)))
                .unwrap_or_default(),
            hit.cell_id
        ));
        top = Some(hit.unit.pcode.clone());
        candidates.push(json!({
            "pcode": hit.unit.pcode, "level": hit.unit.level, "level_name": level_label(cat, hit.unit.level),
            "name_en": hit.unit.name_en, "name_si": hit.unit.name_si, "name_ta": hit.unit.name_ta,
            "ancestors": hit.ancestors.iter().map(unit_json).collect::<Vec<_>>(),
            "confidence": if hit.match_kind == "contains" { 1.0 } else { 0.8 },
            "match": hit.match_kind, "distance_m": hit.distance_m, "cell_id": hit.cell_id,
        }));
    } else {
        let q = a.query.clone().unwrap_or_default();
        if let Some(l) = a.level {
            check_level(cat, l.0)?;
        }
        let found = places::place_search(
            &ctx.pool,
            cat,
            &PlaceQuery {
                q: q.clone(),
                level: a.level.map(|l| l.0),
                parent: a.parent.clone(),
                limit: a.limit,
            },
        )
        .await?;
        text.push_str(&format!("{} candidate(s) for {:?}\n", found.len(), q));
        for (n, m) in found.iter().enumerate() {
            text.push_str(&format!(
                "{}. {} — {} (level {} {}), confidence {} [{}]; {}\n",
                n + 1,
                m.unit.pcode,
                m.unit.name_en,
                m.unit.level,
                level_label(cat, m.unit.level),
                num(m.confidence),
                m.match_kind,
                chain(&m.ancestors, None),
            ));
            candidates.push(json!({
                "pcode": m.unit.pcode, "level": m.unit.level, "level_name": level_label(cat, m.unit.level),
                "name_en": m.unit.name_en, "name_si": m.unit.name_si, "name_ta": m.unit.name_ta,
                "ancestors": m.ancestors.iter().map(unit_json).collect::<Vec<_>>(),
                "confidence": m.confidence, "match": m.match_kind,
            }));
        }
        if found.is_empty() {
            text.push_str("No unit matched. Check the spelling, try the other script, or search by a larger place (district) and use `parent`.\n");
        } else if found.len() > 1 && found[0].confidence < 1.0 {
            text.push_str("Several candidates: ask the user which is meant when the level or parent is unclear.\n");
        }
        top = found.first().map(|m| m.unit.pcode.clone());
    }
    let mut out = Out::new(json!({ "candidates": candidates }), text);
    out = out.datasets(cat.boundary_dataset().map(str::to_owned));
    if let Some(p) = top {
        out = out
            .next(Next::new(
                "get_area_profile",
                json!({ "geo": p }),
                "the latest value of every indicator for this area",
            ))
            .next(Next::new(
                "search_catalog",
                json!({ "query": "population" }),
                "find an indicator to query for this area",
            ));
    }
    Ok(out)
}

fn chain(ancestors: &[query::AreaRef], own: Option<&query::AreaRef>) -> String {
    let mut parts: Vec<String> = ancestors
        .iter()
        .map(|a| format!("{} ({})", a.name_en, a.pcode))
        .collect();
    if let Some(o) = own {
        parts.push(format!("{} ({})", o.name_en, o.pcode));
    }
    parts.join(" > ")
}

// ───────────────────────────── get_area_profile ────────────────────────────

pub fn profile_schema() -> Value {
    schema(
        json!({
            "geo": { "type": "string", "description": "p-code: LK (country), LK1 (province), LK11 (district), LK1103 (DS division), LK1103005 (GN division)" },
            "themes": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Only indicators of datasets in these themes (ECON, SOCI, …)" },
            "classes": { "type": ["array", "string"], "items": { "type": "string", "enum": ["official", "third_party", "derived", "estimated"] }, "description": "Only these indicator classes" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 500, "description": "Maximum values returned (default 200)" }
        }),
        &["geo"],
    )
}

#[derive(Deserialize)]
struct ProfileArgs {
    geo: String,
    themes: Option<StringList>,
    classes: Option<StringList>,
    limit: Option<usize>,
}

const CLASS_ORDER: [&str; 4] = ["official", "third_party", "derived", "estimated"];

fn themes_of(cat: &Catalog, dataset_id: &str) -> Vec<String> {
    let mut t: Vec<String> = cat
        .datasets
        .get(dataset_id)
        .map(|d| {
            d.source_ids
                .iter()
                .filter_map(|s| cat.sources.get(s))
                .flat_map(|s| s.themes.iter().cloned())
                .collect()
        })
        .unwrap_or_default();
    t.sort();
    t.dedup();
    t
}

fn indicator_json(i: &AreaIndicator) -> Value {
    json!({
        "indicator_id": i.indicator_id, "label": i.label, "unit": i.unit, "unit_mult": i.unit_mult,
        "class": i.class, "dataset": i.dataset_id, "period": i.period, "completed_from": i.completed_from,
        "values": i.values.iter().map(|v| json!({
            "dims": v.dims, "value": v.value, "lo": v.lo, "hi": v.hi,
            "basis": v.basis, "status": v.status, "flags": v.flags, "obs_id": v.obs_id,
        })).collect::<Vec<_>>(),
    })
}

pub async fn get_area_profile(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: ProfileArgs = parse("get_area_profile", args)?;
    let pcode = query::parse_pcode(cat, a.geo.trim()).map_err(|e| {
        ToolError::from(e).next(Next::new(
            "resolve_place",
            json!({ "query": a.geo }),
            "find the p-code for a name",
        ))
    })?;
    let (p, touched) = query::area_profile(&ctx.pool, cat, &pcode).await?;
    let classes: Vec<String> = a
        .classes
        .map(|c| c.0.into_iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    if let Some(bad) = classes.iter().find(|c| !CLASS_ORDER.contains(&c.as_str())) {
        return Err(ToolError::bad(format!("class {bad:?}")).hints(json!({ "valid": CLASS_ORDER })));
    }
    let themes: Vec<String> = a
        .themes
        .map(|t| t.0.into_iter().map(|s| s.to_uppercase()).collect())
        .unwrap_or_default();
    let limit = a.limit.unwrap_or(200).clamp(1, MAX_ROWS);

    let kept: Vec<&AreaIndicator> = p
        .indicators
        .iter()
        .filter(|i| classes.is_empty() || classes.contains(&i.class))
        .filter(|i| {
            themes.is_empty()
                || themes_of(cat, &i.dataset_id)
                    .iter()
                    .any(|t| themes.contains(t))
        })
        .collect();

    // Group by class, official first; cap the number of values.
    let mut budget = limit;
    let mut groups = serde_json::Map::new();
    let mut shown_ids: Vec<&str> = Vec::new();
    let mut dropped = 0usize;
    let mut estimate = false;
    let mut text_groups = String::new();
    for class in CLASS_ORDER {
        let members: Vec<&&AreaIndicator> = kept.iter().filter(|i| i.class == class).collect();
        if members.is_empty() {
            continue;
        }
        let mut rows = Vec::new();
        let mut lines = String::new();
        for i in members {
            let take = i.values.len().min(budget);
            if take == 0 {
                dropped += 1;
                continue;
            }
            budget -= take;
            let mut ij = indicator_json(i);
            if take < i.values.len() {
                ij["values"] = Value::Array(
                    ij["values"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .take(take)
                        .collect(),
                );
                dropped += i.values.len() - take;
            }
            estimate |= is_estimate_indicator(&i.indicator_id, &i.class)
                || i.values.iter().any(|v| is_estimate_basis(&v.basis));
            shown_ids.push(i.indicator_id.as_str());
            rows.push(ij);
            let vals: Vec<String> = i
                .values
                .iter()
                .take(4)
                .map(|v| {
                    format!(
                        "{}{}{}",
                        if v.dims.is_empty() {
                            String::new()
                        } else {
                            format!("{}: ", v.dims)
                        },
                        value_text(v.value, i.unit.as_deref(), i.unit_mult),
                        match (v.lo, v.hi) {
                            (Some(lo), Some(hi)) =>
                                format!(" [{}–{}] ({}, {})", num(lo), num(hi), v.basis, v.status),
                            _ => format!(" ({}, {})", v.basis, v.status),
                        }
                    )
                })
                .collect();
            lines.push_str(&format!(
                "- {} [{}] {}: {}{}\n",
                i.label,
                i.indicator_id,
                i.period,
                vals.join("; "),
                if i.values.len() > 4 {
                    format!("; +{} more dims", i.values.len() - 4)
                } else {
                    String::new()
                }
            ));
        }
        text_groups.push_str(&format!("\n{}\n{lines}", class.to_uppercase()));
        groups.insert(class.to_owned(), Value::Array(rows));
    }
    let mut children = p.children.clone();
    let child_total = children.len();
    cap_rows(&mut children);

    let mut text = format!(
        "AREA {} — {} ({}){}\nchain: {}\n",
        p.pcode,
        p.name_en,
        level_label(cat, p.level),
        p.area_km2
            .map(|a| format!(", {} km²", num(a)))
            .unwrap_or_default(),
        chain(&p.ancestors, None),
    );
    if let Some([lat, lon]) = p.centroid {
        text.push_str(&format!("centroid: {lat}, {lon}\n"));
    }
    if !p.children.is_empty() {
        text.push_str(&format!(
            "children ({}): {}{}\n",
            child_total,
            p.children
                .iter()
                .take(12)
                .map(|c| format!("{} {}", c.pcode, c.name_en))
                .collect::<Vec<_>>()
                .join(", "),
            if child_total > 12 { ", …" } else { "" }
        ));
    }
    text.push_str(&format!(
        "{} indicator(s) with a latest value{}\n",
        kept.len(),
        if kept.len() < p.indicators.len() {
            format!(" (of {} before filters)", p.indicators.len())
        } else {
            String::new()
        }
    ));
    text.push_str(&text_groups);
    if dropped > 0 {
        text.push_str(&format!("\n… {dropped} more value(s) not shown (raise `limit` up to {MAX_ROWS}, or filter by classes/themes)\n"));
    }

    let result = json!({
        "area": {
            "pcode": p.pcode, "level": p.level, "level_name": level_label(cat, p.level),
            "name_en": p.name_en, "name_si": p.name_si, "name_ta": p.name_ta,
            "boundary_set": p.boundary_set, "area_km2": p.area_km2, "centroid_lat_lon": p.centroid, "bbox": p.bbox,
            "ancestors": p.ancestors.iter().map(unit_json).collect::<Vec<_>>(),
            "children": children.iter().map(unit_json).collect::<Vec<_>>(),
            "children_total": child_total,
        },
        "indicators_total": kept.len(),
        "groups": Value::Object(groups),
    });
    let mut datasets: Vec<String> = touched.clone();
    datasets.extend(cat.boundary_dataset().map(str::to_owned));
    let mut out = Out::new(result, text).datasets(datasets).estimate(estimate);
    for id in shown_ids.iter().take(2) {
        out = out.next(Next::new(
            "query_indicator",
            json!({ "indicator_id": id, "geo": [p.pcode] }),
            format!("time series of {id} for {}", p.pcode),
        ));
    }
    if let (Some(first), Some(parent)) = (shown_ids.first(), p.ancestors.last())
        && p.level > 0
    {
        out = out.next(Next::new(
            "rank_areas",
            json!({ "indicator_id": first, "level": p.level, "parent": parent.pcode, "order": "desc" }),
            format!("rank {} among its peers under {}", p.name_en, parent.name_en),
        ));
    }
    Ok(out)
}
