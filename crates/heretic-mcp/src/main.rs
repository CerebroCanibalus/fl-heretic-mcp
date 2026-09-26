//! `fl-heretic-mcp` — binario MCP server (FlojoMCP, stdio NDJSON).
//!
//! Punto de entrada: arranca el server FlojoMCP que habla JSON-RPC sobre stdio
//! y reenvía cada tool call al daemon blindado via Named Pipe.

use flojo_mcp::prelude::*;
use tracing_subscriber::EnvFilter;

// Re-export para forzar el registro de los `#[tool]` (inventory)
// (Comentario: el `use` no es estrictamente necesario — `inventory::collect!`
//  en FlojoMCP escanea el binario. Pero referenciar el módulo asegura
//  que las funciones no se eliminen por optimización.)
#[allow(unused_imports)]
use heretic_mcp::tools as _tools;

/// Struct vacío que FlojoMCP descubre automáticamente las tools con `#[tool]`.
#[flojo_mcp(name = "daw-heretic-mcp", version = "0.1.0")]
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
        "daw-heretic-mcp {} arrancando",
        env!("CARGO_PKG_VERSION"),
    );

    flojo_run_stdio(FlHereticMcpServer::new()).await?;
    Ok(())
}