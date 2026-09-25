//! Named Pipe server + JSON-RPC dispatch.
//!
//! Topologia completa:
//!
//! ```text
//! [MCP server] --Named Pipe+HMAC--> [daemon] --file-RPC--> [FL Heretic Bridge] --FL API--> FL Studio
//!                                       |                                              ^
//!                                       +--MIDI out (wake)--------------------------+
//!                                       +--WM_CLOSE / CreateProcess (proceso FL)----+
//! ```
//!
//! El daemon tiene dos vias hacia FL Studio, porque el sandbox del script de FL
//! no permite nada de lo que hace falta para gestionar proyectos:
//!
//! | Necesidad | Via | Por que
//! |---|---|---|
//! | Guardar proyecto | bridge (`FPT_Save`) | el script si puede ejecutar el atajo
//! | Abrir proyecto   | daemon (`CreateProcess`) | `dir(general)` no tiene nada de abrir
//! | Cerrar proyecto  | daemon (`WM_CLOSE`)    | no hay `FPT_Close`
//! | Despertar FL     | daemon (MIDI out)      | `OnIdle` no se dispara; el pump real es `OnMidiIn`
//!
//! ## Handlers disponibles
//!
//! | Metodo              | Handler      | Action del bridge           |
//! |---------------------|--------------|-----------------------------|
//! | `ping`              | transport    | `meta.ping`                 |
//! | `health`            | transport    | ping real                   |
//! | `get_tempo`         | transport    | `transport.status`          |
//! | `set_tempo`         | transport    | `transport.setTempo`        |
//! | `play`              | transport    | `transport.start`           |
//! | `stop`              | transport    | `transport.stop`            |
//! | `get_play_state`    | transport    | `transport.status`          |
//! | `get_song_position` | transport    | `transport.status`          |
//! | `set_song_position` | transport    | `transport.setPosition`     |
//! | `call`              | transport    | cualquier action           |
//! | `fl_open`           | lifecycle    | `CreateProcess`             |
//! | `fl_close`          | lifecycle    | `WM_CLOSE`                  |
//! | `fl_status`         | lifecycle    | proceso + ping              |

use std::path::PathBuf;
use std::sync::Arc;

use heretic_core::{
    AuditEvent, AuditLog, AuditStatus, AuthChallenge, AuthResponse, AuthVerifier, HereticError,
    Request, Response, Token, TokenStore,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use heretic_fl::{BridgeConfig, FlBridge};
use crate::handlers::{Lifecycle, Transport};

/// Config del daemon.
#[derive(Debug, Clone)]
pub struct Config {
    pub pipe_name: String,
    pub token_path: PathBuf,
    pub audit_path: PathBuf,
    /// Directorio del FL Heretic Bridge (donde vive el controller script).
    pub script_dir: PathBuf,
    /// Si true, verifica el bridge con meta.ping antes de aceptar clientes.
    pub wait_for_bridge: bool,
}

impl Config {
    pub fn from_opts(pipe: Option<String>, token: Option<String>, audit: Option<String>) -> Self {
        Self {
            pipe_name: pipe.unwrap_or_else(default_pipe_name),
            token_path: token
                .map(PathBuf::from)
                .unwrap_or_else(TokenStore::default_path),
            audit_path: audit
                .map(PathBuf::from)
                .unwrap_or_else(AuditLog::default_path),
            script_dir: heretic_fl::default_script_dir(),
            wait_for_bridge: true,
        }
    }
}

/// Pipe name default: `\\.\pipe\fl-heretic-<pid>`.
pub fn default_pipe_name() -> String {
    let pid = std::process::id();
    format!(r"\\.\pipe\fl-heretic-{pid}")
}

/// Arranca el daemon (loop infinito).
pub fn run(
    pipe: Option<String>,
    token_path: Option<String>,
    audit_path: Option<String>,
) -> Result<(), HereticError> {
    let config = Config::from_opts(pipe, token_path, audit_path);

    tracing::info!("daemon arrancando");
    tracing::info!("  pipe:       {}", config.pipe_name);
    tracing::info!("  token:      {}", config.token_path.display());
    tracing::info!("  audit:      {}", config.audit_path.display());
    tracing::info!("  bridge:     {}", config.script_dir.display());

    // Cargar token (crear si no existe)
    let store = TokenStore::new(config.token_path.clone());
    let token = store.load_or_create()?;
    tracing::info!("  token cargado ({} chars)", token.as_str().len());

    // Audit log
    let audit = Arc::new(AuditLog::open(config.audit_path.clone())?);
    tracing::info!(
        "  audit log abierto ({} eventos previos)",
        audit.len().unwrap_or(0)
    );

    // Tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("fl-heretic")
        .build()
        .map_err(|e| HereticError::Other(format!("creando runtime tokio: {e}")))?;

    rt.block_on(async move {
        serve(config, token, audit).await
    })
}

/// Tokio async: crea bridge fLMCP + bind Named Pipe + accept loop.
async fn serve(
    config: Config,
    token: Token,
    audit: Arc<AuditLog>,
) -> Result<(), HereticError> {
    // 1. Cliente file-RPC del FL Heretic Bridge.
    let bridge_config = BridgeConfig {
        script_dir: config.script_dir.clone(),
        timeout: std::time::Duration::from_secs(10),
    };
    let bridge = FlBridge::with_config(bridge_config)
        .map_err(|e| HereticError::Other(format!("creando FL Heretic Bridge: {e}")))?;

    // El wake por MIDI necesita los puertos OUT ABIERTOS de forma persistente.
    // Abrirlos y cerrarlos en cada peticion hacia que FL no reciba nada y el
    // pump no se dispare nunca (medido: pump_count se queda en 0).
    let midi_ports = heretic_fl::midi::open_all();
    tracing::info!("  MIDI out:   {midi_ports} puerto(s) abiertos (wake)");

    if config.wait_for_bridge {
        tracing::info!("verificando FL Heretic Bridge (meta.ping)...");
        match bridge.ping().await {
            Ok(info) => {
                tracing::info!(
                    "  bridge OK: v={} fl={} uptime={}s",
                    info.bridge_version, info.fl_version, info.uptime_sec
                );
            }
            Err(e) => {
                tracing::error!("  bridge no responde: {e}");
                return Err(HereticError::Other(format!(
                    "FL Heretic Bridge no responde: {e}.\n\
                     ¿Está FL Studio abierto con el controller script \
                     'FL Heretic Bridge' seleccionado en Options > MIDI Settings?"
                )));
            }
        }
    }

    let transport = Arc::new(Transport::new(bridge.clone()));
    let lifecycle = Arc::new(Lifecycle::new(bridge));

    // 2. Bind Named Pipe
    let mut server = ServerOptions::new()
        .create(&config.pipe_name)
        .map_err(|e| HereticError::Other(format!("create named pipe {}: {e}", config.pipe_name)))?;
    tracing::info!("Named Pipe server bound: {}", config.pipe_name);

    // 3. Accept loop
    loop {
        if let Err(e) = server.connect().await {
            tracing::error!("accept error: {e}");
            return Err(HereticError::Io(e));
        }
        // Tras connect(), el MISMO server queda listo para I/O.
        let client = server;
        server = ServerOptions::new()
            .create(&config.pipe_name)
            .map_err(|e| HereticError::Other(format!("recreate named pipe: {e}")))?;
        let verifier = Arc::new(AuthVerifier::new(token.clone()));
        let audit = audit.clone();
        let transport = transport.clone();
        let lifecycle = lifecycle.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(client, verifier, audit, transport, lifecycle).await {
                tracing::warn!("client disconnected: {e}");
            }
        });
    }
}

