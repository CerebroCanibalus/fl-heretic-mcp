//! Handlers JSON-RPC del daemon.
//!
//! Cada tool del MCP server (vía `fl-heretic-mcp`) se traduce a un método
//! JSON-RPC que este dispatcher atiende. Hay dos familias:
//!
//! ## `Transport` — operaciones dentro de FL Studio
//!
//! Van al bridge por file-RPC (con wake MIDI). El escape hatch `call` da
//! acceso a las 67 actions del bridge sin escribir un handler por una.
//!
//! - `ping`, `health`
//! - `get_tempo`, `set_tempo`
//! - `play`, `stop`, `get_play_state`
//! - `get_song_position`, `set_song_position`
//! - `call`, `actions`
//!
//! ## `Lifecycle` — el proceso de FL Studio y el proyecto
//!
//! NO van por el bridge, porque el sandbox del script de FL no expone la API
//! de proyecto: no existen `general.saveProject` ni `FPT_Open` / `FPT_Close`.
//! El daemon controla el proceso directamente.
//!
//! - `open`, `launch`   -> `CreateProcess` con el `.flp`
//! - `save`, `save_as`  -> bridge (`FPT_Save` / `FPT_SaveNew`)
//! - `close`            -> `WM_CLOSE` a la ventana de FL
//! - `status`, `wait_ready`

pub mod lifecycle;
pub mod transport;

pub use lifecycle::Lifecycle;
pub use transport::Transport;
