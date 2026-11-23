//! The tool registry and dispatch. One [`ToolDef`] per tool: the schema, the
//! description and the handler live together, so `tools/list` and
//! `GET /v2/mcp` are generated from the same list.

use std::net::{IpAddr, SocketAddr};

use lanka_core::Catalog;
use rmcp::model::{CallToolResult, Extensions};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::McpContext;
use crate::out::{self, Out, ToolError};

pub mod analysis;
pub mod catalog;
pub mod data;
pub mod features;
pub mod maplink;
pub mod places;

/// What a tool needs beyond the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    /// The analysis engine (`lanka_analysis` pool).
    Analysis,
    /// The app plane (`LANKA_APP_PLANE=on`).
    AppPlane,
}

/// Who is calling: the address behind a Streamable HTTP request (for the
/// snapshot rate limit); nothing over stdio.
#[derive(Debug, Clone, Default)]
pub struct Caller {
    pub ip: Option<IpAddr>,
}

impl Caller {
    pub(crate) fn from_extensions(ext: &Extensions) -> Self {
        let Some(parts) = ext.get::<axum::http::request::Parts>() else {
            return Self::default();
        };
        let peer = parts
            .extensions
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        Self {
            ip: lanka_app::rate::client_ip(&parts.headers, peer),
        }
    }
}

type Handler = for<'a> fn(
    &'a McpContext,
    &'a Catalog,
    Value,
    &'a Caller,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Out, ToolError>> + Send + 'a>,
>;

/// One registered tool.
pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// JSON Schema of the arguments.
    pub schema: fn() -> Value,
    /// `false` only for `create_map_link`.
    pub read_only: bool,
    pub needs: Needs,
    handler: Handler,
}

/// Boxes an async tool function into a [`Handler`].
macro_rules! handler {
    ($f:path) => {{
        fn h<'a>(
            ctx: &'a McpContext,
            cat: &'a Catalog,
            args: Value,
            caller: &'a Caller,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Out, ToolError>> + Send + 'a>,
        > {
            Box::pin($f(ctx, cat, args, caller))
        }
        h as Handler
    }};
}

fn all() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "search_catalog",
            title: "Search the data catalogue",
            description: "Find datasets and indicators by words (title, description, indicator label or id), optionally by theme, admin level or class. Returns coverage, licence status and the next calls. Start here when you do not know an indicator id.",
            schema: catalog::search_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(catalog::search_catalog),
        },
        ToolDef {
            name: "describe_dataset",
            title: "Describe a dataset",
            description: "The dataset card: its limits first (what it cannot tell you), then description, sources and licences, indicators, levels, periods, freshness and ready-made example calls.",
            schema: catalog::describe_dataset_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(catalog::describe_dataset),
        },
        ToolDef {
            name: "describe_indicator",
            title: "Describe an indicator",
            description: "Definition, unit, dimensions and their codes, admin levels, period type, class (official, third_party, derived, estimated), the dataset's limits and the roll-ups that complete it.",
            schema: catalog::describe_indicator_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(catalog::describe_indicator),
        },
        ToolDef {
            name: "resolve_place",
            title: "Resolve a place name or point to p-codes",
            description: "Turn a place name (English, Sinhala or Tamil) or a latitude/longitude into candidate admin units with p-code, level, parent chain and confidence. p-codes are the only join keys: never pass a name to the data tools.",
            schema: places::resolve_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(places::resolve_place),
        },
        ToolDef {
            name: "get_area_profile",
            title: "Profile of an area",
            description: "The latest value of every indicator held for one admin unit (country, province, district, DS division, GN division), grouped by class (official, third_party, derived, estimated) with period, basis and source, plus its parent chain and children.",
            schema: places::profile_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(places::get_area_profile),
        },
        ToolDef {
            name: "query_indicator",
            title: "Query an indicator",
            description: "Values of one indicator: a time series for up to 25 p-codes (geo), or every unit of one admin level (level, optional parent) for one period. Rows carry basis, status and lo/hi intervals; at most 500 rows (overflow → download link in next).",
            schema: data::query_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(data::query_indicator),
        },
        ToolDef {
            name: "rank_areas",
            title: "Rank areas by an indicator",
            description: "The top or bottom units of one admin level (optionally below a parent) for one indicator and period.",
            schema: data::rank_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(data::rank_areas),
        },
        ToolDef {
            name: "compare_areas",
            title: "Compare areas",
            description: "A table of the latest values of 1–8 indicators for 2–25 p-codes, with each area's rank among all units of its level.",
            schema: data::compare_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(data::compare_areas),
        },
        ToolDef {
            name: "find_features",
            title: "Find features",
            description: "Points, lines and polygons of a features dataset (hospitals, schools, roads, …): inside an admin unit or bounding box, or the nearest to a point; filter by kind or name. Returns keys, kinds, names, p-codes and representative points (no geometry).",
            schema: features::find_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(features::find_features),
        },
        ToolDef {
            name: "list_releases",
            title: "List releases",
            description: "Data releases, newest first (data_version, time, notes). Every result's data_version is one of these.",
            schema: catalog::releases_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(catalog::list_releases),
        },
        ToolDef {
            name: "list_analysis_methods",
            title: "List analysis methods",
            description: "The statistical methods of the analysis engine, with roles, supports, minimum n, guards and when to use each. analyze_relationship runs `relate` and `regress`.",
            schema: analysis::methods_schema,
            read_only: true,
            needs: Needs::Nothing,
            handler: handler!(analysis::list_analysis_methods),
        },
        ToolDef {
            name: "analyze_relationship",
            title: "Analyse the relationship between two layers",
            description: "How two indicators relate across areas: correlations with intervals, spatially adjusted significance, OLS and robust fits, optional controls. The result is NEVER causal; the text leads with NOT_CAUSAL and every warning guard before the numbers.",
            schema: analysis::relationship_schema,
            read_only: true,
            needs: Needs::Analysis,
            handler: handler!(analysis::analyze_relationship),
        },
        ToolDef {
            name: "get_analysis",
            title: "Fetch a stored analysis",
            description: "A stored analysis result by id (`an_…`), rendered like analyze_relationship. A queued result says so: repeat the call.",
            schema: analysis::get_schema,
            read_only: true,
            needs: Needs::Analysis,
            handler: handler!(analysis::get_analysis),
        },
        ToolDef {
            name: "create_map_link",
            title: "Create a map permalink",
            description: "Store a map of the given layers (indicators by admin level, admin boundaries, features datasets) as an immutable, content-addressed snapshot and return its link `/m/{id}`. The only tool that stores anything; calling it again with the same layers returns the same link.",
            schema: maplink::schema,
            read_only: false,
            needs: Needs::AppPlane,
            handler: handler!(maplink::create_map_link),
        },
    ]
}

