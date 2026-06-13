use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::session_token::validate_session_token,
    models::signaling::{PeerRole, SignalingMessage},
    services::signaling::SignalingState,
};

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State((state, db, config)): State<(SignalingState, DbPool, Arc<Config>)>,
) -> Response {
    // Gate the upgrade on a valid session token (query param, since browsers
    // can't set WS headers) so bots can't join signaling without Turnstile.
    let token = params.get("token").map(|s| s.as_str()).unwrap_or("");
    if !validate_session_token(token, &config.session_token.jwt_secret) {
        return (StatusCode::UNAUTHORIZED, "session token required").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state, db))
}

async fn handle_socket(socket: WebSocket, state: SignalingState, db: DbPool) {
    let (mut sender, mut receiver) = socket.split();
    let peer_id = Uuid::new_v4().to_string();

    let (tx, mut rx) = mpsc::unbounded_channel();
    state.register_connection(peer_id.clone(), tx);
    tracing::info!("[P2P-SIG] ws connected peer={}", peer_id);

    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let peer_id_clone = peer_id.clone();
    let state_clone = state.clone();
    let db_clone = db.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            if let Err(e) =
                handle_message(&text, &peer_id_clone, &state_clone, &db_clone).await
            {
                tracing::error!("Error handling message: {}", e);
                let error_msg = SignalingMessage::Error {
                    message: e.to_string(),
                };
                let _ = state_clone.send_to_peer(&peer_id_clone, error_msg);
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
        }
        _ = (&mut recv_task) => {
            send_task.abort();
        }
    }

    cleanup_peer(&peer_id, &state, &db).await;
}

pub async fn dispatch_message(
    text: &str,
    peer_id: &str,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    handle_message(text, peer_id, state, db).await
}

pub async fn cleanup_peer_public(peer_id: &str, state: &SignalingState, db: &DbPool) {
    cleanup_peer(peer_id, state, db).await
}

fn message_label(msg: &SignalingMessage) -> &'static str {
    match msg {
        SignalingMessage::UploaderReady { .. } => "uploader_ready",
        SignalingMessage::DownloaderJoin { .. } => "downloader_join",
        SignalingMessage::Offer { .. } => "offer",
        SignalingMessage::Answer { .. } => "answer",
        SignalingMessage::IceCandidate { .. } => "ice_candidate",
        SignalingMessage::DownloaderArrived { .. } => "downloader_arrived",
        SignalingMessage::TransferComplete { .. } => "transfer_complete",
        SignalingMessage::UploaderCancelled { .. } => "uploader_cancelled",
        SignalingMessage::FileRequest { .. } => "file_request",
        SignalingMessage::Ping {} => "ping",
        _ => "other",
    }
}

async fn handle_message(
    text: &str,
    peer_id: &str,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let msg: SignalingMessage = serde_json::from_str(text)?;

    if !matches!(msg, SignalingMessage::Ping {}) {
        tracing::info!("[P2P-SIG] recv type={} peer={}", message_label(&msg), peer_id);
    }

    match msg {
        SignalingMessage::UploaderReady {
            share_code,
            peer_id: _,
            device_info,
        } => {
            handle_uploader_ready(share_code, peer_id, device_info, state, db).await?;
        }
        SignalingMessage::DownloaderJoin {
            share_code,
            peer_id: _,
            file_name,
            device_info,
            password,
        } => {
            handle_downloader_join(share_code, peer_id, file_name, device_info, password, state, db).await?;
        }
        SignalingMessage::Offer {
            share_code,
            sdp,
            peer_id: _,
        } => {
            relay_to_downloader(
                &share_code,
                peer_id,
                SignalingMessage::Offer {
                    share_code: share_code.clone(),
                    sdp,
                    peer_id: peer_id.to_string(),
                },
                state,
            )
            .await?;
        }
        SignalingMessage::Answer {
            share_code,
            sdp,
            peer_id: _,
        } => {
            relay_to_uploader(
                &share_code,
                SignalingMessage::Answer {
                    share_code: share_code.clone(),
                    sdp,
                    peer_id: peer_id.to_string(),
                },
                state,
            )
            .await?;
        }
        SignalingMessage::IceCandidate {
            share_code,
            candidate,
            sdp_mid,
            sdp_m_line_index,
            peer_id: _,
        } => {
            relay_ice_candidate(
                share_code,
                candidate,
                sdp_mid,
                sdp_m_line_index,
                peer_id,
                state,
            )
            .await?;
        }
        SignalingMessage::DownloaderArrived {
            share_code,
            peer_id: _,
            device_info,
        } => {
            handle_downloader_arrived(share_code, peer_id, device_info, state).await?;
        }
        SignalingMessage::TransferComplete { share_code } => {
            handle_transfer_complete(share_code, state, db).await?;
        }
        SignalingMessage::UploaderCancelled { share_code } => {
            let downloader = state.find_downloader(&share_code);
            tracing::info!("[P2P-SIG] uploader_cancelled share={} downloader_found={}", share_code, downloader.is_some());
            if let Some(downloader_peer_id) = downloader {
                let _ = state.send_to_peer(
                    &downloader_peer_id,
                    SignalingMessage::UploaderCancelled {
                        share_code: share_code.clone(),
                    },
                );
            }
        }
        SignalingMessage::FileRequest { share_code, file_name } => {
            relay_to_uploader(
                &share_code,
                SignalingMessage::FileRequest {
                    share_code: share_code.clone(),
                    file_name,
                },
                state,
            )
            .await?;
        }
        SignalingMessage::Ping {} => {
            state.send_to_peer(peer_id, SignalingMessage::Pong {})?;
        }
        _ => {}
    }

    Ok(())
}

