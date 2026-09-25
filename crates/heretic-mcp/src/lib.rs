//! `heretic-mcp` — MCP server stdio (FlojoMCP) que reenvía comandos al daemon.
//!
//! ## Topología
//!
//! ```text
//! [OpenCode/Claude]
//!      │ stdio (JSON-RPC MCP)
//!      ▼
//! [fl-heretic-mcp (este crate)]  ←──── FlojoMCP server
//!      │ Named Pipe (JSON-RPC interno)
//!      ▼
//! [fl-heretic daemon]  ←──── heretic-fl bridge (MIDI SysEx)
//!      │
//!      ▼
//! [FL Studio]
//! ```
//!
//! Cada `#[tool]` aquí es un thin proxy: valida args, abre conexión al daemon,
//! hace handshake HMAC, envía request, devuelve response.
//!
//! Para Fase 2 hay 7 tools (transport). Fase 3 añadirá el resto (67 tools del FLStudioMCP).

#![forbid(unsafe_code)]

pub mod client;
pub mod tools;