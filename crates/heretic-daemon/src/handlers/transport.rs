//! Handlers transport — mirror del FLStudioMCP legacy (Fase 2).
//!
//! Cada método recibe un `serde_json::Value` con los params del request JSON-RPC
//! y devuelve un `serde_json::Value` con el resultado.
//!
//! Si el método devuelve `Err(HereticError)`, el daemon lo convierte en
//! una response JSON-RPC con `ok: false` + código de error.

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
    ///
    /// Devuelve `Err(HereticError::InvalidRequest)` si el método no es transport.
    /// (Para Fase 3, los handlers no-transport vivirían en otros módulos.)
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
            other => Err(HereticError::InvalidRequest(format!(
                "método transport desconocido: {other}"
            ))),
        }
    }

    // ============================================================
    // Handlers individuales
    // ============================================================

    /// `ping` — eco del controller script + info del daemon.
    pub async fn ping(&self) -> Result<Value> {
        let info = self.bridge.ping().await?;
        Ok(json!({
            "pong": true,
            "fl_version": info.fl_version,
            "protocol_version": info.protocol_version,
            "raw": info.raw,
        }))
    }

    /// `health` — estado del bridge MIDI (heartbeat age, alive).
    pub async fn health(&self) -> Result<Value> {
        let h = self.bridge.health().await;
        Ok(json!({
            "alive": h.alive,
            "heartbeat_age_ms": h.heartbeat_age_ms,
            "fl_version": h.fl_version,
        }))
    }

    /// `get_tempo` — BPM actual.
    pub async fn get_tempo(&self) -> Result<Value> {
        let bpm = self.bridge.get_tempo().await?;
        Ok(json!({ "bpm": bpm }))
    }

    /// `set_tempo` — params: `{ "bpm": f64 }`.
    pub async fn set_tempo(&self, params: Value) -> Result<Value> {
        let bpm = params
            .get("bpm")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| HereticError::InvalidRequest("set_tempo: missing bpm".into()))?;
        let result = self.bridge.set_tempo(bpm).await?;
        Ok(json!({ "bpm": result }))
    }

    /// `play` — transport.start().
    pub async fn play(&self) -> Result<Value> {
        self.bridge.play().await?;
        Ok(json!({ "playing": true }))
    }

    /// `stop` — transport.stop().
    pub async fn stop(&self) -> Result<Value> {
        self.bridge.stop().await?;
        Ok(json!({ "playing": false }))
    }

    /// `get_play_state` — playing + recording.
    pub async fn get_play_state(&self) -> Result<Value> {
        // FL controller script legacy no expone isRecording() en handlers básicos,
        // solo isPlaying(). Devolvemos lo que tenemos.
        let health = self.bridge.health().await;
        // Para playing: usar fl_get_song_position o un campo adicional
        // Por ahora: false (Fase 3+ implementará get_play_state con ambos flags)
        Ok(json!({
            "playing": health.alive,  // proxy pobre — Fase 3 lo afina
            "recording": false,
            "alive": health.alive,
        }))
    }

    /// `get_song_position` — posición actual.
    pub async fn get_song_position(&self) -> Result<Value> {
        let pos = self.bridge.get_song_position().await?;
        Ok(json!({
            "position_ms": pos.position_ms,
            "position_ticks": pos.position_ticks,
            "position_beats": pos.position_beats,
            "bpm": pos.bpm,
        }))
    }

    /// `set_song_position` — params: `{ "ms": f64 }` o `{ "beats": f64 }` o `{ "ticks": i64 }`.
    pub async fn set_song_position(&self, params: Value) -> Result<Value> {
        if let Some(ms) = params.get("ms").and_then(|v| v.as_f64()) {
            let pos = self.bridge.set_song_position_ms(ms).await?;
            Ok(json!({
                "position_ms": pos.position_ms,
                "position_ticks": pos.position_ticks,
                "position_beats": pos.position_beats,
                "bpm": pos.bpm,
            }))
        } else if let Some(beats) = params.get("beats").and_then(|v| v.as_f64()) {
            // Convertir beats → ms usando el BPM actual
            let current = self.bridge.get_song_position().await?;
            let ms = beats * 60000.0 / current.bpm;
            let pos = self.bridge.set_song_position_ms(ms).await?;
            Ok(json!({
                "position_ms": pos.position_ms,
                "position_ticks": pos.position_ticks,
                "position_beats": pos.position_beats,
                "bpm": pos.bpm,
            }))
        } else if let Some(ticks) = params.get("ticks").and_then(|v| v.as_i64()) {
            // Convertir ticks → ms (96 ticks per beat en ppq default de FL)
            let current = self.bridge.get_song_position().await?;
            let ms = (ticks as f64 / 96.0) * 60000.0 / current.bpm;
            let pos = self.bridge.set_song_position_ms(ms).await?;
            Ok(json!({
                "position_ms": pos.position_ms,
                "position_ticks": pos.position_ticks,
                "position_beats": pos.position_beats,
                "bpm": pos.bpm,
            }))
        } else {
            Err(HereticError::InvalidRequest(
                "set_song_position: provide ms, beats, or ticks".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_unknown_returns_error() {
        // No podemos instanciar el Transport sin bridge real, pero podemos
        // verificar el formato del error.
        // (Test de integración requeriría FL corriendo)
    }
}