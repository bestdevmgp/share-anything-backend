use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    db::{repository, DbPool},
    models::signaling::{PeerRole, SignalingMessage},
    services::signaling::SignalingState,
};

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State((state, db)): State<(SignalingState, DbPool)>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, db))
}

async fn handle_socket(socket: WebSocket, state: SignalingState, db: DbPool) {
    let (mut sender, mut receiver) = socket.split();
    let peer_id = Uuid::new_v4().to_string();

    let (tx, mut rx) = mpsc::unbounded_channel();
    state.register_connection(peer_id.clone(), tx);

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

async fn handle_message(
    text: &str,
    peer_id: &str,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let msg: SignalingMessage = serde_json::from_str(text)?;

    match msg {
        SignalingMessage::UploaderReady {
            share_code,
            peer_id: _,
        } => {
            handle_uploader_ready(share_code, peer_id, state, db).await?;
        }
        SignalingMessage::DownloaderJoin {
            share_code,
            peer_id: _,
        } => {
            handle_downloader_join(share_code, peer_id, state, db).await?;
        }
        SignalingMessage::Offer {
            share_code,
            sdp,
            peer_id: _,
        } => {
            relay_to_uploader(
                &share_code,
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
            relay_to_downloader(
                &share_code,
                peer_id,
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
        SignalingMessage::TransferComplete { share_code } => {
            handle_transfer_complete(share_code, db).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn handle_uploader_ready(
    share_code: String,
    peer_id: &str,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_share = repository::find_file_share_by_code(db, &share_code)
        .await?
        .ok_or("Share code not found")?;

    if file_share.transfer_type != "p2p" {
        return Err("This share is not configured for P2P transfer".into());
    }

    state.register_uploader(share_code.clone(), peer_id.to_string());
    repository::update_p2p_status(db, &share_code, "waiting", Some(peer_id.to_string())).await?;

    Ok(())
}

async fn handle_downloader_join(
    share_code: String,
    peer_id: &str,
    state: &SignalingState,
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let uploader_peer_id = state
        .find_uploader(&share_code)
        .ok_or("Uploader is not online")?;

    state.send_to_peer(
        &uploader_peer_id,
        SignalingMessage::PeerMatched {
            peer_id: peer_id.to_string(),
            role: PeerRole::Downloader,
        },
    )?;

    state.send_to_peer(
        peer_id,
        SignalingMessage::PeerMatched {
            peer_id: uploader_peer_id.clone(),
            role: PeerRole::Uploader,
        },
    )?;

    repository::update_p2p_status(db, &share_code, "connected", None).await?;

    Ok(())
}

async fn relay_to_uploader(
    share_code: &str,
    message: SignalingMessage,
    state: &SignalingState,
) -> Result<(), Box<dyn std::error::Error>> {
    let uploader_peer_id = state
        .find_uploader(share_code)
        .ok_or("Uploader is not online")?;

    state.send_to_peer(&uploader_peer_id, message)?;

    Ok(())
}

async fn relay_to_downloader(
    _share_code: &str,
    downloader_peer_id: &str,
    message: SignalingMessage,
    state: &SignalingState,
) -> Result<(), Box<dyn std::error::Error>> {
    state.send_to_peer(downloader_peer_id, message)?;
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

    let target_peer_id = if peer_id == uploader_peer_id {
        return Err("Downloader peer ID not tracked".into());
    } else {
        uploader_peer_id
    };

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
    db: &DbPool,
) -> Result<(), Box<dyn std::error::Error>> {
    repository::complete_p2p_transfer(db, &share_code).await?;
    Ok(())
}

async fn cleanup_peer(peer_id: &str, state: &SignalingState, db: &DbPool) {
    let share_codes: Vec<String> = state
        .uploaders
        .iter()
        .filter(|entry| entry.value() == peer_id)
        .map(|entry| entry.key().clone())
        .collect();

    for share_code in share_codes {
        state.remove_uploader(&share_code);
        let _ = repository::update_p2p_status(db, &share_code, "failed", None).await;
    }

    state.remove_connection(peer_id);
}
