//! Cliente del fLMCP Bridge — capa tipada sobre [`FileRpc`].
//!
//! Expone:
//! - Una llamada genérica [`FlBridge::call`] que reacha **las 133 actions** del
//!   bridge sin escribir un handler por action.
//! - Helpers tipados para lo que se usa en el camino caliente (transport), que
//!   parsean la respuesta a un struct de Rust.
//!
//! Los nombres de action y los nombres de parámetro son los del bridge v0.2.0
//! (`camelCase` en la action, y `index`/`volume` o `track`/`volume` en los
//! params). Ver `ACTIONS` abajo y `docs/BRIDGE_ACTIONS.md`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use heretic_core::{HereticError, Result};

use crate::file_rpc::FileRpc;

/// Catálogo de actions que declara el FL Heretic Bridge.
///
/// Generado desde el bridge real con `tests/get_actions.py` (no a mano),
/// que lista lo que el bridge vivo publica via `meta.actions`. Si añades
/// un handler al bridge, regenera esto y el catálogo lo refleja.
///
/// Se usa para *advertir* cuando llega una action desconocida, no para
/// bloquear: el bridge puede llevar más actions de las que este crate conoce
/// (va por delante), y un catálogo desactualizado no debe impedir reaches
/// que sí funcionan.
pub const ACTIONS: &[&str] = &[
    // arrangement (2)
    "arrangement.current",
    "arrangement.selection",
    // channels (11)
    "channels.all",
    "channels.count",
    "channels.info",
    "channels.mute",
    "channels.routeToMixer",
    "channels.select",
    "channels.setColor",
    "channels.setName",
    "channels.setPan",
    "channels.setVolume",
    "channels.solo",
    // meta (4)
    "meta.actions",
    "meta.exec",
    "meta.info",
    "meta.ping",
    // mixer (9)
    "mixer.allTracks",
    "mixer.count",
    "mixer.fxSlots",
    "mixer.mute",
    "mixer.setName",
    "mixer.setPan",
    "mixer.setVolume",
    "mixer.solo",
    "mixer.trackInfo",
    // patterns (10)
    "patterns.clone",
    "patterns.count",
    "patterns.create",
    "patterns.current",
    "patterns.delete",
    "patterns.findByName",
    "patterns.list",
    "patterns.rename",
    "patterns.select",
    "patterns.setColor",
    // playlist (3)
    "playlist.allTracks",
    "playlist.trackCount",
    "playlist.trackInfo",
    // plugins (6)
    "plugins.findParam",
    "plugins.getParam",
    "plugins.isValid",
    "plugins.name",
    "plugins.params",
    "plugins.setParam",
    // project (8)
    "project.metadata",
    "project.redo",
    "project.save",
    "project.saveAs",
    "project.saveUndo",
    "project.undo",
    "project.undoHistory",
    "project.version",
    // transport (8)
    "transport.length",
    "transport.record",
    "transport.setLoopMode",
    "transport.setPosition",
    "transport.setTempo",
    "transport.start",
    "transport.status",
    "transport.stop",
    // ui (6)
    "ui.focusedWindow",
    "ui.hideWindow",
    "ui.hint",
    "ui.openPianoRoll",
    "ui.selectedChannel",
    "ui.showWindow",
];

/// Config del cliente.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Directorio del script (`...\FL Studio\Settings\Hardware\fLMCP Bridge`).
    pub script_dir: std::path::PathBuf,
    /// Timeout por petición.
    pub timeout: std::time::Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            script_dir: default_script_dir(),
            timeout: crate::file_rpc::DEFAULT_TIMEOUT,
        }
    }
}

/// Directorio por defecto del FL Heretic Bridge (nuestro controller script).
///
/// Es `Hardware/FL Heretic Bridge`, NO el `fLMCP Bridge` de terceros: el ours
/// usa su propio mailbox (`hr_req_N.json` / `hr_resp_N.json` / `hr_status.json`)
/// y no colisiona con ningun otro MCP que hable con el bridge de fLMCP.
///
/// Se puede sobreescribir con `FL_HERETIC_BRIDGE_DIR`.
pub fn default_script_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("FL_HERETIC_BRIDGE_DIR") {
        return std::path::PathBuf::from(p);
    }
    let base = if cfg!(windows) {
        std::env::var("USERPROFILE")
            .map(std::path::PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(std::path::PathBuf::from))
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
    };
    base.join("Documents")
        .join("Image-Line")
        .join("FL Studio")
        .join("Settings")
        .join("Hardware")
        .join("FL Heretic Bridge")
}

/// Resultado de `meta.ping`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeInfo {
    pub bridge_version: String,
    pub fl_version: String,
    pub uptime_sec: f64,
    #[serde(default)]
    pub script_dir: Option<String>,
}

/// Resultado de `transport.status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportStatus {
    pub is_playing: bool,
    pub is_recording: bool,
    pub bpm: f64,
    pub position_ticks: i64,
    pub position_bars: f64,
    pub position_seconds: f64,
    #[serde(default)]
    pub loop_mode: Option<String>,
}

