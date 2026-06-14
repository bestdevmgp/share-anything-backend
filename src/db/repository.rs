use crate::models::*;
use crate::models::personal_token::{PersonalToken, Scope};
use crate::models::api_key::ApiKey;
use crate::models::session::{CreateSessionDto, Session, TrustedDevice};
use chrono::Utc;
use sqlx::{FromRow, MySqlPool};
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
        INSERT INTO users (id, oauth_provider, oauth_id, email, name, profile_image, notify_language, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&provider_str)
    .bind(&dto.oauth_id)
    .bind(&dto.email)
    .bind(&dto.name)
    .bind(&dto.profile_image)
    .bind(&dto.notify_language)
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
) -> Result<(bool, bool, bool, bool, String, String), sqlx::Error> {
    let result = sqlx::query_as::<_, (bool, bool, bool, bool, String, String)>(
        r#"
        SELECT notify_upload, notify_download, notify_download_alert, notify_security, notify_language, default_expiration FROM users WHERE id = ?
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

pub async fn reactivate_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE users SET status = 'active', updated_at = UTC_TIMESTAMP() WHERE id = ?"#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn hard_delete_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(r#"DELETE FROM users WHERE id = ?"#)
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
    notify_security: bool,
    notify_language: &str,
    default_expiration: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE users SET notify_upload = ?, notify_download = ?, notify_download_alert = ?, notify_security = ?, notify_language = ?, default_expiration = ?, updated_at = UTC_TIMESTAMP() WHERE id = ?
        "#,
    )
    .bind(notify_upload)
    .bind(notify_download)
    .bind(notify_download_alert)
    .bind(notify_security)
    .bind(notify_language)
    .bind(default_expiration)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn create_file_share(
    pool: &MySqlPool,
    share_group_id: Option<String>,
    user_id: Option<String>,
    created_via_api_key_id: Option<String>,
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
    image_width: Option<i32>,
    image_height: Option<i32>,
    display_order: i32,
    device_id: Option<String>,
) -> Result<FileShare, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO file_shares
        (id, share_group_id, user_id, created_via_api_key_id, share_code, file_name, file_size, file_type, transfer_type, storage_key,
         description, password_hash, is_one_time, is_quick_access, expires_at, created_at, updated_at,
         image_width, image_height, display_order, device_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&share_group_id)
    .bind(&user_id)
    .bind(&created_via_api_key_id)
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
    .bind(image_width)
    .bind(image_height)
    .bind(display_order)
    .bind(&device_id)
    .execute(pool)
    .await?;

    find_file_share_by_id(pool, &id).await?.ok_or(sqlx::Error::RowNotFound)
}

pub async fn sum_active_storage_for_api_key(
    pool: &MySqlPool,
    api_key_id: &str,
) -> Result<i64, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT COALESCE(CAST(SUM(file_size) AS SIGNED), 0)
        FROM file_shares
        WHERE created_via_api_key_id = ?
          AND expires_at > UTC_TIMESTAMP()
        "#,
    )
    .bind(api_key_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0.unwrap_or(0))
}

pub async fn find_file_share_by_code(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<Option<FileShare>, sqlx::Error> {
    let rows = sqlx::query_as::<_, FileShare>(
        r#"
        (SELECT fs.* FROM file_shares fs
         INNER JOIN public_share_grants g ON g.file_share_id = fs.id
         WHERE g.share_code = ? AND g.expires_at > UTC_TIMESTAMP()
         LIMIT 1)
        UNION ALL
        (SELECT fs.* FROM file_shares fs
         WHERE fs.share_code = ? AND fs.expires_at > UTC_TIMESTAMP()
         LIMIT 1)
        "#,
    )
    .bind(share_code)
    .bind(share_code)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().next())
}

pub async fn find_file_shares_by_code(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<Vec<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        (SELECT fs.* FROM file_shares fs
         INNER JOIN public_share_grants g ON g.file_share_id = fs.id
         WHERE g.share_code = ? AND g.expires_at > UTC_TIMESTAMP())
        UNION ALL
        (SELECT fs.* FROM file_shares fs
         WHERE fs.share_code = ? AND fs.expires_at > UTC_TIMESTAMP())
        ORDER BY display_order ASC, created_at ASC
        "#,
    )
    .bind(share_code)
    .bind(share_code)
    .fetch_all(pool)
    .await
}

