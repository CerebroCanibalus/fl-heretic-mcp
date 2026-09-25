//! Tipos de error para FL Heretic MCP.
//!
//! `HereticError` es el error canónico que cruza todas las capas del daemon.
//! Cada variante lleva contexto suficiente para que el cliente sepa qué hacer.

use std::path::PathBuf;
use thiserror::Error;

/// Resultado con error tipado de FL Heretic.
pub type Result<T> = std::result::Result<T, HereticError>;

/// Errores del daemon blindado.
#[derive(Debug, Error)]
pub enum HereticError {
    /// Fallo de I/O (filesystem, sockets, named pipes).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Error de serialización / deserialización JSON.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// SQLite / audit log error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Poison error del Mutex (un task panicó sosteniendo el lock).
    #[error("mutex poisoned: {0}")]
    Poisoned(String),

    /// Token no encontrado, expirado, o path inválido.
    #[error("token error: {0}")]
    Token(String),

    /// HMAC inválido (firma no verifica) o payload manipulado.
    #[error("auth failed: {0}")]
    AuthFailed(String),

    /// Rate limit excedido para una tool específica.
    #[error("rate limited: tool={tool}, retry_after_ms={retry_after_ms}")]
    RateLimited {
        tool: String,
        retry_after_ms: u64,
    },

    /// Circuit breaker abierto (FL no responde).
    #[error("circuit open: {0}")]
    CircuitOpen(String),

    /// Permiso denegado por el capability ACL.
    #[error("acl denied: tool={0}")]
    AclDenied(String),

    /// Audit log operation failed (genérico, para rotación etc.).
    #[error("audit log error: {0}")]
    Audit(String),

    /// Comando desconocido o malformado.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Tool específica del daemon devolvió un error (con código para el cliente).
    #[error("tool error [{code}]: {message}")]
    Tool {
        code: String,
        message: String,
    },

    /// Estado inconsistente (ej: sesión sin daemon, snapshot corrupto).
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// Recurso no encontrado.
    #[error("not found: {0}")]
    NotFound(String),

    /// Otro error con contexto.
    #[error("{0}")]
    Other(String),
}

impl HereticError {
    /// Código de error para serialización JSON-RPC.
    pub fn code(&self) -> String {
        match self {
            HereticError::Io(_)              => "io_error".into(),
            HereticError::Json(_)            => "json_error".into(),
            HereticError::Sqlite(_)          => "sqlite_error".into(),
            HereticError::Poisoned(_)        => "mutex_poisoned".into(),
            HereticError::Token(_)           => "token_error".into(),
            HereticError::AuthFailed(_)      => "auth_failed".into(),
            HereticError::RateLimited { .. } => "rate_limited".into(),
            HereticError::CircuitOpen(_)     => "circuit_open".into(),
            HereticError::AclDenied(_)       => "acl_denied".into(),
            HereticError::Audit(_)           => "audit_error".into(),
            HereticError::InvalidRequest(_)  => "invalid_request".into(),
            HereticError::Tool { code, .. }  => code.clone(),
            HereticError::InvalidState(_)    => "invalid_state".into(),
            HereticError::NotFound(_)        => "not_found".into(),
            HereticError::Other(_)           => "other".into(),
        }
    }

    /// ¿Es un error transitorio (cliente debería reintentar)?
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            HereticError::Io(_)
            | HereticError::RateLimited { .. }
            | HereticError::CircuitOpen(_)
            | HereticError::Audit(_)
        )
    }
}

/// Helper para construir errores desde paths.
pub fn path_error(path: impl Into<PathBuf>, msg: impl Into<String>) -> HereticError {
    HereticError::Other(format!("{}: {}", path.into().display(), msg.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        // Si cambias un código, es un breaking change del protocolo.
        assert_eq!(HereticError::Token("x".into()).code(), "token_error");
        assert_eq!(HereticError::AuthFailed("x".into()).code(), "auth_failed");
        assert_eq!(HereticError::AclDenied("fl_ping".into()).code(), "acl_denied");
    }

    #[test]
    fn retryable_classification() {
        assert!(HereticError::CircuitOpen("fl froze".into()).is_retryable());
        assert!(!HereticError::AuthFailed("bad hmac".into()).is_retryable());
        assert!(!HereticError::AclDenied("fl_ping".into()).is_retryable());
    }
}