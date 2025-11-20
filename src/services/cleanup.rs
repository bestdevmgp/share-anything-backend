use crate::db::{repository, DbPool};
use crate::services::StorageService;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

/// Background task that periodically cleans up expired file shares
pub async fn start_cleanup_task(pool: DbPool, storage: StorageService) {
    let mut interval = time::interval(Duration::from_secs(3600)); // Run every hour

    loop {
        interval.tick().await;

        info!("Running cleanup task for expired file shares");

        match repository::delete_expired_file_shares(&pool).await {
            Ok(storage_keys) => {
                info!("Deleted {} expired file shares from database", storage_keys.len());

                // Delete files from storage
                if !storage_keys.is_empty() {
                    match storage.delete_files(storage_keys.clone()).await {
                        Ok(_) => {
                            info!("Deleted {} files from object storage", storage_keys.len());
                        }
                        Err(e) => {
                            error!("Failed to delete files from storage: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to delete expired file shares: {}", e);
            }
        }
    }
}
