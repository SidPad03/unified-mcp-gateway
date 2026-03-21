use axum::{
    Router,
    http::{header, HeaderValue, Method},
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod agent;
mod api;
mod audit;
mod backends;
mod config;
mod db;
mod errors;
mod gateway;
mod metrics;
mod policy;

pub use errors::AppError;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub jwt_secret: String,
    pub metrics: Arc<metrics::MetricsCollector>,
    pub audit: Option<Arc<audit::AuditRecorder>>,
    pub backend_manager: Arc<backends::BackendManager>,
    pub agent_registry: Arc<agent::AgentRegistry>,
    pub agent_release_cache: Arc<tokio::sync::Mutex<Option<(std::time::Instant, Vec<api::agent_releases::AgentRelease>)>>>,
    /// Broadcast channel for live audit events → dashboard WebSocket clients
    pub event_tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "mcp_gateway_server=info,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://mcpgateway:mcpgateway@localhost:5432/mcpgateway".into());

    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "mcpgw-dev-secret-change-in-production".into());
    if jwt_secret == "mcpgw-dev-secret-change-in-production" {
        tracing::warn!("Using default JWT secret — set JWT_SECRET env var for production!");
    }

    let listen_addr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3200".into());

    tracing::info!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    tracing::info!("Running migrations...");
    db::run_migrations(&pool).await?;

    tracing::info!("Seeding default data...");
    db::seed::seed_defaults(&pool).await?;

    let metrics_collector = Arc::new(metrics::MetricsCollector::new());
    let (event_tx, _) = broadcast::channel::<String>(512);
    let audit_recorder = Arc::new(audit::AuditRecorder::new(pool.clone(), event_tx.clone()));
    let backend_manager = Arc::new(backends::BackendManager::new());
    let agent_registry = Arc::new(agent::AgentRegistry::new());

    tracing::info!("Starting enabled backends...");
    start_backends(&pool, &backend_manager).await;

    let state = AppState {
        db: pool,
        jwt_secret,
        metrics: metrics_collector,
        audit: Some(audit_recorder),
        backend_manager,
        agent_registry,
        agent_release_cache: Arc::new(tokio::sync::Mutex::new(None)),
        event_tx,
    };

    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:8080".parse::<HeaderValue>().unwrap(),
            "http://localhost:5173".parse::<HeaderValue>().unwrap(),
        ])
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
        .allow_credentials(true);

    let app = Router::new()
        .merge(api::mcp::mcp_router())
        .route("/agent/ws", axum::routing::get(agent::agent_ws_handler))
        .route("/api/v1/ws/live", axum::routing::get(api::live::live_ws_handler))
        .nest("/api/v1", api::router())
        .route("/metrics", axum::routing::get(metrics::prometheus_handler))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    tracing::info!("MCP Gateway Server listening on {}", listen_addr);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn start_backends(pool: &sqlx::PgPool, manager: &Arc<backends::BackendManager>) {
    let rows: Result<Vec<(uuid::Uuid, String, String, serde_json::Value)>, _> = sqlx::query_as(
        "SELECT backend_id, name, transport, config FROM backends WHERE is_enabled = TRUE"
    )
    .fetch_all(pool)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to load backends: {}", e);
            return;
        }
    };

    for (backend_id, name, transport, config) in rows {
        let result = match transport.as_str() {
            "stdio" => manager.spawn_backend(backend_id, &name, &config).await,
            "streamable-http" => backends::BackendManager::discover_http_tools(&name, &config).await,
            "sse" => backends::BackendManager::discover_sse_tools(&name, &config).await,
            "agent" => {
                tracing::info!(backend = %name, "Agent backend waiting for remote agent connection");
                continue;
            }
            other => {
                tracing::warn!(backend = %name, transport = %other, "Skipping backend with unknown transport");
                continue;
            }
        };

        match result {
            Ok(tools) => {
                register_discovered_tools(pool, backend_id, &name, &tools).await;
                let _ = sqlx::query("UPDATE backends SET health_status = 'healthy', last_health_check = NOW() WHERE backend_id = $1")
                    .bind(backend_id).execute(pool).await;
                tracing::info!(backend = %name, transport = %transport, tools = tools.len(), "Backend started successfully");
            }
            Err(e) => {
                let _ = sqlx::query("UPDATE backends SET health_status = 'unhealthy', last_health_check = NOW() WHERE backend_id = $1")
                    .bind(backend_id).execute(pool).await;
                tracing::error!(backend = %name, transport = %transport, error = %e, "Failed to start backend");
            }
        }
    }
}

pub async fn register_discovered_tools(
    pool: &sqlx::PgPool,
    backend_id: uuid::Uuid,
    backend_name: &str,
    tools: &[backends::DiscoveredTool],
) {
    // Collect the original_names we're about to register so we can prune stale tools after.
    let mut registered_names: Vec<String> = Vec::with_capacity(tools.len());

    for tool in tools {
        let namespaced = format!("{}__{}", backend_name, tool.name);
        let auto_risk = backends::classifier::classify_tool(&tool.name, &tool.description);
        let tool_id = uuid::Uuid::new_v4();

        // UPSERT: insert new tools with auto-classification, but preserve the
        // existing risk_category for tools that already exist (manual overrides).
        let result = sqlx::query(
            "INSERT INTO tool_registry (tool_id, tool_name, backend_id, original_name, description, input_schema, risk_category, is_enabled, last_seen)
             VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, NOW())
             ON CONFLICT (backend_id, original_name) DO UPDATE SET
               tool_name = EXCLUDED.tool_name,
               description = EXCLUDED.description,
               input_schema = EXCLUDED.input_schema,
               is_enabled = TRUE,
               last_seen = NOW()"
        )
        .bind(tool_id)
        .bind(&namespaced)
        .bind(backend_id)
        .bind(&tool.name)
        .bind(&tool.description)
        .bind(&tool.input_schema)
        .bind(auto_risk)
        .execute(pool)
        .await;

        if let Err(e) = result {
            tracing::warn!(tool = %namespaced, error = %e, "Failed to register tool");
        }

        registered_names.push(tool.name.clone());
    }

    // Remove tools that are no longer provided by this backend
    if !registered_names.is_empty() {
        let result = sqlx::query(
            "DELETE FROM tool_registry WHERE backend_id = $1 AND original_name != ALL($2)"
        )
        .bind(backend_id)
        .bind(&registered_names)
        .execute(pool)
        .await;

        if let Err(e) = result {
            tracing::warn!(backend_id = %backend_id, error = %e, "Failed to prune stale tools");
        }
    }
}
