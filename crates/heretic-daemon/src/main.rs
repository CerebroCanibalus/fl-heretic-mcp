//! `fl-heretic` — binario principal con subcommand dispatcher.
//!
//! Subcommands (Fase 1):
//! - `mcp`     — arranca el MCP server (stdio) — STUB por ahora, Fase 2
//! - `daemon`  — arranca el daemon blindado sobre Named Pipe
//! - `doctor`  — diagnóstico del entorno
//! - `token`   — `generate | rotate | show | path`
//!
//! Uso:
//! ```text
//! fl-heretic daemon                 # arranca el daemon
//! fl-heretic token generate         # crea token nuevo
//! fl-heretic doctor                 # chequea entorno (puertos MIDI, FL alive, etc.)
//! ```

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod pipe;
mod commands;
mod handlers;
use commands::TokenCmd;

/// Binario principal de FL Heretic MCP.
#[derive(Debug, Parser)]
#[command(
    name = "fl-heretic",
    version,
    about = "Daemon blindado + MCP server para FL Studio sobre FlojoMCP",
    long_about = "FL Heretic MCP — Daemons blindados para un DAW amurallado."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Nivel de log override (info, debug, trace). Default: RUST_LOG o "info".
    #[arg(long, global = true, env = "RUST_LOG")]
    log: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Arranca el daemon blindado sobre Named Pipe.
    Daemon {
        /// Path del Named Pipe (default: `\\.\pipe\fl-heretic-<pid>`).
        #[arg(long)]
        pipe: Option<String>,
        /// Path del token (default: `%LOCALAPPDATA%\fl-heretic\token`).
        #[arg(long)]
        token_path: Option<String>,
        /// Path del audit log SQLite (default: `%LOCALAPPDATA%\fl-heretic\audit.db`).
        #[arg(long)]
        audit_path: Option<String>,
    },
    /// Arranca el MCP server (stdio). STUB en Fase 1, implementación en Fase 2.
    Mcp {
        #[arg(long)]
        daemon_pipe: Option<String>,
    },
    /// Diagnóstico del entorno (puertos MIDI, FL alive, token path, etc.).
    Doctor,
    /// Gestión del token Bearer.
    Token {
        #[command(subcommand)]
        action: TokenCmd,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Tracing init
    let filter = cli.log.as_deref().unwrap_or("info");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .with_target(false)
        .with_writer(std::io::stderr) // MCP server usa stderr para no contaminar stdout
        .try_init();

    tracing::info!(
        "fl-heretic {} (heretic-core v{}, protocolo v{})",
        env!("CARGO_PKG_VERSION"),
        heretic_core::CRATE_VERSION,
        heretic_core::PROTOCOL_VERSION,
    );

    match cli.cmd {
        Cmd::Daemon { pipe, token_path, audit_path } => {
            match pipe::run(pipe, token_path, audit_path) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("daemon error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Cmd::Mcp { daemon_pipe: _ } => {
            tracing::error!("`mcp` subcommand es STUB en Fase 1 — implementación en Fase 2");
            ExitCode::from(1)
        }
        Cmd::Doctor => match commands::doctor::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!("doctor error: {e}");
                ExitCode::from(1)
            }
        },
        Cmd::Token { action } => match commands::token::run(action) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!("token error: {e}");
                ExitCode::from(1)
            }
        },
    }
}