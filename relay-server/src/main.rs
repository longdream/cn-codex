mod pc_handler;
mod phone_handler;
mod protocol;
mod room;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{any, get};
use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

use room::RoomManager;

pub struct AppState {
    pub room_manager: RoomManager,
    pub static_dir: PathBuf,
}

#[derive(Parser)]
#[command(name = "cn-codex-relay", about = "Relay server for cn-codex mobile")]
struct Args {
    /// Server port
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Path to mobile-dist static files
    #[arg(short, long, default_value = "./mobile-dist")]
    static_dir: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cn_codex_relay=info".into()),
        )
        .init();

    let args = Args::parse();
    let state = Arc::new(AppState {
        room_manager: RoomManager::new(),
        static_dir: args.static_dir.clone(),
    });

    let app = Router::new()
        .route("/pc/{room_id}", get(pc_handler::pc_ws_handler))
        .route("/ws/{room_id}", get(phone_handler::phone_ws_handler))
        .route("/api/{room_id}/{*path}", any(phone_handler::phone_api_proxy))
        .route("/m/{room_id}", get(serve_spa_index))
        .route("/m/{room_id}/{*rest}", get(serve_spa_index_with_rest))
        .nest_service("/assets", ServeDir::new(args.static_dir.join("assets")))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    info!("Relay server starting on {addr}");
    info!("Static files from: {:?}", args.static_dir);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn serve_spa_index(
    Path(_room_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    serve_index_html(&state.static_dir).await
}

async fn serve_spa_index_with_rest(
    Path((_room_id, _rest)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    serve_index_html(&state.static_dir).await
}

async fn serve_index_html(static_dir: &std::path::Path) -> axum::response::Response {
    let index_path = static_dir.join("index.html");
    match tokio::fs::read_to_string(&index_path).await {
        Ok(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "index.html not found").into_response(),
    }
}
