//! Injects `x-codeSamples` vendor extensions into the OpenAPI document.
//!
//! Scalar (and Redoc) renders these as language tabs in the API reference UI,
//! so a developer can pick their language and copy a ready-to-run starting
//! point. The samples are illustrative — they are not run through our CI and
//! external consumers should verify them in their own environment.

use std::collections::HashMap;

use serde_json::json;
use utoipa::openapi::{path::PathItemType, OpenApi};
use utoipa::Modify;

pub struct CodeSamples;

impl Modify for CodeSamples {
    fn modify(&self, openapi: &mut OpenApi) {
        inject(
            openapi,
            "/v1/ws/signaling",
            PathItemType::Get,
            signaling_samples(),
        );
    }
}

fn inject(
    openapi: &mut OpenApi,
    path: &str,
    op_type: PathItemType,
    samples: serde_json::Value,
) {
    let Some(item) = openapi.paths.paths.get_mut(path) else {
        return;
    };
    let Some(op) = item.operations.get_mut(&op_type) else {
        return;
    };
    let mut ext = op.extensions.clone().unwrap_or_else(HashMap::new);
    ext.insert("x-codeSamples".to_string(), samples);
    op.extensions = Some(ext);
}

fn signaling_samples() -> serde_json::Value {
    // `lang` is a free-form full language name so Scalar groups our custom
    // samples into a separate "Code Examples" category (it does not merge
    // them into the auto-generated client tabs even when the value matches a
    // httpsnippet key). The `label` is what shows up in the dropdown, so we
    // lead with the language name and keep the library as a secondary hint
    // in parentheses — this makes the dropdown scannable by language.
    json!([
        { "lang": "JavaScript", "label": "JavaScript - Downloader (Browser)",        "source": JS_DOWNLOADER },
        { "lang": "JavaScript", "label": "JavaScript - Uploader (Browser)",          "source": JS_UPLOADER },
        { "lang": "TypeScript", "label": "TypeScript - Downloader (Node.js)",        "source": NODE_TS_DOWNLOADER },
        { "lang": "TypeScript", "label": "TypeScript - Uploader (Node.js)",          "source": NODE_TS_UPLOADER },
        { "lang": "Python",     "label": "Python - Downloader (aiortc)",             "source": PYTHON_DOWNLOADER },
        { "lang": "Python",     "label": "Python - Uploader (aiortc)",               "source": PYTHON_UPLOADER },
        { "lang": "Rust",       "label": "Rust - Downloader (webrtc-rs)",            "source": RUST_DOWNLOADER },
        { "lang": "Rust",       "label": "Rust - Uploader (webrtc-rs)",              "source": RUST_UPLOADER },
        { "lang": "Go",         "label": "Go - Downloader (pion/webrtc)",            "source": GO_DOWNLOADER },
        { "lang": "Go",         "label": "Go - Uploader (pion/webrtc)",              "source": GO_UPLOADER },
        { "lang": "Kotlin",     "label": "Kotlin - Downloader (Spring Boot)",        "source": KOTLIN_DOWNLOADER },
        { "lang": "Kotlin",     "label": "Kotlin - Uploader (Spring Boot)",          "source": KOTLIN_UPLOADER },
        { "lang": "Java",       "label": "Java - Downloader (Spring Boot)",          "source": JAVA_DOWNLOADER },
        { "lang": "Java",       "label": "Java - Uploader (Spring Boot)",            "source": JAVA_UPLOADER },
        { "lang": "PHP",        "label": "PHP - Downloader (signaling only)",        "source": PHP_DOWNLOADER },
        { "lang": "PHP",        "label": "PHP - Uploader (signaling only)",          "source": PHP_UPLOADER },
    ])
}

const JS_DOWNLOADER: &str = r#"// Share-Anything P2P Downloader — Browser JavaScript
// Receives a P2P share over WebRTC DataChannel and assembles each file as a Blob.

const API_BASE  = 'https://share-api.mingyu.dev';
const apiKey    = 'sak_xxxxxxxxxxxx';   // your API key
const shareCode = '482917';
const password  = undefined;            // set if the share is password-protected

(async () => {
  // 1. Fresh TURN/STUN credentials (24h TTL, fetch per session).
  const { ice_servers } = await fetch(`${API_BASE}/v1/turn/credentials`, {
    headers: { 'X-API-Key': apiKey },
  }).then(r => r.json());

  const pc = new RTCPeerConnection({ iceServers: ice_servers });

  // 2. WebSocket signaling. The api-key is carried via Sec-WebSocket-Protocol
  //    because browsers cannot set custom headers on WebSocket connections.
  const ws = new WebSocket(
    `${API_BASE.replace(/^http/, 'ws')}/v1/ws/signaling`,
    ['share-anything.v1', `api-key.${apiKey}`],
  );

  // 3. Receive flow on the DataChannel:
  //    text JSON {fileName,fileSize,fileType,...} → start of a file
  //    binary chunks                              → file payload
  //    text "__EOF__"                             → end of current file
  let meta = null;
  let parts = [];
  const received = [];
  pc.ondatachannel = ({ channel }) => {
    channel.binaryType = 'arraybuffer';
    channel.onmessage = ({ data }) => {
      if (typeof data === 'string') {
        if (data === '__EOF__' && meta) {
          const blob = new Blob(parts, { type: meta.fileType });
          received.push({ name: meta.fileName, blob });
          // hand off to UI / save-to-disk here
          meta = null;
          parts = [];
        } else {
          try { meta = JSON.parse(data); parts = []; } catch {}
        }
      } else {
        parts.push(data);
      }
    };
  };

  // 4. Trickle our ICE candidates to the uploader.
  pc.onicecandidate = ({ candidate }) => {
    if (candidate && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({
        type: 'ice_candidate',
        share_code: shareCode,
        candidate: candidate.candidate,
        sdp_mid: candidate.sdpMid,
        sdp_m_line_index: candidate.sdpMLineIndex,
        peer_id: '',
      }));
    }
  };

  ws.onopen = () => {
    ws.send(JSON.stringify({
      type: 'downloader_join',
      share_code: shareCode,
      peer_id: crypto.randomUUID(),
      file_name: null,
      ...(password ? { password } : {}),
    }));
  };

  ws.onmessage = async ({ data }) => {
    const m = JSON.parse(data);
    if (m.type === 'offer') {
      await pc.setRemoteDescription({ type: 'offer', sdp: m.sdp });
      const answer = await pc.createAnswer();
      await pc.setLocalDescription(answer);
      ws.send(JSON.stringify({
        type: 'answer', share_code: shareCode, sdp: answer.sdp, peer_id: '',
      }));
    } else if (m.type === 'ice_candidate') {
      await pc.addIceCandidate({
        candidate: m.candidate, sdpMid: m.sdp_mid, sdpMLineIndex: m.sdp_m_line_index,
      });
    } else if (m.type === 'error') {
      console.error('signaling error:', m.message);
      ws.close();
    }
  };
})();
"#;

const NODE_TS_DOWNLOADER: &str = r#"// Share-Anything P2P Downloader — Node.js / TypeScript
// npm i ws @roamhq/wrtc node-fetch
import WebSocket from 'ws';
import wrtc from '@roamhq/wrtc';
import { randomUUID } from 'node:crypto';
import { writeFile } from 'node:fs/promises';

const { RTCPeerConnection } = wrtc;

const API_BASE  = 'https://share-api.mingyu.dev';
const apiKey    = process.env.API_KEY!;
const shareCode = '482917';
const password: string | undefined = undefined;

async function main() {
  const { ice_servers } = await fetch(`${API_BASE}/v1/turn/credentials`, {
    headers: { 'X-API-Key': apiKey },
  }).then(r => r.json());

  const pc = new RTCPeerConnection({ iceServers: ice_servers });

  let meta: { fileName: string; fileType: string } | null = null;
  let parts: Buffer[] = [];

  pc.ondatachannel = ({ channel }) => {
    channel.binaryType = 'arraybuffer';
    channel.onmessage = async ({ data }) => {
      if (typeof data === 'string') {
        if (data === '__EOF__' && meta) {
          await writeFile(meta.fileName, Buffer.concat(parts));
          console.log('saved', meta.fileName);
          meta = null; parts = [];
        } else {
          try { meta = JSON.parse(data); parts = []; } catch {}
        }
      } else {
        parts.push(Buffer.from(data as ArrayBuffer));
      }
    };
  };

  const ws = new WebSocket(
    `${API_BASE.replace(/^http/, 'ws')}/v1/ws/signaling`,
    ['share-anything.v1', `api-key.${apiKey}`],
  );

  pc.onicecandidate = ({ candidate }) => {
    if (candidate && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({
        type: 'ice_candidate',
        share_code: shareCode,
        candidate: candidate.candidate,
        sdp_mid: candidate.sdpMid,
        sdp_m_line_index: candidate.sdpMLineIndex,
        peer_id: '',
      }));
    }
  };

  ws.on('open', () => {
    ws.send(JSON.stringify({
      type: 'downloader_join',
      share_code: shareCode,
      peer_id: randomUUID(),
      ...(password ? { password } : {}),
    }));
  });

  ws.on('message', async raw => {
    const m = JSON.parse(raw.toString());
    if (m.type === 'offer') {
      await pc.setRemoteDescription({ type: 'offer', sdp: m.sdp });
      const answer = await pc.createAnswer();
      await pc.setLocalDescription(answer);
      ws.send(JSON.stringify({ type: 'answer', share_code: shareCode, sdp: answer.sdp, peer_id: '' }));
    } else if (m.type === 'ice_candidate') {
      await pc.addIceCandidate({
        candidate: m.candidate, sdpMid: m.sdp_mid, sdpMLineIndex: m.sdp_m_line_index,
      });
    } else if (m.type === 'error') {
      console.error('signaling error:', m.message);
      ws.close();
    }
  });
}

