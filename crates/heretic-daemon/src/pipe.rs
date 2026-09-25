//! Named Pipe server + JSON-RPC dispatch (Fase 2).
//!
//! Flujo:
//! 1. Cargar token + audit log.
//! 2. Abrir bridge MIDI (heretic-fl) — abre loopMIDI ports + MIDI worker.
//! 3. Esperar primer heartbeat (FL está corriendo con controller script).
//! 4. Bind Named Pipe + accept loop.
//! 5. Por cada cliente: handshake HMAC + dispatch loop usando `Transport`.
//!
//! ## Handlers disponibles (Fase 2)
//!
//! | Método                 | Handler      | Notas                          |
//! |------------------------|--------------|--------------------------------|
//! | `ping`                 | daemon       | Eco simple, no requiere FL     |
//! | `health`               | daemon       | Estado del bridge MIDI         |
//! | `get_tempo`            | transport    | BPM actual                     |
//! | `set_tempo`            | transport    | Set BPM (10-999)               |
//! | `play`                 | transport    | transport.start()              |
//! | `stop`                 | transport    | transport.stop()               |
//! | `get_play_state`       | transport    | playing + recording            |
//! | `get_song_position`    | transport    | ms, ticks, beats, bpm           |
//! | `set_song_position`    | transport    | por ms, beats, o ticks         |
//!
//! Fase 3 añadirá el resto de tools del FLStudioMCP legacy.

use std::path::PathBuf;
use std::sync::Arc;

use heretic_core::{
    AuditEvent, AuditLog, AuditStatus, AuthChallenge, AuthResponse, AuthVerifier, HereticError,
    Request, Response, Token, TokenStore,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::handlers::Transport;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

#[cfg(windows)]
use heretic_fl::{BridgeConfig, FlBridge};

/// Config del daemon.
#[derive(Debug, Clone)]
pub struct Config {
    pub pipe_name: String,
    pub token_path: PathBuf,
    pub audit_path: PathBuf,
    /// Patrón del puerto MIDI output. Default: `FLStudioMCP RX`.
    pub midi_port_to_fl: Option<String>,
    /// Patrón del puerto MIDI input. Default: `FLStudioMCP TX`.
    pub midi_port_from_fl: Option<String>,
    /// Cliente MIDI name (visible en FL > MIDI Settings). Default: `FLHeretic`.
    pub midi_client_name: String,
    /// Esperar primer heartbeat antes de retornar de `run()`. Default: true.
    pub wait_for_heartbeat: bool,
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
            midi_port_to_fl: None,
            midi_port_from_fl: None,
            midi_client_name: "FLHeretic".into(),
            wait_for_heartbeat: true,
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
    tracing::info!("  pipe:    {}", config.pipe_name);
    tracing::info!("  token:   {}", config.token_path.display());
    tracing::info!("  audit:   {}", config.audit_path.display());
    tracing::info!("  midi client: {}", config.midi_client_name);

    // Cargar token (crear si no existe)
    let store = TokenStore::new(config.token_path.clone());
    let token = store.load_or_create()?;
    tracing::info!("  token cargado ({} chars)", token.as_str().len());

    // Audit log
    let audit = Arc::new(AuditLog::open(config.audit_path.clone())?);
    tracing::info!("  audit log abierto ({} eventos previos)", audit.len().unwrap_or(0));

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

/// Tokio async: abre MIDI bridge + bind Named Pipe + accept loop.
#[cfg(windows)]
async fn serve(
    config: Config,
    token: Token,
    audit: Arc<AuditLog>,
) -> Result<(), HereticError> {
    // 1. Abrir MIDI bridge
    let bridge_config = BridgeConfig {
        port_to_fl: config
            .midi_port_to_fl
            .clone()
            .unwrap_or_else(|| "FLStudioMCP RX".into()),
        port_from_fl: config
            .midi_port_from_fl
            .clone()
            .unwrap_or_else(|| "FLStudioMCP TX".into()),
        client_name: config.midi_client_name.clone(),
        default_timeout: std::time::Duration::from_secs(5),
        wait_for_first_heartbeat: true,  // el bridge espera internamente (5s)
    };
    let bridge = FlBridge::connect(bridge_config)
        .map_err(|e| HereticError::Other(format!("abriendo MIDI bridge: {e}")))?;

    if config.wait_for_heartbeat {
        tracing::info!("esperando primer heartbeat del controller script...");
        bridge
            .wait_ready()
            .await
            .map_err(|e| HereticError::Other(format!("FL no responde: {e}")))?;
        tracing::info!("FL alive — heartbeat recibido");
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

#[cfg(not(windows))]
async fn serve(
    _config: Config,
    _token: Token,
    _audit: Arc<AuditLog>,
) -> Result<(), HereticError> {
    Err(HereticError::Other(
        "daemon solo soporta Windows (Named Pipes + MIDI)".into(),
    ))
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
        auth_envelope.get("data")
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
    // Intentar transport primero (cubre todos los métodos MIDI)
    match transport.dispatch(req.tool_name(), req.params_or_empty()).await {
        Ok(data) => Response::ok(req.id.clone(), data),
        Err(e) => Response::from_heretic(&req.id, &e),
    }
}