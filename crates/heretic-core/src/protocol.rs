//! JSON-RPC 2.0 envelope sobre Named Pipe (NDJSON framing).
//!
//! El daemon y el MCP server hablan JSON-RPC 2.0 sobre un Named Pipe de Windows
//! con framing NDJSON (un objeto JSON por línea terminada en `\n`).
//!
//! ## Request
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": "abc123",
//!   "method": "fl_ping",
//!   "params": { ... }
//! }
//! ```
//!
//! ## Response (success)
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": "abc123",
//!   "result": { ... }
//! }
//! ```
//!
//! ## Response (error)
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": "abc123",
//!   "error": { "code": -32601, "message": "Method not found", "data": { ... } }
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::HereticError;

/// ID de un request (string opaco para correlación).
pub type RequestId = String;

/// Nombre de una tool (ej: `fl_ping`, `fl_set_mixer_volume`).
pub type ToolName = String;

/// Parámetros de una tool (objeto JSON arbitrario).
pub type ToolParams = Value;

/// Resultado de una tool (objeto JSON arbitrario).
pub type ToolResult = Value;

/// Request JSON-RPC 2.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<ToolParams>,
    /// Timestamp unix (segundos) — parte de la firma HMAC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    /// Firma HMAC del payload — hex-encoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Request {
    pub fn new(id: impl Into<String>, method: impl Into<String>, params: ToolParams) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: id.into(),
            method: method.into(),
            params: Some(params),
            timestamp: None,
            signature: None,
        }
    }

    /// Nombre de la tool (= method).
    pub fn tool_name(&self) -> &str {
        &self.method
    }

    /// Params como Value (default = objeto vacío).
    pub fn params_or_empty(&self) -> ToolParams {
        self.params.clone().unwrap_or(Value::Object(Default::default()))
    }
}

/// Response JSON-RPC 2.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// Resultado de un request: success (result) o error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Success { result: ToolResult },
    Error { error: ProtocolError },
}

impl Response {
    pub fn ok(id: impl Into<String>, result: ToolResult) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: id.into(),
            outcome: Outcome::Success { result },
        }
    }

    pub fn err(id: impl Into<String>, error: ProtocolError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: id.into(),
            outcome: Outcome::Error { error },
        }
    }

    pub fn from_heretic(id: impl Into<String>, err: &HereticError) -> Self {
        Self::err(
            id,
            ProtocolError {
                code: jsonrpc_code(err),
                message: err.to_string(),
                data: None,
            },
        )
    }
}

/// Error JSON-RPC 2.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::enum_variant_names)]
pub struct ProtocolError {
    /// Código de error JSON-RPC estándar (-32600 a -32603) o custom.
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Convierte un HereticError a un código JSON-RPC estándar + custom cuando aplica.
fn jsonrpc_code(err: &HereticError) -> i32 {
    use HereticError as H;
    match err {
        H::InvalidRequest(_) => -32600,  // Invalid Request
        H::Json(_)           => -32700,  // Parse error (per protocol spec)
        H::AuthFailed(_) | H::Token(_) => -32001, // custom: auth
        H::AclDenied(_)      => -32002, // custom: permission denied
        H::RateLimited { .. } => -32003, // custom: rate limit
        H::CircuitOpen(_)    => -32004, // custom: circuit open
        H::Tool { .. } => -32010, // custom: tool-specific (data.code = code)
        H::NotFound(_)       => -32020,
        _                    => -32603,  // Internal error
    }
}

/// Mensaje de bienvenida / handshake inicial (no JSON-RPC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    pub protocol_version: u32,
    pub server_version: String,
    pub challenge_nonce: String,
    pub challenge_timestamp: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_serialization() {
        let req = Request::new("req-1", "fl_ping", json!({}));
        let s = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.id, "req-1");
        assert_eq!(parsed.method, "fl_ping");
    }

    #[test]
    fn response_success_roundtrip() {
        let resp = Response::ok("req-1", json!({"pong": true}));
        let s = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&s).unwrap();
        match parsed.outcome {
            Outcome::Success { result } => assert_eq!(result["pong"], true),
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn response_error_roundtrip() {
        let resp = Response::err("req-1", ProtocolError {
            code: -32001,
            message: "auth failed".into(),
            data: None,
        });
        let s = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&s).unwrap();
        match parsed.outcome {
            Outcome::Error { error } => {
                assert_eq!(error.code, -32001);
                assert_eq!(error.message, "auth failed");
            }
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn jsonrpc_error_codes() {
        let auth = HereticError::AuthFailed("x".into());
        let acl = HereticError::AclDenied("fl_ping".into());
        let invalid = HereticError::InvalidRequest("x".into());
        assert_eq!(jsonrpc_code(&auth), -32001);
        assert_eq!(jsonrpc_code(&acl), -32002);
        assert_eq!(jsonrpc_code(&invalid), -32600);
    }
}