main().catch(console.error);
"#;

const PYTHON_DOWNLOADER: &str = r#"# Share-Anything P2P Downloader — Python (aiortc)
# pip install aiortc websockets aiohttp
import asyncio, json, os, uuid
import aiohttp, websockets
from aiortc import (
    RTCPeerConnection, RTCSessionDescription, RTCIceCandidate,
    RTCConfiguration, RTCIceServer,
)

API_BASE   = "https://share-api.mingyu.dev"
API_KEY    = os.environ["API_KEY"]
SHARE_CODE = "482917"
PASSWORD   = None  # set for password-protected shares


async def main():
    async with aiohttp.ClientSession() as http:
        async with http.get(f"{API_BASE}/v1/turn/credentials",
                            headers={"X-API-Key": API_KEY}) as r:
            ice = (await r.json())["ice_servers"]

    pc = RTCPeerConnection(RTCConfiguration([
        RTCIceServer(urls=s["urls"], username=s.get("username"),
                     credential=s.get("credential")) for s in ice
    ]))

    state = {"meta": None, "data": bytearray()}

    @pc.on("datachannel")
    def on_dc(channel):
        @channel.on("message")
        def on_msg(msg):
            if isinstance(msg, str):
                if msg == "__EOF__" and state["meta"]:
                    name = state["meta"]["fileName"]
                    with open(name, "wb") as f:
                        f.write(state["data"])
                    print("saved", name)
                    state["meta"] = None
                    state["data"] = bytearray()
                else:
                    try:
                        state["meta"] = json.loads(msg)
                        state["data"] = bytearray()
                    except json.JSONDecodeError:
                        pass
            else:
                state["data"].extend(msg)

    ws = await websockets.connect(
        f"{API_BASE.replace('http', 'ws')}/v1/ws/signaling",
        subprotocols=["share-anything.v1", f"api-key.{API_KEY}"],
    )

    join = {"type": "downloader_join", "share_code": SHARE_CODE,
            "peer_id": str(uuid.uuid4())}
    if PASSWORD: join["password"] = PASSWORD
    await ws.send(json.dumps(join))

    @pc.on("icecandidate")
    async def on_ice(c):
        if c is not None:
            await ws.send(json.dumps({
                "type": "ice_candidate", "share_code": SHARE_CODE,
                "candidate": c.candidate, "sdp_mid": c.sdpMid,
                "sdp_m_line_index": c.sdpMLineIndex, "peer_id": "",
            }))

    async for raw in ws:
        m = json.loads(raw)
        if m["type"] == "offer":
            await pc.setRemoteDescription(RTCSessionDescription(sdp=m["sdp"], type="offer"))
            ans = await pc.createAnswer()
            await pc.setLocalDescription(ans)
            await ws.send(json.dumps({"type": "answer", "share_code": SHARE_CODE,
                                      "sdp": ans.sdp, "peer_id": ""}))
        elif m["type"] == "ice_candidate":
            await pc.addIceCandidate(RTCIceCandidate(
                candidate=m["candidate"], sdpMid=m.get("sdp_mid"),
                sdpMLineIndex=m.get("sdp_m_line_index"),
            ))
        elif m["type"] == "error":
            print("signaling error:", m["message"])
            break

asyncio.run(main())
"#;

const RUST_DOWNLOADER: &str = r#"// Share-Anything P2P Downloader — Rust (webrtc-rs)
// Cargo.toml:
//   webrtc            = "0.11"
//   tokio             = { version = "1", features = ["full"] }
//   tokio-tungstenite = { version = "0.21", features = ["native-tls"] }
//   reqwest           = { version = "0.12", features = ["json"] }
//   serde_json        = "1"
//   uuid              = { version = "1", features = ["v4"] }
//   anyhow            = "1"
//   bytes             = "1"
use std::sync::Arc;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::{Message, client::IntoClientRequest, http::HeaderValue}};
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::{configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription};
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;

const API_BASE: &str   = "https://share-api.mingyu.dev";
const SHARE_CODE: &str = "482917";

