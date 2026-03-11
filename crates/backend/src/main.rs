mod db;
mod git;
mod routes;
mod state;
mod task_runner;

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use tracing::info;

use routes::{
    tasks::{cancel_task, create_task, get_task, list_tasks},
    ws::ws_handler,
};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present (dev convenience)
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");

    let db = sqlx::PgPool::connect(&database_url).await?;

    // Run migrations embedded in the binary
    sqlx::migrate!("../../migrations").run(&db).await?;

    info!("database migrations applied");

    let state = AppState::new(db);
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()
        .expect("PORT must be a number");

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./dist".into());

    let app = Router::new()
        // REST API
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/:id", get(get_task))
        .route("/api/tasks/:id", delete(cancel_task))
        // WebSocket
        .route("/ws", get(ws_handler))
        // Static frontend (Dioxus WASM build)
        .fallback_service(ServeDir::new(&static_dir))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("claude-container listening on http://0.0.0.0:{port}");

    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        info!("No ANTHROPIC_API_KEY — will use ~/.claude/ subscription credentials");
    }

    axum::serve(listener, app).await?;

    Ok(())
}
