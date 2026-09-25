//! Dispatcher de subcomandos del CLI (no JSON-RPC; solo `doctor` y `token`).

pub mod doctor;
pub mod token;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum TokenCmd {
    /// Genera un token nuevo y lo persiste (sobrescribe si existe).
    Generate,
    /// Rota el token (genera uno nuevo, persiste el anterior en backup).
    Rotate,
    /// Muestra el token actual.
    Show,
    /// Muestra el path del archivo de token.
    Path,
}