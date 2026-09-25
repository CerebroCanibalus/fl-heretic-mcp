//! `#[tool]` handlers de FlojoMCP que delegan al daemon blindado.
//!
//! Cada tool:
//! 1. Recibe args validados por schemars.
//! 2. Construye params JSON.
//! 3. Abre conexión al daemon (Named Pipe + HMAC handshake).
//! 4. Envía request JSON-RPC.
//! 5. Devuelve el resultado.
//!
//! Para Fase 2: solo transport tools (7).
//! Para Fase 3: añadir el resto (67 tools del FLStudioMCP legacy).

use flojo_mcp::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};

use crate::client::DaemonClient;

/// Convierte cualquier error del daemon en ToolError para que FlojoMCP lo reporte al cliente.
fn daemon_err(e: impl std::fmt::Display) -> ToolError {
    ToolError::internal_error(format!("daemon error: {e}"))
}

/// Helper que crea un DaemonClient por tool call. Fase 5 introducirá pooling.
async fn daemon() -> std::result::Result<DaemonClient, ToolError> {
    DaemonClient::from_env().map_err(daemon_err)
}

// ============================================================================
// Tools transport (mirror del FLStudioMCP legacy, Fase 2)
// ============================================================================

#[derive(Debug, Serialize)]
struct PingResult {
    fl_version: String,
    protocol_version: u32,
    pong: bool,
}

#[tool(
    name = "fl_ping",
    description = "Ping the FL Studio daemon. Returns FL version + protocol version if alive."
)]
pub async fn fl_ping() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("ping", json!({})).await.map_err(daemon_err)?;
    // data = {"pong": true, "protocol_version": 1, "crate_version": "..."}
    // El FL version viene del heartbeat en el daemon, no del ping directo. Para
    // simplicidad Fase 2, devolvemos lo que el ping del daemon reporta.
    Ok(data)
}

#[tool(
    name = "fl_get_tempo",
    description = "Get the current FL Studio tempo in BPM. The raw internal value is bpm*1000."
)]
pub async fn fl_get_tempo() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("get_tempo", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SetTempoArgs {
    /// BPM to set (range 10-999).
    #[schemars(range(min = 10.0, max = 999.0))]
    pub bpm: f64,
}

#[tool(
    name = "fl_set_tempo",
    description = "Set the FL Studio tempo in BPM. Range 10-999. May be ignored if FL is in a modal dialog."
)]
pub async fn fl_set_tempo(
    #[tool_param(description = "BPM value, 10-999")] bpm: f64,
) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("set_tempo", json!({ "bpm": bpm }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(
    name = "fl_play",
    description = "Start FL Studio transport playback. Idempotent: no-op if already playing."
)]
pub async fn fl_play() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("play", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(
    name = "fl_stop",
    description = "Stop FL Studio transport playback. Idempotent: no-op if already stopped."
)]
pub async fn fl_stop() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("stop", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(
    name = "fl_get_song_position",
    description = "Get current FL Studio song position (ms, ticks, beats) + current BPM."
)]
pub async fn fl_get_song_position() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("get_song_position", json!({}))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SetSongPositionArgs {
    /// Position in milliseconds.
    #[schemars(range(min = 0.0))]
    pub ms: f64,
}

#[tool(
    name = "fl_set_song_position",
    description = "Set FL Studio song position in milliseconds. Returns the new position (ms/ticks/beats)."
)]
pub async fn fl_set_song_position(
    #[tool_param(description = "Position in milliseconds (>= 0)")] ms: f64,
) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("set_song_position", json!({ "ms": ms }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(
    name = "fl_get_play_state",
    description = "Get FL Studio transport state (playing/recording)."
)]
pub async fn fl_get_play_state() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("get_play_state", json!({}))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}