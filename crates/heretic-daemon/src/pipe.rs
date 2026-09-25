//! Named Pipe server + JSON-RPC dispatch (Fase 2.5).
//!
//! Adaptado al protocolo fLMCP Bridge (file-RPC + TCP). NO usa MIDI SysEx.
//!
//! Flujo:
//! 1. Cargar token + audit log.
//! 2. Crear `FlBridge` (fLMCP Bridge) — file-RPC + TCP con fallback.
//! 3. Health check: `meta.ping` para verificar que el bridge responde.
//! 4. Bind Named Pipe + accept loop.
//! 5. Por cada cliente: handshake HMAC + dispatch loop usando `Transport`.
//!
//! ## Handlers disponibles (Fase 2.5)
//!
//! | Método                 | Handler      | Action fLMCP Bridge    |
//! |------------------------|--------------|------------------------|
//! | `ping`                 | daemon       | (no requiere bridge)   |
//! | `health`               | daemon       | bridge.health()        |
//! | `get_tempo`            | transport    | `transport.status`     |
//! | `set_tempo`            | transport    | `transport.set_tempo`  |
//! | `play`                 | transport    | `transport.start`      |
//! | `stop`                 | transport    | `transport.stop`       |
//! | `get_play_state`       | transport    | `transport.status`     |
//! | `get_song_position`    | transport    | `transport.status`     |
//! | `set_song_position`    | transport    | `transport.set_position`|
//!
//! Fase 3 añadirá el resto de las 133 actions (mixer, channels, plugins, etc.)

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
use crate::handlers::Transport;

/// Config del daemon.
#[derive(Debug, Clone)]
pub struct Config {
    pub pipe_name: String,
    pub token_path: PathBuf,
    pub audit_path: PathBuf,
    /// Directorio del script fLMCP Bridge (donde está `device_FLStudioMCP.py`).
    /// Default: `%USERPROFILE%\Documents\Image-Line\FL Studio\Settings\Hardware\fLMCP Bridge`.
    pub script_dir: PathBuf,
    /// Si true, intenta TCP antes que file-RPC.
    pub try_tcp: bool,
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
            try_tcp: true,
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
    tracing::info!("  script_dir: {}", config.script_dir.display());

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
    // 1. Crear bridge fLMCP (file-RPC + TCP)
    let bridge_config = BridgeConfig {
        script_dir: config.script_dir.clone(),
        try_tcp: config.try_tcp,
        tcp_host: heretic_fl::DEFAULT_TCP_HOST.into(),
        tcp_port: heretic_fl::DEFAULT_TCP_PORT,
        timeout: std::time::Duration::from_secs(10),
    };
    let bridge = FlBridge::connect(bridge_config)
        .map_err(|e| HereticError::Other(format!("creando bridge fLMCP: {e}")))?;
    tracing::info!("  bridge: {} + {}", bridge.health().primary, bridge.health().fallback);

    if config.wait_for_bridge {
        tracing::info!("verificando conexión al fLMCP Bridge (meta.ping)...");
        match bridge.ping().await {
            Ok(info) => {
                tracing::info!(
                    "  fLMCP Bridge OK: bridge={} fl={} uptime={}s",
                    info.bridge_version, info.fl_version, info.uptime_sec
                );
            }
            Err(e) => {
                tracing::error!("  fLMCP Bridge no responde: {e}");
                return Err(HereticError::Other(format!(
                    "fLMCP Bridge no responde: {e}. ¿FL Studio está corriendo con el controller script instalado?"
                )));
            }
        }
    }

    let transport = Arc::new(Transport::new(bridge));

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
        tokio::spawn(async move {
            if let Err(e) = handle_client(client, verifier, audit, transport).await {
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
        let response = dispatch(&request, &transport).await;
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

/// Dispatch un Request al handler apropiado. Primero intenta transport; si no
/// es un método transport, usa el fallback del daemon (`ping`/`health`).
async fn dispatch(req: &Request, transport: &Transport) -> Response {
    match transport.dispatch(req.tool_name(), req.params_or_empty()).await {
        Ok(data) => Response::ok(req.id.clone(), data),
        Err(e) => Response::from_heretic(&req.id, &e),
    }
}