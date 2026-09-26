//! `heretic-fl` — cliente del FL Heretic Bridge (controller script de FL Studio).
//!
//! # Topología
//!
//! ```text
//! [MCP server] ──Named Pipe──> [daemon blindado] ──file-RPC──> [FL Heretic Bridge] ──FL API──> FL Studio
//!                                  (Rust)                 (2 ficheros JSON)
//! ```
//!
//! # Transporte
//!
//! Solo file-RPC. El bridge ofrece también un listener TCP en `127.0.0.1:9876`,
//! pero **no funciona en FL Studio 2025**: el sandbox del sub-intérprete de
//! Python no deja crear el objeto socket
//! (`<slot wrapper '__init__' of '_socket.socket' objects> returned NULL`), y el
//! propio bridge degrada a file-RPC. Medido: TCP 0/5, file-RPC 5/5 a ~42 ms
//! (`tests/test_transports.py`). El TCP no mejoraría la latencia de todos modos,
//! porque el bridge bombea ambos transportes desde el mismo `OnIdle()`.
//!
//! # Actions
//!
//! El bridge expone 67 actions (`bridge::ACTIONS`). Este crate da:
//! - `FlBridge::call(action, params)` para reacharlas todas.
//! - Helpers tipados (`play`, `set_tempo`, ...) para el camino caliente.

// `deny` en vez de `forbid` porque el modulo `process` necesita `unsafe` para
// la API Win32 (EnumWindows / PostMessageW). El `unsafe` esta acotado a ese
// modulo: el resto del crate es safe.
#![deny(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod bridge;
pub mod dialogs;
pub mod file_rpc;
pub mod midi;
pub mod process;
pub mod newproj;

pub use bridge::{
    default_script_dir, BridgeConfig, BridgeInfo, FlBridge, TransportStatus, ACTIONS,
};
pub use dialogs::{blocking_modal_class, close_welcome_wizard};
pub use file_rpc::FileRpc;
pub use process::{FlProcess, FlStatus, close, find_fl_exe, kill, launch, running_process, status};
pub use newproj::{
    Config, config_path, create as create_project_file, data_dir, default_projects_dir,
    find_template, load_config, save_config, set_template,
};

/// Versión de crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
