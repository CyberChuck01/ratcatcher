//! Tunnel R.A.T. — the live, API-keyed link to the paired hub.
//!
//! `GET /api/tunnel-rat/link` upgrades to a WebSocket. The API key from pairing
//! goes in `Authorization: Bearer <key>` (or `?key=<key>` for small clients that
//! can't set headers). While a paired hub is connected the rat is green; when the
//! last one drops, or goes quiet for `SILENCE_LIMIT`, it turns red.
//!
//! The link doesn't care how the hub reaches the Orbic: the Orbic's own hotspot,
//! a network the Orbic joined with Rayhunter's Wi-Fi client mode, or a USB-C
//! tether all work. The hub connects to whichever address answers.
//!
//! Orbic -> hub, JSON text frames:
//!   {"type":"hello","device":"orbic","firmware":"0.13.0","link_version":1,"paired_as":"PiPack"}
//!   {"type":"cells","cells":[[5230,101],...]}        every cell this session, once at connect
//!   {"type":"status","recording":true,"cells":12,"stingrays":0}   every 2 s
//!   {"type":"cell","earfcn":5230,"pci":101}           a cell not heard before this session
//!   {"type":"alert","severity":"High","stingrays":1}  a new stingray encounter
//!   {"type":"new_session"}                            recording restarted, counters reset
//!   {"type":"pong"} | {"type":"ack",...} | {"type":"error","error":"..."}
//! Hub -> Orbic:
//!   {"type":"ping"}                                   send at least every 20 s
//!   {"type":"gps_fix","lat":39.36,"lon":-76.52}       needs gps_mode = 2 (API)
//!   {"type":"command","command":"start_recording"}    or "stop_recording"

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, Uri, header::AUTHORIZATION};
use axum::response::{IntoResponse, Response};
use log::{info, warn};
use serde_json::{Value, json};

use super::pairing;
use super::status::{self, Event, Link};
use crate::config::GpsMode;
use crate::diag::{DiagDeviceCtrlMessage, start_recording, stop_recording};
use crate::gps::GpsData;
use crate::server::ServerState;

/// Bumped when the message format changes in a way hubs need to know about.
pub const LINK_VERSION: u32 = 1;
const STATUS_EVERY: Duration = Duration::from_secs(2);
/// A hub that sends nothing for this long is treated as gone (catches dead Wi-Fi).
const SILENCE_LIMIT: Duration = Duration::from_secs(20);
const TICK: Duration = Duration::from_millis(500);

/// Open links. The rat is green while this is above zero.
static CLIENTS: AtomicU32 = AtomicU32::new(0);

/// GET /api/tunnel-rat/link (WebSocket upgrade). 401 without the paired API key.
pub async fn link(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let key = bearer_key(&headers).or_else(|| query_key(&uri));
    if !key.as_deref().is_some_and(pairing::is_valid_key) {
        return (
            StatusCode::UNAUTHORIZED,
            "Tunnel R.A.T. link: pair first, then send the API key\n",
        )
            .into_response();
    }
    let paired_as = pairing::paired_name().unwrap_or_default();
    ws.on_upgrade(move |socket| run(socket, state, paired_as))
}

fn bearer_key(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let key = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    Some(key.trim().to_string())
}

fn query_key(uri: &Uri) -> Option<String> {
    url::form_urlencoded::parse(uri.query()?.as_bytes())
        .find(|(name, _)| name == "key")
        .map(|(_, value)| value.trim().to_string())
}

/// Counts an open link for as long as it lives, even if the task panics.
struct OpenLink {
    paired_as: String,
    restarting: bool,
}

impl OpenLink {
    fn open(paired_as: String) -> Self {
        let open = CLIENTS.fetch_add(1, Ordering::SeqCst) + 1;
        status::set_link(Link::Connected);
        info!("tunnel-rat: {paired_as} connected over the live link ({open} open)");
        OpenLink {
            paired_as,
            restarting: false,
        }
    }
}

impl Drop for OpenLink {
    fn drop(&mut self) {
        let left = CLIENTS.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
        info!(
            "tunnel-rat: {} disconnected from the live link ({left} open)",
            self.paired_as
        );
        // A config save restarts Rayhunter in-process; the hub reconnects in a
        // second or two, so don't flash red for that.
        if left == 0 && !self.restarting && pairing::paired_name().is_some() {
            status::set_link(Link::Lost);
        }
    }
}

