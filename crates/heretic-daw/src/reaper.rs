//! Cliente de REAPER sobre file-RPC.
//!
//! # Nombres de accion: generados, no escritos
//!
//! Los nombres publicos del bridge son `transport_set_bpm`, `track_create`,
//! `fx_add`... separador `_`, verbos en pasado. **No** son `transport.setTempo`
//! ni `track.create` con punto, que es lo que se escribiría por costumbre y lo
//! que escribió la primera version de este fichero: las tres primeras
//! pruebas contra Reaper real fallaron con `Unknown command:`.
//!
//! El catalogo completo esta en [`crate::actions`], extraido del bridge.
//!
//! # Por que file-RPC y no `python-reapy`
//!
//! Hay dos vias para controlar REAPER desde fuera, y se elige la primera:
//!
//! | | file-RPC (ReaScript Lua) | python-reapy (TCP) |
//! |---|---|---|
//! | Latencia | ~5-35 ms (bucle `defer` a 30 Hz) | ~15-30 ms (mismo bucle) |
//! | Setup | copiar un `.lua` + `__startup.lua` | Preferences + DLL de Python + web interface |
//! | Dependencias | ninguna | `python-reapy` + Python dentro de Reaper |
//! | Superficie | las mismas 900+ funciones | las mismas 900+ funciones |
//!
//! La diferencia es de setup, no de capacidad. file-RPC evita meter Python
//! dentro de Reaper, que es lo que se rompe primero (DTM, versión, bits). Si
//! algun dia hay que hablar con varios REAPER a la vez, TCP tiene ventaja.

use serde_json::{json, Value};

