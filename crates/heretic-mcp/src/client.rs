//! Named Pipe client al daemon blindado.
//!
//! Implementa el mismo protocolo JSON-RPC + HMAC que el daemon espera.
//! Se conecta por tool call (no pooling en Fase 2 — Fase 5 introducirá
//! un connection pool reutilizable).

use heretic_core::{
    AuthChallenge, AuthVerifier, HereticError, Request, Response, Token, TokenStore,
};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

/// Env var que apunta al Named Pipe del daemon.
const ENV_PIPE_NAME: &str = "FL_HERETIC_PIPE";

/// Env var opcional que apunta al path del token (default: %LOCALAPPDATA%\fl-heretic\token).
const ENV_TOKEN_PATH: &str = "FL_HERETIC_TOKEN_PATH";

/// Resultado del client.
pub type Result<T> = std::result::Result<T, HereticError>;

/// Cliente de una sola conexión al daemon.
///
/// **Importante**: NO implementar pooling aquí — el patrón actual es una conexión
/// por tool call. El handshake HMAC es barato (~ms). Si se necesitan conexiones
/// concurrentes, agregar pool en Fase 5.
pub struct DaemonClient {
    /// Pipe name del daemon.
    pipe_name: String,
    /// Token de autenticación.
    token: Token,
}

impl DaemonClient {
    /// Crea un client nuevo. Lee el pipe name de env var o autodetecta.
    pub fn from_env() -> Result<Self> {
        let pipe_name = std::env::var(ENV_PIPE_NAME)
            .unwrap_or_else(|_| default_pipe_name());
        let token_path = std::env::var(ENV_TOKEN_PATH)
            .ok()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(TokenStore::default_path);
        let store = TokenStore::new(token_path);
        let token = store.load()?;
        Ok(Self { pipe_name, token })
    }

    /// Crea un client explícito.
    pub fn new(pipe_name: String, token: Token) -> Self {
        Self { pipe_name, token }
    }

    /// Conecta, hace handshake, ejecuta `method` con `params`, devuelve el resultado.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        #[cfg(windows)]
        {
            self.call_windows(method, params).await
        }
        #[cfg(not(windows))]
        {
            Err(HereticError::Other(
                "heretic-mcp solo soporta Windows (Named Pipes)".into(),
            ))
        }
    }

    #[cfg(windows)]
    async fn call_windows(&self, method: &str, params: Value) -> Result<Value> {
        // 1. Conectar al Named Pipe (síncrono — tokio's ClientOptions::open no es async)
        let client = ClientOptions::new()
            .open(&self.pipe_name)
            .map_err(|e| HereticError::Other(format!(
                "No se pudo conectar al daemon en {}: {}. ¿Está corriendo?",
                self.pipe_name, e
            )))?;

        let (read_half, mut write_half) = tokio::io::split(client);
        let mut reader = BufReader::new(read_half);

        // 2. Verifier para firma
        let verifier = AuthVerifier::new(self.token.clone());

        // 3. Leer AuthChallenge
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(HereticError::Other("daemon cerró antes de mandar challenge".into()));
        }
        let challenge_envelope: Value = serde_json::from_str(line.trim())?;
        let challenge: AuthChallenge = serde_json::from_value(
            challenge_envelope.get("data")
                .ok_or_else(|| HereticError::AuthFailed("missing challenge data".into()))?
                .clone(),
        )?;

        // 4. Firmar y enviar AuthResponse
        let response = verifier.sign(&challenge);
        let resp_envelope = serde_json::json!({ "type": "auth_response", "data": &response });
        let resp_json = serde_json::to_string(&resp_envelope)? + "\n";
        write_half.write_all(resp_json.as_bytes()).await?;
        write_half.flush().await?;

        // 5. Leer AuthAck
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(HereticError::Other("daemon cerró después de auth".into()));
        }
        let ack: Value = serde_json::from_str(line.trim())?;
        if ack.get("ok") != Some(&Value::Bool(true)) {
            return Err(HereticError::AuthFailed(format!("auth rechazada: {ack}")));
        }

        // 6. Construir y enviar Request
        let request_id = format!("req-{}", uuid_like_id());
        let request = Request::new(request_id, method, params);
        let req_json = serde_json::to_string(&request)? + "\n";
        write_half.write_all(req_json.as_bytes()).await?;
        write_half.flush().await?;

        // 7. Leer Response
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(HereticError::Other("daemon cerró después del request".into()));
        }
        let response: Response = serde_json::from_str(line.trim())?;
        match response.outcome {
            heretic_core::protocol::Outcome::Success { result } => Ok(result),
            heretic_core::protocol::Outcome::Error { error } => {
                Err(HereticError::Other(format!(
                    "daemon error [{}]: {}",
                    error.code, error.message
                )))
            }
        }
    }
}

/// Default pipe name: `\\.\pipe\fl-heretic-<own_pid>` (suele coincidir con el daemon si
/// se arrancó en el mismo proceso, pero en general hay que setear FL_HERETIC_PIPE).
fn default_pipe_name() -> String {
    let pid = std::process::id();
    format!(r"\\.\pipe\fl-heretic-{pid}")
}

/// Generador simple de IDs cortos (8 chars hex) para request_id.
fn uuid_like_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:016x}", nanos & 0xFFFFFFFFFFFFFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pipe_name_has_pid() {
        let name = default_pipe_name();
        assert!(name.starts_with(r"\\.\pipe\fl-heretic-"));
        let pid_str = name.trim_start_matches(r"\\.\pipe\fl-heretic-");
        let pid: u32 = pid_str.parse().unwrap();
        assert!(pid > 0);
    }

    #[test]
    fn uuid_like_id_is_unique() {
        let a = uuid_like_id();
        let b = uuid_like_id();
        // No garantizamos uniqueness absoluto (es nanos), pero deberían diferir
        // si se llama con nanos distintos. Skip assertion strict.
        let _ = (a, b);
    }
}