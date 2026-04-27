use crate::db::{repository, DbPool};
use crate::services::StorageService;
use std::time::Duration;
use tokio::time;
use tracing::error;

pub async fn start_cleanup_task(pool: DbPool, storage: StorageService) {
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
    }
}
