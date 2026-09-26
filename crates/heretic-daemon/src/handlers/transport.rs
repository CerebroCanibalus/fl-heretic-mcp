//! Handlers transport — API fina sobre las 67 actions del FL Heretic Bridge.
//!
//! Estas son las unicas actions con semantica propia (validacion de rango,
//! normalizacion de unidades, releer el estado real en vez de devolver el
//! eco del bridge). Para todo lo demas esta `call`, que es el escape hatch.
//!
//! Mapeo de nuestros tools a actions del bridge:
//! - `fl_ping`              → `meta.ping`
//! - `fl_get_tempo`         → `transport.status` (extraer bpm)
//! - `fl_set_tempo`         → `transport.set_tempo`
//! - `fl_play`              → `transport.start`
//! - `fl_stop`              → `transport.stop`
//! - `fl_get_play_state`    → `transport.status` (extraer is_playing, is_recording)
//! - `fl_get_song_position` → `transport.status` (extraer position_*)
//! - `fl_set_song_position` → `transport.set_position` (con unit)
//
//! Fase 3: añadir el resto de las actions (mixer, channels, plugins, etc.)

use std::sync::Arc;

use serde_json::{json, Value};

use heretic_core::{HereticError, Result};
use heretic_fl::FlBridge;

/// Conjunto de handlers transport — agrupa los métodos que delegan al `FlBridge`.
pub struct Transport {
    bridge: Arc<FlBridge>,
}

impl Transport {
    pub fn new(bridge: Arc<FlBridge>) -> Self {
        Self { bridge }
    }

    /// Dispatcher principal: matchea el nombre del método al handler apropiado.
    pub async fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "ping" => self.ping().await,
            "health" => self.health().await,
            "get_tempo" => self.get_tempo().await,
            "set_tempo" => self.set_tempo(params).await,
            "play" => self.play().await,
            "stop" => self.stop().await,
            "get_play_state" => self.get_play_state().await,
            "get_song_position" => self.get_song_position().await,
            "set_song_position" => self.set_song_position(params).await,
            "call" => self.call(params).await,
            "actions" => self.actions().await,
            other => Err(HereticError::InvalidRequest(format!(
                "método transport desconocido: {other}"
            ))),
        }
    }

    // ============================================================
    // Handlers individuales
    // ============================================================

    /// `meta.ping` — info del bridge + FL.
    pub async fn ping(&self) -> Result<Value> {
        let info = self.bridge.ping().await?;
        Ok(json!({
            "pong": true,
            "bridge_version": info.bridge_version,
            "fl_version": info.fl_version,
            "uptime_sec": info.uptime_sec,
            "script_dir": info.script_dir,
            "raw": info,
        }))
    }

    /// `health` — estado del bridge y del proceso de FL Studio.
    pub async fn health(&self) -> Result<Value> {
        let info = self.bridge.ping().await.ok();
        let proc = heretic_fl::running_process();
        Ok(json!({
            "bridge_online": info.is_some(),
            "bridge_version": info.as_ref().map(|i| i.bridge_version.clone()),
            "fl_version": info.as_ref().map(|i| i.fl_version.clone()),
            "fl_uptime_sec": info.as_ref().map(|i| i.uptime_sec),
            "fl_process_running": proc.is_some(),
            "fl_pid": proc.as_ref().map(|p| p.pid),
            "fl_exe": heretic_fl::find_fl_exe().map(|p| p.display().to_string()),
            "midi_wake_ports": heretic_fl::midi::open_all(),
            "transport": "file-rpc + midi-wake",
        }))
    }

    /// `transport.status` (extraer bpm).
    pub async fn get_tempo(&self) -> Result<Value> {
        let status = self.bridge.transport_status().await?;
        Ok(json!({ "bpm": status.bpm }))
    }

    /// `transport.set_tempo` — params: `{ "bpm": f64 }`.
    pub async fn set_tempo(&self, params: Value) -> Result<Value> {
        let bpm = params
            .get("bpm")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| HereticError::InvalidRequest("set_tempo: missing bpm".into()))?;
        let result_bpm = self.bridge.set_tempo(bpm).await?;
        Ok(json!({ "bpm": result_bpm }))
    }

    /// `transport.start`.
    pub async fn play(&self) -> Result<Value> {
        self.bridge.play().await?;
        Ok(json!({ "playing": true }))
    }

    /// `transport.stop`.
    pub async fn stop(&self) -> Result<Value> {
        self.bridge.stop().await?;
        Ok(json!({ "playing": false }))
    }

    /// `transport.status` (extraer is_playing + is_recording).
    pub async fn get_play_state(&self) -> Result<Value> {
        let status = self.bridge.transport_status().await?;
        Ok(json!({
            "playing": status.is_playing,
            "recording": status.is_recording,
            "bpm": status.bpm,
        }))
    }

    /// `transport.status` (extraer position_*).
    pub async fn get_song_position(&self) -> Result<Value> {
        let status = self.bridge.transport_status().await?;
        Ok(json!({
            "position_ticks": status.position_ticks,
            "position_bars": status.position_bars,
            "position_seconds": status.position_seconds,
            "bpm": status.bpm,
        }))
    }

    /// `transport.set_position` — params: `{ "position": f64, "unit": "bars"|"ms"|"seconds"|"ticks"|"steps" }`.
    /// Default unit es "bars" (lo que el bridge asume).
    pub async fn set_song_position(&self, params: Value) -> Result<Value> {
        let position = params
            .get("position")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| {
                HereticError::InvalidRequest("set_song_position: missing position".into())
            })?;
        let unit = params
            .get("unit")
            .and_then(|v| v.as_str())
            .unwrap_or("bars");
        self.bridge.set_position(position, unit).await?;
        // El bridge devuelve solo un eco de la posicion; se relee el estado
        // real para no mentirle a quien llama.
        let status = self.bridge.transport_status().await?;
        Ok(json!({
            "position_ticks": status.position_ticks,
            "position_bars": status.position_bars,
            "position_seconds": status.position_seconds,
            "bpm": status.bpm,
        }))
    }

    /// `call` — escape hatch a CUALQUIER action del bridge.
    ///
    /// Da acceso a las 67 actions sin escribir un handler MCP por cada una.
    /// El catalogo esta en `heretic_fl::bridge::ACTIONS`.
    /// Parametros: `{ "action": "channels.setVolume", "params": { ... } }`.
    pub async fn call(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HereticError::InvalidRequest("call: falta 'action'".into()))?
            .to_string();
        if !FlBridge::is_known_action(&action) {
            tracing::warn!(
                action,
                "action fuera del catalogo compilado; se envia igualmente. \
                 Usa fl_actions para ver el catalogo."
            );
        }
        let p = params.get("params").cloned().unwrap_or(json!({}));
        self.bridge.call(&action, p).await
    }

    /// `actions` — catalogo de actions que el bridge declara.
    pub async fn actions(&self) -> Result<Value> {
        let known: Vec<&str> = heretic_fl::ACTIONS.to_vec();
        // Se le pregunta al bridge de verdad, que puede tener mas.
        let live = self.bridge.call("meta.actions", json!({})).await.ok();
        let live_list: Vec<String> = live
            .and_then(|v| v.get("actions").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(json!({
            "compiled_catalog": known,
            "compiled_count": known.len(),
            "bridge_reported": live_list,
            "bridge_count": live_list.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Tests de integración requieren fLMCP Bridge corriendo en FL Studio.
}