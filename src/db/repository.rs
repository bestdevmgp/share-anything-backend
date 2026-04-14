use crate::models::*;
use crate::models::personal_token::PersonalToken;
use chrono::Utc;
use sqlx::MySqlPool;
use uuid::Uuid;
use crate::models::email_auth::EmailAuthSession;

pub async fn create_user(
    pool: &MySqlPool,
    dto: CreateUserDto,
) -> Result<User, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let provider_str = dto.oauth_provider.to_string();

    sqlx::query(
        r#"
        INSERT INTO users (id, oauth_provider, oauth_id, email, name, profile_image, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&provider_str)
    .bind(&dto.oauth_id)
    .bind(&dto.email)
    .bind(&dto.name)
    .bind(&dto.profile_image)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    find_user_by_id(pool, &id).await?.ok_or(sqlx::Error::RowNotFound)
}

pub async fn find_user_by_oauth(
    pool: &MySqlPool,
    provider: &OAuthProvider,
    oauth_id: &str,
) -> Result<Option<User>, sqlx::Error> {
    let provider_str = provider.to_string();

    sqlx::query_as::<_, User>(
        r#"
        SELECT * FROM users
        WHERE oauth_provider = ? AND oauth_id = ?
        "#,
    )
    .bind(&provider_str)
    .bind(oauth_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_user_by_id(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT * FROM users WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_user_notification_settings(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<(bool, bool, bool, String), sqlx::Error> {
    let result = sqlx::query_as::<_, (bool, bool, bool, String)>(
        r#"
        SELECT notify_upload, notify_download, notify_download_alert, notify_language FROM users WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(result)
}

pub async fn update_user_name(
    pool: &MySqlPool,
    user_id: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE users SET name = ?, updated_at = UTC_TIMESTAMP() WHERE id = ?"#,
    )
    .bind(name)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn soft_delete_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE users SET status = 'deleted', updated_at = UTC_TIMESTAMP() WHERE id = ?"#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn revoke_all_personal_tokens(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE personal_tokens SET revoked_at = UTC_TIMESTAMP() WHERE user_id = ? AND revoked_at IS NULL"#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_user_notification_settings(
    pool: &MySqlPool,
    user_id: &str,
    notify_upload: bool,
    notify_download: bool,
    notify_download_alert: bool,
    notify_language: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE users SET notify_upload = ?, notify_download = ?, notify_download_alert = ?, notify_language = ?, updated_at = UTC_TIMESTAMP() WHERE id = ?
        "#,
    )
    .bind(notify_upload)
    .bind(notify_download)
    .bind(notify_download_alert)
    .bind(notify_language)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn create_file_share(
    pool: &MySqlPool,
    share_group_id: Option<String>,
    user_id: Option<String>,
    share_code: String,
    file_name: String,
    file_size: i64,
    file_type: String,
    transfer_type: String,
    storage_key: String,
    description: Option<String>,
    password_hash: Option<String>,
    is_one_time: bool,
    is_quick_access: bool,
    expires_at: chrono::DateTime<Utc>,
) -> Result<FileShare, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO file_shares
        (id, share_group_id, user_id, share_code, file_name, file_size, file_type, transfer_type, storage_key,
         description, password_hash, is_one_time, is_quick_access, expires_at, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&share_group_id)
    .bind(&user_id)
    .bind(&share_code)
    .bind(&file_name)
    .bind(file_size)
    .bind(&file_type)
    .bind(&transfer_type)
    .bind(&storage_key)
    .bind(&description)
    .bind(&password_hash)
    .bind(is_one_time)
    .bind(is_quick_access)
    .bind(expires_at)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    find_file_share_by_id(pool, &id).await?.ok_or(sqlx::Error::RowNotFound)
}

pub async fn find_file_share_by_code(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<Option<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        SELECT * FROM file_shares
        WHERE share_code = ? AND expires_at > UTC_TIMESTAMP()
        LIMIT 1
        "#,
    )
    .bind(share_code)
    .fetch_optional(pool)
    .await
}

pub async fn find_file_shares_by_code(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<Vec<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        SELECT * FROM file_shares
        WHERE share_code = ? AND expires_at > UTC_TIMESTAMP()
        ORDER BY created_at ASC
        "#,
    )
    .bind(share_code)
    .fetch_all(pool)
    .await
}

pub async fn find_file_share_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        SELECT * FROM file_shares WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_file_shares_by_user(
    pool: &MySqlPool,
    user_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        SELECT * FROM file_shares
        WHERE user_id = ? AND is_quick_access = false
        ORDER BY created_at DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn find_quick_access_files_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        SELECT * FROM file_shares
        WHERE user_id = ? AND is_quick_access = true AND expires_at > UTC_TIMESTAMP()
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn delete_file_share(
    pool: &MySqlPool,
    id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM file_shares WHERE id = ?
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn delete_all_user_file_shares(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let files = sqlx::query_as::<_, (String,)>(
        r#"
        SELECT storage_key FROM file_shares
        WHERE user_id = ? AND is_quick_access = false
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let storage_keys: Vec<String> = files.into_iter().map(|(k,)| k).collect();

    sqlx::query(
        r#"
        DELETE FROM file_shares WHERE user_id = ? AND is_quick_access = false
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(storage_keys)
}

pub async fn delete_expired_file_shares(
    pool: &MySqlPool,
) -> Result<Vec<String>, sqlx::Error> {
    let expired_files = sqlx::query_as::<_, (String,)>(
        r#"
        SELECT storage_key FROM file_shares
        WHERE expires_at <= UTC_TIMESTAMP()
        "#,
    )
    .fetch_all(pool)
    .await?;

    let storage_keys: Vec<String> = expired_files.into_iter().map(|(k,)| k).collect();

    sqlx::query(
        r#"
        DELETE FROM file_shares WHERE expires_at <= UTC_TIMESTAMP()
        "#,
    )
    .execute(pool)
    .await?;

    Ok(storage_keys)
}

pub async fn check_code_exists(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT COUNT(*) FROM file_shares WHERE share_code = ?
        "#,
    )
    .bind(share_code)
    .fetch_one(pool)
    .await?;

    Ok(result.0 > 0)
}

pub async fn create_download_log(
    pool: &MySqlPool,
    dto: CreateDownloadLogDto,
    device_platform: Option<String>,
) -> Result<DownloadLog, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO download_logs
        (id, file_share_id, downloader_user_id, ip_address, user_agent, device_platform, downloaded_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&dto.file_share_id)
    .bind(&dto.downloader_user_id)
    .bind(&dto.ip_address)
    .bind(&dto.user_agent)
    .bind(&device_platform)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, DownloadLog>(
        r#"
        SELECT * FROM download_logs WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}

pub async fn find_download_logs_by_file_share(
    pool: &MySqlPool,
    file_share_id: &str,
) -> Result<Vec<DownloadLog>, sqlx::Error> {
    sqlx::query_as::<_, DownloadLog>(
        r#"
        SELECT * FROM download_logs
        WHERE file_share_id = ?
        ORDER BY downloaded_at DESC
        "#,
    )
    .bind(file_share_id)
    .fetch_all(pool)
    .await
}

pub async fn count_downloads_by_file_share(
    pool: &MySqlPool,
    file_share_id: &str,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT COUNT(*) FROM download_logs WHERE file_share_id = ?
        "#,
    )
    .bind(file_share_id)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}

pub async fn update_p2p_status(
    pool: &MySqlPool,
    share_code: &str,
    status: &str,
    uploader_peer_id: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE file_shares
        SET p2p_status = ?, uploader_peer_id = ?, updated_at = UTC_TIMESTAMP()
        WHERE share_code = ?
        "#,
    )
    .bind(status)
    .bind(uploader_peer_id)
    .bind(share_code)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn complete_p2p_transfer(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM file_shares
        WHERE share_code = ? AND transfer_type = 'p2p'
        "#,
    )
    .bind(share_code)
    .execute(pool)
    .await?;

    Ok(())
}

// Upload Session functions for presigned upload
#[allow(dead_code)]
#[derive(Debug, sqlx::FromRow)]
pub struct UploadSession {
    pub id: String,
    pub share_code: String,
    pub user_id: Option<String>,
    pub description: Option<String>,
    pub password_hash: Option<String>,
    pub is_one_time: bool,
    pub is_quick_access: bool,
    pub expiration_period: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub completed: bool,
    pub created_at: chrono::DateTime<Utc>,
}

pub async fn create_upload_session(
    pool: &MySqlPool,
    id: &str,
    share_code: &str,
    user_id: Option<&str>,
    description: Option<&str>,
    password_hash: Option<&str>,
    is_one_time: bool,
    is_quick_access: bool,
    expiration_period: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO upload_sessions
        (id, share_code, user_id, description, password_hash, is_one_time, is_quick_access, expiration_period, expires_at, completed, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, false, ?)
        "#,
    )
    .bind(id)
    .bind(share_code)
    .bind(user_id)
    .bind(description)
    .bind(password_hash)
    .bind(is_one_time)
    .bind(is_quick_access)
    .bind(expiration_period)
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_upload_session(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<UploadSession>, sqlx::Error> {
    sqlx::query_as::<_, UploadSession>(
        r#"
        SELECT * FROM upload_sessions WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn complete_upload_session(
    pool: &MySqlPool,
    id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE upload_sessions SET completed = true WHERE id = ?
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn create_email_auth_session(
    pool: &MySqlPool,
    id: &str,
    email: &str,
    token: &str,
    code: &str,
    ip_address: &str,
    device_id: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query(
        r#"
        INSERT INTO email_auth_sessions (id, email, token, verification_code, status, request_ip, device_id, expires_at, created_at)
        VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(email)
    .bind(token)
    .bind(code)
    .bind(ip_address)
    .bind(device_id)
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_email_auth_session_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<EmailAuthSession>, sqlx::Error> {
    sqlx::query_as::<_, EmailAuthSession>(
        r#"SELECT * FROM email_auth_sessions WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_email_auth_session_by_token(
    pool: &MySqlPool,
    token: &str,
) -> Result<Option<EmailAuthSession>, sqlx::Error> {
    sqlx::query_as::<_, EmailAuthSession>(
        r#"SELECT * FROM email_auth_sessions WHERE token = ? AND expires_at > UTC_TIMESTAMP()"#,
    )
    .bind(token)
    .fetch_optional(pool)
    .await
}

pub async fn update_email_auth_session_status(
    pool: &MySqlPool,
    id: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE email_auth_sessions SET status = ? WHERE id = ?"#,
    )
    .bind(status)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_recent_email_auth_session(
    pool: &MySqlPool,
    email: &str,
) -> Result<Option<EmailAuthSession>, sqlx::Error> {
    sqlx::query_as::<_, EmailAuthSession>(
        r#"
        SELECT * FROM email_auth_sessions
        WHERE email = ? AND created_at > DATE_SUB(UTC_TIMESTAMP(), INTERVAL 60 SECOND)
        ORDER BY created_at DESC LIMIT 1
        "#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

pub async fn find_user_by_email(
    pool: &MySqlPool,
    email: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"SELECT * FROM users WHERE email = ? LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

pub async fn create_personal_token(
    pool: &MySqlPool,
    id: &str,
    user_id: &str,
    token_hash: &str,
    token_prefix: &str,
    name: &str,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> Result<PersonalToken, sqlx::Error> {
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO personal_tokens (id, user_id, token_hash, token_prefix, name, expires_at, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(token_hash)
    .bind(token_prefix)
    .bind(name)
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, PersonalToken>(
        r#"SELECT * FROM personal_tokens WHERE id = ?"#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn find_personal_token_by_hash(
    pool: &MySqlPool,
    token_hash: &str,
) -> Result<Option<PersonalToken>, sqlx::Error> {
    sqlx::query_as::<_, PersonalToken>(
        r#"SELECT * FROM personal_tokens WHERE token_hash = ? AND revoked_at IS NULL"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

pub async fn find_personal_token_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<PersonalToken>, sqlx::Error> {
    sqlx::query_as::<_, PersonalToken>(
        r#"SELECT * FROM personal_tokens WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_personal_tokens_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<PersonalToken>, sqlx::Error> {
    sqlx::query_as::<_, PersonalToken>(
        r#"
        SELECT * FROM personal_tokens
        WHERE user_id = ? AND revoked_at IS NULL
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke_personal_token(
    pool: &MySqlPool,
    id: &str,
    user_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"UPDATE personal_tokens SET revoked_at = UTC_TIMESTAMP() WHERE id = ? AND user_id = ?"#,
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn update_personal_token_last_used(
    pool: &MySqlPool,
    id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE personal_tokens SET last_used_at = UTC_TIMESTAMP() WHERE id = ?"#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn create_cli_auth_session(
    pool: &MySqlPool,
    id: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO cli_auth_sessions (id, status, expires_at) VALUES (?, 'pending', ?)"#,
    )
    .bind(id)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn find_cli_auth_session(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<(String, Option<String>, Option<String>, chrono::DateTime<Utc>)>, sqlx::Error> {
    // Returns (status, personal_token_value, user_name, expires_at)
    sqlx::query_as::<_, (String, Option<String>, Option<String>, chrono::DateTime<Utc>)>(
        r#"
        SELECT s.status, s.personal_token_value, u.name, s.expires_at
        FROM cli_auth_sessions s
        LEFT JOIN users u ON s.user_id COLLATE utf8mb4_unicode_ci = u.id
        WHERE s.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn complete_cli_auth_session(
    pool: &MySqlPool,
    session_id: &str,
    user_id: &str,
    personal_token_id: &str,
    personal_token_value: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE cli_auth_sessions
        SET status = 'completed', user_id = ?, personal_token_id = ?, personal_token_value = ?, completed_at = UTC_TIMESTAMP()
        WHERE id = ? AND status = 'pending'
        "#,
    )
    .bind(user_id)
    .bind(personal_token_id)
    .bind(personal_token_value)
    .bind(session_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn clear_cli_auth_session_token(
    pool: &MySqlPool,
    session_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"UPDATE cli_auth_sessions SET personal_token_value = NULL WHERE id = ? AND personal_token_value IS NOT NULL"#,
    )
    .bind(session_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn delete_expired_cli_auth_sessions(
    pool: &MySqlPool,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"DELETE FROM cli_auth_sessions
           WHERE (expires_at < UTC_TIMESTAMP() AND status != 'completed')
              OR (status = 'completed' AND completed_at < DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 HOUR))"#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

