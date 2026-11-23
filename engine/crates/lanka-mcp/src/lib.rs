//! `lanka-mcp` — the Lanka Data Layer MCP server (WP-14, WP-34; 06 §4, 11 §11.2).
//!
//! Anonymous and read-only (one exception: `create_map_link` stores an
//! immutable, content-addressed map snapshot through the app plane's own
//! route logic). Every tool calls the same query-core functions the REST v2
//! routes call, so the numbers agree by construction (ADR 0015).
//!
//! Transports (thin):
//!
//! - **Streamable HTTP** at `/mcp`, mounted into the REST axum app by
//!   `lanka-server` ([`router`]). Stateless (no session store): every POST
//!   is answered as one JSON body, so it works behind nginx and Cloudflare
//!   without sticky routing. Clients must send
//!   `Accept: application/json, text/event-stream`.
//! - **stdio** for `lanka mcp` ([`serve_stdio`]).
//!
//! Protocol revisions: [`PROTOCOL_VERSIONS`] — every revision the pinned SDK
//! (`rmcp` 3.4.0) implements. `initialize` answers with the client's
//! revision when it has an `initialize` handshake and is listed, else with
//! the newest handshake revision.

use std::borrow::Cow;
use std::sync::Arc;

use axum::Router;
use lanka_core::Catalog;
use lanka_core::query::ReverseResult;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;

pub mod out;
pub mod tools;

pub use out::{MAX_RESULT_BYTES, MAX_ROWS};

/// The SDK this server is built on, pinned exactly in the workspace.
pub const SDK: &str = "rmcp 3.4.0";

/// Every MCP protocol revision the pinned SDK implements, oldest first.
/// The one list: `initialize` negotiation, per-request version checks,
/// `server/discover` and `GET /v2/mcp` all read it.
pub const PROTOCOL_VERSIONS: &[ProtocolVersion] = ProtocolVersion::KNOWN_VERSIONS;

/// `serverInfo.name`.
pub const SERVER_NAME: &str = "lanka-data-layer";
/// `serverInfo.title`.
pub const SERVER_TITLE: &str = "Lanka Data Layer";
/// The crate version, reported as `serverInfo.version` and by `GET /v2/mcp`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where the Streamable HTTP transport is mounted.
pub const ENDPOINT: &str = "/mcp";
/// The CLI command that runs the stdio transport.
pub const STDIO_COMMAND: &str = "lanka mcp";

/// Guidance every client receives at `initialize`.
const INSTRUCTIONS: &str = "Open, read-only data for Sri Lanka, keyed by COD-AB p-codes (LK, LK1, LK11, LK1103, …). \
Start with search_catalog to find an indicator, resolve_place to turn a name or a point into a p-code, then get_area_profile / query_indicator / rank_areas / compare_areas. \
Every result carries data_version, attribution[], licences[] and next[] (ready-to-send follow-up calls): cite the attribution. \
Each value has a `basis` (reported, computed, estimated, forecast): never present an estimated or forecast value as an official figure. \
Associations from analyze_relationship are never causal; read the guards first.";

/// Resolves a point through the server's grid fast path (WP-05) when the
/// cell lies wholly inside one unit; `None` = use the polygon query.
pub type ReverseFn = Arc<dyn Fn(f64, f64) -> Option<ReverseResult> + Send + Sync>;

/// The current catalogue snapshot (the server's atomically swapped one).
pub type CatalogFn = lanka_app::CatalogFn;

/// What the tools read from. Built by `lanka-server` (it owns the pool, the
/// catalogue reload, the grids, the analysis engine and the app plane) and
/// by `lanka mcp`.
#[derive(Clone)]
pub struct McpContext {
    pub pool: PgPool,
    pub catalog: CatalogFn,
    /// The analysis engine; without it `analyze_relationship` and
    /// `get_analysis` are not registered.
    pub analysis: Option<Arc<lanka_analysis::Engine>>,
    /// The app plane; without it `create_map_link` is not registered.
    pub app: Option<lanka_app::AppPlane>,
    pub reverse: Option<ReverseFn>,
}

impl McpContext {
    /// The origin of absolute links (`LANKA_PUBLIC_BASE_URL`; `""` =
    /// relative), as the REST server computes it.
    #[must_use]
    pub fn public_base(&self) -> String {
        let cat = (self.catalog)();
        lanka_core::PublicBase::global().origin(
            cat.profile
                .as_deref()
                .map(|p| p.spec.site.base_url.as_str()),
        )
    }