#[tokio::main]
async fn main() -> Result<()> {
    let api_key  = std::env::var("API_KEY")?;
    let password: Option<String> = None;

    let ice_json: Value = reqwest::Client::new()
        .get(format!("{API_BASE}/v1/turn/credentials"))
        .header("X-API-Key", &api_key)
        .send().await?.json().await?;
    let ice_servers: Vec<RTCIceServer> = ice_json["ice_servers"].as_array().unwrap()
        .iter().map(|s| RTCIceServer {
            urls: s["urls"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect(),
            username:   s.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            credential: s.get("credential").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        }).collect();

    let api = APIBuilder::new().build();
    let pc = Arc::new(api.new_peer_connection(RTCConfiguration { ice_servers, ..Default::default() }).await?);

    let received: Arc<tokio::sync::Mutex<Vec<u8>>> = Arc::new(Default::default());
    let meta: Arc<tokio::sync::Mutex<Option<Value>>> = Arc::new(Default::default());
    let (rx_meta, rx_data) = (meta.clone(), received.clone());
    pc.on_data_channel(Box::new(move |dc| {
        let (m, d) = (rx_meta.clone(), rx_data.clone());
        Box::pin(async move {
            dc.on_message(Box::new(move |msg: DataChannelMessage| {
                let (m, d) = (m.clone(), d.clone());
                Box::pin(async move {
                    if msg.is_string {
                        let text = String::from_utf8_lossy(&msg.data).into_owned();
                        if text == "__EOF__" {
                            let meta = m.lock().await.take();
                            let bytes = std::mem::take(&mut *d.lock().await);
                            if let Some(meta) = meta {
                                let name = meta["fileName"].as_str().unwrap_or("download");
                                tokio::fs::write(name, &bytes).await.ok();
                                println!("saved {}", name);
                            }
                        } else if let Ok(v) = serde_json::from_str::<Value>(&text) {
                            *m.lock().await = Some(v);
                            d.lock().await.clear();
                        }
                    } else {
                        d.lock().await.extend_from_slice(&msg.data);
                    }
                })
            }));
        })
    }));

    let ws_url = format!("{}/v1/ws/signaling", API_BASE.replacen("http", "ws", 1));
    let mut req = ws_url.into_client_request()?;
    req.headers_mut().insert("Sec-WebSocket-Protocol",
        HeaderValue::from_str(&format!("share-anything.v1, api-key.{api_key}"))?);
    let (ws, _) = connect_async(req).await?;
    let (mut tx, mut rx) = ws.split();

    let mut join = json!({"type":"downloader_join","share_code":SHARE_CODE,
                          "peer_id":uuid::Uuid::new_v4().to_string()});
    if let Some(p) = &password { join["password"] = json!(p); }
    tx.send(Message::Text(join.to_string())).await?;

    while let Some(Ok(Message::Text(raw))) = rx.next().await {
        let m: Value = serde_json::from_str(&raw)?;
        match m["type"].as_str() {
            Some("offer") => {
                pc.set_remote_description(RTCSessionDescription::offer(m["sdp"].as_str().unwrap().into())?).await?;
                let ans = pc.create_answer(None).await?;
                pc.set_local_description(ans.clone()).await?;
                tx.send(Message::Text(json!({
                    "type":"answer","share_code":SHARE_CODE,"sdp":ans.sdp,"peer_id":""
                }).to_string())).await?;
            }
            Some("ice_candidate") => {
                pc.add_ice_candidate(RTCIceCandidateInit {
                    candidate: m["candidate"].as_str().unwrap_or("").to_string(),
                    sdp_mid: m["sdp_mid"].as_str().map(|s| s.to_string()),
                    sdp_mline_index: m["sdp_m_line_index"].as_u64().map(|v| v as u16),
                    ..Default::default()
                }).await?;
            }
            Some("error") => { eprintln!("signaling: {}", m["message"]); break; }
            _ => {}
        }
    }
    Ok(())
}
"#;

const GO_DOWNLOADER: &str = r#"// Share-Anything P2P Downloader — Go (pion/webrtc)
// go get github.com/pion/webrtc/v3 github.com/gorilla/websocket
package main

import (
    "encoding/json"
    "fmt"
    "net/http"
    "net/url"
    "os"

    "github.com/google/uuid"
    "github.com/gorilla/websocket"
    "github.com/pion/webrtc/v3"
)

const (
    apiBase   = "https://share-api.mingyu.dev"
    shareCode = "482917"
)

type iceResp struct {
    IceServers []struct {
        URLs       []string `json:"urls"`
        Username   string   `json:"username,omitempty"`
        Credential string   `json:"credential,omitempty"`
    } `json:"ice_servers"`
}

func main() {
    apiKey := os.Getenv("API_KEY")
    password := "" // set for password-protected shares

    // 1. TURN credentials
    req, _ := http.NewRequest("GET", apiBase+"/v1/turn/credentials", nil)
    req.Header.Set("X-API-Key", apiKey)
    res, err := http.DefaultClient.Do(req)
    must(err)
    defer res.Body.Close()
    var ir iceResp
    must(json.NewDecoder(res.Body).Decode(&ir))

    var ice []webrtc.ICEServer
    for _, s := range ir.IceServers {
        ice = append(ice, webrtc.ICEServer{URLs: s.URLs, Username: s.Username, Credential: s.Credential})
    }

    pc, err := webrtc.NewPeerConnection(webrtc.Configuration{ICEServers: ice})
    must(err)

    var meta map[string]any
    var buf []byte
    pc.OnDataChannel(func(dc *webrtc.DataChannel) {
        dc.OnMessage(func(msg webrtc.DataChannelMessage) {
            if msg.IsString {
                txt := string(msg.Data)
                if txt == "__EOF__" && meta != nil {
                    name, _ := meta["fileName"].(string)
                    os.WriteFile(name, buf, 0o644)
                    fmt.Println("saved", name)
                    meta = nil; buf = nil
                } else if err := json.Unmarshal(msg.Data, &meta); err == nil {
                    buf = buf[:0]
                }
            } else {
                buf = append(buf, msg.Data...)
            }
        })
    })

    // 2. WebSocket signaling
    u, _ := url.Parse(apiBase + "/v1/ws/signaling")
    u.Scheme = "wss"
    hdr := http.Header{}
    hdr.Set("Sec-WebSocket-Protocol", "share-anything.v1, api-key."+apiKey)
    ws, _, err := websocket.DefaultDialer.Dial(u.String(), hdr)
    must(err)
    defer ws.Close()

    pc.OnICECandidate(func(c *webrtc.ICECandidate) {
        if c == nil { return }
        j := c.ToJSON()
        ws.WriteJSON(map[string]any{
            "type":             "ice_candidate",
            "share_code":       shareCode,
            "candidate":        j.Candidate,
            "sdp_mid":          j.SDPMid,
            "sdp_m_line_index": j.SDPMLineIndex,
            "peer_id":          "",
        })
    })

    join := map[string]any{"type": "downloader_join", "share_code": shareCode, "peer_id": uuid.NewString()}
    if password != "" { join["password"] = password }
    ws.WriteJSON(join)

    for {
        var m map[string]any
        if err := ws.ReadJSON(&m); err != nil { return }
        switch m["type"] {
        case "offer":
            must(pc.SetRemoteDescription(webrtc.SessionDescription{Type: webrtc.SDPTypeOffer, SDP: m["sdp"].(string)}))
            ans, err := pc.CreateAnswer(nil); must(err)
            must(pc.SetLocalDescription(ans))
            ws.WriteJSON(map[string]any{"type": "answer", "share_code": shareCode, "sdp": ans.SDP, "peer_id": ""})
        case "ice_candidate":
            mid, _ := m["sdp_mid"].(string)
            idx, _ := m["sdp_m_line_index"].(float64)
            idxU16 := uint16(idx)
            pc.AddICECandidate(webrtc.ICECandidateInit{
                Candidate: m["candidate"].(string), SDPMid: &mid, SDPMLineIndex: &idxU16,
            })
        case "error":
            fmt.Println("signaling error:", m["message"])
            return
        }
    }
}

func must(err error) { if err != nil { panic(err) } }
"#;

const KOTLIN_DOWNLOADER: &str = r#"// Share-Anything P2P Downloader — Kotlin / Spring Boot
// build.gradle.kts:
//   implementation("org.springframework.boot:spring-boot-starter-webflux")
//   implementation("dev.onvoid.webrtc:webrtc-java:0.10.0")
//   implementation("com.fasterxml.jackson.module:jackson-module-kotlin")
package com.example.shareanything

import com.fasterxml.jackson.databind.ObjectMapper
import dev.onvoid.webrtc.*
import dev.onvoid.webrtc.media.*
import org.springframework.stereotype.Service
import org.springframework.web.reactive.socket.WebSocketHandler
import org.springframework.web.reactive.socket.WebSocketMessage
import org.springframework.web.reactive.socket.client.ReactorNettyWebSocketClient
import reactor.core.publisher.Mono
import java.net.URI
import java.nio.ByteBuffer
import java.nio.file.Files
import java.nio.file.Path
import java.util.*

@Service
class P2PDownloader(private val mapper: ObjectMapper) {
    private val apiBase   = "https://share-api.mingyu.dev"
    private val apiKey    = System.getenv("API_KEY")
    private val shareCode = "482917"
    private val password: String? = null

    fun start() {
        // 1. Pull TURN credentials over plain WebClient (omitted for brevity) and build:
        val ice = listOf(
            RTCIceServer().apply {
                urls = listOf("stun:stun.cloudflare.com:3478")
            }
        )

        val factory = PeerConnectionFactory()
        val config = RTCConfiguration().apply { iceServers = ice }

        var meta: Map<String, Any>? = null
        val buffer = mutableListOf<ByteArray>()

        val observer = object : PeerConnectionObserver {
            override fun onIceCandidate(c: RTCIceCandidate) { wsSend(mapOf(
                "type" to "ice_candidate", "share_code" to shareCode,
                "candidate" to c.sdp, "sdp_mid" to c.sdpMid,
                "sdp_m_line_index" to c.sdpMLineIndex, "peer_id" to ""
            )) }
            override fun onDataChannel(dc: RTCDataChannel) {
                dc.registerObserver(object : RTCDataChannelObserver {
                    override fun onMessage(buf: RTCDataChannelBuffer) {
                        val bytes = ByteArray(buf.data.remaining())
                        buf.data.get(bytes)
                        if (buf.binary) { buffer.add(bytes); return }
                        val text = String(bytes)
                        if (text == "__EOF__" && meta != null) {
                            val name = meta!!["fileName"] as String
                            Files.write(Path.of(name), buffer.reduce(ByteArray::plus))
                            println("saved $name"); meta = null; buffer.clear()
                        } else {
                            runCatching {
                                meta = mapper.readValue(text, Map::class.java) as Map<String, Any>
                                buffer.clear()
                            }
                        }
                    }
                    override fun onStateChange() {}
                    override fun onBufferedAmountChange(prev: Long) {}
                })
            }
            // ... other observer methods omitted
        }

        val pc = factory.createPeerConnection(config, observer)

        val wsUri = URI.create("${apiBase.replace("http", "ws")}/v1/ws/signaling")
        ReactorNettyWebSocketClient { it.addHeader("Sec-WebSocket-Protocol", "share-anything.v1, api-key.$apiKey") }
            .execute(wsUri, WebSocketHandler { session ->
                // send downloader_join
                val join = mutableMapOf<String, Any>(
                    "type" to "downloader_join",
                    "share_code" to shareCode,
                    "peer_id" to UUID.randomUUID().toString()
                )
                password?.let { join["password"] = it }
                session.send(Mono.just(session.textMessage(mapper.writeValueAsString(join))))
                    .thenMany(session.receive().flatMap { msg: WebSocketMessage ->
                        val m = mapper.readValue(msg.payloadAsText, Map::class.java) as Map<String, Any>
                        when (m["type"]) {
                            "offer" -> {
                                pc.setRemoteDescription(RTCSessionDescription(RTCSdpType.OFFER, m["sdp"] as String), null)
                                val ans = pc.createAnswer(RTCAnswerOptions())
                                pc.setLocalDescription(ans, null)
                                session.send(Mono.just(session.textMessage(mapper.writeValueAsString(mapOf(
                                    "type" to "answer", "share_code" to shareCode,
                                    "sdp" to ans.sdp, "peer_id" to ""
                                )))))
                            }
                            "ice_candidate" -> {
                                pc.addIceCandidate(RTCIceCandidate(
                                    m["sdp_mid"] as? String ?: "",
                                    (m["sdp_m_line_index"] as? Int) ?: 0,
                                    m["candidate"] as String
                                ))
                                Mono.empty()
                            }
                            else -> Mono.empty()
                        }
                    }).then()
            }).block()
    }

    private fun wsSend(payload: Map<String, Any>) {
        // Maintain a reference to the active session and write JSON here.
    }
}
"#;

const JAVA_DOWNLOADER: &str = r#"// Share-Anything P2P Downloader — Java / Spring Boot
// pom.xml:
//   org.springframework.boot:spring-boot-starter-webflux
//   dev.onvoid.webrtc:webrtc-java:0.10.0
//   com.fasterxml.jackson.core:jackson-databind
package com.example.shareanything;

import com.fasterxml.jackson.databind.ObjectMapper;
import dev.onvoid.webrtc.*;
import org.springframework.stereotype.Service;
import org.springframework.web.reactive.socket.WebSocketHandler;
import org.springframework.web.reactive.socket.client.ReactorNettyWebSocketClient;
import reactor.core.publisher.Mono;

import java.net.URI;
import java.nio.file.*;
import java.util.*;

@Service
public class P2PDownloader {
    private static final String API_BASE   = "https://share-anything.mingyu.dev";
    private static final String SHARE_CODE = "482917";

    private final ObjectMapper mapper = new ObjectMapper();
    private final String apiKey   = System.getenv("API_KEY");
    private final String password = null;

    public void start() throws Exception {
        // 1. Fetch TURN credentials (omitted here — use WebClient with X-API-Key).
        List<RTCIceServer> ice = List.of(/* populated from /v1/turn/credentials */);

        PeerConnectionFactory factory = new PeerConnectionFactory();
        RTCConfiguration config = new RTCConfiguration();
        config.iceServers = ice;

        Map<String, Object>[] meta = new Map[]{null};
        List<byte[]> buffer = new ArrayList<>();

        PeerConnectionObserver observer = new PeerConnectionObserver() {
            @Override
            public void onIceCandidate(RTCIceCandidate c) {
                wsSend(Map.of(
                    "type", "ice_candidate", "share_code", SHARE_CODE,
                    "candidate", c.sdp, "sdp_mid", c.sdpMid,
                    "sdp_m_line_index", c.sdpMLineIndex, "peer_id", ""
                ));
            }
            @Override
            public void onDataChannel(RTCDataChannel dc) {
                dc.registerObserver(new RTCDataChannelObserver() {
                    @Override public void onStateChange() {}
                    @Override public void onBufferedAmountChange(long previousAmount) {}
                    @Override
                    public void onMessage(RTCDataChannelBuffer buf) {
                        byte[] bytes = new byte[buf.data.remaining()];
                        buf.data.get(bytes);
                        if (buf.binary) { buffer.add(bytes); return; }
                        String text = new String(bytes);
                        try {
                            if (text.equals("__EOF__") && meta[0] != null) {
                                String name = (String) meta[0].get("fileName");
                                byte[] full = concat(buffer);
                                Files.write(Path.of(name), full);
                                System.out.println("saved " + name);
                                meta[0] = null; buffer.clear();
                            } else {
                                meta[0] = mapper.readValue(text, Map.class);
                                buffer.clear();
                            }
                        } catch (Exception ignore) {}
                    }
                });
            }
            // … other observer methods omitted
        };

        RTCPeerConnection pc = factory.createPeerConnection(config, observer);

        URI uri = URI.create(API_BASE.replace("http", "ws") + "/v1/ws/signaling");
        new ReactorNettyWebSocketClient(httpClient -> httpClient
                .headers(h -> h.add("Sec-WebSocket-Protocol", "share-anything.v1, api-key." + apiKey)))
            .execute(uri, (WebSocketHandler) session -> {
                Map<String, Object> join = new HashMap<>();
                join.put("type", "downloader_join");
                join.put("share_code", SHARE_CODE);
                join.put("peer_id", UUID.randomUUID().toString());
                if (password != null) join.put("password", password);
                return session.send(Mono.just(session.textMessage(toJson(join))))
                    .thenMany(session.receive().flatMap(msg -> {
                        try {
                            Map<String, Object> m = mapper.readValue(msg.getPayloadAsText(), Map.class);
                            switch ((String) m.get("type")) {
                                case "offer":
                                    pc.setRemoteDescription(
                                        new RTCSessionDescription(RTCSdpType.OFFER, (String) m.get("sdp")), null);
                                    RTCSessionDescription ans = pc.createAnswer(new RTCAnswerOptions());
                                    pc.setLocalDescription(ans, null);
                                    return session.send(Mono.just(session.textMessage(toJson(Map.of(
                                        "type", "answer", "share_code", SHARE_CODE,
                                        "sdp", ans.sdp, "peer_id", "")))));
                                case "ice_candidate":
                                    pc.addIceCandidate(new RTCIceCandidate(
                                        (String) m.getOrDefault("sdp_mid", ""),
                                        ((Number) m.getOrDefault("sdp_m_line_index", 0)).intValue(),
                                        (String) m.get("candidate")));
                                    return Mono.empty();
                                default:
                                    return Mono.empty();
                            }
                        } catch (Exception e) { return Mono.empty(); }
                    })).then();
            }).block();
    }

    private String toJson(Object o) {
        try { return mapper.writeValueAsString(o); } catch (Exception e) { throw new RuntimeException(e); }
    }
    private static byte[] concat(List<byte[]> parts) {
        int n = parts.stream().mapToInt(b -> b.length).sum();
        byte[] out = new byte[n]; int off = 0;
        for (byte[] p : parts) { System.arraycopy(p, 0, out, off, p.length); off += p.length; }
        return out;
    }
    private void wsSend(Map<String, Object> payload) {
        // Keep a reference to the active WebSocketSession and write JSON here.
    }
}
"#;

const PHP_DOWNLOADER: &str = r#"<?php
// Share-Anything P2P — PHP
//
// ⚠️ IMPORTANT: PHP does not have a production-grade native WebRTC peer
// implementation (no equivalent of pion/webrtc, aiortc, or webrtc-rs).
// This example demonstrates ONLY the signaling WebSocket layer — opening
// the connection with the api-key subprotocol and exchanging messages.
//
// For an actual peer-to-peer transfer from PHP, the recommended approaches
// are:
//   1. Spawn a Node.js subprocess (`@roamhq/wrtc` example above) and
//      have PHP drive it over stdio.
//   2. Run a small headless browser (e.g. Puppeteer) for the WebRTC bits
//      and have PHP send the share_code + api-key.
//   3. Offload the user to a browser flow entirely.
//
// composer require ratchet/pawl react/event-loop
require __DIR__ . '/vendor/autoload.php';

use Ratchet\Client\Connector;
use Ratchet\Client\WebSocket;
use Ratchet\RFC6455\Messaging\Frame;

$apiKey    = getenv('API_KEY');
$shareCode = '482917';
$password  = null;

$loop = React\EventLoop\Loop::get();
$connector = new Connector($loop);

// `Sec-WebSocket-Protocol` is what the server uses to authenticate the WS.
$headers = [
    'Sec-WebSocket-Protocol' => "share-anything.v1, api-key.$apiKey",
];

$connector("wss://share-api.mingyu.dev/v1/ws/signaling", [], $headers)
    ->then(function (WebSocket $ws) use ($shareCode, $password) {
        $join = [
            'type'       => 'downloader_join',
            'share_code' => $shareCode,
            'peer_id'    => bin2hex(random_bytes(16)),
        ];
        if ($password !== null) {
            $join['password'] = $password;
        }
        $ws->send(json_encode($join));

        $ws->on('message', function ($msg) use ($ws) {
            $m = json_decode($msg, true);
            switch ($m['type'] ?? '') {
                case 'peer_matched':
                    // From here, an offer arrives next. Your PHP code would need
                    // to hand the SDP/ICE exchange to a real WebRTC peer (see
                    // the note at the top of this file).
                    break;
                case 'offer':
                    // Forward $m['sdp'] to your WebRTC implementation.
                    break;
                case 'error':
                    echo "signaling error: " . $m['message'] . PHP_EOL;
                    $ws->close();
                    break;
            }
        });

        $ws->on('close', function ($code, $reason) {
            echo "ws closed: $code $reason" . PHP_EOL;
        });
    }, function (\Exception $e) {
        echo "could not connect: " . $e->getMessage() . PHP_EOL;
    });

$loop->run();
"#;

// =============================================================================
// UPLOADER samples
// =============================================================================

const JS_UPLOADER: &str = r#"// Share-Anything P2P Uploader — Browser JavaScript
// Creates a P2P share and streams one or more File/Blob objects to the
// downloader over a WebRTC DataChannel.

const API_BASE = 'https://share-api.mingyu.dev';
const apiKey   = 'sak_xxxxxxxxxxxx';
const password = undefined;                  // set to require a password
const files    = /* HTMLInputElement.files */ [];

(async () => {
  const auth = { 'X-API-Key': apiKey };

  // 1. Create the P2P session (returns share_code; reserved for 24h).
  const session = await fetch(`${API_BASE}/v1/p2p/sessions`, {
    method: 'POST',
    headers: { ...auth, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      files: [...files].map(f => ({ name: f.name, size: f.size, type: f.type || 'application/octet-stream' })),
      ...(password ? { password } : {}),
    }),
  }).then(r => r.json());
  const shareCode = session.share_code;
  console.log('share with this code:', shareCode);

  // 2. Fresh TURN/STUN credentials.
  const { ice_servers } = await fetch(`${API_BASE}/v1/turn/credentials`, { headers: auth }).then(r => r.json());

  const pc = new RTCPeerConnection({ iceServers: ice_servers });
  const dc = pc.createDataChannel('share', { ordered: true });
  dc.binaryType = 'arraybuffer';

  const ws = new WebSocket(
    `${API_BASE.replace(/^http/, 'ws')}/v1/ws/signaling`,
    ['share-anything.v1', `api-key.${apiKey}`],
  );

  pc.onicecandidate = ({ candidate }) => {
    if (candidate && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({
        type: 'ice_candidate', share_code: shareCode,
        candidate: candidate.candidate, sdp_mid: candidate.sdpMid,
        sdp_m_line_index: candidate.sdpMLineIndex, peer_id: '',
      }));
    }
  };

  ws.onopen = () => {
    ws.send(JSON.stringify({
      type: 'uploader_ready',
      share_code: shareCode,
      peer_id: crypto.randomUUID(),
      device_info: navigator.userAgent,
    }));
  };

  ws.onmessage = async ({ data }) => {
    const m = JSON.parse(data);
    if (m.type === 'peer_matched') {
      // A downloader is here — make the offer.
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      ws.send(JSON.stringify({ type: 'offer', share_code: shareCode, sdp: offer.sdp, peer_id: '' }));
    } else if (m.type === 'answer') {
      await pc.setRemoteDescription({ type: 'answer', sdp: m.sdp });
    } else if (m.type === 'ice_candidate') {
      await pc.addIceCandidate({
        candidate: m.candidate, sdpMid: m.sdp_mid, sdpMLineIndex: m.sdp_m_line_index,
      });
    } else if (m.type === 'error') {
      console.error('signaling error:', m.message);
      ws.close();
    }
  };

  // 3. Send files over the DataChannel: metadata JSON → binary chunks → "__EOF__".
  dc.onopen = async () => {
    const CHUNK = 64 * 1024;
    const HIGH  = 1 * 1024 * 1024;
    for (const file of files) {
      dc.send(JSON.stringify({
        type: 'file_metadata',
        fileName: file.name,
        fileSize: file.size,
        fileType: file.type || 'application/octet-stream',
      }));
      const buf = new Uint8Array(await file.arrayBuffer());
      for (let off = 0; off < buf.length; off += CHUNK) {
        while (dc.bufferedAmount > HIGH) await new Promise(r => setTimeout(r, 10));
        dc.send(buf.subarray(off, Math.min(off + CHUNK, buf.length)));
      }
      dc.send('__EOF__');
    }
    ws.send(JSON.stringify({ type: 'transfer_complete', share_code: shareCode }));
  };
})();
"#;

