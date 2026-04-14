use std::{net::SocketAddr, sync::Arc};

use axum::{middleware, routing::get, Router};
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use media::handler::rest;
use media::handler::MediaHandler;
use media::manager::biz::MediaBiz;
use media::manager::repository::MediaRepository;
use media::pb::service::media::media_service_server::MediaServiceServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let rust_log = std::env::var("RUST_LOG").ok();
    philand_logging::init(
        "media",
        rust_log.as_deref().or(Some("media=debug,tower_http=debug")),
    );

    let app_info = philand_application::from_env_with_prefix("MEDIA_APP");
    tracing::info!("starting {}", app_info.user_agent());

    let config = philand_configs::MediaServiceConfig::from_env()
        .map_err(|e| anyhow::anyhow!("Failed to load config: {e}"))?;

    let grpc_host = config.grpc_host.clone();
    let grpc_port = config.grpc_port;
    let http_host = config.http_host.clone();
    let http_port = config.http_port;

    let repo = MediaRepository::new(&config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to init media repository: {e}"))?;

    let biz = Arc::new(
        MediaBiz::new(repo, config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to init media business layer: {e}"))?,
    );

    let grpc_handler = MediaHandler::new(biz.clone());
    let rest_handler = Arc::new(MediaHandler::new(biz.clone()));

    let x_request_id = axum::http::HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/health", get(health_check))
        .merge(rest::router())
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .layer(middleware::from_fn(rest::request_logging_middleware))
        .with_state(rest_handler);

    let grpc_addr: SocketAddr = format!("{}:{}", grpc_host, grpc_port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid gRPC bind address: {e}"))?;
    let grpc_server = tonic::transport::Server::builder()
        .add_service(MediaServiceServer::new(grpc_handler))
        .serve(grpc_addr);

    tracing::info!("gRPC server listening on {}", grpc_addr);

    let addr: SocketAddr = format!("{}:{}", http_host, http_port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address: {e}"))?;

    tracing::info!("HTTP server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tokio::select! {
        res = grpc_server => {
            if let Err(e) = res {
                tracing::error!("gRPC server error: {}", e);
            }
        }
        res = axum::serve(listener, app) => {
            if let Err(e) = res {
                tracing::error!("HTTP server error: {}", e);
            }
        }
    }

    Ok(())
}

async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "service": "media",
    }))
}
