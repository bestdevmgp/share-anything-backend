use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::{
    db::{repository, DbPool},
    models::{FileShareResponse, TransferType},
    services::email::{EmailService, FileNotificationInfo},
};

/// 사용자 대면 알림(이메일) 발송을 담당. 핸들러에서 직접 EmailService를 부르지 않고
/// 이 서비스를 통해 한 번에 다운로더+업로더 알림 같은 결합 로직을 처리한다.
pub struct NotificationService {
    db: DbPool,
    email: Arc<EmailService>,
}

impl NotificationService {
    pub fn new(db: DbPool, email: Arc<EmailService>) -> Arc<Self> {
        Arc::new(Self { db, email })
    }

    /// 다운로드 시 알림: 다운로더(자신의 다운로드 알림)와 업로더(누가 받았는지 알림)에게
    /// 각각 이메일을 보낸다. self-download(업로더가 본인 파일 다운로드)일 때는 업로더 알림 생략.
    pub async fn notify_download(
        &self,
        share_code: &str,
        files: Vec<FileNotificationInfo>,
        downloader_user_id: Option<&str>,
        uploader_user_id: Option<&str>,
        downloader_ip: &str,
    ) {
        let downloader = match downloader_user_id {
            Some(id) => repository::find_user_by_id(&self.db, id).await.ok().flatten(),
            None => None,
        };
        let uploader = match uploader_user_id {
            Some(id) => repository::find_user_by_id(&self.db, id).await.ok().flatten(),
            None => None,
        };

        // Notify downloader
        if let Some(ref user) = downloader {
            if user.notify_download {
                self.email.send_download_notification(
                    &user.name,
                    &user.email,
                    share_code,
                    files.clone(),
                    uploader.as_ref().map(|u| u.name.as_str()),
                    &user.notify_language,
                );
            }
        }

        // Notify uploader unless self-download
        let is_self_download = match (&downloader, &uploader) {
            (Some(d), Some(u)) => d.id == u.id,
            _ => false,
        };
        if is_self_download {
            return;
        }

        if let Some(uploader) = uploader {
            if uploader.notify_download_alert {
                self.email.send_download_alert_notification(
                    &uploader.name,
                    &uploader.email,
                    downloader.as_ref().map(|d| d.name.as_str()),
                    share_code,
                    files,
                    downloader_ip,
                    &uploader.notify_language,
                );
            }
        }
    }

    /// 업로드 시 알림: 업로더에게 본인 파일이 업로드 됐다는 메일. P2P 전송은 서버에 파일이
    /// 남지 않으므로 알림 대상에서 제외한다.
    pub async fn notify_upload(
        &self,
        uploader_user_id: &str,
        share_code: &str,
        uploaded_files: &[FileShareResponse],
        expires_at: DateTime<Utc>,
        password: Option<String>,
        description: Option<String>,
        transfer_type: TransferType,
    ) {
        if matches!(transfer_type, TransferType::P2p) {
            return;
        }

        let user = match repository::find_user_by_id(&self.db, uploader_user_id).await {
            Ok(Some(u)) => u,
            _ => return,
        };
        if !user.notify_upload {
            return;
        }

        let notification_files: Vec<FileNotificationInfo> = uploaded_files
            .iter()
            .map(|f| FileNotificationInfo {
                file_name: f.file_name.clone(),
                file_size: f.file_size,
                file_type: f.file_type.clone(),
            })
            .collect();

        self.email.send_upload_notification(
            &user.name,
            &user.email,
            share_code,
            notification_files,
            expires_at,
            password,
            description,
            &user.notify_language,
        );
    }
}