async fn handle_uploader_ready(
    share_code: String,
    peer_id: &str,
    device_info: Option<String>,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_share = repository::find_file_share_by_code(db, &share_code)
        .await?
        .ok_or("Share code not found")?;

    if file_share.transfer_type != "p2p" {
        return Err("This share is not configured for P2P transfer".into());
    }

    state.register_uploader_with_device(share_code.clone(), peer_id.to_string(), device_info);
    repository::update_p2p_status(db, &share_code, "waiting", Some(peer_id.to_string())).await?;
    tracing::info!("[P2P-SIG] uploader registered share={} peer={}", share_code, peer_id);

    Ok(())
}

async fn handle_downloader_join(
    share_code: String,
    peer_id: &str,
    file_name: Option<String>,
    device_info: Option<String>,
    password: Option<String>,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_share = repository::find_file_share_by_code(db, &share_code)
        .await?
        .ok_or("Share code not found")?;

    if let Some(stored_hash) = &file_share.password_hash {
        let supplied = password.unwrap_or_default();
        let matches = bcrypt::verify(&supplied, stored_hash).unwrap_or(false);
        if !matches {
            return Err("Incorrect password for this share".into());
        }
    }

    let uploader = state.find_uploader_with_device(&share_code);
    tracing::info!(
        "[P2P-SIG] downloader_join share={} peer={} uploader_found={}",
        share_code, peer_id, uploader.is_some()
    );
    let (uploader_peer_id, uploader_device_info) = uploader.ok_or("Uploader is not online")?;

    state.register_downloader(share_code.clone(), peer_id.to_string());

    state.send_to_peer(
        &uploader_peer_id,
        SignalingMessage::PeerMatched {
            peer_id: peer_id.to_string(),
            role: PeerRole::Downloader,
            file_name: file_name.clone(),
            device_info: device_info.clone(),
        },
    )?;

    state.send_to_peer(
        peer_id,
        SignalingMessage::PeerMatched {
            peer_id: uploader_peer_id.clone(),
            role: PeerRole::Uploader,
            file_name,
            device_info: uploader_device_info,
        },
    )?;

    repository::update_p2p_status(db, &share_code, "connected", None).await?;
    tracing::info!("[P2P-SIG] peer_matched sent share={} uploader={} downloader={}", share_code, uploader_peer_id, peer_id);

    Ok(())
}

async fn handle_downloader_arrived(
    share_code: String,
    peer_id: &str,
    device_info: Option<String>,
    state: &SignalingState,
) -> Result<(), Box<dyn std::error::Error>> {
    let uploader = state.find_uploader(&share_code);
    tracing::info!("[P2P-SIG] downloader_arrived share={} peer={} uploader_found={}", share_code, peer_id, uploader.is_some());
    let uploader_peer_id = uploader.ok_or("Uploader is not online")?;

    state.register_arrived_downloader(peer_id.to_string(), share_code.clone());

    state.send_to_peer(
        &uploader_peer_id,
        SignalingMessage::DownloaderArrived {
            share_code,
            peer_id: peer_id.to_string(),
            device_info,
        },
    )?;

    Ok(())
}

