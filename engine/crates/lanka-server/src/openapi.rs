//! OpenAPI 3.1 document, generated from the handler annotations (utoipa).

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Lanka Data Layer API v2",
        description = "Open, p-code keyed data for Sri Lanka with row-level provenance. \
                       Every response is `{success, message, payload, meta}`; `meta` cites the \
                       release (data_version), exact dataset versions, sources and licences.",
        license(name = "Code: MIT; data: per-dataset licences in meta.licences"),
    ),
    paths(
        super::health,
        super::status_summary,
        super::status_sources,
        super::status_datasets,
        super::status_runs,
        super::status_events,
        super::status_incidents,
        super::catalog,
        super::dataset,
        super::area,
        super::series,
        super::indicator_map,
        super::frame,
        super::frame_release,
        super::provenance,
        super::reverse,
        super::oembed,
        super::grids::point,
        super::grids::radius_handler,
        super::grids::list,
        super::grids::detail,
        super::grids::tile,
        super::grids::values_tile,
    ),
    tags(
        (name = "ops", description = "Health and status: emitted for external monitoring (no alert delivery)"),
        (name = "catalog", description = "Datasets, sources, licences, freshness"),
        (name = "areas", description = "Admin units by p-code; reverse geocoding"),
        (name = "indicators", description = "Series, choropleth data and frames"),
        (name = "provenance", description = "Row-level lineage of every value"),
        (name = "embeds", description = "oEmbed for map URLs"),
        (name = "grids", description = "Canonical 0.001° grid layers: point values, radius statistics, PNG tiles"),
    )
)]
pub struct ApiDoc;