async fn run(mut socket: WebSocket, state: Arc<ServerState>, paired_as: String) {
    let mut open = OpenLink::open(paired_as.clone());

    let mut seq = status::latest_seq();
    let cells: Vec<[u32; 2]> = status::cells_list()
        .into_iter()
        .map(|(earfcn, pci)| [earfcn, u32::from(pci)])
        .collect();
    let hello = json!({
        "type": "hello",
        "device": "orbic",
        "firmware": env!("CARGO_PKG_VERSION"),
        "link_version": LINK_VERSION,
        "paired_as": paired_as.as_str(),
    });
    let mut alive = send(&mut socket, hello).await
        && send(&mut socket, json!({"type": "cells", "cells": cells})).await
        && send(&mut socket, status_json()).await;

    let mut tick = tokio::time::interval(TICK);
    let mut last_status = Instant::now();
    let mut last_heard = Instant::now();
    let restart = state.daemon_restart_token.clone();

    while alive {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    last_heard = Instant::now();
                    if let Some(reply) = handle(&state, text.as_str()).await {
                        alive = send(&mut socket, reply).await;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => alive = false,
                // Protocol pings/pongs and binary frames still prove the hub is there.
                Some(Ok(_)) => last_heard = Instant::now(),
            },
            _ = tick.tick() => {
                if last_heard.elapsed() > SILENCE_LIMIT {
                    warn!("tunnel-rat: {paired_as} went quiet for {SILENCE_LIMIT:?}, dropping the link");
                    alive = false;
                } else {
                    let (events, next) = status::events_after(seq);
                    seq = next;
                    for event in events {
                        if !send(&mut socket, event_json(event)).await {
                            alive = false;
                            break;
                        }
                    }
                    if alive && last_status.elapsed() >= STATUS_EVERY {
                        last_status = Instant::now();
                        alive = send(&mut socket, status_json()).await;
                    }
                }
            },
            _ = restart.cancelled() => {
                open.restarting = true;
                let _ = socket.send(Message::Close(None)).await;
                alive = false;
            },
        }
    }
    drop(open);
}

async fn send(socket: &mut WebSocket, msg: Value) -> bool {
    socket.send(Message::Text(msg.to_string().into())).await.is_ok()
}

fn status_json() -> Value {
    let s = status::snapshot();
    json!({
        "type": "status",
        "recording": s.recording,
        "cells": s.cells,
        "stingrays": s.stingrays,
    })
}

fn event_json(event: Event) -> Value {
    match event {
        Event::Cell { earfcn, pci } => json!({"type": "cell", "earfcn": earfcn, "pci": pci}),
        Event::Alert {
            severity,
            stingrays,
        } => json!({"type": "alert", "severity": severity, "stingrays": stingrays}),
        Event::NewSession => json!({"type": "new_session"}),
    }
}

fn error_json(error: impl Into<String>) -> Value {
    json!({"type": "error", "error": error.into()})
}

/// Answer one message from the hub. `None` means nothing to send back.
async fn handle(state: &Arc<ServerState>, text: &str) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(text) else {
        return Some(error_json("not JSON"));
    };
    match msg.get("type").and_then(Value::as_str).unwrap_or("") {
        "ping" => Some(json!({"type": "pong"})),
        "gps_fix" => gps_fix(state, &msg).await,
        "command" => {
            let cmd = msg.get("command").and_then(Value::as_str).unwrap_or("");
            Some(command(state, cmd).await)
        }
        other => Some(error_json(format!("unknown message type {other:?}"))),
    }
}

/// Same effect as `POST /api/gps`: tags the recording with the hub's position.
async fn gps_fix(state: &Arc<ServerState>, msg: &Value) -> Option<Value> {
    if state.config.gps_mode != GpsMode::Api {
        return Some(error_json(
            "gps_mode isn't API (2); set GPS to \"API endpoint\" in Rayhunter's config",
        ));
    }
    let lat = msg.get("lat").and_then(Value::as_f64);
    let lon = msg.get("lon").and_then(Value::as_f64);
    let (Some(lat), Some(lon)) = (lat, lon) else {
        return Some(error_json("gps_fix needs lat and lon"));
    };
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Some(error_json("gps_fix lat/lon out of range"));
    }
    *state.gps_state.write().await = Some(GpsData {
        latitude: lat,
        longitude: lon,
    });
    match state
        .diag_device_ctrl_sender
        .send(DiagDeviceCtrlMessage::GpsUpdate { lat, lon })
        .await
    {
        Ok(()) => None,
        Err(e) => Some(error_json(format!("couldn't record GPS: {e}"))),
    }
}

async fn command(state: &Arc<ServerState>, cmd: &str) -> Value {
    let result = match cmd {
        "start_recording" => start_recording(State(state.clone())).await,
        "stop_recording" => stop_recording(State(state.clone())).await,
        _ => return error_json(format!("unknown command {cmd:?}")),
    };
    let (ok, message) = match result {
        Ok((_, message)) => (true, message),
        Err((_, message)) => (false, message),
    };
    json!({"type": "ack", "command": cmd, "ok": ok, "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_key_in_header_or_query() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer_key(&headers), None);
        headers.insert(AUTHORIZATION, "Bearer abc123 ".parse().unwrap());
        assert_eq!(bearer_key(&headers).as_deref(), Some("abc123"));

        let uri: Uri = "/api/tunnel-rat/link?x=1&key=k%2B9".parse().unwrap();
        assert_eq!(query_key(&uri).as_deref(), Some("k+9"));
        let uri: Uri = "/api/tunnel-rat/link".parse().unwrap();
        assert_eq!(query_key(&uri), None);
    }

    #[test]
    fn events_serialize_for_the_hub() {
        assert_eq!(
            event_json(Event::Cell {
                earfcn: 5230,
                pci: 101
            }),
            json!({"type": "cell", "earfcn": 5230, "pci": 101})
        );
        assert_eq!(
            event_json(Event::Alert {
                severity: "High".into(),
                stingrays: 2
            })["stingrays"],
            2
        );
    }
}
