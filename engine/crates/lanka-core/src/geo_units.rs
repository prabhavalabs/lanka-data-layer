//! Bulk unit lists and unit geometry (`/v2/units`, `/v2/units/{pcode}/geometry`).
//!
//! Both read the reference boundary set of the admin scheme; coordinates
//! leave with at most 6 decimals (`ST_AsGeoJSON(…, 6)`, [`round6`]).

use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::catalog::Catalog;
use crate::error::{CoreError, CoreResult};

/// Default and maximum page size of `/v2/units` (all 14,043 GN divisions
/// fit in one page at the maximum).
pub const UNITS_DEFAULT_LIMIT: i64 = 2_000;
pub const UNITS_MAX_LIMIT: i64 = 20_000;

/// Stored display simplification of `geo.unit_geoms.geom_simplified` (degrees).
pub const DISPLAY_TOLERANCE: f64 = 0.0005;
/// Largest accepted numeric `simplify` tolerance (degrees).
pub const MAX_TOLERANCE: f64 = 0.1;

#[derive(Debug, Clone, Default)]
pub struct UnitsQuery {
    pub level: i16,
    /// Any ancestor code: units at `level` below it.
    pub parent: Option<String>,
    /// Exclusive: the last `pcode` of the previous page.
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct UnitItem {
    pub pcode: String,
    pub level: i16,
    /// Parent code (`null` for the country).
    pub parent: Option<String>,
    pub name_en: String,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
    /// Point on surface, `[lon, lat]` (GeoJSON order), 6 decimals.
    pub centroid: Option<[f64; 2]>,
    pub area_km2: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnitList {
    pub scheme: String,
    pub level: i16,
    pub parent: Option<String>,
    pub boundary_set: String,
    /// Units matching `level` (+ `parent`) over all pages.
    pub total: i64,
    pub limit: i64,
    pub units: Vec<UnitItem>,
    /// Pass as `cursor` for the next page; `null` on the last page.
    pub next_cursor: Option<String>,
}

#[derive(FromRow)]
struct UnitRow {
    pcode: String,
    level: i16,
    parent: Option<String>,
    name_en: String,
    name_si: Option<String>,
    name_ta: Option<String>,
    lon: Option<f64>,
    lat: Option<f64>,
    area_km2: Option<f64>,
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn reference_set(cat: &Catalog) -> CoreResult<&str> {
    cat.reference_set
        .as_deref()
        .ok_or_else(|| CoreError::not_found("reference boundary set (load the boundaries first)"))
}

/// Clamps a requested page size.
pub fn page_limit(limit: Option<i64>) -> CoreResult<i64> {
    match limit {
        None => Ok(UNITS_DEFAULT_LIMIT),
        Some(n) if (1..=UNITS_MAX_LIMIT).contains(&n) => Ok(n),
        Some(n) => Err(CoreError::bad_request(format!(
            "limit {n}: expected 1–{UNITS_MAX_LIMIT}"
        ))),
    }
}

/// Units of one level (optionally below `parent`), ordered by code, keyset-paginated.
pub async fn units(pool: &PgPool, cat: &Catalog, q: &UnitsQuery) -> CoreResult<UnitList> {
    let profile = cat.profile()?;
    let max = i16::from(profile.admin.max_level());
    if !(0..=max).contains(&q.level) {
        return Err(CoreError::bad_request(format!(
            "level {}: expected 0–{max}",
            q.level
        )));
    }
    let limit = page_limit(q.limit)?;
    let scheme = profile.admin_scheme().to_owned();
    let parent = match q.parent.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => {
            let p = profile
                .admin
                .parse(s)
                .map_err(|e| CoreError::bad_request(e.to_string()))?;
            if i16::from(p.level()) >= q.level {
                return Err(CoreError::bad_request(format!(
                    "parent {} is level {}: it must be above level {}",
                    p.as_str(),
                    p.level(),
                    q.level
                )));
            }
            Some(p.as_str().to_owned())
        }
    };
    let cursor = match q.cursor.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(c) => Some(
            profile
                .admin
                .parse(c)
                .map_err(|_| CoreError::bad_request(format!("cursor {c:?} is not a code")))?
                .as_str()
                .to_owned(),
        ),
    };
    let set = reference_set(cat)?;
    let parent = match parent {
        None => None,
        Some(code) => {
            let key: i32 = sqlx::query_scalar("SELECT key FROM geo.units WHERE code = $1")
                .bind(&code)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| CoreError::not_found(format!("no admin unit \"{code}\"")))?;
            Some((code, key))
        }
    };
    let parent_key = parent.as_ref().map(|p| p.1);
    let (total, rows) = tokio::join!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM geo.units u
             JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $2
             WHERE u.scheme_id = $1 AND u.level = $3 AND ($4::int IS NULL OR $4 = ANY(u.anc_keys))",
        )
        .bind(&scheme)
        .bind(set)
        .bind(q.level)
        .bind(parent_key)
        .fetch_one(pool),
        sqlx::query_as::<_, UnitRow>(
            "SELECT u.code AS pcode, u.level, p.code AS parent, u.name_en, u.name_si, u.name_ta,
                    ST_X(g.centroid) AS lon, ST_Y(g.centroid) AS lat, g.area_km2
             FROM geo.units u
             JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $2
             LEFT JOIN geo.units p ON p.key = u.parent_key
             WHERE u.scheme_id = $1 AND u.level = $3 AND ($4::int IS NULL OR $4 = ANY(u.anc_keys))
               AND ($5::text IS NULL OR u.code > $5)
             ORDER BY u.code LIMIT $6",
        )
        .bind(&scheme)
        .bind(set)
        .bind(q.level)
        .bind(parent_key)
        .bind(cursor.as_deref())
        .bind(limit + 1)
        .fetch_all(pool),
    );
    let (total, mut rows) = (total?, rows?);
    let more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
    if more {
        rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    let next_cursor = if more {
        rows.last().map(|r| r.pcode.clone())
    } else {
        None
    };
    Ok(UnitList {
        scheme,
        level: q.level,
        parent: parent.map(|p| p.0),
        boundary_set: set.to_owned(),
        total,
        limit,
        units: rows
            .into_iter()
            .map(|r| UnitItem {
                centroid: r.lon.zip(r.lat).map(|(x, y)| [round6(x), round6(y)]),
                area_km2: r.area_km2.map(|a| (a * 1e4).round() / 1e4),
                pcode: r.pcode,
                level: r.level,
                parent: r.parent,
                name_en: r.name_en,
                name_si: r.name_si,
                name_ta: r.name_ta,
            })
            .collect(),
        next_cursor,
    })
}

