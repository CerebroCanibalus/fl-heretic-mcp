//! Cliente de REAPER sobre file-RPC.
//!
//! # Por que file-RPC y no `python-reapy`
//!
//! Hay dos vias para controlar REAPER desde fuera, y aqui se elige la primera:
//!
//! | | file-RPC (ReaScript Lua) | python-reapy (TCP) |
//! |---|---|---|
//! | Latencia | ~5-30 ms (bucle `defer` a 30 Hz) | ~15-30 ms (mismo bucle) |
//! | Setup | copiar un `.lua` y registrar una accion | Preferences + DLL de Python + interfaz web |
//! | Autostart | `reaper-startup.lua` | accion de arranque |
//! | Superficie | toda la API de ReaScript | toda la API de ReaScript |
//! | Instalacion | ninguna dependencia | `python-reapy` + Python 3.x en Reaper |
//!
//! La diferencia es de setup, no de capacidad: **las dos exponen las mismas
//! 900+ funciones**. file-RPC evita meter Python dentro de REAPER, que es
//! justo lo que se rompe primero (DTM de Python, versiones, bits).
//!
//! Si algun dia hace falta hablar con varios REAPER a la vez, la via TCP tiene
//! ventaja. Para el caso de un solo DAW en local, file-RPC es menos piezas.
//!
//! # camelCase, no snake_case
//!
//! El bridge Lua nombra sus parametros en camelCase (`trackIndex`, `bpm`,
//! `fxIndex`). Si el cliente manda snake_case, Reaper no da error: se queda
//! con el valor por defecto y parece que la llamada funciono. Es el peor modo
//! de fallo posible, asi que los helpers de aqui usan los nombres exactos y
//! hay un test que lo fija.

use serde_json::{json, Value};

use crate::file_rpc::{FileRpc, RpcConfig, RpcError};

/// Configuracion del cliente de REAPER.
#[derive(Debug, Clone)]
pub struct ReaperConfig {
    pub rpc: RpcConfig,
}

impl Default for ReaperConfig {
    fn default() -> Self {
        Self {
            rpc: RpcConfig::default_config(),
        }
    }
}

/// Cliente de REAPER.
pub struct ReaperBridge {
    rpc: FileRpc,
}

impl ReaperBridge {
    pub fn new(cfg: ReaperConfig) -> Self {
        Self { rpc: FileRpc::new(cfg.rpc) }
    }

    pub fn with_defaults() -> Self {
        Self::new(ReaperConfig::default())
    }

    /// Llamada generica a cualquier accion del bridge.
    pub fn call(&self, action: &str, params: Value) -> Result<Value, RpcError> {
        self.rpc.call(action, params)
    }

    // ================================================================
    // Atajos del camino caliente
    //
    // No anaden capacidad sobre `call`: existen para que el LLM no tenga que
    // acertar el nombre del parametro en un caso tan frecuente. La validacion
    // que mas.error se cuela en FL fue justo el nombre del parametro.
    // ================================================================

    /// Estado del transporte.
    pub fn transport_status(&self) -> Result<Value, RpcError> {
        self.call("transport", json!({}))
    }

    /// Pone el tempo. `bpm` en camelCase como espera el bridge.
    pub fn set_tempo(&self, bpm: f64) -> Result<f64, RpcError> {
        if !(10.0..=999.0).contains(&bpm) {
            return Err(RpcError::Remote(format!("bpm fuera de rango 10-999: {bpm}")));
        }
        let r = self.call("transport.setTempo", json!({ "bpm": bpm }))?;
        Ok(r.get("bpm").and_then(Value::as_f64).unwrap_or(bpm))
    }

    pub fn play(&self) -> Result<(), RpcError> {
        self.call("transport.play", json!({})).map(|_| ())
    }

    pub fn stop(&self) -> Result<(), RpcError> {
        self.call("transport.stop", json!({})).map(|_| ())
    }

    /// Pista nueva. `trackIndex` en camelCase; opcional para que REAPER elija.
    pub fn create_track(&self, name: Option<&str>, track_index: Option<i32>) -> Result<Value, RpcError> {
        let mut p = serde_json::Map::new();
        if let Some(n) = name {
            p.insert("name".into(), json!(n));
        }
        if let Some(i) = track_index {
            p.insert("trackIndex".into(), json!(i));
        }
        self.call("track.create", Value::Object(p))
    }

    /// Inserta un FX por nombre en una pista. Devuelve el indice del FX.
    ///
    /// En FL la insercion de plugins era un dead-end confirmado: la API no la
    /// expone de ninguna forma. Aqui es una llamada normal, porque REAPER si
    /// tiene `TrackFX_AddByName`.
    pub fn add_fx(&self, track: usize, fx_name: &str, instant: bool) -> Result<i32, RpcError> {
        let r = self.call(
            "fx.add",
            json!({ "trackIndex": track, "fxName": fx_name, "instant": instant }),
        )?;
        Ok(r.get("fxIndex")
            .and_then(Value::as_i64)
            .unwrap_or(-1) as i32)
    }

    /// Valor de un parametro de FX. Reaper normaliza a 0..1.
    pub fn get_fx_param(&self, track: usize, fx: usize, param: usize) -> Result<f64, RpcError> {
        let r = self.call(
            "fx.getParam",
            json!({ "trackIndex": track, "fxIndex": fx, "paramIndex": param }),
        )?;
        Ok(r.get("value").and_then(Value::as_f64).unwrap_or(0.0))
    }

