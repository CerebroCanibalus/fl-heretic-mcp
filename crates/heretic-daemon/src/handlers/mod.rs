//! Handlers JSON-RPC del daemon.
//!
//! Cada tool del MCP server (vía `fl-heretic-mcp`) se traduce a un método
//! JSON-RPC que este dispatcher atiende. Los handlers invocan el `FlBridge`
//! (heretic-fl) para hablar con FL Studio vía MIDI SysEx.
//!
//! ## Fase 2
//! Solo handlers transport (mirror del FLStudioMCP legacy):
//! - `ping`, `health`
//! - `get_tempo`, `set_tempo`
//! - `play`, `stop`, `get_play_state`
//! - `get_song_position`, `set_song_position`
//!
//! ## Fase 3 (próximo)
//! Port del resto de tools del legacy (mixer, channels, plugins, etc.).

pub mod transport;

pub use transport::Transport;