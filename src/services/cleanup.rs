use crate::db::{repository, DbPool};
use crate::services::StorageService;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

pub async fn start_cleanup_task(pool: DbPool, storage: StorageService) {
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;

        info!("Running cleanup task for expired file shares");

        match repository::delete_expired_file_shares(&pool).await {
            Ok(storage_keys) => {
                if storage_keys.is_empty() {
                    info!("No expired file shares found");
                } else {
                    info!("Deleted {} expired file shares from database", storage_keys.len());

                    match storage.delete_files(storage_keys.clone()).await {
                        Ok(_) => {
                            info!(
                                "Successfully deleted {} files from object storage. Keys: {:?}",
                                storage_keys.len(),
                                storage_keys
                            );
                        }
                        Err(e) => {
                            error!(
                                "Failed to delete files from storage: {}. Keys: {:?}",
                                e, storage_keys
                            );
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to delete expired file shares from database: {}", e);
            }
        }
    }
}
