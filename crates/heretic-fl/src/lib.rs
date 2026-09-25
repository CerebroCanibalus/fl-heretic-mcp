//! `heretic-fl` — bridge MIDI SysEx entre el daemon Rust y el controller script Python en FL Studio.
//!
//! ## Arquitectura
//!
//! ```text
//! [daemon (Rust)]                                       [FL Studio]
//!      │                                                      │
//!      │ MIDI SysEx (heretic-fl)                              │ Python controller script
//!      │ <----- requests -----  (loopMIDI / IAC Driver)       │ (legacy/fl_controller/...)
//!      │ ----- responses ---->                                │
//!      │ <----- heartbeats ---- (cada 500ms desde OnIdle)    │
//! ```
//!
//! El controller script Python NO se reescribe (sigue corriendo en el sandbox de FL Studio).
//! Lo que portamos es el lado Rust: descubrimiento de puertos, encoding/decoding SysEx,
//! request/response correlation, y heartbeat detection.
//!
//! ## Protocolo SysEx (mirror de `legacy/src/fl_studio_mcp/protocol.py`)
//!
//! ```text
//! F0 7D 4D 43 50 <dir> <id8> <base64_json> F7
//!  │  │  │  │  │   │    │      │
//!  │  │  │  │  │   │    │      └─ payload (UTF-8 JSON, base64-encoded)
//!  │  │  │  │  │   │    └─ 8 chars ASCII [a-z0-9] = request id
//!  │  │  │  │  │   └─ direction: 0x01=request, 0x02=response, 0x03=heartbeat
//!  │  │  │  │  └─ magic "MCP" (0x4D 0x43 0x50)
//!  │  │  │  └─ manufacturer 0x7D (private use per MIDI spec)
//!  │  │  └─ magic "C"
//!  │  └─ magic "M"
//!  └─ SysEx start
//!              └─ trailing SysEx end (added by mido on send, stripped on receive)
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod sysex;
pub mod midi;
pub mod heartbeat;
pub mod bridge;

pub use bridge::{FlBridge, BridgeConfig, FlVersionInfo, SongPosition};
pub use heartbeat::HeartbeatTracker;
pub use midi::{MidiPorts, open_midi_ports};
pub use sysex::{Direction, ProtocolVersion, encode_message, decode_message, new_request_id};

/// Versión de protocolo MIDI (mirror de `PROTOCOL_VERSION = 2` en legacy).
pub const MIDI_PROTOCOL_VERSION: u32 = 2;

/// Heartbeat esperado cada 500ms desde el controller script (`OnIdle`).
pub const HEARTBEAT_INTERVAL_MS: u64 = 500;

/// Consideramos FL muerto si no hay heartbeat en este tiempo.
pub const HEARTBEAT_STALE_MS: u64 = 3_000;

/// Default names de loopMIDI (Windows) / IAC Driver (macOS).
pub const DEFAULT_PORT_TO_FL: &str = "FLStudioMCP RX";
pub const DEFAULT_PORT_FROM_FL: &str = "FLStudioMCP TX";