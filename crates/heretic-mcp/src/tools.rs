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

// ============================================================================
// Escape hatch — acceso a las 67 actions del bridge
// ============================================================================

#[tool(description = "Call ANY action of the FL Heretic Bridge directly. Use fl_actions to list them. Escape hatch to every capability without a dedicated tool: channels, mixer, plugins, patterns, playlist, pianoroll, project, ui, automation, plus meta.exec for arbitrary Python inside FL. Params must be a JSON object string, e.g. index 0 volume 0.5.")]
pub async fn fl_call(
    action: String,
    params_json: String,
) -> std::result::Result<Value, ToolError> {
    let params: Value = serde_json::from_str(&params_json).map_err(|e| {
        ToolError::invalid_params(format!("params_json no es JSON valido: {e}"))
    })?;
    let client = daemon().await?;
    let data = client
        .call("call", json!({ "action": action, "params": params }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "List every action the FL Heretic Bridge exposes. Returns both the catalog compiled into the daemon and the list the bridge reports live. Use this before fl_call when you don't know the exact action name.")]
pub async fn fl_actions() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("actions", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

// ============================================================================
// Ciclo de vida de FL Studio
// ============================================================================

#[tool(description = "Is FL Studio running? Returns the process id, the executable path, whether the bridge (controller script) is responding, and how many MIDI wake ports are open. Start here when something is not working.")]
pub async fn fl_status() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("fl_status", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Wait until FL Studio and the bridge are ready. Use after launching FL Studio or opening a project, so you don't race the daemon against FL's startup. Returns as soon as the bridge answers a ping.")]
pub async fn fl_wait_ready(
    timeout_sec: f64,
) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("wait_ready", json!({ "timeout": timeout_sec }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Open a .flp project in FL Studio. Launches FL Studio if it isn't running, or opens the file in the existing instance if it is. After this, call fl_wait_ready so the bridge is responsive before doing anything else.")]
pub async fn fl_open(
    path: String,
) -> std::result::Result<Value, ToolError> {
    if !path.to_lowercase().ends_with(".flp") {
        return Err(ToolError::invalid_params(format!(
            "la ruta debe terminar en .flp: {path}"
        )));
    }
    let client = daemon().await?;
    let data = client
        .call("open", json!({ "path": path }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Save the current FL Studio project (same as Ctrl+S). WARNING: if the project has never been saved, FL opens a modal 'Save as' dialog and blocks until a human closes it. Check fl_project_info first if unsure.")]
pub async fn fl_save() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client.call("save", json!({})).await.map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Close FL Studio politely (WM_CLOSE). By default it saves first if there are unsaved changes, so FL won't show a modal dialog. Set force=true to kill the process after the timeout, which LOSES unsaved changes. This closes the whole application, not just the project.")]
pub async fn fl_close(
    force: bool,
) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("close", json!({ "force": force }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Project info: title, author, tempo, PPQ, pattern and channel counts, and crucially 'changed' (true = unsaved edits) and 'has_file' (false = never saved, so fl_save would open a modal dialog). Use this before fl_save or fl_close.")]
pub async fn fl_project_info() -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let data = client
        .call("call", json!({ "action": "project.metadata", "params": {} }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}

#[tool(description = "Create a NEW FL Studio project in FL's projects folder and open it. Copies a template .flp to the destination and launches FL with it, which is fully automatic and needs no keyboard or dialogs. This is the reliable way to make a new project: FL's own Save As dialog cannot be automated because its fields are internal Delphi controls that close without saving. Template defaults to the most recent .flp in FL's projects folder that is not a backup.")]
pub async fn fl_create_project(
    name: String,
    dir: String,
    template: String,
) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let mut params = json!({ "name": name });
    if !dir.is_empty() {
        params["dir"] = json!(dir);
    }
    if !template.is_empty() {
        params["template"] = json!(template);
    }
    let data = client
        .call("create_project", params)
        .await
        .map_err(daemon_err)?;
    Ok(data)
}