use crate::file_rpc::{FileRpc, RpcConfig, RpcError};
use crate::actions;

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

    /// ¿Hay un bridge vivo dentro de Reaper?
    ///
    /// Milisegundos, frente a los 10 s que costaria descubrirlo agotando el
    /// timeout de una llamada. El LLM puede preguntar esto antes de intentarlo.
    pub fn is_alive(&self) -> bool {
        self.rpc.is_bridge_alive()
    }

    /// Llamada generica a cualquier accion del bridge.
    pub fn call(&self, command: &str, params: Value) -> Result<Value, RpcError> {
        self.rpc.call(command, params)
    }

    // ================================================================
    // Atajos del camino caliente
    //
    // No anaden capacidad sobre `call`: existen para que el LLM no tenga que
    // acertar el nombre del parametro en un caso tan frecuente. El bug mas
    // caro de FL fue justo un param mal escrito, y Reaper no avisa cuando un
    // param no existe: usa el valor por defecto y parece que funciono.
    // ================================================================

    /// Estado del transporte. Devuelve `bpm`, `playing`, `position`, etc.
    pub fn transport_state(&self) -> Result<Value, RpcError> {
        self.call("transport_get_state", json!({}))
    }

    /// Pone el tempo.
    pub fn set_tempo(&self, bpm: f64) -> Result<f64, RpcError> {
        if !(10.0..=999.0).contains(&bpm) {
            return Err(RpcError::Remote(format!(
                "bpm fuera de rango 10-999: {bpm}"
            )));
        }
        self.call("transport_set_bpm", json!({ "bpm": bpm }))?;
        // Releer en vez de devolver el eco: no presume que se aplico.
        let st = self.transport_state()?;
        Ok(st.get("bpm")
            .and_then(Value::as_f64)
            .or_else(|| st.get("tempo").and_then(Value::as_f64))
            .unwrap_or(bpm))
    }

    pub fn play(&self) -> Result<Value, RpcError> {
        self.call("transport_play", json!({}))
    }

    pub fn stop(&self) -> Result<Value, RpcError> {
        self.call("transport_stop", json!({}))
    }

    /// Pista nueva. `name` opcional: si no, Reaper pone el suyo.
    pub fn create_track(&self, name: Option<&str>) -> Result<Value, RpcError> {
        let mut p = serde_json::Map::new();
        if let Some(n) = name {
            p.insert("name".into(), json!(n));
        }
        self.call("track_create", Value::Object(p))
    }

    /// Todas las pistas del proyecto.
    pub fn tracks(&self) -> Result<Value, RpcError> {
        self.call("track_get_all", json!({}))
    }

    /// Inserta un FX por nombre en una pista. Devuelve su indice.
    ///
    /// En FL la insercion de plugins era un dead-end confirmado: su API no la
    /// expone de ninguna forma. Aqui es una llamada normal, porque REAPER tiene
    /// `TrackFX_AddByName`. Esta es la via para tener FLEX: cargar
    /// `FL Studio VSTi (Multi)` en una pista de instrumento.
    pub fn add_fx(&self, track: usize, fx_name: &str) -> Result<Value, RpcError> {
        self.call(
            "fx_add",
            json!({ "trackIndex": track, "fxName": fx_name }),
        )
    }

    /// Parametros de un FX con nombre y rango. Los valores van 0..1.
    pub fn fx_params(&self, track: usize, fx: usize) -> Result<Value, RpcError> {
        self.call(
            "fx_get_params",
            json!({ "trackIndex": track, "fxIndex": fx }),
        )
    }

    /// Plugins instalados que coinciden con un texto. Para no adivinar nombres.
    pub fn search_fx(&self, query: &str) -> Result<Value, RpcError> {
        self.call("fx_list_installed", json!({ "query": query }))
    }

    /// Info del proyecto. Contiene `bpm`, `name`, `dirty`, etc.
    pub fn project_info(&self) -> Result<Value, RpcError> {
        self.call("project_get_info", json!({}))
    }

    /// Guarda el proyecto.
    pub fn save_project(&self) -> Result<Value, RpcError> {
        self.call("project_save", json!({}))
    }

    /// Crea un item MIDI vacio en una pista. Posicion en **beats**.
    pub fn create_midi_item(&self, track: usize, start_beat: f64, length_beats: f64) -> Result<Value, RpcError> {
        self.call(
            "item_create_midi",
            json!({
                "trackIndex": track,
                "position": start_beat,
                "length": length_beats,
            }),
        )
    }

    /// Escribe una nota MIDI. `start` y `length` en beats.
    pub fn add_note(
        &self,
        track: usize,
        item: usize,
        pitch: i32,
        start: f64,
        length: f64,
        velocity: f64,
    ) -> Result<Value, RpcError> {
        self.call(
            "midi_insert_note",
            json!({
                "trackIndex": track,
                "itemIndex": item,
                "pitch": pitch,
                "start": start,
                "length": length,
                "velocity": velocity.clamp(0.0, 1.0),
            }),
        )
    }

    /// Comprueba si un nombre de accion existe en el catalogo generado.
    pub fn is_known_action(command: &str) -> bool {
        actions::ACTIONS.contains(&command)
    }

    /// Acciones que empiezan por un prefijo, p.ej. `track_`.
    pub fn actions_with_prefix(prefix: &str) -> Vec<&'static str> {
        actions::ACTIONS
            .iter()
            .copied()
            .filter(|a| a.starts_with(prefix))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogo_sin_duplicados() {
        let mut v = actions::ACTIONS.to_vec();
        v.sort_unstable();
        let antes = v.len();
        v.dedup();
        assert_eq!(antes, v.len(), "el catalogo tiene acciones repetidas");
    }

    #[test]
    fn todo_grupo_esta_en_el_catalogo() {
        // Si un grupo lista una accion que no esta en ACTIONS, `daw_catalog`
        // prometeria algo que luego daria "Unknown command".
        for (grupo, acts) in actions::ACTION_GROUPS {
            for a in *acts {
                assert!(
                    actions::ACTIONS.contains(a),
                    "el grupo '{grupo}' lista '{a}', que no esta en ACTIONS"
                );
            }
        }
    }

    #[test]
    fn los_grupos_cubren_todas_las_acciones() {
        let mut desde_grupos: Vec<&str> = actions::ACTION_GROUPS
            .iter()
            .flat_map(|(_, a)| a.iter().copied())
            .collect();
        desde_grupos.sort_unstable();
        let mut todas = actions::ACTIONS.to_vec();
        todas.sort_unstable();
        assert_eq!(
            desde_grupos, todas,
            "hay acciones en ACTIONS que ningun grupo lista"
        );
    }

    #[test]
    fn los_nombres_reales_estan_en_el_catalogo() {
        // Los nombres que se usan en los atajos tienen que existir de verdad.
        // Esta es la prueba que habria evitado los 3 fallos iniciales.
        for real in [
            "transport_get_state",
            "transport_set_bpm",
            "transport_play",
            "transport_stop",
            "track_create",
            "track_get_all",
            "project_get_info",
            "project_save",
            "item_create_midi",
            "midi_insert_note",
            "midi_insert_notes_batch",
            "fx_add",
            "fx_get_params",
            "fx_list_installed",
        ] {
            assert!(ReaperBridge::is_known_action(real), "falta '{real}'");
        }
    }

    #[test]
    fn los_nombres_inventados_no_estan_en_el_catalogo() {
        // Los que se escribieron por costumbre. Si alguno apareciera, seria
        // que el catalogo se genero de otra cosa.
        for falso in [
            "transport.setTempo",
            "track.create",
            "ping",
            "transport",
            "project.info",
        ] {
            assert!(
                !ReaperBridge::is_known_action(falso),
                "'{falso}' no deberia existir en el catalogo"
            );
        }
    }

    #[test]
    fn filtro_por_prefijo() {
        let t = ReaperBridge::actions_with_prefix("transport_");
        assert!(!t.is_empty());
        assert!(t.iter().all(|a| a.starts_with("transport_")), "{t:?}");
    }

    #[test]
    fn set_tempo_rechaza_rango_invalido_sin_tocar_el_bridge() {
        // Timeout 0: si aun asi falla con "fuera de rango", la validacion
        // esta antes del transporte.
        let mut cfg = ReaperConfig::default();
        cfg.rpc.timeout = std::time::Duration::from_millis(1);
        cfg.rpc.lock = std::env::temp_dir().join("no-existe.lock");
        let b = ReaperBridge::new(cfg);
        for bad in [0.0, 5.0, 1000.0, -1.0] {
            let e = b.set_tempo(bad).unwrap_err();
            assert!(
                e.to_string().contains("fuera de rango"),
                "bpm={bad} deberia rechazarse por rango, dio: {e}"
            );
        }
    }
}
