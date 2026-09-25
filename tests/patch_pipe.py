#!/usr/bin/env python3
"""Reescribe pipe.rs del daemon para la API file-RPC + modulo de proceso."""
import io
import re
import sys

PATH = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\pipe.rs"

src = io.open(PATH, encoding="utf-8").read()

# ---- 1. Cabecera del modulo: documentacion nueva ----
old_head = src[:src.index("use std::path::PathBuf;")]
new_head = '''//! Named Pipe server + JSON-RPC dispatch.
//!
//! Topologia completa:
//!
//! ```text
//! [MCP server] --Named Pipe+HMAC--> [daemon] --file-RPC--> [FL Heretic Bridge] --FL API--> FL Studio
//!                                       |                                              ^
//!                                       +--MIDI out (wake)--------------------------+
//!                                       +--WM_CLOSE / CreateProcess (proceso FL)----+
//! ```
//!
//! El daemon tiene dos vias hacia FL Studio, porque el sandbox del script de FL
//! no permite nada de lo que hace falta para gestionar proyectos:
//!
//! | Necesidad | Via | Por que
//! |---|---|---|
//! | Guardar proyecto | bridge (`FPT_Save`) | el script si puede ejecutar el atajo
//! | Abrir proyecto   | daemon (`CreateProcess`) | `dir(general)` no tiene nada de abrir
//! | Cerrar proyecto  | daemon (`WM_CLOSE`)    | no hay `FPT_Close`
//! | Despertar FL     | daemon (MIDI out)      | `OnIdle` no se dispara; el pump real es `OnMidiIn`
//!
//! ## Handlers disponibles
//!
//! | Metodo              | Handler      | Action del bridge           |
//! |---------------------|--------------|-----------------------------|
//! | `ping`              | transport    | `meta.ping`                 |
//! | `health`            | transport    | ping real                   |
//! | `get_tempo`         | transport    | `transport.status`          |
//! | `set_tempo`         | transport    | `transport.setTempo`        |
//! | `play`              | transport    | `transport.start`           |
//! | `stop`              | transport    | `transport.stop`            |
//! | `get_play_state`    | transport    | `transport.status`          |
//! | `get_song_position` | transport    | `transport.status`          |
//! | `set_song_position` | transport    | `transport.setPosition`     |
//! | `call`              | transport    | cualquier action           |
//! | `fl_open`           | lifecycle    | `CreateProcess`             |
//! | `fl_close`          | lifecycle    | `WM_CLOSE`                  |
//! | `fl_status`         | lifecycle    | proceso + ping              |

'''
src = new_head + src[src.index("use std::path::PathBuf;"):]

# ---- 2. Config: recuperar script_dir ----
old_cfg = src[src.index("pub struct Config {"):src.index("/// Pipe name default:")]
new_cfg = '''pub struct Config {
    pub pipe_name: String,
    pub token_path: PathBuf,
    pub audit_path: PathBuf,
    /// Directorio del FL Heretic Bridge (donde vive el controller script).
    pub script_dir: PathBuf,
    /// Si true, verifica el bridge con meta.ping antes de aceptar clientes.
    pub wait_for_bridge: bool,
}

impl Config {
    pub fn from_opts(pipe: Option<String>, token: Option<String>, audit: Option<String>) -> Self {
        Self {
            pipe_name: pipe.unwrap_or_else(default_pipe_name),
            token_path: token
                .map(PathBuf::from)
                .unwrap_or_else(TokenStore::default_path),
            audit_path: audit
                .map(PathBuf::from)
                .unwrap_or_else(AuditLog::default_path),
            script_dir: heretic_fl::default_script_dir(),
            wait_for_bridge: true,
        }
    }
}

'''
src = src.replace(old_cfg, new_cfg)

# ---- 3. Log de arranque: anadir bridge + midi ----
src = src.replace(
    '    tracing::info!("  audit:      {}", config.audit_path.display());',
    '    tracing::info!("  audit:      {}", config.audit_path.display());\n'
    '    tracing::info!("  bridge:     {}", config.script_dir.display());'
)

# ---- 4. serve(): construir el bridge file-RPC ----
start = src.index("    // 1. Crear bridge fLMCP")
end = src.index("    let transport = Arc::new(Transport::new(bridge));")
new_serve = '''    // 1. Cliente file-RPC del FL Heretic Bridge.
    let bridge_config = BridgeConfig {
        script_dir: config.script_dir.clone(),
        timeout: std::time::Duration::from_secs(10),
    };
    let bridge = FlBridge::with_config(bridge_config)
        .map_err(|e| HereticError::Other(format!("creando FL Heretic Bridge: {e}")))?;

    // El wake por MIDI necesita los puertos OUT ABIERTOS de forma persistente.
    // Abrirlos y cerrarlos en cada peticion hacia que FL no reciba nada y el
    // pump no se dispare nunca (medido: pump_count se queda en 0).
    let midi_ports = heretic_fl::midi::open_all();
    tracing::info!("  MIDI out:   {midi_ports} puerto(s) abiertos (wake)");

    if config.wait_for_bridge {
        tracing::info!("verificando FL Heretic Bridge (meta.ping)...");
        match bridge.ping().await {
            Ok(info) => {
                tracing::info!(
                    "  bridge OK: v={} fl={} uptime={}s",
                    info.bridge_version, info.fl_version, info.uptime_sec
                );
            }
            Err(e) => {
                tracing::error!("  bridge no responde: {e}");
                return Err(HereticError::Other(format!(
                    "FL Heretic Bridge no responde: {e}.\\n\\
                     ¿Está FL Studio abierto con el controller script \\
                     'FL Heretic Bridge' seleccionado en Options > MIDI Settings?"
                )));
            }
        }
    }

'''
src = src[:start] + new_serve + src[end:]

io.open(PATH, "w", encoding="utf-8", newline="\n").write(src)
print("pipe.rs reescrito: %d lineas" % len(src.splitlines()))