pub async fn find_file_shares_by_code_for_revoke(
    pool: &MySqlPool,
    code: &str,
) -> Result<Vec<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>("SELECT * FROM file_shares WHERE share_code = ?")
        .bind(code)
        .fetch_all(pool)
        .await
}

pub async fn find_file_shares_by_code_with_uploader(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<Vec<(FileShare, Option<String>)>, sqlx::Error> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        (SELECT fs.*, u.name AS uploader_name FROM file_shares fs
         INNER JOIN public_share_grants g ON g.file_share_id = fs.id
         LEFT JOIN users u ON u.id = fs.user_id
         WHERE g.share_code = ? AND g.expires_at > UTC_TIMESTAMP())
        UNION ALL
        (SELECT fs.*, u.name AS uploader_name FROM file_shares fs
         LEFT JOIN users u ON u.id = fs.user_id
         WHERE fs.share_code = ? AND fs.expires_at > UTC_TIMESTAMP())
        ORDER BY display_order ASC, created_at ASC
        "#,
    )
    .bind(share_code)
    .bind(share_code)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let uploader_name: Option<String> = row.try_get("uploader_name")?;
            let file_share = FileShare::from_row(&row)?;
            Ok((file_share, uploader_name))
        })
        .collect()
}

pub async fn find_file_share_by_code_with_uploader(
    pool: &MySqlPool,
    share_code: &str,
) -> Result<Option<(FileShare, Option<String>)>, sqlx::Error> {
    Ok(find_file_shares_by_code_with_uploader(pool, share_code)
        .await?
        .into_iter()
        .next())
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

pub async fn find_file_shares_with_download_count_by_user(
    pool: &MySqlPool,
    user_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<(FileShare, i64)>, sqlx::Error> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        SELECT fs.*, COALESCE(COUNT(dl.id), 0) AS download_count
        FROM file_shares fs
        LEFT JOIN download_logs dl ON dl.file_share_id = fs.id
        WHERE fs.user_id = ? AND fs.is_quick_access = false AND fs.transfer_type != 'p2p'
        GROUP BY fs.id
        ORDER BY fs.created_at DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let download_count: i64 = row.try_get("download_count")?;
            let file_share = FileShare::from_row(&row)?;
            Ok((file_share, download_count))
        })
        .collect()
}

pub async fn find_active_qa_grant_shares_with_download_count_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<(FileShare, i64)>, sqlx::Error> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        SELECT fs.*,
               g.share_code AS grant_code,
               g.expires_at AS grant_expires_at,
               g.created_at AS grant_created_at,
               (SELECT COUNT(*) FROM download_logs dl WHERE dl.file_share_id = fs.id) AS download_count
        FROM file_shares fs
        INNER JOIN public_share_grants g ON g.file_share_id = fs.id
        WHERE fs.user_id = ? AND g.expires_at > UTC_TIMESTAMP()
        ORDER BY g.created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let download_count: i64 = row.try_get("download_count")?;
            let mut file_share = FileShare::from_row(&row)?;
            file_share.share_code = row.try_get("grant_code")?;
            file_share.expires_at = row.try_get("grant_expires_at")?;
            file_share.created_at = row.try_get("grant_created_at")?;
            Ok((file_share, download_count))
        })
        .collect()
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
        WHERE user_id = ? AND is_quick_access = false AND transfer_type != 'p2p'
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
    let share_code: Option<(String,)> =
        sqlx::query_as("SELECT share_code FROM file_shares WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    let result = sqlx::query("DELETE FROM file_shares WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    if let Some((code,)) = share_code {
        release_share_code(pool, &code).await?;
    }

    Ok(result.rows_affected())
}