const NODE_TS_UPLOADER: &str = r#"// Share-Anything P2P Uploader — Node.js / TypeScript
// npm i ws @roamhq/wrtc node-fetch
import WebSocket from 'ws';
import wrtc from '@roamhq/wrtc';
import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { basename } from 'node:path';

const { RTCPeerConnection } = wrtc;

const API_BASE = 'https://share-api.mingyu.dev';
const apiKey   = process.env.API_KEY!;
const password: string | undefined = undefined;
const paths    = ['./report.pdf'];

async function main() {
  const auth = { 'X-API-Key': apiKey };
  const filesMeta = await Promise.all(paths.map(async p => {
    const data = await readFile(p);
    return { name: basename(p), size: data.length, type: 'application/octet-stream', data };
  }));

  const session = await fetch(`${API_BASE}/v1/p2p/sessions`, {
    method: 'POST',
    headers: { ...auth, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      files: filesMeta.map(({ name, size, type }) => ({ name, size, type })),
      ...(password ? { password } : {}),
    }),
  }).then(r => r.json());
  const shareCode = session.share_code;
  console.log('share_code:', shareCode);

  const { ice_servers } = await fetch(`${API_BASE}/v1/turn/credentials`, { headers: auth }).then(r => r.json());
  const pc = new RTCPeerConnection({ iceServers: ice_servers });
  const dc = pc.createDataChannel('share', { ordered: true });

  const ws = new WebSocket(
    `${API_BASE.replace(/^http/, 'ws')}/v1/ws/signaling`,
    ['share-anything.v1', `api-key.${apiKey}`],
  );

  pc.onicecandidate = ({ candidate }) => {
    if (candidate && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({
        type: 'ice_candidate', share_code: shareCode,
        candidate: candidate.candidate, sdp_mid: candidate.sdpMid,
        sdp_m_line_index: candidate.sdpMLineIndex, peer_id: '',
      }));
    }
  };

  ws.on('open', () => {
    ws.send(JSON.stringify({
      type: 'uploader_ready', share_code: shareCode, peer_id: randomUUID(),
      device_info: 'node-uploader',
    }));
  });

  ws.on('message', async raw => {
    const m = JSON.parse(raw.toString());
    if (m.type === 'peer_matched') {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      ws.send(JSON.stringify({ type: 'offer', share_code: shareCode, sdp: offer.sdp, peer_id: '' }));
    } else if (m.type === 'answer') {
      await pc.setRemoteDescription({ type: 'answer', sdp: m.sdp });
    } else if (m.type === 'ice_candidate') {
      await pc.addIceCandidate({
        candidate: m.candidate, sdpMid: m.sdp_mid, sdpMLineIndex: m.sdp_m_line_index,
      });
    }
  });

  dc.onopen = async () => {
    const CHUNK = 64 * 1024;
    const HIGH  = 1 * 1024 * 1024;
    for (const f of filesMeta) {
      dc.send(JSON.stringify({
        type: 'file_metadata', fileName: f.name, fileSize: f.size, fileType: f.type,
      }));
      for (let off = 0; off < f.data.length; off += CHUNK) {
        while (dc.bufferedAmount > HIGH) await new Promise(r => setTimeout(r, 10));
        dc.send(f.data.subarray(off, Math.min(off + CHUNK, f.data.length)));
      }
      dc.send('__EOF__');
    }
    ws.send(JSON.stringify({ type: 'transfer_complete', share_code: shareCode }));
  };
}

