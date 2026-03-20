use axum::extract::ws::Message;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::models::signaling::SignalingMessage;

pub type PeerId = String;
pub type ShareCode = String;

#[derive(Clone)]
pub struct UploaderInfo {
    pub peer_id: String,
    pub device_info: Option<String>,
}

#[derive(Clone)]
pub struct SignalingState {
    pub uploaders: Arc<DashMap<ShareCode, UploaderInfo>>,
    pub downloaders: Arc<DashMap<ShareCode, PeerId>>,
    pub connections: Arc<DashMap<PeerId, mpsc::UnboundedSender<Message>>>,
    pub arrived_downloaders: Arc<DashMap<PeerId, ShareCode>>,
}

impl SignalingState {
    pub fn new() -> Self {
        Self {
            uploaders: Arc::new(DashMap::new()),
            downloaders: Arc::new(DashMap::new()),
            connections: Arc::new(DashMap::new()),
            arrived_downloaders: Arc::new(DashMap::new()),
        }
    }

    pub fn register_uploader_with_device(&self, share_code: String, peer_id: String, device_info: Option<String>) {
        self.uploaders.insert(share_code, UploaderInfo {
            peer_id,
            device_info,
        });
    }

    pub fn find_uploader(&self, share_code: &str) -> Option<String> {
        self.uploaders.get(share_code).map(|v| v.peer_id.clone())
    }

    pub fn find_uploader_with_device(&self, share_code: &str) -> Option<(String, Option<String>)> {
        self.uploaders.get(share_code).map(|v| (v.peer_id.clone(), v.device_info.clone()))
    }

    pub fn remove_uploader(&self, share_code: &str) {
        self.uploaders.remove(share_code);
    }

    pub fn register_downloader(&self, share_code: String, peer_id: String) {
        self.downloaders.insert(share_code, peer_id);
    }

    pub fn find_downloader(&self, share_code: &str) -> Option<String> {
        self.downloaders.get(share_code).map(|v| v.clone())
    }

    pub fn remove_downloader(&self, share_code: &str) {
        self.downloaders.remove(share_code);
    }

    pub fn register_connection(&self, peer_id: String, sender: mpsc::UnboundedSender<Message>) {
        self.connections.insert(peer_id, sender);
    }

    pub fn remove_connection(&self, peer_id: &str) {
        self.connections.remove(peer_id);
    }

    pub fn register_arrived_downloader(&self, peer_id: String, share_code: String) {
        self.arrived_downloaders.insert(peer_id, share_code);
    }

    pub fn remove_arrived_downloader(&self, peer_id: &str) -> Option<(PeerId, ShareCode)> {
        self.arrived_downloaders.remove(peer_id)
    }

    pub fn send_to_peer(&self, peer_id: &str, message: SignalingMessage) -> Result<(), String> {
        if let Some(sender) = self.connections.get(peer_id) {
            let json = serde_json::to_string(&message)
                .map_err(|e| format!("Failed to serialize message: {}", e))?;
            sender.send(Message::Text(json))
                .map_err(|e| format!("Failed to send message: {}", e))?;
            Ok(())
        } else {
            Err(format!("Peer {} not connected", peer_id))
        }
    }
}