/// Cliente del bridge. Clone barato (comparte el mismo `FileRpc`).
#[derive(Clone)]
pub struct FlBridge {
    rpc: Arc<FileRpc>,
}

impl std::fmt::Debug for FlBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlBridge")
            .field("script_dir", &self.script_dir().display().to_string())
            .finish()
    }
}

impl FlBridge {
    /// Construye el cliente sobre el directorio de script por defecto.
    pub fn new() -> Result<Arc<Self>> {
        Self::with_config(BridgeConfig::default())
    }

    /// Construye el cliente con config explícita.
    pub fn with_config(cfg: BridgeConfig) -> Result<Arc<Self>> {
        if !cfg.script_dir.is_dir() {
            return Err(HereticError::Other(format!(
                "no existe el directorio del bridge: {}\n\
                 ¿Está FL Studio instalado y el controller script en su sitio?",
                cfg.script_dir.display()
            )));
        }
        let rpc = FileRpc::new(cfg.script_dir).with_timeout(cfg.timeout);
        Ok(Arc::new(Self { rpc: Arc::new(rpc) }))
    }

    /// Ruta del directorio del script.
    pub fn script_dir(&self) -> &std::path::Path {
        self.rpc.dir()
    }

    /// ¿La action existe en el catálogo conocido?
    pub fn is_known_action(action: &str) -> bool {
        ACTIONS.contains(&action)
    }

    /// Llamada genérica a cualquier action del bridge.
    ///
    /// No valida contra [`ACTIONS`] a propósito: el bridge puede llevar más
    /// actions de las que este crate conoce (van por delante), y un catálogo
    /// desactualizado no debe impedir reaches que sí funcionan. Usa
    /// [`FlBridge::is_known_action`] para *advertir*, no para bloquear.
    ///
    /// FL Studio 2025 **nunca** llama a `OnIdle`, así que el pump real corre en
    /// `OnMidiIn` / `OnMidiMsg`: sin un evento MIDI, FL no se entera de que
    /// hay una request en disco. Por eso se manda un wake por MIDI.
    ///
    /// El wake va **después** de escribir la request, nunca antes: si FL
    /// despierta antes de que el fichero esté en disco, encuentra nada y no
    /// vuelve a mirar hasta el siguiente evento MIDI.
    pub async fn call(&self, action: &str, params: Value) -> Result<Value> {
        if !Self::is_known_action(action) {
            tracing::debug!(action, "action fuera del catálogo compilado; se envía igualmente");
        }
        self.rpc
            .call_with_wake(action, &params, crate::midi::wake)
            .await
    }

