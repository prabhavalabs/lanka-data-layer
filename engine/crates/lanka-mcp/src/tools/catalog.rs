//! `search_catalog`, `describe_dataset`, `describe_indicator`, `list_releases`.

use lanka_core::Catalog;
use lanka_core::catalog::IndicatorInfo;
use lanka_core::places;
use lanka_core::query::{self, CatalogQuery};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, Level, parse, schema, search_hint};
use crate::McpContext;
use crate::out::{Next, Out, ToolError, is_estimate_indicator, level_word};

// ───────────────────────────── search_catalog ──────────────────────────────

pub fn search_schema() -> Value {
    schema(
        json!({
            "query": { "type": "string", "description": "Words to look for, e.g. \"poverty rate\" or \"flood\". All words must match." },
            "theme": { "type": "string", "description": "EU DCAT theme code: ECON, SOCI, REGI, ENVI, GOVE, HEAL, EDUC, TRAN, AGRI, ENER, TECH, INTR, JUST, OP_DATPRO" },
            "level": { "type": ["integer", "string"], "description": "Only indicators published at this admin level: 0 (national) … 4, or \"adm3\"" },
            "class": { "type": "string", "enum": ["official", "third_party", "derived", "estimated"], "description": "Only indicators of this class" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Results per list (default 15)" }
        }),
        &["query"],
    )
}

#[derive(Deserialize)]
struct SearchArgs {
    #[serde(default)]
    query: String,
    theme: Option<String>,
    level: Option<Level>,
    class: Option<String>,
    limit: Option<usize>,
}

fn indicator_row(cat: &Catalog, i: &IndicatorInfo) -> Value {
    let ds = cat.datasets.get(&i.dataset_id);
    json!({
        "id": i.id,
        "label": i.label,
        "dataset": i.dataset_id,
        "dataset_title": ds.map(|d| d.title.clone()),
        "unit": i.unit_label.clone().or_else(|| i.unit.clone()),
        "class": i.class,
        "levels": i.geo_levels,
        "dims": i.dims,
        "frequency": i.freq,
        "published": ds.is_some_and(|d| d.current.is_some()),
        "estimate": is_estimate_indicator(&i.id, &i.class),
    })
}

pub async fn search_catalog(
    _ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: SearchArgs = parse("search_catalog", args)?;
    let limit = a.limit.unwrap_or(15).clamp(1, 50);
    let tokens: Vec<String> = a.query.split_whitespace().map(str::to_lowercase).collect();
    if let Some(l) = a.level
        && !(0..=9).contains(&l.0)
    {
        return Err(ToolError::bad("level must be 0..4"));
    }
    let level = a.level.map(|l| level_word(i64::from(l.0)));
    let class = a.class.as_deref().map(str::to_lowercase);
    if let Some(c) = &class
        && !["official", "third_party", "derived", "estimated"].contains(&c.as_str())
    {
        return Err(ToolError::bad(format!("class {c:?}"))
            .hints(json!({ "valid": ["official", "third_party", "derived", "estimated"] })));
    }

    // Datasets: every word must hit (catalog_list matches one phrase at a time).
    let mut datasets: Vec<query::CatalogEntry> = Vec::new();
    if tokens.is_empty() {
        datasets = query::catalog_list(
            cat,
            &CatalogQuery {
                q: None,
                theme: a.theme.clone(),
            },
        );
    } else {
        for (n, t) in tokens.iter().enumerate() {
            let hits = query::catalog_list(
                cat,
                &CatalogQuery {
                    q: Some(t.clone()),
                    theme: a.theme.clone(),
                },
            );
            if n == 0 {
                datasets = hits;
            } else {
                datasets.retain(|d| hits.iter().any(|h| h.id == d.id));
            }
        }
    }

    let ind_ok = |i: &IndicatorInfo| {
        class.as_deref().is_none_or(|c| i.class == c)
            && level
                .as_deref()
                .is_none_or(|l| i.geo_levels.iter().any(|g| g == l))
    };
    if class.is_some() || level.is_some() {
        datasets.retain(|d| {
            d.kind == "indicators"
                && cat.datasets.get(&d.id).is_some_and(|ds| {
                    ds.indicator_ids
                        .iter()
                        .filter_map(|i| cat.indicators.get(i))
                        .any(ind_ok)
                })
        });
    }
    let title_hits = |d: &query::CatalogEntry| {
        let t = d.title.to_lowercase();
        !tokens.is_empty() && tokens.iter().all(|k| t.contains(k))
    };
    datasets.sort_by(|a, b| {
        (!a.published, !title_hits(a), &a.id).cmp(&(!b.published, !title_hits(b), &b.id))
    });

    // Indicators: every word in id + label + dataset title.
    let mut inds: Vec<(&IndicatorInfo, bool, usize)> = cat
        .indicators
        .values()
        .filter(|i| ind_ok(i))
        .filter(|i| {
            if let Some(t) = &a.theme {
                let t = t.to_uppercase();
                let themes = cat.datasets.get(&i.dataset_id).map(|d| {
                    d.source_ids
                        .iter()
                        .filter_map(|s| cat.sources.get(s))
                        .flat_map(|s| s.themes.iter())
                        .any(|x| *x == t)
                });
                if themes != Some(true) {
                    return false;
                }
            }
            true
        })
        .filter_map(|i| {
            let ds = cat.datasets.get(&i.dataset_id);
            let hay = format!(
                "{} {} {}",
                i.id.to_lowercase(),
                i.label.to_lowercase(),
                ds.map(|d| d.title.to_lowercase()).unwrap_or_default()
            );
            if !tokens.iter().all(|t| hay.contains(t.as_str())) {
                return None;
            }
            let in_label = tokens
                .iter()
                .filter(|t| {
                    i.label.to_lowercase().contains(t.as_str()) || i.id.contains(t.as_str())
                })
                .count();
            Some((i, ds.is_some_and(|d| d.current.is_some()), in_label))
        })
        .collect();
    inds.sort_by(|a, b| {
        (!a.1, std::cmp::Reverse(a.2), &a.0.id).cmp(&(!b.1, std::cmp::Reverse(b.2), &b.0.id))
    });

    let total_ds = datasets.len();
    let total_ind = inds.len();
    datasets.truncate(limit);
    inds.truncate(limit);

    let ds_rows: Vec<Value> = datasets
        .iter()
        .map(|d| {
            json!({
                "id": d.id, "title": d.title, "kind": d.kind, "publication_class": d.publication_class,
                "themes": d.themes, "licence_statuses": d.licence_statuses,
                "indicator_count": d.indicator_count, "published": d.published, "dsv": d.dsv,
            })
        })
        .collect();
    let ind_rows: Vec<Value> = inds.iter().map(|(i, _, _)| indicator_row(cat, i)).collect();

    let mut text = format!(
        "{total_ds} dataset(s) and {total_ind} indicator(s) match {:?}{}{}{}.\n",
        a.query,
        a.theme
            .as_deref()
            .map(|t| format!(", theme {t}"))
            .unwrap_or_default(),
        level
            .as_deref()
            .map(|l| format!(", level {l}"))
            .unwrap_or_default(),
        class
            .as_deref()
            .map(|c| format!(", class {c}"))
            .unwrap_or_default(),
    );
    if !datasets.is_empty() {
        text.push_str("\nDATASETS\n");
        for d in &datasets {
            text.push_str(&format!(
                "- {} — {} [{}; {}; licence: {}; {}]\n",
                d.id,
                d.title,
                d.kind,
                if d.themes.is_empty() {
                    "no theme".to_owned()
                } else {
                    d.themes.join(",")
                },
                if d.licence_statuses.is_empty() {
                    "unknown".to_owned()
                } else {
                    d.licence_statuses.join(",")
                },
                if d.published {
                    format!("{} indicators, published", d.indicator_count)
                } else {
                    "not published yet".to_owned()
                },
            ));
        }
    }
    if !inds.is_empty() {
        text.push_str("\nINDICATORS\n");
        for (i, published, _) in &inds {
            text.push_str(&format!(
                "- {} — {} [{}; {}; levels {}; {}{}]\n",
                i.id,
                i.label,
                i.unit_label
                    .as_deref()
                    .or(i.unit.as_deref())
                    .unwrap_or("no unit"),
                i.class,
                i.geo_levels.join(","),
                i.dataset_id,
                if *published {
                    ""
                } else {
                    "; not published yet"
                },
            ));
        }
    }
    if total_ds == 0 && total_ind == 0 {
        text.push_str(
            "Nothing matched. Try fewer or broader words, or drop the theme/level/class filter.\n",
        );
    }

    let mut out = Out::new(
        json!({
            "query": a.query, "theme": a.theme, "level": level, "class": class,
            "datasets": ds_rows, "indicators": ind_rows,
            "totals": { "datasets": total_ds, "indicators": total_ind },
        }),
        text,
    );
    out = out.datasets(datasets.iter().take(10).map(|d| d.id.clone()));
    out = out.datasets(inds.iter().take(10).map(|(i, _, _)| i.dataset_id.clone()));
    if let Some(d) = datasets.first() {
        out = out.next(Next::new(
            "describe_dataset",
            json!({ "dataset_id": d.id }),
            "limits, sources, indicators and example calls",
        ));
    }
    for (i, published, _) in inds.iter().take(2) {
        out = out.next(Next::new(
            "describe_indicator",
            json!({ "indicator_id": i.id }),
            "definition, dims and levels",
        ));
        if *published {
            out = out.next(Next::new(
                "query_indicator",
                preset_query(i),
                "latest values",
            ));
            break;
        }
    }
    Ok(out.estimate(
        inds.iter()
            .any(|(i, _, _)| is_estimate_indicator(&i.id, &i.class)),
    ))
}

/// A ready-to-send `query_indicator` call for an indicator: national if it
/// has a national level, else its coarsest admin level.
fn preset_query(i: &IndicatorInfo) -> Value {
    if i.geo_levels.iter().any(|g| g == "national") {
        json!({ "indicator_id": i.id, "geo": ["LK"] })
    } else {
        let level = i
            .geo_levels
            .iter()
            .filter_map(|g| g.strip_prefix("adm").and_then(|n| n.parse::<i64>().ok()))
            .min()
            .unwrap_or(1);
        json!({ "indicator_id": i.id, "level": level })
    }
}

// ───────────────────────────── describe_dataset ────────────────────────────

pub fn describe_dataset_schema() -> Value {
    schema(
        json!({ "dataset_id": { "type": "string", "description": "Dataset id, e.g. dcs-census-2024-population" } }),
        &["dataset_id"],
    )
}

#[derive(Deserialize)]
struct DatasetArgs {
    dataset_id: String,
}

pub async fn describe_dataset(
    ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: DatasetArgs = parse("describe_dataset", args)?;
    let card = query::dataset_card(&ctx.pool, cat, a.dataset_id.trim())
        .await
        .map_err(|e| ToolError::from(e).next(search_hint(&a.dataset_id)))?;
    let d = &card.dataset;
    let mut text = format!("DATASET {} — {}\n", d.id, d.title);
    text.push_str(&format!(
        "kind {}; class {}; {}; period {}–{}\n",
        d.kind,
        d.publication_class,
        d.current.as_ref().map_or_else(
            || "NOT PUBLISHED yet".to_owned(),
            |c| format!("version {} ({} rows)", c.dsv, c.row_count)
        ),
        d.time_from.as_deref().unwrap_or("?"),
        d.time_to.as_deref().unwrap_or("?"),
    ));
    text.push_str("\nLIMITS (read first — what this dataset cannot tell you)\n");
    if d.limits.is_empty() {
        text.push_str("- none recorded\n");
    }
    for l in &d.limits {
        text.push_str(&format!("- {l}\n"));
    }
    if !d.description.trim().is_empty() {
        text.push_str(&format!("\nDESCRIPTION\n{}\n", d.description.trim()));
    }
    if !d.method.trim().is_empty() {
        text.push_str(&format!("\nMETHOD\n{}\n", d.method.trim()));
    }
    text.push_str("\nSOURCES AND LICENCES\n");
    for s in &card.sources {
        let lic = card.licences.iter().find(|l| l.id == s.licence_id);
        text.push_str(&format!(
            "- {} ({}) — licence {} [{}]; cadence {}\n",
            s.title,
            s.publisher,
            s.licence_id,
            lic.map_or("unknown", |l| l.status.as_str()),
            s.cadence
        ));
    }
    if !card.freshness.is_empty() {
        text.push_str("\nFRESHNESS\n");
        for f in &card.freshness {
            text.push_str(&format!(
                "- {}: {}{}\n",
                f.source_id,
                f.state,
                f.last_success_at
                    .map(|t| format!(", last success {}", t.format("%Y-%m-%d")))
                    .unwrap_or_default()
            ));
        }
    }
    if !card.indicators.is_empty() {
        text.push_str(&format!("\nINDICATORS ({})\n", card.indicators.len()));
        for i in card.indicators.iter().take(60) {
            text.push_str(&format!(
                "- {} — {} [{}; {}; levels {}; dims {}; freq {}]\n",
                i.id,
                i.label,
                i.unit_label
                    .as_deref()
                    .or(i.unit.as_deref())
                    .unwrap_or("no unit"),
                i.class,
                i.geo_levels.join(","),
                if i.dims.is_empty() {
                    "none".to_owned()
                } else {
                    i.dims.join(",")
                },
                i.freq
            ));
        }
        if card.indicators.len() > 60 {
            text.push_str(&format!(
                "… and {} more (see structured content)\n",
                card.indicators.len() - 60
            ));
        }
    }
    let estimate = card
        .indicators
        .iter()
        .any(|i| is_estimate_indicator(&i.id, &i.class));
    let mut result = serde_json::to_value(&card)
        .map_err(|e| ToolError::new("internal", format!("serialising the card: {e}")))?;
    if let Some(o) = result.as_object_mut() {
        // limits first, then the rest
        let limits = o.remove("limits").unwrap_or(Value::Null);
        let mut ordered = serde_json::Map::new();
        ordered.insert("limits".to_owned(), limits);
        for (k, v) in std::mem::take(o) {
            ordered.insert(k, v);
        }
        *o = ordered;
    }
    let mut out = Out::new(result, text)
        .datasets([d.id.clone()])
        .estimate(estimate);
    if d.kind == "features" {
        out = out.next(Next::new(
            "find_features",
            json!({ "dataset": d.id, "limit": 10 }),
            "a first page of features",
        ));
    }
    for i in card
        .indicators
        .iter()
        .filter(|_| d.current.is_some())
        .take(2)
    {
        out = out.next(Next::new(
            "query_indicator",
            preset_query(i),
            format!("latest values of {}", i.id),
        ));
    }
    Ok(out)
}

// ───────────────────────────── describe_indicator ──────────────────────────

pub fn describe_indicator_schema() -> Value {
    schema(
        json!({ "indicator_id": { "type": "string", "description": "Indicator id, e.g. census24.pop_total" } }),
        &["indicator_id"],
    )
}

#[derive(Deserialize)]
struct IndicatorArgs {
    indicator_id: String,
}

pub async fn describe_indicator(
    _ctx: &McpContext,
    cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: IndicatorArgs = parse("describe_indicator", args)?;
    let id = a.indicator_id.trim();
    let Some(i) = cat.indicators.get(id) else {
        let tail = id.rsplit('.').next().unwrap_or(id).replace('_', " ");
        return Err(
            ToolError::new("not_found", format!("not_found: no indicator \"{id}\""))
                .next(search_hint(&tail)),
        );
    };
    let ds = cat.datasets.get(&i.dataset_id);
    let dims: Vec<Value> = i
        .dims
        .iter()
        .map(|d| json!({ "dim": d, "codes": cat.dims.codes_of(d) }))
        .collect();
    let completed_by: Vec<&String> = cat
        .completed_by
        .get(&i.id)
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    let siblings: Vec<&str> = ds
        .map(|d| {
            d.indicator_ids
                .iter()
                .filter(|x| **x != i.id)
                .map(String::as_str)
                .take(12)
                .collect()
        })
        .unwrap_or_default();
    let published = ds.and_then(|d| d.current.as_ref());
    let mut text = format!("INDICATOR {} — {}\n", i.id, i.label);
    text.push_str(&format!(
        "unit: {}{}; measure {}; aggregation {}; class {}{}\n",
        i.unit_label
            .as_deref()
            .or(i.unit.as_deref())
            .unwrap_or("none"),
        if i.unit_mult == 0 {
            String::new()
        } else {
            format!(" (values ×10^{})", i.unit_mult)
        },
        i.measure_type,
        i.aggregation,
        i.class,
        if i.time_invariant {
            "; time-invariant (the period is when it was made)"
        } else {
            ""
        },
    ));
    text.push_str(&format!(
        "levels: {}; frequency {} ({}); dataset {}{}\n",
        i.geo_levels.join(", "),
        i.freq,
        i.period_type,
        i.dataset_id,
        published.map_or("; NOT PUBLISHED yet".to_owned(), |c| format!(
            " (version {})",
            c.dsv
        )),
    ));
    if let Some(c) = &i.completes {
        text.push_str(&format!(
            "completes {c} at levels its source does not print (basis computed)\n"
        ));
    }
    if !completed_by.is_empty() {
        text.push_str(&format!(
            "roll-ups that complete it: {}\n",
            completed_by
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for d in &dims {
        let codes: Vec<&str> = d["codes"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        text.push_str(&format!(
            "dim {}: {}\n",
            d["dim"].as_str().unwrap_or("?"),
            if codes.is_empty() {
                "no codes recorded".to_owned()
            } else {
                codes.join(", ")
            }
        ));
    }
    if let Some(d) = ds {
        if !d.limits.is_empty() {
            text.push_str("\nLIMITS of the dataset\n");
            for l in &d.limits {
                text.push_str(&format!("- {l}\n"));
            }
        }
        if !d.indicator_ids.is_empty() && !siblings.is_empty() {
            text.push_str(&format!("\nsame dataset: {}\n", siblings.join(", ")));
        }
    }
    let estimate = is_estimate_indicator(&i.id, &i.class);
    let result = json!({
        "indicator": i,
        "dimensions": dims,
        "dataset": ds.map(|d| json!({ "id": d.id, "title": d.title, "kind": d.kind, "limits": d.limits, "time_from": d.time_from, "time_to": d.time_to, "current": d.current })),
        "completed_by": completed_by,
        "same_dataset": siblings,
        "levels_note": "levels are those the registry declares for the indicator; the latest period at a level comes with query_indicator or rank_areas",
    });
    let mut out = Out::new(result, text)
        .datasets([i.dataset_id.clone()])
        .estimate(estimate);
    if published.is_some() {
        out = out.next(Next::new("query_indicator", preset_query(i), "values"));
        if let Some(l) = i
            .geo_levels
            .iter()
            .filter_map(|g| g.strip_prefix("adm").and_then(|n| n.parse::<i64>().ok()))
            .min()
        {
            out = out.next(Next::new(
                "rank_areas",
                json!({ "indicator_id": i.id, "level": l, "order": "desc", "limit": 10 }),
                "highest areas",
            ));
        }
    }
    out = out.next(Next::new(
        "describe_dataset",
        json!({ "dataset_id": i.dataset_id }),
        "limits, sources and licence",
    ));
    Ok(out)
}

// ───────────────────────────── list_releases ───────────────────────────────

pub fn releases_schema() -> Value {
    schema(
        json!({
            "since": { "type": "string", "description": "Release id (YYYYMMDD.N) or date (YYYYMMDD): releases at or after it" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20" }
        }),
        &[],
    )
}

#[derive(Deserialize)]
struct ReleasesArgs {
    since: Option<String>,
    limit: Option<i64>,
}

pub async fn list_releases(
    ctx: &McpContext,
    _cat: &Catalog,
    args: Value,
    _caller: &Caller,
) -> Result<Out, ToolError> {
    let a: ReleasesArgs = parse("list_releases", args)?;
    let rows = places::releases(&ctx.pool, a.since.as_deref(), a.limit).await?;
    let mut text = format!("{} release(s), newest first\n", rows.len());
    for r in &rows {
        text.push_str(&format!(
            "- {}  {}{}\n",
            r.data_version,
            r.created_at.format("%Y-%m-%d %H:%M UTC"),
            r.notes
                .as_deref()
                .filter(|n| !n.is_empty())
                .map(|n| format!("  — {n}"))
                .unwrap_or_default()
        ));
    }
    Ok(Out::new(json!({ "releases": rows }), text))
}