pub async fn delete_all_user_file_shares(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT storage_key, share_code FROM file_shares
        WHERE user_id = ? AND is_quick_access = false
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let (storage_keys, share_codes): (Vec<String>, Vec<String>) = rows.into_iter().unzip();

    sqlx::query(
        r#"
        DELETE FROM file_shares WHERE user_id = ? AND is_quick_access = false
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    if !share_codes.is_empty() {
        let placeholders = vec!["?"; share_codes.len()].join(", ");
        let sql = format!("DELETE FROM share_codes WHERE code IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for code in &share_codes {
            q = q.bind(code);
        }
        q.execute(pool).await?;
    }

    Ok(storage_keys)
}

pub async fn delete_file_shares_by_code(
    pool: &MySqlPool,
    code: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String,)>(
        "SELECT storage_key FROM file_shares WHERE share_code = ?",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;

    let storage_keys: Vec<String> =
        rows.into_iter().map(|r| r.0).filter(|k| !k.is_empty()).collect();

    sqlx::query("DELETE FROM file_shares WHERE share_code = ?")
        .bind(code)
        .execute(pool)
        .await?;

    release_share_code(pool, code).await?;

    Ok(storage_keys)
}

pub async fn delete_expired_file_shares(
    pool: &MySqlPool,
) -> Result<Vec<String>, sqlx::Error> {
    let expired_files = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT fs.storage_key, fs.share_code FROM file_shares fs
        WHERE fs.expires_at <= UTC_TIMESTAMP()
          AND NOT EXISTS (
              SELECT 1 FROM public_share_grants g
              WHERE g.file_share_id = fs.id
                AND g.expires_at > UTC_TIMESTAMP()
          )
        "#,
    )
    .fetch_all(pool)
    .await?;

    let (storage_keys, share_codes): (Vec<String>, Vec<String>) =
        expired_files.into_iter().unzip();

    sqlx::query(
        r#"
        DELETE FROM file_shares
        WHERE expires_at <= UTC_TIMESTAMP()
          AND NOT EXISTS (
              SELECT 1 FROM public_share_grants g
              WHERE g.file_share_id = file_shares.id
                AND g.expires_at > UTC_TIMESTAMP()
          )
        "#,
    )
    .execute(pool)
    .await?;

    if !share_codes.is_empty() {
        let placeholders = vec!["?"; share_codes.len()].join(", ");
        let sql = format!("DELETE FROM share_codes WHERE code IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for code in &share_codes {
            q = q.bind(code);
        }
        q.execute(pool).await?;
    }

    Ok(storage_keys)
}

pub async fn create_public_share_grant(
    pool: &MySqlPool,
    share_code: &str,
    file_share_id: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO public_share_grants (share_code, file_share_id, expires_at, created_at)
        VALUES (?, ?, ?, UTC_TIMESTAMP())
        "#,
    )
    .bind(share_code)
    .bind(file_share_id)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_expired_public_share_grants(
    pool: &MySqlPool,
) -> Result<u64, sqlx::Error> {
    let expired_codes: Vec<(String,)> = sqlx::query_as(
        "SELECT share_code FROM public_share_grants WHERE expires_at <= UTC_TIMESTAMP()",
    )
    .fetch_all(pool)
    .await?;

    let result = sqlx::query(
        r#"
        DELETE FROM public_share_grants WHERE expires_at <= UTC_TIMESTAMP()
        "#,
    )
    .execute(pool)
    .await?;

    if !expired_codes.is_empty() {
        let placeholders = vec!["?"; expired_codes.len()].join(", ");
        let sql = format!("DELETE FROM share_codes WHERE code IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for (code,) in &expired_codes {
            q = q.bind(code);
        }
        q.execute(pool).await?;
    }

    Ok(result.rows_affected())
}

pub async fn find_file_share_by_grant_code(
    pool: &MySqlPool,
    code: &str,
) -> Result<Option<FileShare>, sqlx::Error> {
    sqlx::query_as::<_, FileShare>(
        r#"
        SELECT fs.* FROM file_shares fs
        INNER JOIN public_share_grants g ON g.file_share_id = fs.id
        WHERE g.share_code = ?
        LIMIT 1
        "#,
    )
    .bind(code)
    .fetch_optional(pool)
    .await
}

pub async fn delete_public_share_grant_by_code(
    pool: &MySqlPool,
    code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM public_share_grants WHERE share_code = ?")
        .bind(code)
        .execute(pool)
        .await?;
    release_share_code(pool, code).await?;
    Ok(())
}

const MAX_SHARE_CODE_RESERVATION_ATTEMPTS: u32 = 16;