main().catch(console.error);
"#;

const PYTHON_UPLOADER: &str = r#"# Share-Anything P2P Uploader — Python (aiortc)
# pip install aiortc websockets aiohttp
import asyncio, json, os, uuid
from pathlib import Path
import aiohttp, websockets
from aiortc import (
    RTCPeerConnection, RTCSessionDescription, RTCIceCandidate,
    RTCConfiguration, RTCIceServer,
)

API_BASE = "https://share-api.mingyu.dev"
API_KEY  = os.environ["API_KEY"]
PASSWORD = None
PATHS    = ["./report.pdf"]
CHUNK    = 64 * 1024
HIGH     = 1024 * 1024


async def main():
    auth = {"X-API-Key": API_KEY}
    files = [{
        "name": Path(p).name,
        "size": Path(p).stat().st_size,
        "type": "application/octet-stream",
        "data": Path(p).read_bytes(),
    } for p in PATHS]

    async with aiohttp.ClientSession() as http:
        body = {"files": [{"name": f["name"], "size": f["size"], "type": f["type"]} for f in files]}
        if PASSWORD: body["password"] = PASSWORD
        async with http.post(f"{API_BASE}/v1/p2p/sessions", json=body, headers=auth) as r:
            session = await r.json()
        async with http.get(f"{API_BASE}/v1/turn/credentials", headers=auth) as r:
            ice = (await r.json())["ice_servers"]

    share_code = session["share_code"]
    print("share_code:", share_code)

    pc = RTCPeerConnection(RTCConfiguration([
        RTCIceServer(urls=s["urls"], username=s.get("username"),
                     credential=s.get("credential")) for s in ice
    ]))
    dc = pc.createDataChannel("share", ordered=True)

    ws = await websockets.connect(
        f"{API_BASE.replace('http', 'ws')}/v1/ws/signaling",
        subprotocols=["share-anything.v1", f"api-key.{API_KEY}"],
    )
    await ws.send(json.dumps({
        "type": "uploader_ready", "share_code": share_code,
        "peer_id": str(uuid.uuid4()), "device_info": "python-uploader",
    }))

    @pc.on("icecandidate")
    async def on_ice(c):
        if c is not None:
            await ws.send(json.dumps({
                "type": "ice_candidate", "share_code": share_code,
                "candidate": c.candidate, "sdp_mid": c.sdpMid,
                "sdp_m_line_index": c.sdpMLineIndex, "peer_id": "",
            }))

    async def signaling_loop():
        async for raw in ws:
            m = json.loads(raw)
            if m["type"] == "peer_matched":
                offer = await pc.createOffer()
                await pc.setLocalDescription(offer)
                await ws.send(json.dumps({"type": "offer", "share_code": share_code,
                                          "sdp": offer.sdp, "peer_id": ""}))
            elif m["type"] == "answer":
                await pc.setRemoteDescription(RTCSessionDescription(sdp=m["sdp"], type="answer"))
            elif m["type"] == "ice_candidate":
                await pc.addIceCandidate(RTCIceCandidate(
                    candidate=m["candidate"], sdpMid=m.get("sdp_mid"),
                    sdpMLineIndex=m.get("sdp_m_line_index"),
                ))
            elif m["type"] == "error":
                print("signaling error:", m["message"]); break

    async def send_files():
        while dc.readyState != "open":
            await asyncio.sleep(0.05)
        for f in files:
            dc.send(json.dumps({"type": "file_metadata", "fileName": f["name"],
                                "fileSize": f["size"], "fileType": f["type"]}))
            data = f["data"]
            for off in range(0, len(data), CHUNK):
                while dc.bufferedAmount > HIGH:
                    await asyncio.sleep(0.01)
                dc.send(data[off:off + CHUNK])
            dc.send("__EOF__")
        await ws.send(json.dumps({"type": "transfer_complete", "share_code": share_code}))

    await asyncio.gather(signaling_loop(), send_files())

