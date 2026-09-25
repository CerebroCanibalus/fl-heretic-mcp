//! `#[tool]` handlers de FlojoMCP que delegan al daemon blindado.
//!
//! Cada tool:
//! 1. Recibe args validados por schemars (FlojoMCP genera el schema auto).
//! 2. Construye params JSON.
//! 3. Abre conexión al daemon (Named Pipe + HMAC handshake).
//! 4. Envía request JSON-RPC.
//! 5. Devuelve el resultado.
//!
//! Para Fase 2: solo transport tools (7).
//! Para Fase 3: añadir el resto (67 tools del FLStudioMCP legacy).

use flojo_mcp::prelude::*;
use serde_json::{json, Value};

use crate::client::DaemonClient;

/// Convierte cualquier error del daemon en ToolError.
fn daemon_err(e: impl std::fmt::Display) -> ToolError {
    ToolError::internal(format!("daemon error: {e}"))
}

/// Helper: crea un DaemonClient por tool call.
async fn daemon() -> std::result::Result<DaemonClient, ToolError> {
    DaemonClient::from_env().map_err(daemon_err)
}

// ============================================================================
// Tools transport (mirror del FLStudioMCP legacy, Fase 2)
// ============================================================================

#[tool(description = "Ping the FL Studio daemon. Returns FL version + protocol version if alive.")]
pub async fn fl_ping() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("ping", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Get the current FL Studio tempo in BPM. The raw internal value is bpm*1000.")]
pub async fn fl_get_tempo() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("get_tempo", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Set the FL Studio tempo in BPM. Range 10-999. May be ignored if FL is in a modal dialog.")]
pub async fn fl_set_tempo(
    bpm: f64,
) -> std::result::Result<Value, ToolError> {
    if !(10.0..=999.0).contains(&bpm) {
        return Err(ToolError::invalid_params(format!(
            "bpm fuera de rango 10-999: {bpm}"
        )));
    }
    let client = daemon().await?;
    let data = client
        .call("set_tempo", json!({ "bpm": bpm }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Start FL Studio transport playback. Idempotent: no-op if already playing.")]
pub async fn fl_play() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("play", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Stop FL Studio transport playback. Idempotent: no-op if already stopped.")]
pub async fn fl_stop() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("stop", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Get FL Studio transport state (playing/recording).")]
pub async fn fl_get_play_state() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("get_play_state", json!({}))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Get current FL Studio song position (ms, ticks, beats) + current BPM.")]
pub async fn fl_get_song_position() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("get_song_position", json!({}))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Set FL Studio song position in milliseconds (>= 0). Returns the new position (ms/ticks/beats).")]
pub async fn fl_set_song_position(
    ms: f64,
) -> std::result::Result<Value, ToolError> {
    if ms < 0.0 {
        return Err(ToolError::invalid_params(format!("ms debe ser >= 0: {ms}")));
    }
    let client = daemon().await?;
    let data = client
        .call("set_song_position", json!({ "ms": ms }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}