pub async fn reserve_share_code(pool: &MySqlPool) -> Result<String, sqlx::Error> {
    use crate::utils::generate_share_code;

    for _ in 0..MAX_SHARE_CODE_RESERVATION_ATTEMPTS {
        let code = generate_share_code();
        let result = sqlx::query("INSERT INTO share_codes (code) VALUES (?)")
            .bind(&code)
            .execute(pool)
            .await;
        match result {
            Ok(_) => return Ok(code),
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23000") => continue,
            Err(e) => return Err(e),
        }
    }

    Err(sqlx::Error::Protocol(
        "Failed to reserve share code after repeated attempts (code pool may be nearly full)"
            .into(),
    ))
}

pub async fn release_share_code(pool: &MySqlPool, code: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM share_codes WHERE code = ?")
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
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

pub async fn batch_create_download_logs(
    pool: &MySqlPool,
    dtos: Vec<CreateDownloadLogDto>,
    device_platform: Option<String>,
) -> Result<(), sqlx::Error> {
    if dtos.is_empty() {
        return Ok(());
    }

    let now = Utc::now();
    let mut sql = String::from(
        "INSERT INTO download_logs \
         (id, file_share_id, downloader_user_id, ip_address, user_agent, device_platform, downloaded_at) \
         VALUES ",
    );
    for i in 0..dtos.len() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push_str("(?, ?, ?, ?, ?, ?, ?)");
    }

    let mut q = sqlx::query(&sql);
    for dto in &dtos {
        q = q
            .bind(Uuid::new_v4().to_string())
            .bind(&dto.file_share_id)
            .bind(&dto.downloader_user_id)
            .bind(&dto.ip_address)
            .bind(&dto.user_agent)
            .bind(&device_platform)
            .bind(now);
    }

    q.execute(pool).await?;
    Ok(())
}

pub async fn find_download_logs_by_user(
    pool: &MySqlPool,
    user_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<(DownloadLog, String, String, i64)>, sqlx::Error> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        SELECT dl.*, fs.share_code, fs.file_name, fs.file_size
        FROM download_logs dl
        INNER JOIN file_shares fs ON fs.id = dl.file_share_id
        WHERE fs.user_id = ?
        ORDER BY dl.downloaded_at DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let share_code: String = row.try_get("share_code")?;
            let file_name: String = row.try_get("file_name")?;
            let file_size: i64 = row.try_get("file_size")?;
            let log = DownloadLog::from_row(&row)?;
            Ok((log, share_code, file_name, file_size))
        })
        .collect()
}

pub async fn find_download_logs_with_downloader_name_by_file_share(
    pool: &MySqlPool,
    file_share_id: &str,
) -> Result<Vec<(DownloadLog, Option<String>)>, sqlx::Error> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        SELECT dl.*, u.name AS downloader_name
        FROM download_logs dl
        LEFT JOIN users u ON u.id = dl.downloader_user_id
        WHERE dl.file_share_id = ?
        ORDER BY dl.downloaded_at DESC
        "#,
    )
    .bind(file_share_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let downloader_name: Option<String> = row.try_get("downloader_name")?;
            let log = DownloadLog::from_row(&row)?;
            Ok((log, downloader_name))
        })
        .collect()
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
        r#"SELECT id, user_id, token_hash, token_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, created_at
           FROM personal_tokens WHERE id = ?"#,
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
        r#"SELECT id, user_id, token_hash, token_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, created_at
           FROM personal_tokens WHERE token_hash = ? AND revoked_at IS NULL"#,
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
        r#"SELECT id, user_id, token_hash, token_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, created_at
           FROM personal_tokens WHERE id = ?"#,
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
        r#"SELECT id, user_id, token_hash, token_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, created_at
           FROM personal_tokens
           WHERE user_id = ? AND revoked_at IS NULL
           ORDER BY created_at DESC"#,
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

pub async fn update_personal_token_last_used_with_platform(
    pool: &MySqlPool,
    id: &str,
    platform: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE personal_tokens
           SET last_used_at = UTC_TIMESTAMP(),
               last_platform = COALESCE(?, last_platform)
           WHERE id = ?"#,
    )
    .bind(platform)
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

