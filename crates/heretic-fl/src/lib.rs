//! `heretic-fl` — cliente TCP Rust al VST3 plugin.
//!
//! ## Topología
//!
//! ```text
//! [daemon Rust] --TCP 127.0.0.1:9790--> [VST3 plugin] --file-RPC--> [FL Heretic Bridge] --FL API--> FL Studio
//! ```
//!
//! El VST3 plugin corre dentro de FL Studio y actúa como proxy TCP↔file-RPC.
//! Este crate es el cliente TCP que habla con él.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod bridge;

pub use bridge::{FlBridge, BridgeConfig, FlVersionInfo, SongPosition, TransportStatus};

/// Default host del VST3 plugin.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Default port del VST3 plugin (TCP server).
pub const DEFAULT_PORT: u16 = 9790;