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
        super::catalog,
        super::dataset,
        super::area,
        super::series,
        super::indicator_map,
        super::provenance,
        super::reverse,
    ),
    tags(
        (name = "meta", description = "Health"),
        (name = "catalog", description = "Datasets, sources, licences, freshness"),
        (name = "areas", description = "Admin units by p-code; reverse geocoding"),
        (name = "indicators", description = "Series and choropleth data"),
        (name = "provenance", description = "Row-level lineage of every value"),
    )
)]
pub struct ApiDoc;