pub async fn create_session(pool: &MySqlPool, dto: CreateSessionDto) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO sessions
        (jti, user_id, device_id, device_label, user_agent, user_agent_hash, ip_address, location, expires_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&dto.jti)
    .bind(&dto.user_id)
    .bind(&dto.device_id)
    .bind(&dto.device_label)
    .bind(&dto.user_agent)
    .bind(&dto.user_agent_hash)
    .bind(&dto.ip_address)
    .bind(&dto.location)
    .bind(dto.expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_session(pool: &MySqlPool, jti: &str) -> Result<Option<Session>, sqlx::Error> {
    sqlx::query_as::<_, Session>(
        "SELECT * FROM sessions WHERE jti = ? AND expires_at > UTC_TIMESTAMP()",
    )
    .bind(jti)
    .fetch_optional(pool)
    .await
}

pub async fn find_sessions_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<Session>, sqlx::Error> {
    sqlx::query_as::<_, Session>(
        r#"
        SELECT * FROM sessions
        WHERE user_id = ?
          AND expires_at > UTC_TIMESTAMP()
          AND last_seen_at > UTC_TIMESTAMP() - INTERVAL 5 MINUTE
        ORDER BY last_seen_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn find_active_cli_sessions_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<PersonalToken>, sqlx::Error> {
    sqlx::query_as::<_, PersonalToken>(
        r#"SELECT id, user_id, token_hash, token_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, created_at
           FROM personal_tokens
           WHERE user_id = ?
             AND revoked_at IS NULL
             AND last_used_at IS NOT NULL
             AND last_used_at > UTC_TIMESTAMP() - INTERVAL 5 MINUTE
           ORDER BY last_used_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn delete_session(
    pool: &MySqlPool,
    user_id: &str,
    jti: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE jti = ? AND user_id = ?")
        .bind(jti)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn delete_other_sessions(
    pool: &MySqlPool,
    user_id: &str,
    keep_jti: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND jti <> ?")
        .bind(user_id)
        .bind(keep_jti)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn touch_session_last_seen(pool: &MySqlPool, jti: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sessions SET last_seen_at = UTC_TIMESTAMP() WHERE jti = ?")
        .bind(jti)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_sessions_by_device(
    pool: &MySqlPool,
    user_id: &str,
    device_id: &str,
) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND device_id = ?")
            .bind(user_id)
            .bind(device_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

pub async fn delete_expired_sessions(pool: &MySqlPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= UTC_TIMESTAMP()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn is_device_trusted(
    pool: &MySqlPool,
    user_id: &str,
    device_id: &str,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM trusted_devices WHERE user_id = ? AND device_id = ?",
    )
    .bind(user_id)
    .bind(device_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0 > 0)
}

pub async fn upsert_trusted_device(
    pool: &MySqlPool,
    user_id: &str,
    device_id: &str,
    user_agent_hash: &str,
    user_agent: Option<&str>,
    ip_address: &str,
    device_label: Option<&str>,
    location: Option<&str>,
) -> Result<(), sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO trusted_devices
        (id, user_id, device_id, user_agent_hash, user_agent, ip_address, device_label, location)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON DUPLICATE KEY UPDATE
            user_agent_hash = VALUES(user_agent_hash),
            user_agent = VALUES(user_agent),
            ip_address = VALUES(ip_address),
            device_label = VALUES(device_label),
            location = VALUES(location),
            trusted_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(device_id)
    .bind(user_agent_hash)
    .bind(user_agent)
    .bind(ip_address)
    .bind(device_label)
    .bind(location)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_trusted_devices_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<TrustedDevice>, sqlx::Error> {
    sqlx::query_as::<_, TrustedDevice>(
        "SELECT * FROM trusted_devices WHERE user_id = ? ORDER BY trusted_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn delete_trusted_device(
    pool: &MySqlPool,
    user_id: &str,
    id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM trusted_devices WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn delete_trusted_device_by_device_id(
    pool: &MySqlPool,
    user_id: &str,
    device_id: &str,
) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM trusted_devices WHERE user_id = ? AND device_id = ?")
            .bind(user_id)
            .bind(device_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

pub async fn create_application(
    pool: &MySqlPool,
    user_id: &str,
    service_name: &str,
    service_url: &str,
    purpose: &str,
    scopes: &str,
    requested_expires_at: Option<chrono::DateTime<Utc>>,
    ip: Option<&str>,
    platform: Option<&str>,
) -> Result<crate::models::ApiKeyApplication, sqlx::Error> {
    let result = sqlx::query(
        r#"INSERT INTO api_key_applications
           (user_id, service_name, service_url, purpose, scopes, requested_expires_at, status, applicant_ip, applicant_platform)
           VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)"#,
    )
    .bind(user_id)
    .bind(service_name)
    .bind(service_url)
    .bind(purpose)
    .bind(scopes)
    .bind(requested_expires_at)
    .bind(ip)
    .bind(platform)
    .execute(pool)
    .await?;

    let id = result.last_insert_id() as i64;
    find_application_by_id(pool, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn find_application_by_id(
    pool: &MySqlPool,
    id: i64,
) -> Result<Option<crate::models::ApiKeyApplication>, sqlx::Error> {
    sqlx::query_as::<_, crate::models::ApiKeyApplication>(
        r#"SELECT id, user_id, service_name, service_url, purpose, scopes, requested_expires_at,
                  status, reject_reason, api_key_id, applicant_ip, applicant_platform,
                  created_at, reviewed_at
           FROM api_key_applications WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_applications_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<crate::models::ApiKeyApplication>, sqlx::Error> {
    sqlx::query_as::<_, crate::models::ApiKeyApplication>(
        r#"SELECT id, user_id, service_name, service_url, purpose, scopes, requested_expires_at,
                  status, reject_reason, api_key_id, applicant_ip, applicant_platform,
                  created_at, reviewed_at
           FROM api_key_applications WHERE user_id = ?
           ORDER BY created_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn find_pending_applications(
    pool: &MySqlPool,
) -> Result<Vec<crate::models::ApiKeyApplication>, sqlx::Error> {
    sqlx::query_as::<_, crate::models::ApiKeyApplication>(
        r#"SELECT id, user_id, service_name, service_url, purpose, scopes, requested_expires_at,
                  status, reject_reason, api_key_id, applicant_ip, applicant_platform,
                  created_at, reviewed_at
           FROM api_key_applications WHERE status = 'pending'
           ORDER BY created_at ASC"#,
    )
    .fetch_all(pool)
    .await
}

pub async fn approve_application(
    pool: &MySqlPool,
    id: i64,
    api_key_token_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE api_key_applications
           SET status = 'approved', api_key_id = ?, reviewed_at = UTC_TIMESTAMP()
           WHERE id = ?"#,
    )
    .bind(api_key_token_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn reject_application(
    pool: &MySqlPool,
    id: i64,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE api_key_applications
           SET status = 'rejected', reject_reason = ?, reviewed_at = UTC_TIMESTAMP()
           WHERE id = ?"#,
    )
    .bind(reason)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns true if the user already has a pending application awaiting review.
pub async fn check_user_pending_application(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<bool, sqlx::Error> {
    let count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM api_key_applications
           WHERE user_id = ? AND status = 'pending'"#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count.0 > 0)
}

/// Returns true if the user has already submitted an application today
/// in their local calendar day (determined by `tz_offset_minutes` from UTC).
pub async fn count_user_applications_today(
    pool: &MySqlPool,
    user_id: &str,
    tz_offset_minutes: i32,
) -> Result<i64, sqlx::Error> {
    let count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM api_key_applications
           WHERE user_id = ?
             AND status NOT IN ('cancelled', 'rejected')
             AND DATE(created_at + INTERVAL ? MINUTE) = DATE(UTC_TIMESTAMP() + INTERVAL ? MINUTE)"#,
    )
    .bind(user_id)
    .bind(tz_offset_minutes)
    .bind(tz_offset_minutes)
    .fetch_one(pool)
    .await?;
    Ok(count.0)
}

/// Cancels a pending application owned by the given user.
/// Returns Ok(()) on success, Err(AppError::NotFound) if the application does not exist,
/// does not belong to the user, or is not in pending status.
pub async fn cancel_application_by_user(
    pool: &MySqlPool,
    application_id: i64,
    user_id: &str,
) -> Result<(), crate::models::AppError> {
    let result = sqlx::query(
        r#"UPDATE api_key_applications
           SET status = 'cancelled'
           WHERE id = ? AND user_id = ? AND status = 'pending'"#,
    )
    .bind(application_id)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|e| crate::models::internal_error(format!("Cancel application failed: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(crate::models::not_found("Request not found"));
    }
    Ok(())
}

pub async fn list_applications_by_status(
    pool: &MySqlPool,
    status: Option<&str>,
) -> Result<Vec<crate::models::ApiKeyApplication>, sqlx::Error> {
    match status {
        Some(s) => {
            sqlx::query_as::<_, crate::models::ApiKeyApplication>(
                r#"SELECT id, user_id, service_name, service_url, purpose, scopes, status,
                          reject_reason, api_key_id, applicant_ip, applicant_platform,
                          created_at, reviewed_at
                   FROM api_key_applications WHERE status = ?
                   ORDER BY created_at DESC"#,
            )
            .bind(s)
            .fetch_all(pool)
            .await
        }
        None => find_pending_applications(pool).await,
    }
}

pub async fn create_api_key(
    pool: &MySqlPool,
    id: &str,
    user_id: &str,
    application_id: i64,
    key_hash: &str,
    key_prefix: &str,
    name: &str,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> Result<ApiKey, sqlx::Error> {
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, user_id, application_id, key_hash, key_prefix, name, expires_at, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(application_id)
    .bind(key_hash)
    .bind(key_prefix)
    .bind(name)
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, ApiKey>(
        r#"SELECT id, user_id, application_id, key_hash, key_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, expiration_notified_at, created_at
           FROM api_keys WHERE id = ?"#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn find_api_key_by_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<ApiKey>, sqlx::Error> {
    sqlx::query_as::<_, ApiKey>(
        r#"SELECT id, user_id, application_id, key_hash, key_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, expiration_notified_at, created_at
           FROM api_keys WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_api_key_by_hash(
    pool: &MySqlPool,
    hash: &str,
) -> Result<Option<ApiKey>, sqlx::Error> {
    sqlx::query_as::<_, ApiKey>(
        r#"SELECT id, user_id, application_id, key_hash, key_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, expiration_notified_at, created_at
           FROM api_keys WHERE key_hash = ?"#,
    )
    .bind(hash)
    .fetch_optional(pool)
    .await
}

pub async fn find_api_keys_by_user(
    pool: &MySqlPool,
    user_id: &str,
) -> Result<Vec<ApiKey>, sqlx::Error> {
    sqlx::query_as::<_, ApiKey>(
        r#"SELECT id, user_id, application_id, key_hash, key_prefix, name,
                  last_used_at, last_platform, expires_at, revoked_at, expiration_notified_at, created_at
           FROM api_keys
           WHERE user_id = ? AND revoked_at IS NULL
           ORDER BY created_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn find_expiring_api_keys(
    pool: &MySqlPool,
) -> Result<Vec<(ApiKey, String, String, String, String)>, sqlx::Error> {
    use sqlx::Row;

    let rows = sqlx::query(
        r#"
        SELECT k.id AS k_id, k.user_id AS k_user_id, k.application_id AS k_application_id,
               k.key_hash AS k_key_hash, k.key_prefix AS k_key_prefix, k.name AS k_name,
               k.last_used_at AS k_last_used_at, k.last_platform AS k_last_platform,
               k.expires_at AS k_expires_at, k.revoked_at AS k_revoked_at,
               k.expiration_notified_at AS k_expiration_notified_at,
               k.created_at AS k_created_at,
               u.email, u.name AS u_name, COALESCE(u.notify_language, 'ko') AS notify_language,
               app.service_name
        FROM api_keys k
        INNER JOIN users u ON u.id = k.user_id
        INNER JOIN api_key_applications app ON app.id = k.application_id
        WHERE k.revoked_at IS NULL
          AND k.expires_at IS NOT NULL
          AND k.expires_at > UTC_TIMESTAMP()
          AND k.expires_at <= UTC_TIMESTAMP() + INTERVAL 3 DAY
          AND k.expiration_notified_at IS NULL
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let key = ApiKey {
            id: row.try_get("k_id")?,
            user_id: row.try_get("k_user_id")?,
            application_id: row.try_get("k_application_id")?,
            key_hash: row.try_get("k_key_hash")?,
            key_prefix: row.try_get("k_key_prefix")?,
            name: row.try_get("k_name")?,
            last_used_at: row.try_get("k_last_used_at")?,
            last_platform: row.try_get("k_last_platform")?,
            expires_at: row.try_get("k_expires_at")?,
            revoked_at: row.try_get("k_revoked_at")?,
            expiration_notified_at: row.try_get("k_expiration_notified_at")?,
            created_at: row.try_get("k_created_at")?,
        };
        let email: String = row.try_get("email")?;
        let user_name: String = row.try_get("u_name")?;
        let notify_language: String = row.try_get("notify_language")?;
        let service_name: String = row.try_get("service_name")?;
        result.push((key, email, user_name, notify_language, service_name));
    }
    Ok(result)
}

pub async fn mark_api_key_notified(
    pool: &MySqlPool,
    api_key_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE api_keys SET expiration_notified_at = UTC_TIMESTAMP() WHERE id = ?"#,
    )
    .bind(api_key_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn revoke_api_key(
    pool: &MySqlPool,
    id: &str,
    user_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"UPDATE api_keys SET revoked_at = UTC_TIMESTAMP() WHERE id = ? AND user_id = ?"#,
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn update_api_key_last_used_with_platform(
    pool: &MySqlPool,
    key_id: &str,
    platform: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE api_keys
           SET last_used_at = UTC_TIMESTAMP(),
               last_platform = COALESCE(?, last_platform)
           WHERE id = ?"#,
    )
    .bind(platform)
    .bind(key_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_key_scopes(
    pool: &MySqlPool,
    api_key_id: &str,
    scopes: &[Scope],
) -> Result<(), sqlx::Error> {
    for scope in scopes {
        sqlx::query(
            r#"INSERT IGNORE INTO key_scopes (api_key_id, scope) VALUES (?, ?)"#,
        )
        .bind(api_key_id)
        .bind(scope.as_str())
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn find_scopes_by_api_key(
    pool: &MySqlPool,
    api_key_id: &str,
) -> Result<Vec<Scope>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"SELECT scope FROM key_scopes WHERE api_key_id = ?"#,
    )
    .bind(api_key_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(s,)| Scope::parse(&s))
        .collect())
}

pub async fn create_api_key_reveal(
    pool: &MySqlPool,
    token: &str,
    api_key_id: &str,
    user_id: &str,
    plaintext_key: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO api_key_reveals (token, api_key_id, user_id, plaintext_key, expires_at, created_at)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(token)
    .bind(api_key_id)
    .bind(user_id)
    .bind(plaintext_key)
    .bind(expires_at)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

pub struct ApiKeyRevealRow {
    pub api_key_id: String,
    pub user_id: String,
    pub plaintext_key: Option<String>,
    pub expires_at: chrono::DateTime<Utc>,
    pub revealed_at: Option<chrono::DateTime<Utc>>,
}

pub async fn find_api_key_reveal_by_token(
    pool: &MySqlPool,
    token: &str,
) -> Result<Option<ApiKeyRevealRow>, sqlx::Error> {
    use sqlx::Row;
    let row = sqlx::query(
        r#"SELECT api_key_id, user_id, plaintext_key, expires_at, revealed_at
           FROM api_key_reveals WHERE token = ?"#,
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ApiKeyRevealRow {
        api_key_id: r.try_get("api_key_id").unwrap_or_default(),
        user_id: r.try_get("user_id").unwrap_or_default(),
        plaintext_key: r.try_get("plaintext_key").ok(),
        expires_at: r.try_get("expires_at").unwrap_or_else(|_| Utc::now()),
        revealed_at: r.try_get("revealed_at").ok(),
    }))
}

pub async fn mark_api_key_reveal_consumed(
    pool: &MySqlPool,
    token: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"UPDATE api_key_reveals
           SET plaintext_key = NULL, revealed_at = UTC_TIMESTAMP()
           WHERE token = ? AND revealed_at IS NULL"#,
    )
    .bind(token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn find_active_reveal_token_for_api_key(
    pool: &MySqlPool,
    api_key_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    use sqlx::Row;
    let row = sqlx::query(
        r#"SELECT token FROM api_key_reveals
           WHERE api_key_id = ?
             AND revealed_at IS NULL
             AND plaintext_key IS NOT NULL
             AND expires_at > UTC_TIMESTAMP()
           ORDER BY created_at DESC
           LIMIT 1"#,
    )
    .bind(api_key_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|r| r.try_get::<String, _>("token").ok()))
}

pub async fn purge_expired_api_key_reveals(pool: &MySqlPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"UPDATE api_key_reveals
           SET plaintext_key = NULL
           WHERE plaintext_key IS NOT NULL
             AND revealed_at IS NULL
             AND expires_at < UTC_TIMESTAMP()"#,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