/// The tools this server instance serves (those whose backing exists).
#[must_use]
pub fn registered(ctx: &McpContext) -> Vec<ToolDef> {
    all()
        .into_iter()
        .filter(|t| match t.needs {
            Needs::Nothing => true,
            Needs::Analysis => ctx.analysis.is_some(),
            Needs::AppPlane => ctx.app.is_some(),
        })
        .collect()
}

/// Runs a tool. `None` = no such tool.
pub async fn call(
    ctx: &McpContext,
    name: &str,
    args: Value,
    caller: &Caller,
) -> Option<CallToolResult> {
    let def = registered(ctx).into_iter().find(|t| t.name == name)?;
    let cat = ctx.catalog();
    let started = std::time::Instant::now();
    let res = match (def.handler)(ctx, &cat, args, caller).await {
        Ok(o) => out::finish(&cat, o),
        Err(e) => out::fail(Some(&cat), e),
    };
    tracing::debug!(
        tool = name,
        ms = started.elapsed().as_millis() as u64,
        error = res.is_error.unwrap_or(false),
        "mcp tool call"
    );
    Some(res)
}

// ───────────────────────────── argument helpers ────────────────────────────

/// Parses a tool's arguments; a failure is a tool error the model can fix.
pub(crate) fn parse<T: DeserializeOwned>(tool: &str, args: Value) -> Result<T, ToolError> {
    serde_json::from_value(args)
        .map_err(|e| ToolError::bad(format!("{tool}: invalid arguments: {e}")))
}

/// A string or a list of strings (comma-separated text is split).
#[derive(Debug, Clone, Default)]
pub struct StringList(pub Vec<String>);

impl<'de> Deserialize<'de> for StringList {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum One {
            Many(Vec<String>),
            One(String),
        }
        let items = match One::deserialize(d)? {
            One::Many(v) => v,
            One::One(s) => s.split(',').map(str::to_owned).collect(),
        };
        Ok(Self(
            items
                .into_iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect(),
        ))
    }
}

/// An admin level: `0`..`4`, `"2"`, `"adm2"`, `"district"`-style words are not guessed.
#[derive(Debug, Clone, Copy)]
pub struct Level(pub i16);

impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum L {
            N(i64),
            S(String),
        }
        let n = match L::deserialize(d)? {
            L::N(n) => n,
            L::S(s) => {
                let t = s.trim().to_lowercase();
                if t == "national" {
                    0
                } else {
                    t.trim_start_matches("adm").parse::<i64>().map_err(|_| {
                        serde::de::Error::custom(format!("level {s:?}: expected 0–4"))
                    })?
                }
            }
        };
        i16::try_from(n)
            .ok()
            .filter(|l| (0..=9).contains(l))
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom(format!("level {n}: expected 0–4")))
    }
}

/// Checks a level against the profile and returns the "available levels" hint.
pub(crate) fn check_level(cat: &Catalog, level: i16) -> Result<(), ToolError> {
    let max = i16::from(cat.profile()?.admin.max_level());
    if (0..=max).contains(&level) {
        Ok(())
    } else {
        Err(ToolError::bad(format!("level {level}: expected 0–{max}"))
            .hints(json!({ "valid": (0..=max).collect::<Vec<_>>(), "levels": level_names(max) })))
    }
}

fn level_names(max: i16) -> Vec<String> {
    (0..=max)
        .map(|l| format!("{l} = {}", out::level_word(i64::from(l))))
        .collect()
}

/// Builds a JSON Schema object.
pub(crate) fn schema(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

/// Common dataset-or-indicator not-found follow-up.
pub(crate) fn search_hint(text: &str) -> out::Next {
    out::Next::new(
        "search_catalog",
        json!({ "query": text }),
        "find the right id",
    )
}
