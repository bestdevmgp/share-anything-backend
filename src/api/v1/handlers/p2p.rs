use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::api::v1::{
    auth::{require_scope, require_token},
    error::PublicApiError,
    V1State,
};
use crate::db::{repository, DbPool};
use crate::handlers::cli::{cli_p2p_create, CliState};
use crate::handlers::turn::{get_turn_credentials, TurnState};
use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::models::personal_token::Scope;
use crate::models::signaling::SignalingMessage;
use crate::models::{
    CliP2PCreateRequest, CliP2PCreateResponse, P2pStatusResponse, TurnCredentialsResponse,
};
use crate::services::signaling::{ActiveSlot, SignalingState, SlotRefusal};
use crate::utils::PrettyJson;

const WS_SUBPROTOCOL: &str = "share-anything.v1";
const API_KEY_PROTOCOL_PREFIX: &str = "api-key.";

/// Create a P2P share session
///
/// Register a share whose files will be transferred peer-to-peer over WebRTC
/// instead of through object storage. The server only mediates session
/// metadata and signaling — file bytes never touch our infrastructure.
///
/// **When to use:** large transfers between two devices that are both online
/// simultaneously, or transfers where the sender wants end-to-end control of
/// the bytes (we cannot read them).
///
/// **Behaviour notes:**
/// - The share is **one-time** by design — it disappears after the first
///   successful download or after 24 hours, whichever comes first.
/// - Recipients need the same `share_code` and (if set) `password`.
/// - The uploader must then open the signaling WebSocket
///   (`GET /v1/ws/signaling`) and send an `uploader_ready` message, otherwise
///   downloaders cannot connect.
///
/// **Required scope:** `p2p_transfer`
#[utoipa::path(
    post,
    path = "/v1/p2p/sessions",
    tag = "p2p",
    request_body = CliP2PCreateRequest,
    responses(
        (status = 200,
            description = "P2P session created. The `share_code` is reserved but \
                           recipients cannot connect until the uploader opens the \
                           signaling WebSocket.",
            body = CliP2PCreateResponse),
        (status = 400,
            description = "Empty `files` array or malformed request body.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `p2p_transfer` scope.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_p2p_session(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Json(req): Json<CliP2PCreateRequest>,
) -> Result<PrettyJson<CliP2PCreateResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::P2pTransfer)?;

    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };

    let response = cli_p2p_create(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Json(req),
    )
    .await
    .map_err(PublicApiError::from)?;

    Ok(response)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct ShareCodePath {
    #[param(example = "482917")]
    pub code: String,
}

/// Check uploader liveness
///
/// Returns whether the uploader for a given P2P share is currently connected
/// to the signaling WebSocket. Use this from the recipient side before
/// attempting a download — if `uploader_online` is `false`, the WebRTC
/// handshake will fail because the uploader cannot respond to the offer.
///
/// **Required scope:** `p2p_transfer`
#[utoipa::path(
    get,
    path = "/v1/p2p/sessions/{code}/status",
    tag = "p2p",
    params(ShareCodePath),
    responses(
        (status = 200, description = "Liveness retrieved", body = P2pStatusResponse),
        (status = 401,
            description = "API key is missing, malformed, revoked, or expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `p2p_transfer` scope.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn get_p2p_status(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(path): Path<ShareCodePath>,
) -> Result<Json<P2pStatusResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::P2pTransfer)?;

    let is_online = state.signaling.find_uploader(&path.code).is_some();

    Ok(Json(P2pStatusResponse {
        share_code: path.code,
        uploader_online: is_online,
    }))
}