/// Maneja una conexión: handshake + dispatch loop.
async fn handle_client(
    client: NamedPipeServer,
    verifier: Arc<AuthVerifier>,
    audit: Arc<AuditLog>,
    transport: Arc<Transport>,
    lifecycle: Arc<Lifecycle>,
) -> Result<(), HereticError> {
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half);

    // 1. Enviar AuthChallenge
    let challenge = AuthChallenge::new();
    let challenge_json = serde_json::to_string(&json!({
        "type": "auth_challenge",
        "data": &challenge,
    }))? + "\n";
    write_half.write_all(challenge_json.as_bytes()).await?;
    write_half.flush().await?;

    // 2. Leer AuthResponse
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Err(HereticError::Other("client disconnected before auth".into()));
    }
    let auth_envelope: Value = serde_json::from_str(line.trim())?;
    let auth_resp: AuthResponse = serde_json::from_value(
        auth_envelope
            .get("data")
            .ok_or_else(|| HereticError::AuthFailed("missing data field".into()))?
            .clone(),
    )?;
    verifier.verify_handshake(&challenge, &auth_resp)?;

    // 3. Enviar AuthAck
    let ack = json!({"type": "auth_ack", "ok": true, "protocol_version": heretic_core::PROTOCOL_VERSION}).to_string() + "\n";
    write_half.write_all(ack.as_bytes()).await?;
    write_half.flush().await?;

    // 4. Dispatch loop
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(()); // EOF limpio
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::from_heretic("?", &HereticError::Json(e));
                send_response(&mut write_half, &resp).await?;
                continue;
            }
        };

        // Dispatch (daemon-level o transport)
        let started = std::time::Instant::now();
        let response = dispatch(&request, &transport, &lifecycle).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        // Audit log
        let status = match &response.outcome {
            heretic_core::protocol::Outcome::Success { .. } => AuditStatus::Ok,
            heretic_core::protocol::Outcome::Error { error } => {
                if error.code == -32001 || error.code == -32002 {
                    AuditStatus::Denied
                } else {
                    AuditStatus::Error
                }
            }
        };
        let mut event = AuditEvent::builder(request.tool_name())
            .status(status)
            .duration_ms(duration_ms)
            .params(&request.params_or_empty());
        if let heretic_core::protocol::Outcome::Error { error } = &response.outcome {
            event = event.error(&error.message);
        }
        if let Err(e) = audit.append(event.build()) {
            tracing::warn!("audit append failed: {e}");
        }

        send_response(&mut write_half, &response).await?;
    }
}

async fn send_response<W: tokio::io::AsyncWrite + Unpin>(
    write_half: &mut W,
    resp: &Response,
) -> Result<(), HereticError> {
    let s = serde_json::to_string(resp)? + "\n";
    write_half.write_all(s.as_bytes()).await?;
    write_half.flush().await?;
    Ok(())
}

/// Nombres de metodo que son de lifecycle (control del proceso de FL), no
/// de FL Studio. Se comprueban ANTES que los de transport para que `status`,
/// por ejemplo, no choque con el `status` del transporte.
const LIFECYCLE_METHODS: &[&str] = &[
    "open", "launch", "save", "save_as", "close", "fl_status", "wait_ready",
];

/// Dispatch un Request al handler apropiado.
async fn dispatch(req: &Request, transport: &Transport, lifecycle: &Lifecycle) -> Response {
    let name = req.tool_name();
    let result = if LIFECYCLE_METHODS.contains(&name) {
        let method = name.strip_prefix("fl_").unwrap_or(name);
        lifecycle.dispatch(method, req.params_or_empty()).await
    } else {
        transport.dispatch(name, req.params_or_empty()).await
    };
    match result {
        Ok(data) => Response::ok(req.id.clone(), data),
        Err(e) => Response::from_heretic(&req.id, &e),
    }
}