asyncio.run(main())
"#;

const RUST_UPLOADER: &str = r#"// Share-Anything P2P Uploader — Rust (webrtc-rs)
// Cargo.toml: see the Downloader sample for crate versions.
use std::sync::Arc;
use anyhow::Result;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::{Message, client::IntoClientRequest, http::HeaderValue}};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::{ice_candidate::RTCIceCandidateInit, ice_server::RTCIceServer};
use webrtc::peer_connection::{configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription};

const API_BASE: &str = "https://share-api.mingyu.dev";
const CHUNK: usize   = 64 * 1024;
const HIGH: usize    = 1024 * 1024;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("API_KEY")?;
    let path    = "./report.pdf";
    let data    = tokio::fs::read(path).await?;
    let name    = std::path::Path::new(path).file_name().unwrap().to_string_lossy().to_string();

    let http = reqwest::Client::new();
    let session: Value = http.post(format!("{API_BASE}/v1/p2p/sessions"))
        .header("X-API-Key", &api_key)
        .json(&json!({ "files": [{ "name": name, "size": data.len(), "type": "application/octet-stream" }] }))
        .send().await?.json().await?;
    let share_code = session["share_code"].as_str().unwrap().to_string();
    println!("share_code: {share_code}");

    let ice_json: Value = http.get(format!("{API_BASE}/v1/turn/credentials"))
        .header("X-API-Key", &api_key).send().await?.json().await?;
    let ice_servers: Vec<RTCIceServer> = ice_json["ice_servers"].as_array().unwrap()
        .iter().map(|s| RTCIceServer {
            urls: s["urls"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect(),
            username:   s.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            credential: s.get("credential").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        }).collect();

    let api = APIBuilder::new().build();
    let pc = Arc::new(api.new_peer_connection(RTCConfiguration { ice_servers, ..Default::default() }).await?);
    let dc = pc.create_data_channel("share", Some(RTCDataChannelInit { ordered: Some(true), ..Default::default() })).await?;

    let ws_url = format!("{}/v1/ws/signaling", API_BASE.replacen("http", "ws", 1));
    let mut req = ws_url.into_client_request()?;
    req.headers_mut().insert("Sec-WebSocket-Protocol",
        HeaderValue::from_str(&format!("share-anything.v1, api-key.{api_key}"))?);
    let (ws, _) = connect_async(req).await?;
    let (tx, mut rx) = ws.split();
    let tx = Arc::new(tokio::sync::Mutex::new(tx));

    tx.lock().await.send(Message::Text(json!({
        "type": "uploader_ready", "share_code": share_code,
        "peer_id": uuid::Uuid::new_v4().to_string(), "device_info": "rust-uploader",
    }).to_string())).await?;

    {
        let tx = tx.clone();
        let share_code = share_code.clone();
        pc.on_ice_candidate(Box::new(move |c| {
            let tx = tx.clone();
            let share_code = share_code.clone();
            Box::pin(async move {
                if let Some(c) = c {
                    if let Ok(init) = c.to_json() {
                        let _ = tx.lock().await.send(Message::Text(json!({
                            "type": "ice_candidate", "share_code": share_code,
                            "candidate": init.candidate, "sdp_mid": init.sdp_mid,
                            "sdp_m_line_index": init.sdp_mline_index, "peer_id": "",
                        }).to_string())).await;
                    }
                }
            })
        }));
    }

    let dc_for_send = dc.clone();
    let send_done = Arc::new(tokio::sync::Notify::new());
    let send_done_clone = send_done.clone();
    let data_clone = data.clone();
    let name_clone = name.clone();
    dc.on_open(Box::new(move || {
        let dc = dc_for_send.clone();
        let done = send_done_clone.clone();
        let data = data_clone.clone();
        let name = name_clone.clone();
        Box::pin(async move {
            dc.send_text(json!({
                "type": "file_metadata", "fileName": name,
                "fileSize": data.len(), "fileType": "application/octet-stream",
            }).to_string()).await.ok();
            let mut off = 0;
            while off < data.len() {
                while dc.buffered_amount().await > HIGH as u64 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                let end = (off + CHUNK).min(data.len());
                dc.send(&Bytes::copy_from_slice(&data[off..end])).await.ok();
                off = end;
            }
            dc.send_text("__EOF__".to_string()).await.ok();
            done.notify_one();
        })
    }));

    let pc_clone = pc.clone();
    let tx_clone = tx.clone();
    let share_code_loop = share_code.clone();
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(raw))) = rx.next().await {
            let m: Value = match serde_json::from_str(&raw) { Ok(v) => v, _ => continue };
            match m["type"].as_str() {
                Some("peer_matched") => {
                    let offer = pc_clone.create_offer(None).await.unwrap();
                    pc_clone.set_local_description(offer.clone()).await.unwrap();
                    tx_clone.lock().await.send(Message::Text(json!({
                        "type": "offer", "share_code": share_code_loop, "sdp": offer.sdp, "peer_id": "",
                    }).to_string())).await.ok();
                }
                Some("answer") => {
                    let ans = RTCSessionDescription::answer(m["sdp"].as_str().unwrap().into()).unwrap();
                    pc_clone.set_remote_description(ans).await.ok();
                }
                Some("ice_candidate") => {
                    pc_clone.add_ice_candidate(RTCIceCandidateInit {
                        candidate: m["candidate"].as_str().unwrap_or("").to_string(),
                        sdp_mid: m["sdp_mid"].as_str().map(|s| s.to_string()),
                        sdp_mline_index: m["sdp_m_line_index"].as_u64().map(|v| v as u16),
                        ..Default::default()
                    }).await.ok();
                }
                _ => {}
            }
        }
    });

    send_done.notified().await;
    tx.lock().await.send(Message::Text(json!({
        "type": "transfer_complete", "share_code": share_code
    }).to_string())).await?;
    Ok(())
}
"#;

const GO_UPLOADER: &str = r#"// Share-Anything P2P Uploader — Go (pion/webrtc)
// go get github.com/pion/webrtc/v3 github.com/gorilla/websocket
package main

import (
    "bytes"
    "encoding/json"
    "fmt"
    "net/http"
    "net/url"
    "os"
    "path/filepath"
    "time"

    "github.com/google/uuid"
    "github.com/gorilla/websocket"
    "github.com/pion/webrtc/v3"
)

const (
    apiBase = "https://share-api.mingyu.dev"
    chunk   = 64 * 1024
    high    = 1024 * 1024
)

