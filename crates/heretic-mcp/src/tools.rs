//! Surface unificada de tools MCP.
//!
//! # Por que 4 tools y no 17
//!
//! La version anterior tenia 17 tools, casi todas wrappers finos de una linea
//! sobre una action del bridge. Eso tiene dos costes reales:
//!
//! 1. **Contexto**: cada tool ocupa su schema en el prompt del LLM, y 17
//!    schemas que dicen lo mismo es ruido que desplaza la atencion.
//! 2. **Superficie de error**: cada wrapper reimplementa la construccion de
//!    params, y por eso se colaron bugs como `fl_set_song_position` mandando
//!    `ms` cuando el daemon leia `position`, o `create_project` sin rutear a
//!    Lifecycle. Con 17 caminos hay 17 sitios donde se puede uno equivocar.
//!
//! El escape hatch (`fl_do`) ya daba acceso a las 67 actions del bridge, asi
//! que las tools finas no anadian capacidad: solo anadian mantenimiento.
//!
//! # Las 4 tools
//!
//! | Tool | Que cubre |
//! |------|-----------|
//! | `fl_do` | las 67 actions del bridge (FL entero) |
//! | `fl_project` | ciclo de vida: status / open / save / close / create / wait |
//! | `fl_diagnose` | un comando que dice si FL esta usable y por que no |
//! | `fl_exec` | Python crudo dentro de FL (las 909 funciones de la API) |
//!
//! # Por que `fl_do` y no un solo mega-tool sin mas
//!
//! El LLM necesita **descubrir** que se puede hacer. `fl_do` responde con un
//! catalogo vivo cuando se llama sin action, y el error de action
//! desconocida incluye los nombres validos mas cercanos. Sin eso habria que
//! adivinar nombres de accion y el modelo se equivocaria.

use flojo_mcp::prelude::*;
use serde_json::{json, Map, Value};

use crate::client::DaemonClient;

/// Convierte cualquier error del daemon en ToolError, con pista util.
fn daemon_err(e: impl std::fmt::Display) -> ToolError {
    let msg = e.to_string();
    // Los dos fallos que mas se repiten en uso real, explicados aqui para que
    // el LLM sepa que hacer en vez de reintentar a ciegas.
    let hint = if msg.contains("unsafe at current time") {
        " FL tiene un dialogo modal abierto, por eso rechaza cambios del proyecto. \
         Usa fl_diagnose: si hay un 'Save changes?' pendiente, guardalo con \
         fl_project save, o cerralo a mano. Reintentar sin cerrar el dialogo \
         no va a funcionar."
    } else if msg.contains("Todas las instancias") || msg.contains("os error 231") {
        " El daemon no tiene una instancia de pipe libre. Reinicia el daemon."
    } else if msg.contains("os error 2") || msg.contains("no se pudo conectar") {
        " El daemon no esta corriendo. Arrancalo con: fl-heretic daemon"
    } else {
        ""
    };
    ToolError::internal(format!("{msg}{hint}"))
}

async fn daemon() -> std::result::Result<DaemonClient, ToolError> {
    DaemonClient::from_env().map_err(daemon_err)
}

/// Llama al daemon y devuelve el resultado, decorate errores con pista.
async fn call(method: &str, params: Value) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    client.call(method, params).await.map_err(daemon_err)
}

// ============================================================================
// 1. fl_do — el bridge entero
// ============================================================================

#[tool(description = "Run any action on the running FL Studio instance. This is the main tool: it reaches all 67 FL actions (transport, channels, mixer, plugins, patterns, playlist, piano roll, project, ui, automation). Call it with no action first to get the catalog with the exact name and required params of every action, so you never have to guess a name. Common actions: 'transport.setTempo' {bpm}, 'transport.play', 'channels.list', 'channels.setName' {index,name}, 'mixer.setVolume' {track,volume}, 'patterns.list', 'patterns.create' {name}, 'pianoroll.getNotes'. FL must be running and the FL Heretic Bridge selected in Options > MIDI Settings.")]
pub async fn fl_do(
    action: Option<String>,
    params: Option<Map<String, Value>>,
) -> std::result::Result<Value, ToolError> {
    let params = params.map(Value::Object).unwrap_or(json!({}));
    match action {
        Some(action) => call("call", json!({ "action": action, "params": params })).await,
        None => {
            // Sin action: el catalogo. Es lo que evita adivinar nombres.
            call("actions", json!({})).await
        }
    }
}

// ============================================================================
// 2. fl_project — ciclo de vida
// ============================================================================

