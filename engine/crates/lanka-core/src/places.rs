//! Place-name search and release history: reads the MCP tools need that
//! REST v2 does not expose as routes yet (`resolve_place` by name,
//! `list_releases`). Like every query-core read: bound statements only, the
//! reference boundary set of the catalogue snapshot, p-codes as the keys.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::catalog::Catalog;
use crate::error::{CoreError, CoreResult};
use crate::query::AreaRef;

/// Largest number of candidates returned.
pub const MAX_PLACES: i64 = 25;
/// Default number of candidates.
pub const DEFAULT_PLACES: i64 = 8;
/// Longest accepted search text.
pub const MAX_QUERY_CHARS: usize = 120;

#[derive(Debug, Clone, Default)]
pub struct PlaceQuery {
    /// Name text (English, Sinhala or Tamil) or a p-code.
    pub q: String,
    /// Restrict to one admin level.
    pub level: Option<i16>,
    /// Restrict to units below this p-code.
    pub parent: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlaceMatch {
    #[serde(flatten)]
    pub unit: AreaRef,
    /// Root first: country, province, district, …
    pub ancestors: Vec<AreaRef>,
    /// 0–1: `1` exact name or code, `0.85` name starts with the text,
    /// `0.7` a word starts with it, `0.5` it occurs inside the name.
    pub confidence: f64,
    /// `code` | `exact` | `prefix` | `word` | `contains`
    pub match_kind: String,
}

#[derive(FromRow)]
struct PlaceRow {
    pcode: String,
    level: i16,
    name_en: String,
    name_si: Option<String>,
    name_ta: Option<String>,
    anc_keys: Vec<i32>,
    score: f64,
}

#[derive(FromRow)]
struct AncRow {
    key: i32,
    pcode: String,
    level: i16,
    name_en: String,
    name_si: Option<String>,
    name_ta: Option<String>,
}

fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn kind_of(score: f64) -> &'static str {
    if score >= 1.05 {
        "code"
    } else if score >= 0.99 {
        "exact"
    } else if score >= 0.8 {
        "prefix"
    } else if score >= 0.65 {
        "word"
    } else {
        "contains"
    }
}

/// Admin units whose name (or p-code) matches `q`, best first: exact, then
/// prefix, then word-start, then substring; ties go to the higher unit
/// (province before ward) and then to the shorter name.
pub async fn place_search(
    pool: &PgPool,
    cat: &Catalog,
    q: &PlaceQuery,
) -> CoreResult<Vec<PlaceMatch>> {
    let text = q.q.trim();
    if text.is_empty() {
        return Err(CoreError::bad_request("query must not be empty"));
    }
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(CoreError::bad_request(format!(
            "query is over {MAX_QUERY_CHARS} characters"
        )));
    }
    let limit = match q.limit {
        None => DEFAULT_PLACES,
        Some(n) if (1..=MAX_PLACES).contains(&n) => n,
        Some(n) => {
            return Err(CoreError::bad_request(format!(
                "limit {n}: expected 1–{MAX_PLACES}"
            )));
        }
    };
    let profile = cat.profile()?;
    let max = i16::from(profile.admin.max_level());
    if let Some(l) = q.level
        && !(0..=max).contains(&l)
    {
        return Err(CoreError::bad_request(format!(
            "level {l}: expected 0–{max}"
        )));
    }
    let set = cat.reference_set.as_deref().ok_or_else(|| {
        CoreError::not_found("reference boundary set (load the boundaries first)")
    })?;
    let parent_key: Option<i32> = match q.parent.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(p) => {
            let code = profile
                .admin
                .parse(p)
                .map_err(|e| CoreError::bad_request(e.to_string()))?;
            Some(
                sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                    .bind(code.as_str())
                    .fetch_optional(pool)
                    .await?
                    .ok_or_else(|| CoreError::not_found(format!("no admin unit \"{p}\"")))?,
            )
        }
    };
    let lower = text.to_lowercase();
    let esc = like_escape(&lower);
    let esc_raw = like_escape(text);
    let rows: Vec<PlaceRow> = sqlx::query_as(
        "WITH m AS (
           SELECT u.key, u.code AS pcode, u.level, u.name_en, u.name_si, u.name_ta, u.anc_keys,
             GREATEST(
               CASE WHEN lower(u.code) = $1 THEN 1.1
                    WHEN lower(u.name_en) = $1 OR u.name_si = $2 OR u.name_ta = $2 THEN 1.0
                    WHEN lower(u.name_en) LIKE $3 || '%' ESCAPE '\\'
                      OR u.name_si LIKE $4 || '%' ESCAPE '\\' OR u.name_ta LIKE $4 || '%' ESCAPE '\\' THEN 0.85
                    WHEN lower(u.name_en) LIKE '% ' || $3 || '%' ESCAPE '\\'
                      OR u.name_si LIKE '% ' || $4 || '%' ESCAPE '\\' OR u.name_ta LIKE '% ' || $4 || '%' ESCAPE '\\' THEN 0.7
                    WHEN lower(u.name_en) LIKE '%' || $3 || '%' ESCAPE '\\'
                      OR u.name_si LIKE '%' || $4 || '%' ESCAPE '\\' OR u.name_ta LIKE '%' || $4 || '%' ESCAPE '\\' THEN 0.5
                    ELSE 0 END, 0)::float8 AS score
           FROM geo.units u
           JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $5
           WHERE u.scheme_id = $6
             AND ($7::smallint IS NULL OR u.level = $7)
             AND ($8::int IS NULL OR $8 = ANY(u.anc_keys))
         )
         SELECT pcode, level, name_en, name_si, name_ta, anc_keys, score FROM m
         WHERE score > 0
         ORDER BY score DESC, level, length(name_en), name_en LIMIT $9",
    )
    .bind(&lower)
    .bind(text)
    .bind(&esc)
    .bind(&esc_raw)
    .bind(set)
    .bind(profile.admin_scheme())
    .bind(q.level)
    .bind(parent_key)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut keys: Vec<i32> = rows
        .iter()
        .flat_map(|r| r.anc_keys.iter().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let anc: Vec<(i32, AreaRef)> = if keys.is_empty() {
        Vec::new()
    } else {
        let refs: Vec<AncRow> = sqlx::query_as(
            "SELECT key, code AS pcode, level, name_en, name_si, name_ta FROM geo.units WHERE key = ANY($1)",
        )
        .bind(&keys)
        .fetch_all(pool)
        .await?;
        refs.into_iter()
            .map(|r| {
                (
                    r.key,
                    AreaRef {
                        pcode: r.pcode,
                        level: r.level,
                        name_en: r.name_en,
                        name_si: r.name_si,
                        name_ta: r.name_ta,
                    },
                )
            })
            .collect()
    };
    Ok(rows
        .into_iter()
        .map(|r| {
            let mut ancestors: Vec<AreaRef> = r
                .anc_keys
                .iter()
                .filter_map(|k| anc.iter().find(|(ak, _)| ak == k).map(|(_, a)| a.clone()))
                .collect();
            ancestors.sort_by_key(|a| a.level);
            PlaceMatch {
                confidence: r.score.min(1.0),
                match_kind: kind_of(r.score).to_owned(),
                unit: AreaRef {
                    pcode: r.pcode,
                    level: r.level,
                    name_en: r.name_en,
                    name_si: r.name_si,
                    name_ta: r.name_ta,
                },
                ancestors,
            }
        })
        .collect())
}