func main() {
    apiKey := os.Getenv("API_KEY")
    path   := "./report.pdf"
    data, err := os.ReadFile(path); must(err)
    name := filepath.Base(path)

    // 1. Create session
    body, _ := json.Marshal(map[string]any{
        "files": []map[string]any{{"name": name, "size": len(data), "type": "application/octet-stream"}},
    })
    req, _ := http.NewRequest("POST", apiBase+"/v1/p2p/sessions", bytes.NewReader(body))
    req.Header.Set("X-API-Key", apiKey)
    req.Header.Set("Content-Type", "application/json")
    res, err := http.DefaultClient.Do(req); must(err)
    defer res.Body.Close()
    var session struct{ ShareCode string `json:"share_code"` }
    must(json.NewDecoder(res.Body).Decode(&session))
    fmt.Println("share_code:", session.ShareCode)

    // 2. ICE servers
    req2, _ := http.NewRequest("GET", apiBase+"/v1/turn/credentials", nil)
    req2.Header.Set("X-API-Key", apiKey)
    res2, err := http.DefaultClient.Do(req2); must(err)
    defer res2.Body.Close()
    var ir struct{ IceServers []struct{ URLs []string `json:"urls"`; Username, Credential string } `json:"ice_servers"` }
    must(json.NewDecoder(res2.Body).Decode(&ir))
    var ice []webrtc.ICEServer
    for _, s := range ir.IceServers {
        ice = append(ice, webrtc.ICEServer{URLs: s.URLs, Username: s.Username, Credential: s.Credential})
    }

    pc, err := webrtc.NewPeerConnection(webrtc.Configuration{ICEServers: ice}); must(err)
    dc, err := pc.CreateDataChannel("share", nil); must(err)

    // 3. WS
    u, _ := url.Parse(apiBase + "/v1/ws/signaling")
    u.Scheme = "wss"
    hdr := http.Header{}
    hdr.Set("Sec-WebSocket-Protocol", "share-anything.v1, api-key."+apiKey)
    ws, _, err := websocket.DefaultDialer.Dial(u.String(), hdr); must(err)
    defer ws.Close()

    pc.OnICECandidate(func(c *webrtc.ICECandidate) {
        if c == nil { return }
        j := c.ToJSON()
        ws.WriteJSON(map[string]any{
            "type": "ice_candidate", "share_code": session.ShareCode,
            "candidate": j.Candidate, "sdp_mid": j.SDPMid,
            "sdp_m_line_index": j.SDPMLineIndex, "peer_id": "",
        })
    })

    ws.WriteJSON(map[string]any{
        "type": "uploader_ready", "share_code": session.ShareCode,
        "peer_id": uuid.NewString(), "device_info": "go-uploader",
    })

    done := make(chan struct{})
    dc.OnOpen(func() {
        meta, _ := json.Marshal(map[string]any{
            "type": "file_metadata", "fileName": name,
            "fileSize": len(data), "fileType": "application/octet-stream",
        })
        dc.SendText(string(meta))
        for off := 0; off < len(data); {
            for dc.BufferedAmount() > high { time.Sleep(10 * time.Millisecond) }
            end := off + chunk
            if end > len(data) { end = len(data) }
            dc.Send(data[off:end])
            off = end
        }
        dc.SendText("__EOF__")
        close(done)
    })

    go func() {
        for {
            var m map[string]any
            if err := ws.ReadJSON(&m); err != nil { return }
            switch m["type"] {
            case "peer_matched":
                offer, err := pc.CreateOffer(nil); must(err)
                must(pc.SetLocalDescription(offer))
                ws.WriteJSON(map[string]any{"type": "offer", "share_code": session.ShareCode, "sdp": offer.SDP, "peer_id": ""})
            case "answer":
                pc.SetRemoteDescription(webrtc.SessionDescription{Type: webrtc.SDPTypeAnswer, SDP: m["sdp"].(string)})
            case "ice_candidate":
                mid, _ := m["sdp_mid"].(string)
                idx, _ := m["sdp_m_line_index"].(float64); idxU16 := uint16(idx)
                pc.AddICECandidate(webrtc.ICECandidateInit{Candidate: m["candidate"].(string), SDPMid: &mid, SDPMLineIndex: &idxU16})
            }
        }
    }()

    <-done
    ws.WriteJSON(map[string]any{"type": "transfer_complete", "share_code": session.ShareCode})
}

func must(err error) { if err != nil { panic(err) } }
"#;

const KOTLIN_UPLOADER: &str = r#"// Share-Anything P2P Uploader — Kotlin / Spring Boot
// build.gradle.kts: see the Downloader sample for dependencies.
package com.example.shareanything

import com.fasterxml.jackson.databind.ObjectMapper
import dev.onvoid.webrtc.*
import org.springframework.stereotype.Service
import org.springframework.web.reactive.function.client.WebClient
import org.springframework.web.reactive.socket.WebSocketHandler
import org.springframework.web.reactive.socket.client.ReactorNettyWebSocketClient
import reactor.core.publisher.Mono
import java.net.URI
import java.nio.ByteBuffer
import java.nio.file.Files
import java.nio.file.Path
import java.util.*

@Service
class P2PUploader(private val mapper: ObjectMapper) {
    private val apiBase = "https://share-api.mingyu.dev"
    private val apiKey  = System.getenv("API_KEY")
    private val path    = Path.of("./report.pdf")
    private val password: String? = null
    private val CHUNK   = 64 * 1024
    private val HIGH    = 1024 * 1024

    suspend fun start() {
        val data = Files.readAllBytes(path)
        val name = path.fileName.toString()

        val client = WebClient.builder().baseUrl(apiBase).defaultHeader("X-API-Key", apiKey).build()
        val session = client.post().uri("/v1/p2p/sessions")
            .bodyValue(mapOf(
                "files" to listOf(mapOf("name" to name, "size" to data.size, "type" to "application/octet-stream")),
            ).let { if (password != null) it + ("password" to password) else it })
            .retrieve().bodyToMono(Map::class.java).block()!!
        val shareCode = session["share_code"] as String
        println("share_code: $shareCode")

        // (Fetch /v1/turn/credentials here and populate `ice` — omitted for brevity.)
        val ice = listOf(RTCIceServer().apply { urls = listOf("stun:stun.cloudflare.com:3478") })

        val factory = PeerConnectionFactory()
        val config = RTCConfiguration().apply { iceServers = ice }

        lateinit var pc: RTCPeerConnection
        lateinit var dc: RTCDataChannel

        val observer = object : PeerConnectionObserver {
            override fun onIceCandidate(c: RTCIceCandidate) {
                wsSend(mapOf(
                    "type" to "ice_candidate", "share_code" to shareCode,
                    "candidate" to c.sdp, "sdp_mid" to c.sdpMid,
                    "sdp_m_line_index" to c.sdpMLineIndex, "peer_id" to "",
                ))
            }
            // ... other observer methods omitted
        }
        pc = factory.createPeerConnection(config, observer)
        val dcInit = RTCDataChannelInit().apply { ordered = true }
        dc = pc.createDataChannel("share", dcInit)

        dc.registerObserver(object : RTCDataChannelObserver {
            override fun onStateChange() {
                if (dc.state == RTCDataChannelState.OPEN) {
                    val meta = mapper.writeValueAsBytes(mapOf(
                        "type" to "file_metadata", "fileName" to name,
                        "fileSize" to data.size, "fileType" to "application/octet-stream",
                    ))
                    dc.send(RTCDataChannelBuffer(ByteBuffer.wrap(meta), false))
                    var off = 0
                    while (off < data.size) {
                        while (dc.bufferedAmount > HIGH) Thread.sleep(10)
                        val end = minOf(off + CHUNK, data.size)
                        dc.send(RTCDataChannelBuffer(ByteBuffer.wrap(data, off, end - off), true))
                        off = end
                    }
                    dc.send(RTCDataChannelBuffer(ByteBuffer.wrap("__EOF__".toByteArray()), false))
                    wsSend(mapOf("type" to "transfer_complete", "share_code" to shareCode))
                }
            }
            override fun onMessage(buf: RTCDataChannelBuffer) {}
            override fun onBufferedAmountChange(prev: Long) {}
        })

        val wsUri = URI.create("${apiBase.replace("http", "ws")}/v1/ws/signaling")
        ReactorNettyWebSocketClient { it.addHeader("Sec-WebSocket-Protocol", "share-anything.v1, api-key.$apiKey") }
            .execute(wsUri, WebSocketHandler { session ->
                session.send(Mono.just(session.textMessage(mapper.writeValueAsString(mapOf(
                    "type" to "uploader_ready", "share_code" to shareCode,
                    "peer_id" to UUID.randomUUID().toString(), "device_info" to "kotlin-uploader",
                )))))
                .thenMany(session.receive().flatMap { msg ->
                    val m = mapper.readValue(msg.payloadAsText, Map::class.java) as Map<String, Any>
                    when (m["type"]) {
                        "peer_matched" -> {
                            val offer = pc.createOffer(RTCOfferOptions())
                            pc.setLocalDescription(offer, null)
                            session.send(Mono.just(session.textMessage(mapper.writeValueAsString(mapOf(
                                "type" to "offer", "share_code" to shareCode,
                                "sdp" to offer.sdp, "peer_id" to "",
                            )))))
                        }
                        "answer" -> {
                            pc.setRemoteDescription(RTCSessionDescription(RTCSdpType.ANSWER, m["sdp"] as String), null)
                            Mono.empty()
                        }
                        "ice_candidate" -> {
                            pc.addIceCandidate(RTCIceCandidate(
                                m["sdp_mid"] as? String ?: "",
                                (m["sdp_m_line_index"] as? Int) ?: 0,
                                m["candidate"] as String,
                            ))
                            Mono.empty()
                        }
                        else -> Mono.empty()
                    }
                }).then()
            }).block()
    }

    private fun wsSend(payload: Map<String, Any>) {
        // Keep a reference to the active WebSocketSession and write JSON here.
    }
}
"#;

