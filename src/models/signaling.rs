use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalingMessage {
    UploaderReady {
        share_code: String,
        peer_id: String,
    },
    DownloaderJoin {
        share_code: String,
        peer_id: String,
    },
    PeerMatched {
        peer_id: String,
        role: PeerRole,
    },
    Offer {
        share_code: String,
        sdp: String,
        peer_id: String,
    },
    Answer {
        share_code: String,
        sdp: String,
        peer_id: String,
    },
    IceCandidate {
        share_code: String,
        candidate: String,
        sdp_mid: Option<String>,
        sdp_m_line_index: Option<u16>,
        peer_id: String,
    },
    Error {
        message: String,
    },
    TransferComplete {
        share_code: String,
    },
    UploaderOffline {
        share_code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerRole {
    Uploader,
    Downloader,
}