/// Get TURN/STUN credentials
///
/// Returns a fresh set of ICE servers usable with any standard WebRTC
/// `RTCPeerConnection`. Credentials are minted on demand by Cloudflare's
/// TURN service.
///
/// **Lifetime:** every issued credential is valid for exactly **24 hours
/// (86 400 seconds)** from the moment of this response. The response does
/// not currently include the expiry as a structured field — calculate it
/// as `now + 24h` if you need to track it. For most use cases simply
/// **request a fresh credential per transfer session** rather than caching;
/// the call is cheap and avoids any risk of an in-flight WebRTC connection
/// failing because a relayed credential expired mid-transfer.
///
/// Use this together with `POST /v1/p2p/sessions` and `GET /v1/ws/signaling`
/// to perform a NAT-traversed peer-to-peer transfer without operating your
/// own TURN infrastructure.
///
/// **Required scope:** `p2p_transfer`
#[utoipa::path(
    get,
    path = "/v1/turn/credentials",
    tag = "p2p",
    responses(
        (status = 200,
            description = "ICE servers retrieved. The list always includes at least one STUN \
                           entry and one TURN entry with a temporary username/credential.",
            body = TurnCredentialsResponse),
        (status = 401,
            description = "API key is missing, malformed, revoked, or expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `p2p_transfer` scope.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 500,
            description = "Upstream TURN provider failed to issue credentials.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn get_v1_turn_credentials(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
) -> Result<Json<TurnCredentialsResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::P2pTransfer)?;

    let turn_state = TurnState {
        config: state.config.clone(),
    };

    let response = get_turn_credentials(State(turn_state))
        .await
        .map_err(PublicApiError::from)?;

    Ok(response)
}