/// English names of admin units by p-code (unknown codes are absent).
pub async fn unit_names(
    pool: &PgPool,
    codes: &[String],
) -> CoreResult<std::collections::HashMap<String, String>> {
    if codes.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT code, name_en FROM geo.units WHERE code = ANY($1)")
            .bind(codes)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

// ───────────────────────────── releases ────────────────────────────────────

/// Largest number of releases returned.
pub const MAX_RELEASES: i64 = 100;

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ReleaseRow {
    pub data_version: String,
    pub created_at: DateTime<Utc>,
    pub notes: Option<String>,
}

/// Releases newest first; `since` (`YYYYMMDD.N` or `YYYYMMDD`) keeps those
/// at or after it.
pub async fn releases(
    pool: &PgPool,
    since: Option<&str>,
    limit: Option<i64>,
) -> CoreResult<Vec<ReleaseRow>> {
    let limit = match limit {
        None => 20,
        Some(n) if (1..=MAX_RELEASES).contains(&n) => n,
        Some(n) => {
            return Err(CoreError::bad_request(format!(
                "limit {n}: expected 1–{MAX_RELEASES}"
            )));
        }
    };
    let since = since.map(str::trim).filter(|s| !s.is_empty());
    let (day, n): (Option<&str>, i32) = match since {
        None => (None, 0),
        Some(s) => {
            let (d, n) = s.split_once('.').unwrap_or((s, "0"));
            let n: Option<i32> = n
                .parse()
                .ok()
                .filter(|_| n.bytes().all(|b| b.is_ascii_digit()));
            match n {
                Some(n) if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) => (Some(d), n),
                _ => {
                    return Err(CoreError::bad_request(
                        "since must be a release id (YYYYMMDD.N) or a date (YYYYMMDD)",
                    ));
                }
            }
        }
    };
    Ok(sqlx::query_as(
        "SELECT data_version, created_at, notes FROM data.releases
         WHERE $1::text IS NULL
            OR (substring(data_version, 1, 8), split_part(data_version, '.', 2)::int) >= ($1::text, $2::int)
         ORDER BY seq DESC LIMIT $3",
    )
    .bind(day)
    .bind(n)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(like_escape("a_b%c\\d"), "a\\_b\\%c\\\\d");
    }

    #[test]
    fn match_kinds_follow_the_score() {
        assert_eq!(kind_of(1.1), "code");
        assert_eq!(kind_of(1.0), "exact");
        assert_eq!(kind_of(0.85), "prefix");
        assert_eq!(kind_of(0.7), "word");
        assert_eq!(kind_of(0.5), "contains");
    }
}
