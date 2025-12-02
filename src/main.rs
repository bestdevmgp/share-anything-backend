mod config;
mod db;
mod docs;
mod handlers;
mod middleware;
mod models;
mod routes;
mod services;
mod utils;

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,share_anything=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Arc::new(config::Config::from_env()?);
    tracing::info!("Configuration loaded successfully");

    let db_pool = db::create_pool(&config.database.url).await?;
    tracing::info!("Database connection pool created");

    let storage = services::StorageService::new(
        config.s3.endpoint.clone(),
        config.s3.region.clone(),
        config.s3.bucket_name.clone(),
        config.s3.access_key_id.clone(),
        config.s3.secret_access_key.clone(),
    )
    .await?;
    tracing::info!("Storage service initialized");

    let cleanup_pool = db_pool.clone();
    let cleanup_storage = storage.clone();
    tokio::spawn(async move {
        services::start_cleanup_task(cleanup_pool, cleanup_storage).await;
    });
    tracing::info!("Cleanup background task started");

    let app = routes::create_router(config.clone(), db_pool, storage);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
