//! `lanka-app` — the app plane (`/app/v1`; 12 §2, ADR 0023), mounted by
//! `lanka-server` when `LANKA_APP_PLANE=on`.
//!
//! Stage 0 (WP-28): anyone can share and embed a composed map without an
//! account.
//!
//! - [`snapshots`]: `app.map_snapshots` — anonymous, immutable,
//!   content-addressed documents (migration 0009).
//! - [`routes`]: `POST /app/v1/maps/snapshots`, `POST /app/v1/maps/resolve`,
//!   `GET /app/v1/maps/{id}`, `/{id}/resolved`, `/{id}/package`.
//! - [`package`]: the Frictionless `datapackage.json` of a map.
//! - [`embed`]: oEmbed for map URLs (served under `/v2/oembed`).
//! - [`scope`]: [`Scope`] and [`Principal`] — every workspace query takes a scope.
//! - [`rate`]: [`RateIdentity`], salted monthly IP hashes, token buckets.
//!
//! Plane rules: the app plane has its own pool (pool C, role `lanka_app`:
//! reads the public plane, writes only `app.*`); `/v2` handlers never see
//! it. Nothing under `/app/` is stored by the proxy (nginx `location /app/`).

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderName, Method, header};
use axum::routing::{get, post};
use lanka_core::Catalog;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Executor, PgPool};
use tower_http::cors::{Any, CorsLayer};

pub mod config;
pub mod embed;
pub mod openapi;
pub mod package;
pub mod rate;
pub mod routes;
pub mod scope;
pub mod snapshots;

pub use config::{AppConfig, Rate};
pub use rate::{IpHasher, Limiter, RateIdentity};
pub use scope::{Principal, Scope};

/// The current catalogue snapshot (the server's atomically swapped one).
pub type CatalogFn = Arc<dyn Fn() -> Arc<Catalog> + Send + Sync>;

/// The role every app-plane connection must run as.
pub const APP_ROLE: &str = "lanka_app";

/// State of the mounted app plane.
#[derive(Clone)]
pub struct AppPlane {
    pub(crate) pool: PgPool,
    pub(crate) catalog: CatalogFn,
    pub(crate) cfg: Arc<AppConfig>,
    pub(crate) hasher: Arc<IpHasher>,
    pub(crate) snapshot_limiter: Arc<Limiter>,
}

impl AppPlane {
    #[must_use]
    pub fn new(pool: PgPool, catalog: CatalogFn, cfg: AppConfig) -> Self {
        let hasher = match &cfg.ip_salt {
            Some(s) => IpHasher::new(s.as_bytes()),
            None => {
                tracing::warn!(
                    "LANKA_IP_HASH_SALT is not set: client-address hashes use a per-process secret"
                );
                IpHasher::random()
            }
        };
        Self {
            pool,
            catalog,
            snapshot_limiter: Arc::new(Limiter::new(cfg.snapshot_rate)),
            hasher: Arc::new(hasher),
            cfg: Arc::new(cfg),
        }
    }

    #[must_use]
    pub fn config(&self) -> &AppConfig {
        &self.cfg
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// The origin used in links: `LANKA_PUBLIC_URL`, else the profile's
    /// `site.base_url`.
    #[must_use]
    pub fn public_url(&self) -> String {
        if let Some(u) = &self.cfg.public_url {
            return u.clone();
        }
        (self.catalog)()
            .profile
            .as_deref()
            .map(|p| p.spec.site.base_url.trim_end_matches('/').to_owned())
            .unwrap_or_default()
    }

    /// The `/app/v1` routes with their own CORS (bearer keys from any
    /// origin, no cookies cross-origin) and body limit.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/app/v1/maps/snapshots", post(routes::create_snapshot))
            .route("/app/v1/maps/resolve", post(routes::resolve_document))
            .route("/app/v1/maps/{id}", get(routes::get_map))
            .route("/app/v1/maps/{id}/resolved", get(routes::get_resolved))
            .route("/app/v1/maps/{id}/package", get(routes::get_package))
            .route("/app/v1/openapi.json", get(routes::openapi_json))
            .with_state(self.clone())
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST])
                    .allow_headers([
                        header::CONTENT_TYPE,
                        header::AUTHORIZATION,
                        header::IF_NONE_MATCH,
                    ])
                    .expose_headers([
                        header::ETAG,
                        header::LOCATION,
                        header::RETRY_AFTER,
                        HeaderName::from_static("x-robots-tag"),
                    ]),
            )
    }

    /// Deletes expired anonymous snapshots once a day (10 §5).
    pub fn spawn_expiry(&self) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(86_400));
            loop {
                every.tick().await;
                match snapshots::expire(&pool).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(deleted = n, "expired anonymous map snapshots"),
                    Err(e) => tracing::warn!(error = %e, "snapshot expiry failed"),
                }
            }
        });
    }
}

/// Connects pool C (8 connections, 12 §2). With `LANKA_APP_DATABASE_URL`
/// the login role must be `lanka_app`; without it, `fallback` (the server's
/// own options) is used with `SET ROLE lanka_app` on every connection.
/// Either way the pool refuses to start unless `current_user` is `lanka_app`.
pub async fn connect(cfg: &AppConfig, fallback: &PgConnectOptions) -> anyhow::Result<PgPool> {
    let (opts, set_role) = match &cfg.database_url {
        Some(u) => (u.parse::<PgConnectOptions>()?, false),
        None => (fallback.clone(), true),
    };
    let opts = opts
        .application_name("lanka-app")
        .options([("statement_timeout", "5s"), ("timezone", "UTC")]);
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .after_connect(move |conn, _| {
            Box::pin(async move {
                if set_role {
                    conn.execute("SET ROLE lanka_app").await?;
                }
                Ok(())
            })
        })
        .connect_with(opts)
        .await?;
    let who: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    anyhow::ensure!(
        who == APP_ROLE,
        "the app plane must run as {APP_ROLE}, not {who} (set LANKA_APP_DATABASE_URL)"
    );
    Ok(pool)
}
