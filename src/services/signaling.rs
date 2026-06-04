use axum::extract::ws::Message;
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::models::signaling::SignalingMessage;

pub type PeerId = String;
pub type ShareCode = String;

/// Per-API-key signaling connection caps. See `SignalingState::acquire_slot`.
pub const MAX_ACTIVE_PER_KEY: usize = 10;
pub const MAX_CONNECT_ATTEMPTS_PER_MINUTE: u32 = 30;

#[derive(Clone)]
pub struct UploaderInfo {
    pub peer_id: String,
    pub device_info: Option<String>,
}

#[derive(Debug)]
pub struct AttemptRecord {
    pub window_start: Instant,
    pub count: u32,
}

#[derive(Clone)]
pub struct SignalingState {
    pub uploaders: Arc<DashMap<ShareCode, UploaderInfo>>,
    pub downloaders: Arc<DashMap<ShareCode, PeerId>>,
    pub connections: Arc<DashMap<PeerId, mpsc::UnboundedSender<Message>>>,
    pub arrived_downloaders: Arc<DashMap<PeerId, ShareCode>>,
    pub active_per_key: Arc<DashMap<String, Arc<AtomicUsize>>>,
    pub attempts_per_key: Arc<DashMap<String, AttemptRecord>>,
}

/// Reason a signaling slot acquisition was refused.
#[derive(Debug, Clone, Copy)]
pub enum SlotRefusal {
    /// More than `MAX_CONNECT_ATTEMPTS_PER_MINUTE` upgrade attempts in the
    /// current 60-second window.
    TooManyAttempts,
    /// `MAX_ACTIVE_PER_KEY` connections are already open for this key.
    TooManyActive,
}

/// RAII handle that decrements the per-key active counter on drop.
pub struct ActiveSlot {
    counter: Arc<AtomicUsize>,
}

impl Drop for ActiveSlot {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

impl SignalingState {
    pub fn new() -> Self {
        Self {
            uploaders: Arc::new(DashMap::new()),
            downloaders: Arc::new(DashMap::new()),
            connections: Arc::new(DashMap::new()),
            arrived_downloaders: Arc::new(DashMap::new()),
            active_per_key: Arc::new(DashMap::new()),
            attempts_per_key: Arc::new(DashMap::new()),
        }
    }

    /// Charge a connect attempt and reserve an active slot for `key`
    /// (typically an API key id). Returns an [`ActiveSlot`] that releases the
    /// slot when dropped.
    pub fn acquire_slot(&self, key: &str) -> Result<ActiveSlot, SlotRefusal> {
        let now = Instant::now();
        let window = Duration::from_secs(60);

        let attempts_exceeded = {
            let mut entry = self.attempts_per_key.entry(key.to_string()).or_insert_with(|| AttemptRecord {
                window_start: now,
                count: 0,
            });
            if now.duration_since(entry.window_start) > window {
                entry.window_start = now;
                entry.count = 1;
                false
            } else {
                entry.count += 1;
                entry.count > MAX_CONNECT_ATTEMPTS_PER_MINUTE
            }
        };
        if attempts_exceeded {
            return Err(SlotRefusal::TooManyAttempts);
        }

        let counter = self
            .active_per_key
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
            .clone();
        let prev = counter.fetch_add(1, Ordering::Relaxed);
        if prev >= MAX_ACTIVE_PER_KEY {
            counter.fetch_sub(1, Ordering::Relaxed);
            return Err(SlotRefusal::TooManyActive);
        }

        Ok(ActiveSlot { counter })
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
