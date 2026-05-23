mod api;
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

    let discord = Arc::new(services::discord::DiscordNotifier::new(
        config.discord.webhook_url.clone(),
    ));
    if discord.is_enabled() {
        tracing::info!("Discord notifications enabled");
    }

    let email = Arc::new(services::email::EmailService::new(
        &config.smtp,
        &config.server.frontend_url,
        &config.server.base_url,
    ));

    let cleanup_pool = db_pool.clone();
    let cleanup_storage = storage.clone();
    let cleanup_email = Arc::clone(&email);
    tokio::spawn(async move {
        services::start_cleanup_task(cleanup_pool, cleanup_storage, cleanup_email).await;
    });
    tracing::info!("Cleanup background task started");
    if email.is_enabled() {
        tracing::info!("Email notifications enabled");
    }

    let app = routes::create_router(config.clone(), db_pool, storage, discord, email);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = ?e, "Failed to install Ctrl-C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("SIGINT received, starting graceful shutdown"),
        _ = terminate => tracing::info!("SIGTERM received, starting graceful shutdown"),
    }
}