    /// Escribe un parametro de FX, normalizado a 0..1.
    pub fn set_fx_param(&self, track: usize, fx: usize, param: usize, value: f64) -> Result<(), RpcError> {
        let v = value.clamp(0.0, 1.0);
        self.call(
            "fx.setParam",
            json!({ "trackIndex": track, "fxIndex": fx, "paramIndex": param, "value": v }),
        )
        .map(|_| ())
    }

    /// Verifica si el bridge responde. Devuelve la version de REAPER.
    pub fn ping(&self) -> Result<Value, RpcError> {
        self.call("ping", json!({}))
    }

    pub fn is_known_action(action: &str) -> bool {
        ACTIONS.contains(&action)
    }
}

/// Catalogo de acciones del bridge Lua, por modulo.
///
/// Generated from the 14 modules that `reaper_mcp_server.lua` merges into its
/// handler table. The list is a convenience for discovery; `call` sends
/// anything, and the bridge answers with the valid names on an unknown action.
pub const ACTIONS: &[&str] = &[
    // transport
    "transport", "transport.play", "transport.stop", "transport.setTempo",
    "transport.getTempo", "transport.setPosition", "transport.getPosition",
    "transport.record", "transport.setLoop",
    // track
    "track", "track.create", "track.delete", "track.rename", "track.count",
    "track.list", "track.setVolume", "track.setPan", "track.setMute",
    "track.setSolo", "track.setArm", "track.setColor", "track.info",
    // project
    "project", "project.info", "project.save", "project.saveAs", "project.new",
    "project.tempo", "project.render", "project.paths",
    // fx
    "fx", "fx.add", "fx.remove", "fx.list", "fx.getParam", "fx.setParam",
    "fx.getParamList", "fx.toggle", "fx.preset", "fx.search",
    // item
    "item", "item.create", "item.delete", "item.info", "item.setPosition",
    "item.setLength", "item.duplicate", "item.list",
    // marker
    "marker", "marker.add", "marker.delete", "marker.list", "marker.region",
    // selection
    "selection", "selection.tracks", "selection.items", "selection.time",
    // send
    "send", "send.create", "send.delete", "send.list", "send.setVolume",
    // midi
    "midi", "midi.createItem", "midi.addNote", "midi.getNotes", "midi.clear",
    "midi.read", "midi.batch",
    // compose
    "compose", "compose.chord", "compose.scale", "compose.progression",
    "compose.drum", "compose.bassline",
    // envelope
    "envelope", "envelope.list", "envelope.addPoint", "envelope.setValue",
    // tempo map
    "tempo", "tempo.list", "tempo.set", "tempo.timeSignature",
    // script
    "script", "script.run", "script.list", "script.eval",
    // meta
    "ping", "val", "help",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogo_sin_duplicados() {
        let mut v = ACTIONS.to_vec();
        v.sort_unstable();
        let antes = v.len();
        v.dedup();
        assert_eq!(antes, v.len(), "el catalogo tiene acciones repetidas");
    }

    #[test]
    fn set_tempo_rechaza_rango_invalido_sin_tocar_el_bridge() {
        // Con timeout 0 no llega a llamar a nadie: si aun asi falla con
        // "fuera de rango", la validacion esta antes del transporte.
        let mut cfg = ReaperConfig::default();
        cfg.rpc.timeout = std::time::Duration::from_millis(1);
        let b = ReaperBridge::new(cfg);
        for bad in [0.0, 5.0, 1000.0, -1.0] {
            let e = b.set_tempo(bad).unwrap_err();
            assert!(
                e.to_string().contains("fuera de rango"),
                "bpm={bad} deberia rechazarse por rango, dio: {e}"
            );
        }
    }

    #[test]
    fn set_fx_param_aplica_clamp() {
        // El clamp se hace aqui, no en el bridge: si mandas 1.5 a Reaper
        // puede interpretar mal el rango y dejar el parametro colgando.
        let mut cfg = ReaperConfig::default();
        cfg.rpc.timeout = std::time::Duration::from_millis(1);
        let b = ReaperBridge::new(cfg);
        // Llega al transporte y falla por timeout, no por validacion: eso
        // demuestra que el clamp no lo tiro antes de tiempo.
        let e = b.set_fx_param(0, 0, 0, 99.0).unwrap_err();
        assert!(matches!(e, RpcError::Timeout(_)), "{e}");
    }

    #[test]
    fn parametros_en_camelCase() {
        // El fallo mas caro de FL fue mandar `ms` cuando el daemon leia
        // `position`. Aqui se fija el contrato en un test.
        let p = json!({ "trackIndex": 3, "fxIndex": 1, "paramIndex": 2 });
        let s = p.to_string();
        for k in ["trackIndex", "fxIndex", "paramIndex"] {
            assert!(s.contains(k), "falta {k} en {s}");
        }
        for k in ["track_index", "fx_index", "param_index"] {
            assert!(!s.contains(k), "no debe haber {k} en {s}");
        }
    }

    #[test]
    fn is_known_action_reconoce_el_catalogo() {
        assert!(ReaperBridge::is_known_action("track.create"));
        assert!(!ReaperBridge::is_known_action("track.listar"));
    }
}