#[tool(description = "Manage the FL Studio project and the FL process. op=status: is FL running, is the bridge awake, is a dialog blocking it. op=open: open a .flp (saves the current project first, so FL never asks 'Save changes?'). op=save: save the current project in place (Ctrl+S); BLOCKS if the project was never saved, because FL opens a modal dialog. op=create: create a NEW project from a template in FL's projects folder and open it. op=close: close FL, saving first by default. op=wait: block until the bridge answers, use after open/create. Use op=status first when something is not working.")]
pub async fn fl_project(
    op: String,
    path: Option<String>,
    name: Option<String>,
    template: Option<String>,
    dir: Option<String>,
    force: Option<bool>,
    timeout_sec: Option<f64>,
) -> std::result::Result<Value, ToolError> {
    let op = op.trim().to_ascii_lowercase();
    let mut p = json!({});

    match op.as_str() {
        "status" => return call("fl_status", json!({})).await,
        "wait" => {
            let t = timeout_sec.unwrap_or(30.0);
            return call("wait_ready", json!({ "timeout": t })).await;
        }
        "save" => return call("save", json!({})).await,
        "open" => {
            let path = path.ok_or_else(|| {
                ToolError::invalid_params("fl_project open necesita 'path' (un .flp)")
            })?;
            if !path.to_lowercase().ends_with(".flp") {
                return Err(ToolError::invalid_params(format!(
                    "la ruta debe terminar en .flp: {path}"
                )));
            }
            p["path"] = json!(path);
        }
        "create" => {
            let name = name.ok_or_else(|| {
                ToolError::invalid_params("fl_project create necesita 'name' (sin .flp)")
            })?;
            p["name"] = json!(name);
            if let Some(d) = dir.filter(|s| !s.is_empty()) {
                p["dir"] = json!(d);
            }
            if let Some(t) = template.filter(|s| !s.is_empty()) {
                p["template"] = json!(t);
            }
        }
        "close" => {
            p["force"] = json!(force.unwrap_or(false));
        }
        other => {
            return Err(ToolError::invalid_params(format!(
                "op desconocido: {other}. Validas: status, open, save, create, close, wait"
            )))
        }
    }
    // El nombre del metodo del daemon no es el mismo que el op. `create`
    // en el daemon es `create_project`; el resto coincide. Sin este mapa,
    // `fl_project create` responde "metodo transport desconocido: create".
    let method = match op.as_str() {
        "create" => "create_project",
        other => other,
    };
    call(method, p).await
}

// ============================================================================
// 3. fl_diagnose
// ============================================================================

#[tool(description = "Diagnose FL Studio when something is not working, in one call. Says whether FL is running, whether the FL Heretic Bridge is awake (the bridge only runs when FL receives a MIDI event, so a silent bridge is normal without a wake), whether a modal dialog is blocking FL (a 'Save changes?' or the 'Welcome to FL Studio' wizard makes FL reject every write with 'Operation unsafe at current time'), and what to do about it. Start here instead of guessing.")]
pub async fn fl_diagnose() -> std::result::Result<Value, ToolError> {
    let mut out = json!({});

    // 1. El daemon responde?
    match call("fl_status", json!({})).await {
        Ok(v) => {
            out["daemon"] = json!("ok");
            out["fl_running"] = v.get("fl_running").cloned().unwrap_or(json!(null));
            out["bridge_online"] = v.get("bridge_online").cloned().unwrap_or(json!(null));
            out["midi_wake_ports"] = v.get("midi_wake_ports").cloned().unwrap_or(json!(null));
        }
        Err(e) => {
            out["daemon"] = json!("ko");
            out["problema"] = json!(e.to_string());
            out["que_hacer"] = json!(
                "El daemon no responde. Arrancalo con 'fl-heretic daemon'."
            );
            return Ok(out);
        }
    }

    // 2. El bridge esta despierto de verdad? (una escritura inocua lo comprueba)
    let wake = call("call", json!({ "action": "meta.ping", "params": {} })).await;
    out["bridge_ping"] = match &wake {
        Ok(v) => json!("ok"),
        Err(e) => json!(e.to_string()),
    };

    // 3. Hay un modal bloqueando? Se deduce de si una escritura pasa.
    match call("call", json!({ "action": "transport.status", "params": {} })).await {
        Ok(v) => {
            let bpm = v.get("bpm").cloned().unwrap_or(json!(null));
            // Escribir el tempo actual es inocuo: no cambia nada, pero FL lo
            // rechaza igual si hay un modal. Es la prueba mas barata.
            let write = call(
                "call",
                json!({ "action": "transport.setTempo", "params": { "bpm": bpm } }),
            )
            .await;
            match write {
                Ok(_) => {
                    out["escrituras"] = json!("ok");
                    out["veredicto"] = json!("FL Studio esta operativo.");
                }
                Err(e) => {
                    let msg = e.to_string();
                    out["escrituras"] = json!("bloqueadas");
                    if msg.contains("unsafe") {
                        out["veredicto"] = json!(
                            "FL tiene un DIALOGO MODAL abierto: por eso rechaza los \
                             cambios del proyecto. El bridge esta sano, pero FL no deja \
                             trabajar hasta que se cierre el dialogo."
                        );
                        out["que_hacer"] = json!({
                            "1. " : "Mira la ventana de FL: casi siempre es un 'Save changes?' de la sesion anterior.",
                            "2. " : "Cierra el dialogo. Si es el 'Welcome to FL Studio', cerralo con la X.",
                            "3. " : "fl_do('project.save') guarda el proyecto y evita que vuelva a aparecer.",
                            "nota": "El daemon ya cierra solo el wizard de bienvenida, asi que si sigue, es otro dialogo: miralo a mano."
                        });
                    } else {
                        out["veredicto"] = json!("FL rechaza las escrituras por otra causa.");
                        out["que_hacer"] = json!(msg);
                    }
                }
            }
        }
        Err(e) => {
            out["lecturas"] = json!("bloqueadas");
            out["veredicto"] = json!(e.to_string());
        }
    }

    Ok(out)
}

// ============================================================================
// 4. fl_exec — Python crudo
// ============================================================================

#[tool(description = "Run arbitrary Python inside FL Studio's own interpreter, with full access to all 909 functions of the FL API across its 11 modules (channels, mixer, patterns, playlist, pianoroll, plugins, transport, general, midi, arrangement, ...). The last loose expression is returned as the result. This is the escape hatch for anything fl_do does not cover, and the way to discover new capability: use dir(module) and help() to explore what this FL build actually exposes. Only use it if fl_do has no action for what you need.")]
pub async fn fl_exec(
    code: String,
) -> std::result::Result<Value, ToolError> {
    call("call", json!({ "action": "meta.exec", "params": { "code": code } })).await
}