    pub(crate) fn catalog(&self) -> Arc<Catalog> {
        (self.catalog)()
    }
}

/// One advertised tool, for `GET /v2/mcp`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSummary {
    pub name: String,
    pub title: String,
    pub description: String,
    pub read_only: bool,
}

/// The discovery document of `GET /v2/mcp` (the web app's "Connect" panel).
/// The tool list is generated from the registered tools.
#[must_use]
pub fn discovery(ctx: &McpContext) -> Value {
    let tools: Vec<ToolSummary> = tools::registered(ctx)
        .into_iter()
        .map(|t| ToolSummary {
            name: t.name.to_owned(),
            title: t.title.to_owned(),
            description: t.description.to_owned(),
            read_only: t.read_only,
        })
        .collect();
    json!({
        "name": SERVER_NAME,
        "title": SERVER_TITLE,
        "version": VERSION,
        "endpoint": ENDPOINT,
        "transports": ["streamable-http", "stdio"],
        "protocol_versions": PROTOCOL_VERSIONS.iter().map(ProtocolVersion::as_str).collect::<Vec<_>>(),
        "stdio_command": STDIO_COMMAND,
        "tools": tools,
    })
}

/// The MCP handler: one cheap clone per HTTP request.
#[derive(Clone)]
pub struct LankaMcp {
    ctx: Arc<McpContext>,
}

impl LankaMcp {
    #[must_use]
    pub fn new(ctx: McpContext) -> Self {
        Self { ctx: Arc::new(ctx) }
    }

    /// The registered tools as MCP `Tool`s.
    #[must_use]
    pub fn tool_list(&self) -> Vec<Tool> {
        tools::registered(&self.ctx)
            .into_iter()
            .map(|d| {
                let schema = match (d.schema)() {
                    Value::Object(m) => m,
                    _ => serde_json::Map::new(),
                };
                let annotations = ToolAnnotations::new()
                    .read_only(d.read_only)
                    .destructive(false)
                    .idempotent(true)
                    .open_world(false);
                Tool::new(d.name, d.description, Arc::new(schema))
                    .with_title(d.title)
                    .with_annotations(annotations)
            })
            .collect()
    }
}

impl ServerHandler for LankaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(SERVER_NAME, VERSION)
                    .with_title(SERVER_TITLE)
                    .with_description("Open geo-data and statistics for Sri Lanka"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(PROTOCOL_VERSIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tool_list()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_list().into_iter().find(|t| t.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let caller = tools::Caller::from_extensions(&context.extensions);
        let args = Value::Object(request.arguments.unwrap_or_default());
        match tools::call(&self.ctx, request.name.as_ref(), args, &caller).await {
            Some(result) => Ok(result.into()),
            None => Err(rmcp::ErrorData::invalid_params(
                format!("unknown tool \"{}\"", request.name),
                Some(
                    json!({ "tools": tools::registered(&self.ctx).iter().map(|t| t.name).collect::<Vec<_>>() }),
                ),
            )),
        }
    }
}

// ───────────────────────────── transports ──────────────────────────────────

/// Hosts the Streamable HTTP transport accepts in `Host`:
/// `LANKA_MCP_ALLOWED_HOSTS` (comma-separated `host` or `host:port`).
/// Unset = any host (the server binds loopback behind nginx, where `Host`
/// is the public name).
fn allowed_hosts() -> Vec<String> {
    std::env::var("LANKA_MCP_ALLOWED_HOSTS")
        .ok()
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The Streamable HTTP transport as a router with `/mcp`: stateless, JSON
/// responses, open CORS (anonymous data; no cookies or credentials).
pub fn router(ctx: McpContext) -> Router {
    use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };
    use tower_http::cors::{Any, CorsLayer};

    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .disable_allowed_hosts();
    let hosts = allowed_hosts();
    if !hosts.is_empty() {
        config = config.with_allowed_hosts(hosts);
    }
    let handler = LankaMcp::new(ctx);
    let service: StreamableHttpService<LankaMcp, NeverSessionManager> = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(Any)
        .expose_headers(Any);
    Router::new().nest_service(ENDPOINT, service).layer(cors)
}

/// Runs the stdio transport until the client closes it.
pub async fn serve_stdio(ctx: McpContext) -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    let service = LankaMcp::new(ctx)
        .serve(rmcp::transport::io::stdio())
        .await
        .map_err(|e| anyhow::anyhow!("mcp stdio: {e}"))?;
    service
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("mcp stdio: {e}"))?;
    Ok(())
}
