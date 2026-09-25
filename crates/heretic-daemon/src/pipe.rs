//! Named Pipe server + JSON-RPC dispatch (Fase 1).
//!
//! Topología:
//!
//! ```text
//! [cliente MCP]  --Named Pipe JSON-RPC NDJSON-->  [este server]
//!                                                        │
//!                                                        ├─> AuthChallenge + AuthVerifier (HMAC)
//!                                                        ├─> AuditLog (SQLite WAL append-only)
//!                                                        └─> Dispatcher → handlers (ping, ...)
//! ```
//!
//! ## Fase 1: solo `ping`
//!
//! Implementa:
//! 1. Bind Named Pipe `\\.\pipe\fl-heretic-<pid>`.
//! 2. Accept connections en loop.
//! 3. Por cada conexión: enviar `AuthChallenge`, esperar `AuthResponse`.
//! 4. Si OK: marcar conexión autenticada.
//! 5. Loop de requests: leer línea (NDJSON), dispatch, escribir response.
//!
//! Handlers implementados: `ping` (eco del daemon), `health` (estado).
//!
//! ## Multi-cliente
//!
//! Named Pipe en Windows permite múltiples instancias del mismo nombre.
//! Cada conexión corre en su propio task. Aislamos por canal (cada conexión
//! tiene su propio AuthVerifier — todos comparten el MISMO Token).
//!
//! ## Fase 2+
//!
//! Añadir: rate limit por tool, capability ACL, circuit breaker, watchdog.

use std::path::PathBuf;
use std::sync::Arc;

use heretic_core::{
    AuditEvent, AuditLog, AuditStatus, AuthChallenge, AuthResponse, AuthVerifier, HereticError,
    Request, Response, Token, TokenStore,
};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// Config del daemon.
#[derive(Debug, Clone)]
pub struct Config {
    pub pipe_name: String,
    pub token_path: PathBuf,
    pub audit_path: PathBuf,
}

impl Config {
    pub fn from_opts(pipe: Option<String>, token: Option<String>, audit: Option<String>) -> Self {
        Self {
            pipe_name: pipe.unwrap_or_else(|| default_pipe_name()),
            token_path: token
                .map(PathBuf::from)
                .unwrap_or_else(TokenStore::default_path),
            audit_path: audit
                .map(PathBuf::from)
                .unwrap_or_else(AuditLog::default_path),
        }
    }
}

/// Pipe name default: `\\.\pipe\fl-heretic-<pid>` (single-instance seguro).
pub fn default_pipe_name() -> String {
    let pid = std::process::id();
    format!(r"\\.\pipe\fl-heretic-{}", pid)
}

/// Arranca el daemon (loop infinito).
pub fn run(
    pipe: Option<String>,
    token_path: Option<String>,
    audit_path: Option<String>,
) -> Result<(), HereticError> {
    let config = Config::from_opts(pipe, token_path, audit_path);

    // Tracing del arranque
    tracing::info!("daemon arrancando");
    tracing::info!("  pipe:    {}", config.pipe_name);
    tracing::info!("  token:   {}", config.token_path.display());
    tracing::info!("  audit:   {}", config.audit_path.display());

    // Cargar token (crear si no existe)
    let store = TokenStore::new(config.token_path.clone());
    let token = store.load_or_create()?;
    tracing::info!("  token cargado ({} chars)", token.as_str().len());

    // Audit log
    let audit = Arc::new(AuditLog::open(config.audit_path.clone())?);
    tracing::info!("  audit log abierto ({} eventos previos)", audit.len().unwrap_or(0));

    // Tokio runtime — el binario debe correr dentro de un runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("fl-heretic")
        .build()
        .map_err(|e| HereticError::Other(format!("creando runtime tokio: {e}")))?;

    rt.block_on(async move {
        serve(config, token, audit).await
    })
}

/// Tokio async: bind + accept loop.
#[cfg(windows)]
async fn serve(
    config: Config,
    token: Token,
    audit: Arc<AuditLog>,
) -> Result<(), HereticError> {
    let mut server = ServerOptions::new()
        .create(&config.pipe_name)
        .map_err(|e| HereticError::Other(format!("create named pipe {}: {e}", config.pipe_name)))?;
    tracing::info!("Named Pipe server bound: {}", config.pipe_name);

    // Loop de accept
    loop {
        if let Err(e) = server.connect().await {
            tracing::error!("accept error: {e}");
            return Err(HereticError::Io(e));
        }
        // Tras connect(), el MISMO server queda listo para I/O.
        // Lo movemos al task del cliente. Creamos uno nuevo para aceptar el siguiente.
        let client = server;
        server = ServerOptions::new()
            .create(&config.pipe_name)
            .map_err(|e| HereticError::Other(format!("recreate named pipe: {e}")))?;
        let verifier = Arc::new(AuthVerifier::new(token.clone()));
        let audit = audit.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(client, verifier, audit).await {
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
        "daemon solo soporta Windows (Named Pipes)".into(),
    ))
}

/// Maneja una conexión: handshake + dispatch loop.
async fn handle_client(
    client: NamedPipeServer,
    verifier: Arc<AuthVerifier>,
    audit: Arc<AuditLog>,
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
    let auth_envelope: serde_json::Value = serde_json::from_str(line.trim())?;
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
        // Parsear request
        let request: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::from_heretic("?", &HereticError::Json(e));
                send_response(&mut write_half, &resp).await?;
                continue;
            }
        };

        // Dispatch
        let started = std::time::Instant::now();
        let response = dispatch(&request, &verifier).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        // Audit log
        let status = match &response.outcome {
            heretic_core::protocol::Outcome::Success { .. } => AuditStatus::Ok,
            heretic_core::protocol::Outcome::Error { error } => {
                // Auth errors → Denied. Otros → Error.
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

        // Enviar response
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

/// Dispatch un Request a su handler. Por ahora solo `ping` y `health`.
async fn dispatch(req: &Request, _verifier: &AuthVerifier) -> Response {
    match req.tool_name() {
        "ping" => Response::ok(
            req.id.clone(),
            json!({
                "pong": true,
                "protocol_version": heretic_core::PROTOCOL_VERSION,
                "crate_version": heretic_core::CRATE_VERSION,
            }),
        ),
        "health" => Response::ok(
            req.id.clone(),
            json!({
                "alive": true,
                "daemon_pid": std::process::id(),
                "uptime_s": 0, // TODO: tracking real
            }),
        ),
        other => Response::err(
            req.id.clone(),
            heretic_core::protocol::ProtocolError {
                code: -32601,
                message: format!("Method not found: {other}"),
                data: None,
            },
        ),
    }
}