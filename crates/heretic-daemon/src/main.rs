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
//! fl-heretic new-project "mi-cancion"       # crea proyecto nuevo y lo abre
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
        /// Path del script dir del fLMCP Bridge (default: %USERPROFILE%\Documents\Image-Line\FL Studio\Settings\Hardware\fLMCP Bridge).
        #[arg(long)]
        script_dir: Option<String>,
    },
    /// Arranca el MCP server (stdio). STUB en Fase 1, implementación en Fase 2.
    Mcp {
        #[arg(long)]
        daemon_pipe: Option<String>,
    },
    /// Crea un proyecto nuevo en la carpeta de FL y lo abre.
    ///
    /// Copia una plantilla .flp al destino y lanza FL con ella. Es la via
    /// determinista: el dialogo de "Save as" de FL no se puede automatizar
    /// porque sus campos son controles Delphi internos que cierran sin
    /// guardar, y la FL Python API no expone la API de proyecto.
    NewProject {
        /// Nombre del proyecto (sin .flp).
        name: String,
        /// Carpeta destino. Por defecto, la de FL.
        #[arg(long)]
        dir: Option<String>,
        /// .flp de partida. Por defecto, el mas reciente de la carpeta de FL.
        #[arg(long)]
        template: Option<String>,
        /// No abrir en FL (solo crear el fichero).
        #[arg(long)]
        no_open: bool,
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
        Cmd::Daemon { pipe, token_path, audit_path, script_dir: _ } => {
            // Por ahora: el script_dir se detecta automáticamente. En Fase 3 se pasa via flag.
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
        Cmd::NewProject { name, dir, template, no_open } => {
            let dirp = dir.map(std::path::PathBuf::from);
            let tpl = template.map(std::path::PathBuf::from);
            let r = heretic_fl::create_project_file(
                &name,
                dirp.as_deref(),
                tpl.as_deref(),
            );
            match r {
                Ok(path) => {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    println!("creado: {} ({} bytes)", path.display(), size);
                    if no_open {
                        return ExitCode::SUCCESS;
                    }
                    match heretic_fl::launch(Some(&path), 25) {
                        Ok(p) => {
                            println!("FL Studio lanzado, pid {}", p.pid);
                            println!("esperando al bridge...");
                            for _ in 0..60 {
                                if heretic_fl::running_process().is_some() {
                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                                break;
                            }
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("no se pudo abrir en FL: {e}");
                            ExitCode::from(1)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("new-project error: {e}");
                    ExitCode::from(1)
                }
            }
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