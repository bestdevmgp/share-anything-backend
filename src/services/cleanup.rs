use crate::db::{repository, DbPool};
use crate::services::email::EmailService;
use crate::services::StorageService;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::error;

pub async fn start_cleanup_task(pool: DbPool, storage: StorageService, email_service: Arc<EmailService>) {
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;

        if let Err(e) = repository::delete_expired_public_share_grants(&pool).await {
            error!("Failed to delete expired public share grants: {}", e);
        }

        match repository::delete_expired_file_shares(&pool).await {
            Ok(storage_keys) => {
                if !storage_keys.is_empty() {
                    if let Err(e) = storage.delete_files(storage_keys.clone()).await {
                        error!(
                            "Failed to delete files from storage: {}. Keys: {:?}",
                            e, storage_keys
                        );
                    }
                }
            }
            Err(e) => {
                error!("Failed to delete expired file shares from database: {}", e);
            }
        }

        if let Err(e) = repository::delete_expired_cli_auth_sessions(&pool).await {
            error!("Failed to delete expired CLI auth sessions: {}", e);
        }

        if let Err(e) = repository::delete_expired_sessions(&pool).await {
            error!("Failed to delete expired sessions: {}", e);
        }

        if let Err(e) = notify_expiring_api_keys(&pool, &email_service).await {
            error!("Failed to notify expiring API keys: {}", e);
        }
    }
}

async fn notify_expiring_api_keys(
    pool: &DbPool,
    email_service: &EmailService,
) -> Result<(), sqlx::Error> {
    let keys = repository::find_expiring_api_keys(pool).await?;
    for (key, email, user_name, notify_lang, service_name) in keys {
        if let Some(expires_at) = key.expires_at {
            if let Err(e) = email_service
                .send_api_key_expiration_warning(
                    &email,
                    &user_name,
                    &service_name,
                    &key.key_prefix,
                    expires_at,
                    &notify_lang,
                )
                .await
            {
                error!("Failed to send expiration email for key {}: {}", key.id, e);
                continue;
            }
            if let Err(e) = repository::mark_api_key_notified(pool, &key.id).await {
                error!("Failed to mark key {} as notified: {}", key.id, e);
            }
        }
    }
    Ok(())
}
