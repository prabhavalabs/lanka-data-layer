//! `query_indicator`, `rank_areas`, `compare_areas`: values of indicators,
//! through the same `query::series` / `query::map` the REST routes call.

use std::collections::BTreeMap;

use lanka_core::Catalog;
use lanka_core::places::unit_names;
use lanka_core::query::{self, MapData, MapQuery, SeriesQuery};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, Level, StringList, check_level, parse, schema, search_hint};
use crate::McpContext;
use crate::out::{
    MAX_ROWS, Next, Out, ToolError, cap_rows, is_estimate_basis, is_estimate_indicator, num,
    value_text,
};

fn indicator_not_found(e: lanka_core::CoreError, id: &str) -> ToolError {
    let tail = id.rsplit('.').next().unwrap_or(id).replace('_', " ");
    ToolError::from(e).next(search_hint(&tail))
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_owned(), num)
}

// ───────────────────────────── query_indicator ─────────────────────────────

pub fn query_schema() -> Value {
    schema(
        json!({
            "indicator_id": { "type": "string", "description": "Indicator id, e.g. census24.pop_total (find ids with search_catalog)" },
            "geo": { "type": ["array", "string"], "items": { "type": "string" }, "maxItems": 25, "description": "p-codes for a time series, e.g. [\"LK\", \"LK11\"]. Use this OR level." },
            "level": { "type": ["integer", "string"], "description": "Admin level 0 (national) … 4 (GN division): every unit of the level for one period. Use this OR geo." },
            "parent": { "type": "string", "description": "With level: only units below this p-code" },
            "from": { "type": "string", "description": "First period (YYYY, YYYY-Qn, YYYY-MM, YYYY-MM-DD). With level: the period (default latest)" },
            "to": { "type": "string", "description": "Last period. With level: the period (default latest)" },
            "dims": { "type": "string", "description": "Dimension filter, canonical form e.g. \"sex=female\" or \"sex=female,age_group=15-24\"" }
        }),
        &["indicator_id"],
    )
}

#[derive(Deserialize)]
struct QueryArgs {
    indicator_id: String,
    geo: Option<StringList>,
    level: Option<Level>,
    parent: Option<String>,
    from: Option<String>,
    to: Option<String>,
    dims: Option<String>,
}