const JAVA_UPLOADER: &str = r#"// Share-Anything P2P Uploader — Java / Spring Boot
// pom.xml: see the Downloader sample for dependencies.
package com.example.shareanything;

import com.fasterxml.jackson.databind.ObjectMapper;
import dev.onvoid.webrtc.*;
import org.springframework.stereotype.Service;
import org.springframework.web.reactive.function.client.WebClient;
import org.springframework.web.reactive.socket.WebSocketHandler;
import org.springframework.web.reactive.socket.client.ReactorNettyWebSocketClient;
import reactor.core.publisher.Mono;

import java.net.URI;
import java.nio.ByteBuffer;
import java.nio.file.*;
import java.util.*;

@Service
public class P2PUploader {
    private static final String API_BASE = "https://share-api.mingyu.dev";
    private static final int CHUNK = 64 * 1024;
    private static final int HIGH  = 1024 * 1024;

    private final ObjectMapper mapper = new ObjectMapper();
    private final String apiKey   = System.getenv("API_KEY");
    private final String password = null;

    public void start() throws Exception {
        Path path = Path.of("./report.pdf");
        byte[] data = Files.readAllBytes(path);
        String name = path.getFileName().toString();

        WebClient http = WebClient.builder().baseUrl(API_BASE).defaultHeader("X-API-Key", apiKey).build();
        Map<?, ?> session = http.post().uri("/v1/p2p/sessions")
            .bodyValue(Map.of(
                "files", List.of(Map.of("name", name, "size", data.length, "type", "application/octet-stream"))
            ))
            .retrieve().bodyToMono(Map.class).block();
        String shareCode = (String) session.get("share_code");
        System.out.println("share_code: " + shareCode);

        // (Fetch /v1/turn/credentials here and populate `ice`.)
        List<RTCIceServer> ice = List.of();

        PeerConnectionFactory factory = new PeerConnectionFactory();
        RTCConfiguration config = new RTCConfiguration();
        config.iceServers = ice;

        RTCPeerConnection[] pcHolder = new RTCPeerConnection[1];
        RTCDataChannel[]    dcHolder = new RTCDataChannel[1];

        PeerConnectionObserver observer = new PeerConnectionObserver() {
            @Override
            public void onIceCandidate(RTCIceCandidate c) {
                wsSend(Map.of(
                    "type", "ice_candidate", "share_code", shareCode,
                    "candidate", c.sdp, "sdp_mid", c.sdpMid,
                    "sdp_m_line_index", c.sdpMLineIndex, "peer_id", ""
                ));
            }
            // ... other observer methods omitted
        };
        pcHolder[0] = factory.createPeerConnection(config, observer);
        RTCDataChannelInit dcInit = new RTCDataChannelInit();
        dcInit.ordered = true;
        dcHolder[0] = pcHolder[0].createDataChannel("share", dcInit);

        dcHolder[0].registerObserver(new RTCDataChannelObserver() {
            @Override public void onBufferedAmountChange(long previousAmount) {}
            @Override public void onMessage(RTCDataChannelBuffer buf) {}
            @Override public void onStateChange() {
                if (dcHolder[0].getState() != RTCDataChannelState.OPEN) return;
                try {
                    byte[] meta = mapper.writeValueAsBytes(Map.of(
                        "type", "file_metadata", "fileName", name,
                        "fileSize", data.length, "fileType", "application/octet-stream"
                    ));
                    dcHolder[0].send(new RTCDataChannelBuffer(ByteBuffer.wrap(meta), false));
                    int off = 0;
                    while (off < data.length) {
                        while (dcHolder[0].getBufferedAmount() > HIGH) Thread.sleep(10);
                        int end = Math.min(off + CHUNK, data.length);
                        dcHolder[0].send(new RTCDataChannelBuffer(ByteBuffer.wrap(data, off, end - off), true));
                        off = end;
                    }
                    dcHolder[0].send(new RTCDataChannelBuffer(ByteBuffer.wrap("__EOF__".getBytes()), false));
                    wsSend(Map.of("type", "transfer_complete", "share_code", shareCode));
                } catch (Exception ignored) {}
            }
        });

        URI uri = URI.create(API_BASE.replace("http", "ws") + "/v1/ws/signaling");
        new ReactorNettyWebSocketClient(httpClient -> httpClient
                .headers(h -> h.add("Sec-WebSocket-Protocol", "share-anything.v1, api-key." + apiKey)))
            .execute(uri, (WebSocketHandler) sock -> sock.send(Mono.just(sock.textMessage(toJson(Map.of(
                "type", "uploader_ready", "share_code", shareCode,
                "peer_id", UUID.randomUUID().toString(), "device_info", "java-uploader"
            )))))
            .thenMany(sock.receive().flatMap(msg -> {
                try {
                    Map<String, Object> m = mapper.readValue(msg.getPayloadAsText(), Map.class);
                    switch ((String) m.get("type")) {
                        case "peer_matched":
                            RTCSessionDescription offer = pcHolder[0].createOffer(new RTCOfferOptions());
                            pcHolder[0].setLocalDescription(offer, null);
                            return sock.send(Mono.just(sock.textMessage(toJson(Map.of(
                                "type", "offer", "share_code", shareCode,
                                "sdp", offer.sdp, "peer_id", "")))));
                        case "answer":
                            pcHolder[0].setRemoteDescription(
                                new RTCSessionDescription(RTCSdpType.ANSWER, (String) m.get("sdp")), null);
                            return Mono.empty();
                        case "ice_candidate":
                            pcHolder[0].addIceCandidate(new RTCIceCandidate(
                                (String) m.getOrDefault("sdp_mid", ""),
                                ((Number) m.getOrDefault("sdp_m_line_index", 0)).intValue(),
                                (String) m.get("candidate")));
                            return Mono.empty();
                        default:
                            return Mono.empty();
                    }
                } catch (Exception e) { return Mono.empty(); }
            })).then()).block();
    }

    private String toJson(Object o) {
        try { return mapper.writeValueAsString(o); } catch (Exception e) { throw new RuntimeException(e); }
    }
    private void wsSend(Map<String, Object> payload) {
        // Keep a reference to the active WebSocketSession and write JSON here.
    }
}
"#;

const PHP_UPLOADER: &str = r#"<?php
// Share-Anything P2P Uploader — PHP
//
// ⚠️ Same caveat as the Downloader: PHP lacks a production-grade native
// WebRTC peer library, so this sample handles ONLY the REST + signaling
// layers. For the actual byte transfer, hand off to a Node.js subprocess
// or headless browser (see PHP Downloader note).
//
// composer require ratchet/pawl react/event-loop guzzlehttp/guzzle
require __DIR__ . '/vendor/autoload.php';

use GuzzleHttp\Client;
use Ratchet\Client\Connector;

$apiKey = getenv('API_KEY');
$path   = './report.pdf';
$bytes  = file_get_contents($path);

// 1. Create the P2P share via REST.
$http = new Client(['base_uri' => 'https://share-api.mingyu.dev']);
$res = $http->post('/v1/p2p/sessions', [
    'headers' => ['X-API-Key' => $apiKey, 'Content-Type' => 'application/json'],
    'json' => [
        'files' => [[
            'name' => basename($path),
            'size' => strlen($bytes),
            'type' => 'application/octet-stream',
        ]],
    ],
]);
$session = json_decode((string) $res->getBody(), true);
$shareCode = $session['share_code'];
echo "share_code: $shareCode\n";

// 2. Open the signaling WebSocket. WebRTC peer/DataChannel work happens
//    elsewhere (e.g. a Node.js helper process you spawn).
$loop = React\EventLoop\Loop::get();
$connector = new Connector($loop);
$headers = ['Sec-WebSocket-Protocol' => "share-anything.v1, api-key.$apiKey"];

$connector("wss://share-api.mingyu.dev/v1/ws/signaling", [], $headers)
    ->then(function ($ws) use ($shareCode) {
        $ws->send(json_encode([
            'type'        => 'uploader_ready',
            'share_code'  => $shareCode,
            'peer_id'     => bin2hex(random_bytes(16)),
            'device_info' => 'php-uploader',
        ]));

        $ws->on('message', function ($msg) use ($ws, $shareCode) {
            $m = json_decode($msg, true);
            switch ($m['type'] ?? '') {
                case 'peer_matched':
                    // Hand off to your WebRTC helper to createOffer().
                    break;
                case 'answer':
                    // Pass $m['sdp'] back to the WebRTC helper as the remote answer.
                    break;
                case 'ice_candidate':
                    // Forward to the WebRTC helper.
                    break;
                case 'error':
                    echo 'signaling error: ' . $m['message'] . PHP_EOL;
                    $ws->close();
                    break;
            }
        });
    }, function (\Exception $e) {
        echo 'could not connect: ' . $e->getMessage() . PHP_EOL;
    });

$loop->run();
"#;
