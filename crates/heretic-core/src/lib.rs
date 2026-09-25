//! `heretic-core` — tipos compartidos, error, auth, audit, protocol para FL Heretic MCP.
//!
//! Este crate NO tiene dependencias específicas de FL Studio ni de MIDI.
//! Define los cimientos del daemon blindado: error tipado, auth HMAC,
//! audit log SQLite append-only, y el envelope JSON-RPC sobre Named Pipe.
//!
//! ## Arquitectura
//!
//! ```text
//! heretic-core
//! ├── error       → HereticError enum (thiserror)
//! ├── auth        → Bearer token + HMAC-SHA256 + TokenStore
//! ├── audit       → SQLite WAL append-only + retention
//! ├── protocol    → JSON-RPC 2.0 envelope sobre Named Pipe
//! ├── ratelimit   → token bucket por tool (Fase 5)
//! ├── circuit     → circuit breaker (Fase 5)
//! └── acl         → capability ACL (Fase 5)
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]
#![warn(rust_2024_compatibility)]

pub mod error;
pub mod auth;
pub mod audit;
pub mod protocol;

pub use error::{HereticError, Result};
pub use auth::{Token, TokenStore, AuthChallenge, AuthResponse, AuthVerifier};
pub use audit::{AuditEvent, AuditLog, AuditStatus};
pub use protocol::{
    Request, Response, ProtocolError,
    RequestId, ToolName, ToolParams, ToolResult, Handshake,
};

/// Versión del protocolo (incrementar en cambios incompatibles).
pub const PROTOCOL_VERSION: u32 = 1;

/// Versión de este crate.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");