pub async fn query_indicator(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: QueryArgs = parse("query_indicator", args)?;
    let id = a.indicator_id.trim().to_owned();
    let geos: Vec<String> = a.geo.map(|g| g.0).unwrap_or_default();
    let dims = a.dims.clone().filter(|s| !s.trim().is_empty());

    if !geos.is_empty() {
        // ── a time series for the listed units ──
        let s = query::series(
            &ctx.pool,
            cat,
            &id,
            &SeriesQuery {
                geos: geos.clone(),
                from: a.from.clone(),
                to: a.to.clone(),
                dims: dims.clone(),
            },
        )
        .await
        .map_err(|e| indicator_not_found(e, &id))?;
        let ind = &s.indicator;
        let mut rows: Vec<Value> = Vec::new();
        let mut estimate = is_estimate_indicator(&ind.id, &ind.class);
        let mut text = format!(
            "{} — {} [{}{}; {}] {}\n",
            ind.id,
            ind.label,
            ind.unit_label
                .as_deref()
                .or(ind.unit.as_deref())
                .unwrap_or("no unit"),
            if ind.unit_mult == 0 {
                String::new()
            } else {
                format!(" ×10^{}", ind.unit_mult)
            },
            ind.class,
            dims.as_deref()
                .map(|d| format!("(dims {d})"))
                .unwrap_or_default()
        );
        for (line_no, line) in s.series.iter().enumerate() {
            for p in &line.points {
                estimate |= is_estimate_basis(&p.basis);
                rows.push(json!({
                    "geo": line.geo, "name": line.name, "dims": line.dims, "period": p.period,
                    "value": p.value, "lo": p.lo, "hi": p.hi, "basis": p.basis, "status": p.status,
                    "flags": p.flags, "obs_id": p.obs_id,
                }));
            }
            if line_no < 30 {
                let tail: Vec<String> = line
                    .points
                    .iter()
                    .rev()
                    .take(10)
                    .rev()
                    .map(|p| {
                        format!(
                            "{}={}{}{}",
                            p.period,
                            pct(p.value),
                            match (p.lo, p.hi) {
                                (Some(lo), Some(hi)) => format!(" [{}–{}]", num(lo), num(hi)),
                                _ => String::new(),
                            },
                            if p.basis == "reported" && p.status == "normal" {
                                String::new()
                            } else {
                                format!(
                                    " ({}{})",
                                    p.basis,
                                    if p.status == "normal" {
                                        String::new()
                                    } else {
                                        format!(", {}", p.status)
                                    }
                                )
                            }
                        )
                    })
                    .collect();
                text.push_str(&format!(
                    "- {} {}{}: {}{}\n",
                    line.geo,
                    line.name.as_deref().unwrap_or(""),
                    if line.dims.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", line.dims)
                    },
                    tail.join(", "),
                    if line.points.len() > 10 {
                        format!(" (last 10 of {})", line.points.len())
                    } else {
                        String::new()
                    }
                ));
            }
        }
        if s.series.is_empty() {
            text.push_str("No rows: this indicator holds no values for those units, periods or dims. Check describe_indicator for levels and dims.\n");
        }
        let total = rows.len();
        let capped = cap_rows(&mut rows);
        if capped.is_some() {
            text.push_str(&format!("\n{total} rows matched; the first {MAX_ROWS} are returned. Narrow from/to or geo, or download the series.\n"));
        }
        let first_geo = s.series.first().map(|l| l.geo.clone());
        let mut out = Out::new(
            json!({
                "indicator": { "id": ind.id, "label": ind.label, "unit": ind.unit, "unit_label": ind.unit_label, "unit_mult": ind.unit_mult, "class": ind.class, "frequency": ind.freq },
                "rows": rows, "row_count": total, "truncated": capped.is_some(),
            }),
            text,
        )
        .datasets([ind.dataset_id.clone()])
        .datasets(cat.boundary_dataset().map(str::to_owned))
        .estimate(estimate);
        if capped.is_some() {
            out = out.next(
                Next::new(
                    "query_indicator",
                    json!({ "indicator_id": ind.id, "geo": geos, "from": a.from, "to": a.to }),
                    "narrow the period or the units",
                )
                .with_href(format!(
                    "/v2/indicators/{}/series?geo={}",
                    ind.id,
                    geos.join(",")
                )),
            );
        }
        if let Some(g) = first_geo {
            out = out.next(Next::new(
                "get_area_profile",
                json!({ "geo": g }),
                "everything held for this area",
            ));
        }
        if let Some(l) = ind
            .geo_levels
            .iter()
            .filter_map(|g| g.strip_prefix("adm").and_then(|n| n.parse::<i64>().ok()))
            .min()
        {
            out = out.next(Next::new(
                "rank_areas",
                json!({ "indicator_id": ind.id, "level": l }),
                "rank all units of a level",
            ));
        }
        return Ok(out);
    }

    // ── every unit of one level, one period ──
    let Some(level) = a.level else {
        return Err(ToolError::bad(
            "give `geo` (p-codes, for a series) or `level` (every unit of a level)",
        )
        .hints(json!({ "geo_example": ["LK", "LK11"], "level_example": 2 })));
    };
    check_level(cat, level.0)?;
    let period = match (a.to.as_ref(), a.from.as_ref()) {
        (Some(t), Some(f)) if t != f => {
            return Err(ToolError::bad(
                "a level query returns one period: give `from` = `to`, or use `geo` for a series",
            ));
        }
        (Some(p), _) | (None, Some(p)) => Some(p.clone()),
        (None, None) => None,
    };
    let m = query::map(
        &ctx.pool,
        cat,
        &id,
        &MapQuery {
            level: level.0,
            period,
            parent: a.parent.clone().filter(|s| !s.trim().is_empty()),
            dims: dims.clone(),
        },
    )
    .await
    .map_err(|e| indicator_not_found(e, &id))?;
    let names = unit_names(&ctx.pool, &m.pcodes).await?;
    let ind = cat.indicators.get(&id);
    let estimate = ind.is_some_and(|i| is_estimate_indicator(&i.id, &i.class))
        || m.basis.iter().any(|b| is_estimate_basis(b));
    let total = m.pcodes.len();
    let mut rows: Vec<Value> = (0..m.pcodes.len())
        .map(|i| {
            json!({
                "geo": m.pcodes[i], "name": names.get(&m.pcodes[i]), "period": m.period, "dims": m.dims,
                "value": m.values[i], "lo": m.lo[i], "hi": m.hi[i], "basis": m.basis[i], "status": m.status[i],
            })
        })
        .collect();
    let capped = cap_rows(&mut rows);
    let with_value = m.values.iter().flatten().count();
    let mut text = format!(
        "{} level {} ({} units, {} with a value), period {}{}{}\n",
        m.indicator_id,
        m.level,
        total,
        with_value,
        m.period,
        if m.dims.is_empty() {
            String::new()
        } else {
            format!(", dims {}", m.dims)
        },
        a.parent
            .as_deref()
            .map(|p| format!(", under {p}"))
            .unwrap_or_default(),
    );
    text.push_str(&format!(
        "unit: {}; quantile breaks {}\n",
        value_text(Some(0.0), m.unit.as_deref(), m.unit_mult).trim_start_matches("0 "),
        m.breaks
            .iter()
            .map(|b| num(*b))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    for r in rows.iter().take(40) {
        text.push_str(&format!(
            "- {} {}: {}{}\n",
            r["geo"].as_str().unwrap_or(""),
            r["name"].as_str().unwrap_or(""),
            r["value"].as_f64().map_or("no value".to_owned(), num),
            if r["basis"] == "reported" {
                String::new()
            } else {
                format!(" ({})", r["basis"].as_str().unwrap_or(""))
            }
        ));
    }
    if total > 40 {
        text.push_str(&format!(
            "… {} more rows in the structured content\n",
            total.min(MAX_ROWS) - 40
        ));
    }
    if capped.is_some() {
        text.push_str(&format!("\n{total} units: the first {MAX_ROWS} are returned; narrow with `parent`, or download the whole frame.\n"));
    }
    let download = format!(
        "/v2/frame?indicators={}&level={}{}&format=csv",
        m.indicator_id,
        m.level,
        a.parent
            .as_deref()
            .map(|p| format!("&parent={p}"))
            .unwrap_or_default()
    );
    let mut out = Out::new(
        json!({
            "indicator": { "id": m.indicator_id, "unit": m.unit, "unit_mult": m.unit_mult, "completed_from": m.completed_from },
            "level": m.level, "period": m.period, "dims": m.dims, "available_dims": m.available_dims,
            "breaks": m.breaks, "rows": rows, "row_count": total, "truncated": capped.is_some(),
        }),
        text,
    )
    .datasets([m.dataset_id.clone()])
    .datasets(cat.boundary_dataset().map(str::to_owned))
    .estimate(estimate);
    if capped.is_some() {
        out = out.next(
            Next::new(
                "rank_areas",
                json!({ "indicator_id": m.indicator_id, "level": m.level, "parent": a.parent }),
                "the top and bottom units",
            )
            .with_href(download),
        );
    }
    out = out.next(Next::new(
        "rank_areas",
        json!({ "indicator_id": m.indicator_id, "level": m.level, "parent": a.parent, "order": "desc", "limit": 10 }),
        "highest units",
    ));
    Ok(out)
}

// ───────────────────────────── rank_areas ──────────────────────────────────

pub fn rank_schema() -> Value {
    schema(
        json!({
            "indicator_id": { "type": "string" },
            "level": { "type": ["integer", "string"], "description": "Admin level 0 … 4" },
            "parent": { "type": "string", "description": "Only units below this p-code" },
            "period": { "type": "string", "description": "Default: the latest held at that level" },
            "dims": { "type": "string", "description": "Dimension filter, e.g. \"sex=total\"" },
            "order": { "type": "string", "enum": ["desc", "asc"], "description": "desc = highest first (default)" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 10" }
        }),
        &["indicator_id", "level"],
    )
}

#[derive(Deserialize)]
struct RankArgs {
    indicator_id: String,
    level: Level,
    parent: Option<String>,
    period: Option<String>,
    dims: Option<String>,
    order: Option<String>,
    limit: Option<usize>,
}

/// `(rank, of)` of `value` among `values` (1 = highest when `desc`); ties share a rank.
fn rank_of(values: &[f64], value: f64, desc: bool) -> usize {
    1 + values
        .iter()
        .filter(|v| if desc { **v > value } else { **v < value })
        .count()
}

pub async fn rank_areas(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: RankArgs = parse("rank_areas", args)?;
    let id = a.indicator_id.trim().to_owned();
    check_level(cat, a.level.0)?;
    let desc = match a.order.as_deref().unwrap_or("desc") {
        "desc" => true,
        "asc" => false,
        o => {
            return Err(
                ToolError::bad(format!("order {o:?}")).hints(json!({ "valid": ["desc", "asc"] }))
            );
        }
    };
    let limit = a.limit.unwrap_or(10).clamp(1, 100);
    let m = query::map(
        &ctx.pool,
        cat,
        &id,
        &MapQuery {
            level: a.level.0,
            period: a.period.clone().filter(|s| !s.trim().is_empty()),
            parent: a.parent.clone().filter(|s| !s.trim().is_empty()),
            dims: a.dims.clone().filter(|s| !s.trim().is_empty()),
        },
    )
    .await
    .map_err(|e| indicator_not_found(e, &id))?;
    let mut idx: Vec<usize> = (0..m.pcodes.len())
        .filter(|i| m.values[*i].is_some())
        .collect();
    let n_with = idx.len();
    let all: Vec<f64> = idx.iter().filter_map(|i| m.values[*i]).collect();
    idx.sort_by(|x, y| {
        let (vx, vy) = (m.values[*x].unwrap_or(0.0), m.values[*y].unwrap_or(0.0));
        let ord = vx.partial_cmp(&vy).unwrap_or(std::cmp::Ordering::Equal);
        (if desc { ord.reverse() } else { ord }).then_with(|| m.pcodes[*x].cmp(&m.pcodes[*y]))
    });
    idx.truncate(limit);
    let codes: Vec<String> = idx.iter().map(|i| m.pcodes[*i].clone()).collect();
    let names = unit_names(&ctx.pool, &codes).await?;
    let ind = cat.indicators.get(&id);
    let mut estimate = ind.is_some_and(|i| is_estimate_indicator(&i.id, &i.class));
    let rows: Vec<Value> = idx
        .iter()
        .map(|i| {
            let v = m.values[*i].unwrap_or(0.0);
            estimate |= is_estimate_basis(&m.basis[*i]);
            json!({
                "rank": rank_of(&all, v, desc), "geo": m.pcodes[*i], "name": names.get(&m.pcodes[*i]),
                "value": v, "lo": m.lo[*i], "hi": m.hi[*i], "basis": m.basis[*i], "status": m.status[*i],
            })
        })
        .collect();
    let mut text = format!(
        "RANK {} at level {}{} — {} first, period {}{} ({} of {} units have a value)\n",
        m.indicator_id,
        m.level,
        a.parent
            .as_deref()
            .map(|p| format!(" under {p}"))
            .unwrap_or_default(),
        if desc { "highest" } else { "lowest" },
        m.period,
        if m.dims.is_empty() {
            String::new()
        } else {
            format!(", dims {}", m.dims)
        },
        n_with,
        m.pcodes.len(),
    );
    text.push_str(&format!(
        "unit: {}\n",
        value_text(Some(0.0), m.unit.as_deref(), m.unit_mult).trim_start_matches("0 ")
    ));
    for r in &rows {
        text.push_str(&format!(
            "{}. {} {} — {}{}\n",
            r["rank"],
            r["geo"].as_str().unwrap_or(""),
            r["name"].as_str().unwrap_or(""),
            r["value"].as_f64().map_or("no value".to_owned(), num),
            if r["basis"] == "reported" {
                String::new()
            } else {
                format!(" ({})", r["basis"].as_str().unwrap_or(""))
            }
        ));
    }
    let mut out = Out::new(
        json!({
            "indicator": { "id": m.indicator_id, "unit": m.unit, "unit_mult": m.unit_mult, "completed_from": m.completed_from },
            "level": m.level, "parent": a.parent, "period": m.period, "dims": m.dims, "order": if desc { "desc" } else { "asc" },
            "units_total": m.pcodes.len(), "units_with_value": n_with, "available_dims": m.available_dims, "rows": rows,
        }),
        text,
    )
    .datasets([m.dataset_id.clone()])
    .datasets(cat.boundary_dataset().map(str::to_owned))
    .estimate(estimate);
    if let Some(top) = codes.first() {
        out = out.next(Next::new(
            "get_area_profile",
            json!({ "geo": top }),
            "everything held for the top unit",
        ));
    }
    out = out.next(Next::new(
        "rank_areas",
        json!({ "indicator_id": m.indicator_id, "level": m.level, "parent": a.parent, "order": if desc { "asc" } else { "desc" }, "limit": limit }),
        if desc { "the lowest units" } else { "the highest units" },
    ));
    Ok(out)
}

// ───────────────────────────── compare_areas ───────────────────────────────

pub fn compare_schema() -> Value {
    schema(
        json!({
            "geo": { "type": ["array", "string"], "items": { "type": "string" }, "minItems": 2, "maxItems": 25, "description": "p-codes to compare, e.g. [\"LK11\", \"LK12\", \"LK21\"]" },
            "indicator_ids": { "type": ["array", "string"], "items": { "type": "string" }, "minItems": 1, "maxItems": 8 },
            "period": { "type": "string", "description": "One period for all (default: the latest held for each area)" },
            "dims": { "type": "string", "description": "Dimension filter, e.g. \"sex=total\"" }
        }),
        &["geo", "indicator_ids"],
    )
}

#[derive(Deserialize)]
struct CompareArgs {
    geo: StringList,
    indicator_ids: StringList,
    period: Option<String>,
    dims: Option<String>,
}

pub async fn compare_areas(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: CompareArgs = parse("compare_areas", args)?;
    let geos = a.geo.0;
    let inds = a.indicator_ids.0;
    if geos.len() < 2 {
        return Err(ToolError::bad(
            "compare_areas needs at least two p-codes in `geo`",
        ));
    }
    if geos.len() > query::MAX_SERIES_GEOS {
        return Err(ToolError::bad(format!(
            "at most {} p-codes",
            query::MAX_SERIES_GEOS
        )));
    }
    if inds.is_empty() || inds.len() > 8 {
        return Err(ToolError::bad("1 to 8 indicator ids"));
    }
    // levels of the requested units (ranks are within a level)
    let mut levels: Vec<u8> = Vec::new();
    for g in &geos {
        levels.push(query::parse_pcode(cat, g)?.level());
    }
    let same_level = levels.windows(2).all(|w| w[0] == w[1]).then(|| levels[0]);
    let dims = a.dims.clone().filter(|s| !s.trim().is_empty());

    let mut rows: Vec<Value> = Vec::new();
    let mut datasets: Vec<String> = Vec::new();
    let mut estimate = false;
    let mut notes: Vec<String> = Vec::new();
    let mut text = format!(
        "COMPARE {} area(s) on {} indicator(s)\n",
        geos.len(),
        inds.len()
    );
    for id in &inds {
        let s = query::series(
            &ctx.pool,
            cat,
            id,
            &SeriesQuery {
                geos: geos.clone(),
                from: a.period.clone(),
                to: a.period.clone(),
                dims: dims.clone(),
            },
        )
        .await
        .map_err(|e| indicator_not_found(e, id))?;
        let ind = &s.indicator;
        datasets.push(ind.dataset_id.clone());
        estimate |= is_estimate_indicator(&ind.id, &ind.class);
        // the latest point of every (geo, dims) line, grouped by dims
        let mut groups: BTreeMap<String, Vec<(&query::SeriesLine, &query::SeriesPoint)>> =
            BTreeMap::new();
        for l in &s.series {
            if let Some(p) = l.points.last() {
                groups.entry(l.dims.clone()).or_default().push((l, p));
            }
        }
        if groups.is_empty() {
            notes.push(format!(
                "{id}: no values for these units{}",
                a.period
                    .as_deref()
                    .map(|p| format!(" in {p}"))
                    .unwrap_or_default()
            ));
            continue;
        }
        text.push_str(&format!(
            "\n{} — {} [{}]\n",
            ind.id,
            ind.label,
            value_text(
                Some(0.0),
                ind.unit_label.as_deref().or(ind.unit.as_deref()),
                ind.unit_mult
            )
            .trim_start_matches("0 ")
        ));
        for (gdims, members) in groups.iter().take(6) {
            // rank among every unit of the level, in the latest period of the group
            let ranking: Option<(MapData, Vec<f64>)> = match same_level {
                Some(level) => {
                    let period = members.iter().map(|(_, p)| p.period.clone()).max();
                    query::map(
                        &ctx.pool,
                        cat,
                        id,
                        &MapQuery {
                            level: i16::from(level),
                            period,
                            parent: None,
                            dims: (!gdims.is_empty()).then(|| gdims.clone()),
                        },
                    )
                    .await
                    .ok()
                    .map(|m| {
                        let v: Vec<f64> = m.values.iter().flatten().copied().collect();
                        (m, v)
                    })
                }
                None => None,
            };
            if !gdims.is_empty() {
                text.push_str(&format!("[{gdims}]\n"));
            }
            for (line, p) in members {
                estimate |= is_estimate_basis(&p.basis);
                let rank = ranking
                    .as_ref()
                    .zip(p.value)
                    .filter(|((m, _), _)| m.period == p.period)
                    .map(|((_, vals), v)| (rank_of(vals, v, true), vals.len()));
                rows.push(json!({
                    "indicator_id": ind.id, "dims": gdims, "geo": line.geo, "name": line.name, "period": p.period,
                    "value": p.value, "lo": p.lo, "hi": p.hi, "basis": p.basis, "status": p.status, "obs_id": p.obs_id,
                    "rank": rank.map(|r| r.0), "of": rank.map(|r| r.1),
                }));
                text.push_str(&format!(
                    "- {} {} ({}): {}{}{}\n",
                    line.geo,
                    line.name.as_deref().unwrap_or(""),
                    p.period,
                    pct(p.value),
                    if p.basis == "reported" {
                        String::new()
                    } else {
                        format!(" ({})", p.basis)
                    },
                    rank.map(|(r, of)| format!(" — rank {r} of {of}"))
                        .unwrap_or_default(),
                ));
            }
        }
    }
    if same_level.is_none() {
        notes.push(
            "the areas are at different levels, so no ranks are given (ranks are within one level)"
                .to_owned(),
        );
    }
    for n in &notes {
        text.push_str(&format!("note: {n}\n"));
    }
    let total = rows.len();
    let capped = cap_rows(&mut rows);
    let mut out = Out::new(
        json!({ "geo": geos, "indicator_ids": inds, "rows": rows, "row_count": total, "notes": notes, "rank_basis": "rank 1 = highest value among all units of the level, same period" }),
        text,
    )
    .datasets(datasets)
    .datasets(cat.boundary_dataset().map(str::to_owned))
    .estimate(estimate);
    if capped.is_some() {
        out = out.notice(format!("{total} rows: the first {MAX_ROWS} are returned"));
    }
    if let (Some(g), Some(i)) = (geos.first(), inds.first()) {
        out = out
            .next(Next::new(
                "query_indicator",
                json!({ "indicator_id": i, "geo": geos }),
                "the full time series",
            ))
            .next(Next::new(
                "get_area_profile",
                json!({ "geo": g }),
                "everything held for the first area",
            ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_share_ties() {
        let v = [10.0, 30.0, 30.0, 5.0];
        assert_eq!(rank_of(&v, 30.0, true), 1);
        assert_eq!(rank_of(&v, 10.0, true), 3);
        assert_eq!(rank_of(&v, 5.0, false), 1);
        assert_eq!(rank_of(&v, 30.0, false), 3);
    }
}