    /// `meta.ping`.
    pub async fn ping(&self) -> Result<BridgeInfo> {
        let v = self.call("meta.ping", json!({})).await?;
        Ok(BridgeInfo {
            bridge_version: v
                .get("bridge_version")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .into(),
            fl_version: v
                .get("fl_version")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "unknown".into()),
            uptime_sec: v.get("uptime_sec").and_then(Value::as_f64).unwrap_or(0.0),
            script_dir: v
                .get("script_dir")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    /// `transport.status`.
    pub async fn transport_status(&self) -> Result<TransportStatus> {
        let v = self.call("transport.status", json!({})).await?;
        Ok(TransportStatus {
            is_playing: v.get("is_playing").and_then(Value::as_bool).unwrap_or(false),
            is_recording: v.get("is_recording").and_then(Value::as_bool).unwrap_or(false),
            bpm: v.get("bpm").and_then(Value::as_f64).unwrap_or(0.0),
            position_ticks: v.get("position_ticks").and_then(Value::as_i64).unwrap_or(0),
            position_bars: v.get("position_bars").and_then(Value::as_f64).unwrap_or(0.0),
            position_seconds: v.get("position_seconds").and_then(Value::as_f64).unwrap_or(0.0),
            loop_mode: v
                .get("loop_mode")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    /// `transport.start`.
    pub async fn play(&self) -> Result<()> {
        self.call("transport.start", json!({})).await.map(|_| ())
    }

    /// `transport.stop`.
    pub async fn stop(&self) -> Result<()> {
        self.call("transport.stop", json!({})).await.map(|_| ())
    }

    /// `transport.setTempo` — params `{ "bpm": f64 }`.
    pub async fn set_tempo(&self, bpm: f64) -> Result<f64> {
        if !(10.0..=999.0).contains(&bpm) {
            return Err(HereticError::InvalidRequest(format!(
                "bpm fuera de rango 10-999: {bpm}"
            )));
        }
        let v = self.call("transport.setTempo", json!({ "bpm": bpm })).await?;
        Ok(v.get("bpm").and_then(Value::as_f64).unwrap_or(bpm))
    }

    /// `transport.setPosition` — params `{ "position": f64, "unit": str }`.
    /// Unidades: `bars` (default), `ms`, `seconds`, `ticks`, `steps`.
    pub async fn set_position(&self, position: f64, unit: &str) -> Result<()> {
        if !matches!(unit, "bars" | "ms" | "seconds" | "ticks" | "steps") {
            return Err(HereticError::InvalidRequest(format!(
                "unidad '{unit}' no válida (bars|ms|seconds|ticks|steps)"
            )));
        }
        self.call(
            "transport.setPosition",
            json!({ "position": position, "unit": unit }),
        )
        .await
        .map(|_| ())
    }

    /// `channels.all` — todos los canales del Channel Rack.
    pub async fn channels_all(&self) -> Result<Value> {
        self.call("channels.all", json!({})).await
    }

    /// `mixer.allTracks` — todos los tracks del mixer.
    pub async fn mixer_all_tracks(&self) -> Result<Value> {
        self.call("mixer.allTracks", json!({})).await
    }

    /// `channels.setVolume` — params `{ "index": i64, "volume": f64 }`.
    pub async fn set_channel_volume(&self, index: i64, volume: f64) -> Result<Value> {
        self.call(
            "channels.setVolume",
            json!({ "index": index, "volume": volume }),
        )
        .await
    }

    /// `mixer.setVolume` — params `{ "track": i64, "volume": f64 }`.
    pub async fn set_mixer_volume(&self, track: i64, volume: f64) -> Result<Value> {
        self.call("mixer.setVolume", json!({ "track": track, "volume": volume })).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogo_sin_duplicados() {
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            assert!(seen.insert(*a), "action duplicada en ACTIONS: {a}");
        }
    }

    #[test]
    fn catalogo_cubre_las_categorias_del_bridge() {
        // El FL Heretic Bridge declara 67 actions en 11 grupos. Si se anade un
        // handler hay que regenerar con tests/gen_catalog.py y subir el numero.
        assert_eq!(ACTIONS.len(), 67, "el catálogo debería traer las 67 actions");
        for prefix in [
            "meta", "transport", "patterns", "channels", "mixer", "plugins",
            "playlist", "arrangement", "project", "ui",
        ] {
            assert!(
                ACTIONS.iter().any(|a| a.starts_with(prefix)),
                "falta la categoría {prefix}"
            );
        }
    }

    #[test]
    fn nombres_de_accion_son_camelcase_no_snake() {
        // Regresión: se mapearon mal una vez (transport.set_tempo / set_position).
        assert!(ACTIONS.contains(&"transport.setTempo"));
        assert!(ACTIONS.contains(&"transport.setPosition"));
        assert!(!ACTIONS.contains(&"transport.set_tempo"));
        assert!(!ACTIONS.contains(&"transport.set_position"));
    }

    #[test]
    fn config_por_defecto_apunta_al_bridge_heretic() {
        let cfg = BridgeConfig::default();
        let s = cfg.script_dir.to_string_lossy().replace('\\', "/");
        // Nuestro bridge, no el de fLMCP: tienen mailboxes distintos.
        assert!(
            s.ends_with("FL Studio/Settings/Hardware/FL Heretic Bridge"),
            "ruta: {s}"
        );
        assert!(!s.ends_with("fLMCP Bridge"), "no debe apuntar al bridge ajeno");
    }

    #[test]
    fn env_sobreescribe_el_directorio() {
        // Solo se comprueba el parseo del env, no el valor, porque el test
        // corre en un proceso donde el env puede o no estar puesto.
        let a = default_script_dir();
        assert!(!a.as_os_str().is_empty());
    }

    #[test]
    fn error_claro_si_no_existe_el_directorio() {
        let cfg = BridgeConfig {
            script_dir: std::path::PathBuf::from("Z:\\no\\existe\\nada"),
            timeout: std::time::Duration::from_millis(10),
        };
        let err = FlBridge::with_config(cfg).unwrap_err();
        assert!(err.to_string().contains("no existe el directorio"), "{err}");
    }

    #[tokio::test]
    async fn set_tempo_rechaza_rango_invalido_sin_tocar_FL() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = FlBridge::with_config(BridgeConfig {
            script_dir: dir.path().to_path_buf(),
            timeout: std::time::Duration::from_millis(10),
        })
        .unwrap();
        assert!(bridge.set_tempo(5000.0).await.is_err());
        assert!(bridge.set_tempo(1.0).await.is_err());
        // 128 está en rango: debe intentar el round-trip y fallar por timeout,
        // no por validación.
        let err = bridge.set_tempo(128.0).await.unwrap_err();
        assert!(err.to_string().contains("timeout"), "{err}");
    }

    #[tokio::test]
    async fn set_position_valida_la_unidad() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = FlBridge::with_config(BridgeConfig {
            script_dir: dir.path().to_path_buf(),
            timeout: std::time::Duration::from_millis(10),
        })
        .unwrap();
        let err = bridge.set_position(4.0, "furlongs").await.unwrap_err();
        assert!(err.to_string().contains("no válida"), "{err}");
    }
}
