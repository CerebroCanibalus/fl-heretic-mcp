#!/usr/bin/env python3
"""Anade las tools MCP de ciclo de vida y el escape hatch `fl_call`."""
import io

PATH = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-mcp\src\tools.rs"
src = io.open(PATH, encoding="utf-8").read()

addition = '''
// ============================================================================
// Escape hatch — acceso a las 67 actions del bridge
// ============================================================================

#[tool(description = "Call ANY action of the FL Heretic Bridge directly. Use fl_actions to list them. This is the escape hatch that gives access to every capability without needing a dedicated tool: channels, mixer, plugins, patterns, playlist, pianoroll, project, ui, automation, plus meta.exec to run arbitrary Python in FL's own interpreter. Example: action='channels.setVolume', params='{\"index\":0,\"volume\":0.5}'")]
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
'''

src = src.rstrip() + "\n" + addition
io.open(PATH, "w", encoding="utf-8", newline="\n").write(src)
print("tools.rs: %d lineas" % len(src.splitlines()))