/// How a unit geometry is simplified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Simplify {
    /// Full-resolution reference geometry.
    Full,
    /// The stored display geometry (`ST_SimplifyPreserveTopology`, 0.0005° ≈ 55 m).
    Display,
    /// `ST_SimplifyPreserveTopology` with this tolerance in degrees.
    Tolerance(f64),
}

impl Simplify {
    /// `full` (default) | `display` | `coarse` (0.005°) | a tolerance in
    /// degrees, `0 < t ≤ 0.1`.
    pub fn parse(s: Option<&str>) -> CoreResult<Self> {
        let Some(s) = s.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::Full);
        };
        match s {
            "full" | "none" | "0" => Ok(Self::Full),
            "display" => Ok(Self::Display),
            "coarse" => Ok(Self::Tolerance(0.005)),
            _ => match s.parse::<f64>() {
                Ok(t) if t.is_finite() && t > 0.0 && t <= MAX_TOLERANCE => Ok(Self::Tolerance(t)),
                _ => Err(CoreError::bad_request(format!(
                    "simplify {s:?}: expected full, display, coarse or a tolerance in degrees (0 < t ≤ {MAX_TOLERANCE})"
                ))),
            },
        }
    }

    /// The tolerance in degrees (`0` = none).
    #[must_use]
    pub fn tolerance(self) -> f64 {
        match self {
            Self::Full => 0.0,
            Self::Display => DISPLAY_TOLERANCE,
            Self::Tolerance(t) => t,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnitGeometryProps {
    pub pcode: String,
    pub level: i16,
    pub parent: Option<String>,
    pub name_en: String,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
    pub boundary_set: String,
    pub area_km2: f64,
    /// Simplification tolerance applied, degrees (`0` = full resolution).
    pub simplify_tolerance: f64,
}

/// A GeoJSON Feature (`id` = p-code).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnitGeometry {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub bbox: [f64; 4],
    pub properties: UnitGeometryProps,
    /// GeoJSON MultiPolygon, coordinates with ≤ 6 decimals.
    #[schema(value_type = Object)]
    pub geometry: serde_json::Value,
}

#[derive(FromRow)]
struct GeomRow {
    pcode: String,
    level: i16,
    parent: Option<String>,
    name_en: String,
    name_si: Option<String>,
    name_ta: Option<String>,
    area_km2: f64,
    west: f64,
    south: f64,
    east: f64,
    north: f64,
    geojson: String,
}

/// One unit's geometry from the reference boundary set.
pub async fn unit_geometry(
    pool: &PgPool,
    cat: &Catalog,
    pcode: &str,
    simplify: Simplify,
) -> CoreResult<UnitGeometry> {
    let set = reference_set(cat)?;
    let row: GeomRow = sqlx::query_as(
        "SELECT u.code AS pcode, u.level, p.code AS parent, u.name_en, u.name_si, u.name_ta, g.area_km2,
                ST_XMin(g.bbox) AS west, ST_YMin(g.bbox) AS south, ST_XMax(g.bbox) AS east, ST_YMax(g.bbox) AS north,
                ST_AsGeoJSON(CASE WHEN $3 = 1 THEN coalesce(g.geom_simplified, g.geom)
                                  WHEN $4 > 0 THEN ST_Multi(ST_SimplifyPreserveTopology(g.geom, $4))
                                  ELSE g.geom END, 6) AS geojson
         FROM geo.units u
         JOIN geo.unit_geoms g ON g.unit_key = u.key AND g.boundary_set = $2
         LEFT JOIN geo.units p ON p.key = u.parent_key
         WHERE u.code = $1",
    )
    .bind(pcode)
    .bind(set)
    .bind(i32::from(simplify == Simplify::Display))
    .bind(match simplify {
        Simplify::Tolerance(t) => t,
        _ => 0.0,
    })
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| CoreError::not_found(format!("no admin unit \"{pcode}\"")))?;
    let geometry: serde_json::Value = serde_json::from_str(&row.geojson)
        .map_err(|e| CoreError::bad_request(format!("geometry of {pcode}: {e}")))?;
    Ok(UnitGeometry {
        kind: "Feature".to_owned(),
        id: row.pcode.clone(),
        bbox: [
            round6(row.west),
            round6(row.south),
            round6(row.east),
            round6(row.north),
        ],
        properties: UnitGeometryProps {
            pcode: row.pcode,
            level: row.level,
            parent: row.parent,
            name_en: row.name_en,
            name_si: row.name_si,
            name_ta: row.name_ta,
            boundary_set: set.to_owned(),
            area_km2: (row.area_km2 * 1e4).round() / 1e4,
            simplify_tolerance: simplify.tolerance(),
        },
        geometry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_parses() {
        assert_eq!(Simplify::parse(None).ok(), Some(Simplify::Full));
        assert_eq!(Simplify::parse(Some("")).ok(), Some(Simplify::Full));
        assert_eq!(Simplify::parse(Some("full")).ok(), Some(Simplify::Full));
        assert_eq!(
            Simplify::parse(Some("display")).ok(),
            Some(Simplify::Display)
        );
        assert_eq!(
            Simplify::parse(Some("coarse")).ok(),
            Some(Simplify::Tolerance(0.005))
        );
        assert_eq!(
            Simplify::parse(Some("0.001")).ok(),
            Some(Simplify::Tolerance(0.001))
        );
        for bad in ["-1", "0.5", "NaN", "inf", "fine"] {
            assert!(Simplify::parse(Some(bad)).is_err(), "{bad}");
        }
        assert!((Simplify::Display.tolerance() - DISPLAY_TOLERANCE).abs() < f64::EPSILON);
    }

    #[test]
    fn limits_clamp() {
        assert_eq!(page_limit(None).ok(), Some(UNITS_DEFAULT_LIMIT));
        assert_eq!(page_limit(Some(1)).ok(), Some(1));
        assert_eq!(
            page_limit(Some(UNITS_MAX_LIMIT)).ok(),
            Some(UNITS_MAX_LIMIT)
        );
        assert!(page_limit(Some(0)).is_err());
        assert!(page_limit(Some(UNITS_MAX_LIMIT + 1)).is_err());
    }
}