async fn relay_to_uploader(
    share_code: &str,
    message: SignalingMessage,
    state: &SignalingState,
) -> Result<(), Box<dyn std::error::Error>> {
    let uploader = state.find_uploader(share_code);
    tracing::info!("[P2P-SIG] relay->uploader share={} type={} uploader_found={}", share_code, message_label(&message), uploader.is_some());
    let uploader_peer_id = uploader.ok_or("Uploader is not online")?;

    state.send_to_peer(&uploader_peer_id, message)?;

    Ok(())
}

async fn relay_to_downloader(
    share_code: &str,
    _sender_peer_id: &str,
    message: SignalingMessage,
    state: &SignalingState,
) -> Result<(), Box<dyn std::error::Error>> {
    let downloader = state.find_downloader(share_code);
    tracing::info!("[P2P-SIG] relay->downloader share={} type={} downloader_found={}", share_code, message_label(&message), downloader.is_some());
    let downloader_peer_id = downloader.ok_or("Downloader is not online")?;

    state.send_to_peer(&downloader_peer_id, message)?;
    Ok(())
}

async fn relay_ice_candidate(
    share_code: String,
    candidate: String,
    sdp_mid: Option<String>,
    sdp_m_line_index: Option<u16>,
    peer_id: &str,
    state: &SignalingState,
) -> Result<(), Box<dyn std::error::Error>> {
    let uploader_peer_id = state
        .find_uploader(&share_code)
        .ok_or("Uploader is not online")?;

    let from_uploader = peer_id == uploader_peer_id;
    let target_peer_id = if from_uploader {
        state
            .find_downloader(&share_code)
            .ok_or("Downloader is not online")?
    } else {
        uploader_peer_id
    };

    tracing::info!("[P2P-SIG] relay ice share={} dir={}", share_code, if from_uploader { "up->down" } else { "down->up" });

    state.send_to_peer(
        &target_peer_id,
        SignalingMessage::IceCandidate {
            share_code,
            candidate,
            sdp_mid,
            sdp_m_line_index,
            peer_id: peer_id.to_string(),
        },
    )?;

    Ok(())
}

async fn handle_transfer_complete(
    share_code: String,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(uploader_peer_id) = state.find_uploader(&share_code) {
        let _ = state.send_to_peer(
            &uploader_peer_id,
            SignalingMessage::TransferComplete { share_code: share_code.clone() },
        );
    }
    repository::complete_p2p_transfer(db, &share_code).await?;
    Ok(())
}

async fn cleanup_peer(peer_id: &str, state: &SignalingState, db: &DbPool) {
    tracing::info!("[P2P-SIG] ws disconnected peer={} (cleanup)", peer_id);
    let uploader_share_codes: Vec<String> = state
        .uploaders
        .iter()
        .filter(|entry| entry.value().peer_id == peer_id)
        .map(|entry| entry.key().clone())
        .collect();

    for share_code in uploader_share_codes {
        state.remove_uploader(&share_code);
        state.remove_downloader(&share_code);
        let _ = repository::update_p2p_status(db, &share_code, "failed", None).await;
        tracing::info!("[P2P-SIG] cleanup removed uploader share={} peer={}", share_code, peer_id);
    }

    let downloader_share_codes: Vec<String> = state
        .downloaders
        .iter()
        .filter(|entry| entry.value() == peer_id)
        .map(|entry| entry.key().clone())
        .collect();

    for share_code in downloader_share_codes {
        if !state.remove_downloader_if_matches(&share_code, peer_id) {
            continue;
        }
        if let Some(uploader_peer_id) = state.find_uploader(&share_code) {
            let _ = state.send_to_peer(
                &uploader_peer_id,
                SignalingMessage::DownloaderOffline {
                    share_code: share_code.clone(),
                },
            );
            tracing::info!("[P2P-SIG] cleanup removed downloader share={} peer={}, sent DownloaderOffline", share_code, peer_id);
        }
        let _ = repository::update_p2p_status(db, &share_code, "waiting", None).await;
    }

    if let Some((_, share_code)) = state.remove_arrived_downloader(peer_id) {
        if state.find_downloader(&share_code).is_none() {
            if let Some(uploader_peer_id) = state.find_uploader(&share_code) {
                let _ = state.send_to_peer(
                    &uploader_peer_id,
                    SignalingMessage::DownloaderOffline {
                        share_code,
                    },
                );
            }
        }
    }

    state.remove_connection(peer_id);
}
