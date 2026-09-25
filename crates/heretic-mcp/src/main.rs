//! `fl-heretic-mcp` — binario MCP server (FlojoMCP, stdio NDJSON).
//!
//! Punto de entrada: arranca el server FlojoMCP que habla JSON-RPC sobre stdio
//! y reenvía cada tool call al daemon blindado via Named Pipe.
//!
//! ## Uso
//!
//! Configurar en `~/.config/opencode/opencode.jsonc` (o equivalente del MCP client):
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "fl-studio": {
//!       "command": "D:\\Mis Juegos\\ClaudeMCPs\\FLHereticMCP\\target\\release\\fl-heretic-mcp.exe",
//!       "env": {
//!         "FL_HERETIC_PIPE": "\\\\.\\pipe\\fl-heretic-12345",
//!         "RUST_LOG": "info"
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! El daemon publica su PID al arrancar en el log; el usuario lo copia al config
//! (Fase 5 introducirá auto-discovery via archivo well-known).

use flojo_mcp::prelude::*;
use tracing_subscriber::EnvFilter;

#[path = "../src/tools.rs"]
mod tools;

/// Struct vacío que FlojoMCP descubre automáticamente las tools con `#[tool]`.
#[flojo_mcp(name = "fl-heretic-mcp", version = "0.1.0")]
struct FlHereticMcpServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tracing a stderr (los clientes MCP no drenan stderr → cuidado con el volumen)
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();

    tracing::info!(
        "fl-heretic-mcp {} arrancando (daemon: {})",
        env!("CARGO_PKG_VERSION"),
        std::env::var("FL_HERETIC_PIPE").unwrap_or_else(|_| "<no FL_HERETIC_PIPE set>".into()),
    );

    flojo_run_stdio(FlHereticMcpServer::new()).await?;
    Ok(())
}