use sqlx::{mysql::MySqlPoolOptions, MySql, Pool};
use std::time::Duration;

pub type DbPool = Pool<MySql>;

pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    MySqlPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}

pub mod repository;
