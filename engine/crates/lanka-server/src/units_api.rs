//! Bulk unit lists and unit geometry.
//!
//! | Route | Payload |
//! |---|---|
//! | `GET /v2/units?level=&parent=&cursor=&limit=` | codes, names (en/si/ta), parent, centroid `[lon, lat]`, area per unit; keyset pages by code |
//! | `GET /v2/units/{pcode}/geometry?simplify=&format=` | one GeoJSON Feature (≤ 6 decimals); `format=geojson` = bare Feature |
//!
//! Both read the reference boundary set; `meta` cites the boundary dataset,
//! so the ETag changes with the release and the boundary dsv.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::get;
use lanka_core::Envelope;
use lanka_core::envelope::Link;
use lanka_core::geo_units::{self, Simplify, UnitGeometry, UnitList, UnitsQuery};
use serde::Deserialize;
use utoipa::{IntoParams, OpenApi};

use crate::{ApiError, ApiResult, AppState, ErrorBody, respond};

#[derive(OpenApi)]
#[openapi(paths(units_list, unit_geometry), tags((name = "areas")))]
pub struct UnitsDoc;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v2/units", get(units_list))
        .route("/v2/units/{pcode}/geometry", get(unit_geometry))
}

fn err(st: &AppState, e: lanka_core::CoreError) -> ApiError {
    ApiError::from(e).with_version(st.catalog().data_version.clone())
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct UnitsParams {
    /// Admin level, 0 (country) … 4 (GN division). Required.
    pub level: Option<i16>,
    /// Any ancestor p-code: only units below it, e.g. `LK1` or `LK1103`.
    pub parent: Option<String>,
    /// `next_cursor` of the previous page (exclusive; the last code returned).
    pub cursor: Option<String>,
    /// 1–20000 (default 2000).
    pub limit: Option<i64>,
}

/// Every unit of one admin level (optionally below a parent): codes, names, parent, centroid, area.
#[utoipa::path(get, path = "/v2/units", tag = "areas", params(UnitsParams),
    responses((status = 200, body = Envelope<UnitList>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn units_list(
    State(st): State<AppState>,
    Query(p): Query<UnitsParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let Some(level) = p.level else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "level is required: /v2/units?level=0…4[&parent=LK1]",
        )
        .with_version(cat.data_version.clone()));
    };
    let q = UnitsQuery {
        level,
        parent: p.parent,
        cursor: p.cursor,
        limit: p.limit,
    };
    let list = geo_units::units(&st.pool, &cat, &q)
        .await
        .map_err(|e| err(&st, e))?;
    let mut next = Vec::new();
    if let Some(c) = &list.next_cursor {
        let kept: Vec<&str> = uri
            .query()
            .unwrap_or_default()
            .split('&')
            .filter(|kv| !kv.is_empty() && !kv.starts_with("cursor="))
            .collect();
        next.push(Link::new(
            "next",
            format!("{}?{}&cursor={c}", uri.path(), kept.join("&")),
        ));
    }
    if let Some(u) = list.units.first() {
        next.push(Link::new(
            "geometry",
            format!("/v2/units/{}/geometry?simplify=display", u.pcode),
        ));
    }
    let meta = cat.meta_for(cat.boundary_dataset(), next);
    respond(&headers, &uri, list, meta)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct GeometryParams {
    /// `full` (default) | `display` (stored, 0.0005° ≈ 55 m) | `coarse` (0.005°) |
    /// a tolerance in degrees, `0 < t ≤ 0.1` (`ST_SimplifyPreserveTopology`).
    pub simplify: Option<String>,
    /// `json` (default, enveloped) | `geojson` (bare Feature, `application/geo+json`).
    pub format: Option<String>,
}

/// One unit's boundary as a GeoJSON Feature (reference boundary set, ≤ 6 decimals).
#[utoipa::path(get, path = "/v2/units/{pcode}/geometry", tag = "areas",
    params(("pcode" = String, Path, description = "p-code, e.g. LK11 or LK1103005"), GeometryParams),
    responses((status = 200, body = Envelope<UnitGeometry>), (status = 400, body = ErrorBody),
              (status = 404, body = ErrorBody)))]
async fn unit_geometry(
    State(st): State<AppState>,
    Path(pcode): Path<String>,
    Query(p): Query<GeometryParams>,
    headers: HeaderMap,
    uri: Uri,
) -> ApiResult {
    let cat = st.catalog();
    let code = crate::parse_pcode(&cat, &pcode)?;
    let simplify = Simplify::parse(p.simplify.as_deref()).map_err(|e| err(&st, e))?;
    let geojson = match p.format.as_deref() {
        None | Some("" | "json") => false,
        Some("geojson") => true,
        Some(f) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("format {f:?}: expected json or geojson"),
            )
            .with_version(cat.data_version.clone()));
        }
    };
    let f = geo_units::unit_geometry(&st.pool, &cat, code.as_str(), simplify)
        .await
        .map_err(|e| err(&st, e))?;
    let meta = cat.meta_for(
        cat.boundary_dataset(),
        vec![Link::new("area", format!("/v2/areas/{}", f.id))],
    );
    if geojson {
        let etag = crate::etag_of(&uri, &meta);
        if let Some(r) = crate::not_modified(&headers, &etag, crate::CACHE_FLOATING) {
            return Ok(r);
        }
        let mut res = axum::Json(&f).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/geo+json"),
        );
        return crate::with_cache(res, &etag, crate::CACHE_FLOATING);
    }
    respond(&headers, &uri, f, meta)
}