/// P2P signaling WebSocket
///
/// Long-lived WebSocket used by both the uploader and the downloader to
/// negotiate a WebRTC `RTCPeerConnection` for a given `share_code`. The
/// server only relays messages between the two peers; it never sees file
/// bytes.
///
/// ## Authentication
///
/// Browsers cannot set custom headers on WebSocket connections, so the API
/// key is passed through the `Sec-WebSocket-Protocol` handshake header
/// instead of `X-API-Key`. Send **two** subprotocol tokens, in this order:
///
/// 1. `share-anything.v1` — the protocol identifier (must be first).
/// 2. `api-key.sak_xxxxxxxxxxxx` — your API key prefixed with `api-key.`.
///
/// The server echoes back only `share-anything.v1` in the upgrade response;
/// the api-key entry is consumed and never reflected.
///
/// **Required scope:** `p2p_transfer`. The server enforces this at the
/// WebSocket upgrade step — keys without `p2p_transfer` cannot open the
/// connection at all, regardless of whether they intend to act as uploader
/// or downloader. Issue a new API key with the **P2P transfer** scope
/// checked if your existing key fails with `403`.
///
/// ### JavaScript example
///
/// ```js
/// const ws = new WebSocket(
///   'wss://share-api.mingyu.dev/v1/ws/signaling',
///   ['share-anything.v1', `api-key.${apiKey}`]
/// );
/// ```
///
/// ### Node.js (`ws` package) example
///
/// ```js
/// import WebSocket from 'ws';
/// const ws = new WebSocket(
///   'wss://share-api.mingyu.dev/v1/ws/signaling',
///   ['share-anything.v1', `api-key.${apiKey}`]
/// );
/// ```
///
/// ## Message envelope
///
/// Every message is a JSON object discriminated by a `type` field, e.g.
/// `{"type":"offer","share_code":"482917","sdp":"...","peer_id":"..."}`.
/// See the `SignalingMessage` schema below for every variant.
///
/// ## Password-protected shares
///
/// If the share was created with a password (see
/// `POST /v1/p2p/sessions` — the `password` field), the **downloader** must
/// include that password directly in the `downloader_join` message:
///
/// ```json
/// { "type": "downloader_join", "share_code": "Ab3xK9", "password": "hunter2" }
/// ```
///
/// The server verifies it against the stored bcrypt hash before matching the
/// peers. A wrong or missing password causes the join to fail with
/// `{"type":"error","message":"Incorrect password for this share"}` and the
/// downloader is never relayed to the uploader. This means **password checks
/// are enforced on the server** — clients cannot bypass them by skipping a
/// REST `verify-password` call.
///
/// The **uploader** never sends a password; uploaders are implicitly
/// authorised because they own the share.
///
/// ## Session flow
///
/// One `RTCPeerConnection` + `RTCDataChannel` is established at the start of
/// the session and **reused for every file in the share**. The downloader
/// requests subsequent files with `file_request` on the same WebSocket — no
/// new ICE handshake per file (which would cost 5–15s on a TURN relay).
///
/// ```mermaid
/// sequenceDiagram
///   participant U as Uploader
///   participant S as Signaling Server
///   participant D as Downloader
///
///   Note over U,D: Both sides connect to /v1/ws/signaling with API key
///   U->>S: uploader_ready { share_code }
///   D->>S: downloader_join { share_code, file_name, password? }
///   Note over S: server verifies password against bcrypt hash<br/>(only if share has_password = true)
///   S->>U: peer_matched { role: downloader, peer_id, file_name }
///   S->>D: peer_matched { role: uploader, peer_id }
///   U->>S: offer { sdp }
///   S->>D: offer { sdp }
///   D->>S: answer { sdp }
///   S->>U: answer { sdp }
///   U-->>S: ice_candidate { candidate } (trickled both ways)
///   Note over U,D: WebRTC DataChannel open — file #1 streams peer-to-peer:<br/>file_metadata → binary chunks → "__EOF__"
///   Note over U,D: (subsequent files reuse the same PC+DC)
///   D->>S: file_request { share_code, file_name }
///   S->>U: file_request { share_code, file_name }
///   Note over U,D: file #2 streams over the SAME DataChannel:<br/>file_metadata → binary chunks → "__EOF__"
///   Note over U,D: (repeat per file…)
///   D->>S: transfer_complete { share_code }
///   S->>U: transfer_complete { share_code }
///   Note over S: server deletes the share record (one-time consumed)
/// ```
///
/// Notes on the flow:
/// - **First file** is requested implicitly by the `file_name` on
///   `downloader_join`. **Subsequent files** are requested with
///   `file_request` on the WebSocket — the server relays it to the uploader,
///   which then streams that file on the **existing DataChannel**.
/// - **`transfer_complete` is sent by the downloader** after every file has
///   been received. The server relays it to the uploader (which exits its
///   send loop) and deletes the share record.
/// - ICE candidates are trickled in both directions during the initial
///   handshake until each side has a complete view. The server mirrors them
///   between the two peers.
/// - `peer_matched` is also emitted to a re-joining downloader if the
///   uploader is still online and the share has not yet been consumed.
///
/// ## DataChannel data format
///
/// Once the WebRTC `RTCDataChannel` is open, the **server is no longer
/// involved** — the bytes flow directly between the two peers. The uploader
/// drives the channel with a fixed framing convention; downloaders MUST
/// implement the same parser.
///
/// **Per-file framing** — for every file in the share, the uploader sends:
///
/// 1. **One text frame** with a JSON metadata object. The discriminator is
///    `type: "file_metadata"` (distinct from signaling messages, which live
///    on the WebSocket, not the DataChannel):
///
///    ```json
///    {
///      "type": "file_metadata",
///      "fileName": "report.pdf",
///      "fileSize": 5242880,
///      "fileType": "application/pdf"
///    }
///    ```
///    `fileName`/`fileSize`/`fileType` use **camelCase** (DataChannel
///    convention) — contrast with the snake_case used on the signaling
///    WebSocket.
///
/// 2. **Zero or more binary frames** containing the file payload, in order,
///    until `fileSize` bytes have been delivered.
///
/// 3. **One text frame** with the literal string `"__EOF__"` to mark the end
///    of the current file. Receivers should commit the accumulated buffer
///    on this frame and reset for the next file.
///
/// **Multi-file shares** repeat steps 1–3 once per file on the **same**
/// DataChannel — the downloader triggers each subsequent file by sending
/// `{"type":"file_request","share_code":"...","file_name":"..."}` on the
/// WebSocket (not the DataChannel), then waits for the uploader to start
/// streaming `file_metadata` for that file on the existing DataChannel.
/// File order is up to the downloader; pick from the `files` list returned
/// by `GET /v1/shares/{code}` (or by the original `POST /v1/p2p/sessions`).
///
/// **Recommended chunk size:** **64 KiB** (65 536 bytes) per binary frame.
/// Larger frames risk hitting SCTP message-size limits in some WebRTC
/// implementations.
///
/// **Back-pressure:** before sending each chunk, check
/// `dataChannel.bufferedAmount` and pause sending once it exceeds **4 MiB**
/// (4 194 304 bytes). Resume when it drops back below **1 MiB** (1 048 576
/// bytes) — use `bufferedAmountLowThreshold` + the `bufferedamountlow`
/// event for an efficient event-driven loop. The 4 MiB high-water mark is
/// sized for typical TURN-relayed BDP (≈200ms RTT × ~20 MB/s) so SCTP can
/// keep the send window full without piling unbounded memory on direct
/// (low-RTT) paths.
///
/// **Cancellation:** if the uploader aborts mid-transfer it should send
/// `{"type":"uploader_cancelled","share_code":"..."}` on the **WebSocket**
/// (not the DataChannel). The server forwards this to the downloader so it
/// can stop the partial download.
///
/// ## Heartbeat
///
/// Send `{"type":"ping"}` periodically (every ~20s) to keep proxies from
/// closing idle connections; the server replies with `{"type":"pong"}`.
///
/// ## Errors
///
/// - **Handshake `401`** — missing or invalid `api-key.*` subprotocol on
///   the upgrade request. The socket never opens.
/// - **Once connected**, protocol-level problems arrive as
///   `{"type":"error","message":"..."}` text frames on the WebSocket.
///   Commonly seen messages:
///   - `"Share code not found"` — the share has expired, been consumed,
///     or never existed.
///   - `"This share is not configured for P2P transfer"` — the share's
///     `transfer_type` is `server`, not `p2p`. Use
///     `GET /v1/shares/{code}/download` instead.
///   - `"Incorrect password for this share"` — wrong `password` field on
///     `downloader_join` for a password-protected share.
///   - `"Uploader is not online"` — the downloader joined before the
///     uploader sent `uploader_ready`, or after the uploader disconnected.
#[utoipa::path(
    get,
    path = "/v1/ws/signaling",
    tag = "p2p",
    responses(
        (status = 101, description = "WebSocket upgrade succeeded. The connection is now bidirectional and carries `SignalingMessage` JSON frames."),
        (status = 401, description = "Missing or invalid `api-key.*` subprotocol on the upgrade request."),
        (status = 403, description = "API key does not have the `p2p_transfer` scope."),
        (status = 429,
            description = "Either `p2p_connection_limit` — too many concurrent P2P WebSocket connections for this API key, or \
                           `p2p_attempt_limit` — too many upgrade attempts within the last minute. \
                           Numeric limits at <https://share.mingyu.dev/api-terms-of-use>.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn signaling_ws(
    State(state): State<V1State>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let user = match authenticate_ws(&headers, &state.db).await {
        Ok(u) => u,
        Err(err) => return err.into_response(),
    };

    let slot = match state.signaling.acquire_slot(&user.personal_token_id) {
        Ok(s) => s,
        Err(SlotRefusal::TooManyAttempts) => {
            return PublicApiError::P2PAttemptLimit.into_response();
        }
        Err(SlotRefusal::TooManyActive) => {
            return PublicApiError::P2PConnectionLimit.into_response();
        }
    };

    let signaling = state.signaling.clone();
    let db = state.db.clone();
    ws.protocols([WS_SUBPROTOCOL])
        .on_upgrade(move |socket| handle_socket_with_slot(socket, signaling, db, slot))
}

async fn handle_socket_with_slot(
    socket: WebSocket,
    state: SignalingState,
    db: DbPool,
    slot: ActiveSlot,
) {
    handle_socket(socket, state, db).await;
    drop(slot);
}

async fn authenticate_ws(
    headers: &HeaderMap,
    db: &DbPool,
) -> Result<PersonalTokenUser, PublicApiError> {
    let mut saw_identifier = false;
    let mut api_key: Option<String> = None;

    for value in headers.get_all(axum::http::header::SEC_WEBSOCKET_PROTOCOL) {
        let raw = value
            .to_str()
            .map_err(|_| PublicApiError::Unauthorized("Invalid Sec-WebSocket-Protocol header".into()))?;
        for entry in raw.split(',') {
            let entry = entry.trim();
            if entry == WS_SUBPROTOCOL {
                saw_identifier = true;
            } else if let Some(rest) = entry.strip_prefix(API_KEY_PROTOCOL_PREFIX) {
                api_key = Some(rest.to_string());
            }
        }
    }

    if !saw_identifier {
        return Err(PublicApiError::Unauthorized(format!(
            "Missing '{}' subprotocol on WebSocket upgrade.",
            WS_SUBPROTOCOL
        )));
    }

    let token = api_key.ok_or_else(|| {
        PublicApiError::Unauthorized(
            "Missing 'api-key.<sak_...>' subprotocol on WebSocket upgrade.".into(),
        )
    })?;

    if !token.starts_with("sak_") {
        return Err(PublicApiError::Unauthorized(
            "Invalid token format. Only API keys (sak_ prefix) are accepted.".into(),
        ));
    }

    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    let api_key_row = repository::find_api_key_by_hash(db, &token_hash)
        .await
        .map_err(|_| PublicApiError::Internal)?
        .ok_or_else(|| PublicApiError::Unauthorized("Invalid API Key".into()))?;

    if let Some(expires_at) = api_key_row.expires_at {
        if expires_at < chrono::Utc::now() {
            return Err(PublicApiError::Unauthorized("API Key has expired".into()));
        }
    }

    if api_key_row.revoked_at.is_some() {
        return Err(PublicApiError::Unauthorized("API Key has been revoked".into()));
    }

    let scopes = repository::find_scopes_by_api_key(db, &api_key_row.id)
        .await
        .map_err(|_| PublicApiError::Internal)?;

    if !scopes.contains(&Scope::P2pTransfer) {
        return Err(PublicApiError::InsufficientScope("p2p_transfer"));
    }

    let key_id = api_key_row.id.clone();
    let db_clone = db.clone();
    tokio::spawn(async move {
        if let Err(e) =
            repository::update_api_key_last_used_with_platform(&db_clone, &key_id, None).await
        {
            tracing::warn!(error = %e, "Failed to update API key last_used_at");
        }
    });

    let api_key_id = api_key_row.id.clone();
    Ok(PersonalTokenUser {
        user_id: api_key_row.user_id,
        personal_token_id: api_key_row.id,
        scopes,
        api_key_id: Some(api_key_id),
    })
}

async fn handle_socket(socket: WebSocket, state: SignalingState, db: DbPool) {
    let (mut sender, mut receiver) = socket.split();
    let peer_id = Uuid::new_v4().to_string();

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
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
                crate::handlers::signaling::dispatch_message(&text, &peer_id_clone, &state_clone, &db_clone).await
            {
                tracing::error!("v1 signaling error: {}", e);
                let error_msg = SignalingMessage::Error {
                    message: e.to_string(),
                };
                let _ = state_clone.send_to_peer(&peer_id_clone, error_msg);
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    crate::handlers::signaling::cleanup_peer_public(&peer_id, &state, &db).await;
}
