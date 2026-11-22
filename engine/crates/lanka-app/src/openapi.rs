//! OpenAPI 3.1 document of the app plane (`/app/v1/openapi.json`). The app
//! plane is a product API versioned separately from the open `/v2` contract.

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Lanka Data Layer app plane v1",
        description = "Share and embed composed maps (ldl-map/1) without an account: \
                       content-addressed snapshots, resolution and data packages. \
                       Never cached by the proxy; snapshot creation is rate-limited.",
        license(name = "Code: MIT; map documents: per document meta.licence"),
    ),
    paths(
        super::routes::create_snapshot,
        super::routes::resolve_document,
        super::routes::get_map,
        super::routes::get_resolved,
        super::routes::get_package,
    ),
    tags((name = "maps", description = "Snapshots, resolution, packages")),
)]
pub struct AppApiDoc;
