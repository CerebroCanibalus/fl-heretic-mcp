//! `FlBridge` — API de alto nivel sobre el bridge MIDI SysEx.
//!
//! Maneja la correlación request/response, el envío/recepción de mensajes,
//! y expone métodos tipados para los tools del daemon.
//!
//! ## Topología interna
//!
//! ```text
//! [tools del daemon]                       [este bridge]
//!      │                                          │
//!      │  call(cmd, params)                        │
//!      │ ────────────────────────────────────────► │
//!      │                                          ├─► genera request_id
//!      │                                          ├─► encode SysEx
//!      │                                          ├─► midi.send()
//!      │                                          ├─► oneshot channel
//!      │ ◄──── result ──────────────────────────  │
//!      │                                          │  midi worker
//!      │                                          │  (background thread)
//!      │                                          │   ├─ incoming_rx.recv()
//!      │                                          │   ├─ decode SysEx
//!      │                                          │   ├─ match request_id
//!      │                                          │   └─ oneshot.send(result)
//! ```
//!
//! El worker es un thread OS dedicado (no async) porque midir no es async-friendly.
//! Los tools hablan con el bridge via mpsc + oneshot.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use heretic_core::{HereticError, Result};
use crate::heartbeat::HeartbeatTracker;
use crate::midi::{open_midi_ports, send_sysex, MidiConnection};
use crate::sysex::{decode_message, encode_message, new_request_id, Direction};
#[allow(unused_imports)]
use std::collections::HashMap;

/// Config del bridge.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Patrón del puerto MIDI output (server → FL). Default: "FLStudioMCP RX"
    pub port_to_fl: String,
    /// Patrón del puerto MIDI input (FL → server). Default: "FLStudioMCP TX"
    pub port_from_fl: String,
    /// Nombre del cliente MIDI (visible en FL > MIDI Settings). Default: "FLHeretic"
    pub client_name: String,
    /// Timeout para una operación round-trip. Default: 5s.
    pub default_timeout: Duration,
    /// Si true, espera al primer heartbeat antes de retornar de `connect()`.
    pub wait_for_first_heartbeat: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            port_to_fl: crate::DEFAULT_PORT_TO_FL.into(),
            port_from_fl: crate::DEFAULT_PORT_FROM_FL.into(),
            client_name: "FLHeretic".into(),
            default_timeout: Duration::from_secs(5),
            wait_for_first_heartbeat: true,
        }
    }
}

/// Versión de FL reportada en el primer heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlVersionInfo {
    pub fl_version: String,
    pub protocol_version: u32,
    pub raw: Value,
}

/// Posición de canción reportada por FL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongPosition {
    pub position_ms: f64,
    pub position_ticks: i64,
    pub position_beats: f64,
    pub bpm: f64,
}

/// Mensaje interno: request del bridge → MIDI worker.
struct BridgeRequest {
    command: String,
    params: Value,
    response: oneshot::Sender<Result<Value>>,
}

/// API pública del bridge MIDI.
#[derive(Clone)]
pub struct FlBridge {
    inner: Arc<FlBridgeInner>,
}

struct FlBridgeInner {
    config: BridgeConfig,
    /// Sender al MIDI worker (requests se envían por aquí).
    request_tx: mpsc::UnboundedSender<BridgeRequest>,
    /// Heartbeat tracker (compartido con el worker).
    heartbeat: HeartbeatTracker,
    /// Cancel token para apagar el worker limpiamente (Fase 5 lo usará para shutdown ordenado).
    #[allow(dead_code)]
    cancel: CancellationToken,
}

impl std::fmt::Debug for FlBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlBridge")
            .field("config", &self.inner.config)
            .field("heartbeat_age_ms", &"<async>")
            .finish()
    }
}

impl FlBridge {
    /// Abre los puertos MIDI y arranca el worker en background.
    pub fn connect(config: BridgeConfig) -> Result<Arc<Self>> {
        let midi_conn = open_midi_ports(
            Some(&config.port_to_fl),
            Some(&config.port_from_fl),
            &config.client_name,
        )?;

        let (request_tx, request_rx) = mpsc::unbounded_channel::<BridgeRequest>();
        let heartbeat = HeartbeatTracker::new();
        let cancel = CancellationToken::new();

        // Spawnea el MIDI worker en un thread OS dedicado.
        // Usa `std::thread` porque midir::MidiInputConnection requiere mantener
        // el connection vivo en un thread con callback, no se puede await.
        let worker_handle = spawn_midi_worker(
            request_rx,
            midi_conn,
            heartbeat.clone(),
            cancel.clone(),
        );

        let bridge = Arc::new(Self {
            inner: Arc::new(FlBridgeInner {
                config,
                request_tx,
                heartbeat,
                cancel,
            }),
        });

        // Worker handle guardado para cleanup futuro (Phase 5: watchdog)
        // Por ahora, el thread se cierra cuando el bridge se dropea (channel se cierra)
        std::mem::forget(worker_handle); // TODO: shutdown limpio

        Ok(bridge)
    }

    /// Espera al primer heartbeat (si `wait_for_first_heartbeat` está habilitado).
    pub async fn wait_ready(&self) -> Result<()> {
        if self.inner.config.wait_for_first_heartbeat {
            self.inner.heartbeat.wait_for_first(Duration::from_secs(5)).await?;
        }
        Ok(())
    }

    /// Estado del bridge (heartbeat age, alive).
    pub async fn health(&self) -> FlHealth {
        let info = self.inner.heartbeat.age().await;
        FlHealth {
            alive: self.inner.heartbeat.is_alive().await,
            heartbeat_age_ms: info.as_ref().map(|i| i.age.as_millis() as u64),
            fl_version: info.and_then(|i| i.fl_version),
        }
    }

    /// Llamada genérica a cualquier comando (escape hatch).
    pub async fn call(&self, command: &str, params: Value) -> Result<Value> {
        self.call_with_timeout(command, params, self.inner.config.default_timeout).await
    }

    /// Llamada con timeout explícito.
    pub async fn call_with_timeout(
        &self,
        command: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        // Verifica que FL está vivo antes de enviar (circuit breaker barato)
        if !self.inner.heartbeat.is_alive().await {
            return Err(HereticError::Other(
                "FL no responde (heartbeat stale). ¿Está corriendo?".into(),
            ));
        }

        let (response_tx, response_rx) = oneshot::channel();
        self.inner
            .request_tx
            .send(BridgeRequest {
                command: command.to_string(),
                params,
                response: response_tx,
            })
            .map_err(|_| HereticError::Other("MIDI worker channel cerrado".into()))?;

        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(HereticError::Other("response channel cerrado".into())),
            Err(_) => Err(HereticError::Other(format!(
                "timeout esperando response a '{}' ({}ms)",
                command,
                timeout.as_millis()
            ))),
        }
    }

    // ============================================================
    // Tools transport (mirror del FLStudioMCP legacy)
    // ============================================================

    /// `fl_ping` — eco del controller script.
    pub async fn ping(&self) -> Result<FlVersionInfo> {
        let data = self.call("ping", json!({})).await?;
        let fl_version = data
            .get("fl_version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HereticError::Other("ping response sin fl_version".into()))?
            .to_string();
        let protocol_version = data
            .get("protocol_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(crate::MIDI_PROTOCOL_VERSION as u64) as u32;
        Ok(FlVersionInfo {
            fl_version,
            protocol_version,
            raw: data,
        })
    }

    /// `fl_get_tempo` — BPM actual.
    pub async fn get_tempo(&self) -> Result<f64> {
        let data = self.call("get_tempo", json!({})).await?;
        data.get("bpm")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| HereticError::Other("get_tempo response sin bpm".into()))
    }

    /// `fl_set_tempo` — setea BPM (rango 10-999).
    pub async fn set_tempo(&self, bpm: f64) -> Result<f64> {
        if bpm < 10.0 || bpm > 999.0 {
            return Err(HereticError::Other(format!("bpm fuera de rango: {bpm}")));
        }
        let data = self.call("set_tempo", json!({ "bpm": bpm })).await?;
        data.get("bpm")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| HereticError::Other("set_tempo response sin bpm".into()))
    }

    /// `fl_play` — transport.start().
    pub async fn play(&self) -> Result<()> {
        let _ = self.call("play", json!({})).await?;
        Ok(())
    }

    /// `fl_stop` — transport.stop().
    pub async fn stop(&self) -> Result<()> {
        let _ = self.call("stop", json!({})).await?;
        Ok(())
    }

    /// `fl_get_song_position` — posición actual.
    pub async fn get_song_position(&self) -> Result<SongPosition> {
        let data = self.call("get_song_position", json!({})).await?;
        Ok(SongPosition {
            position_ms: data.get("position_ms").and_then(|v| v.as_f64()).unwrap_or(0.0),
            position_ticks: data.get("position_ticks").and_then(|v| v.as_i64()).unwrap_or(0),
            position_beats: data.get("position_beats").and_then(|v| v.as_f64()).unwrap_or(0.0),
            bpm: data.get("bpm").and_then(|v| v.as_f64()).unwrap_or(120.0),
        })
    }

    /// `fl_set_song_position` — por ms.
    pub async fn set_song_position_ms(&self, ms: f64) -> Result<SongPosition> {
        let data = self.call("set_song_position", json!({ "ms": ms })).await?;
        Ok(SongPosition {
            position_ms: data.get("position_ms").and_then(|v| v.as_f64()).unwrap_or(0.0),
            position_ticks: data.get("position_ticks").and_then(|v| v.as_i64()).unwrap_or(0),
            position_beats: data.get("position_beats").and_then(|v| v.as_f64()).unwrap_or(0.0),
            bpm: data.get("bpm").and_then(|v| v.as_f64()).unwrap_or(120.0),
        })
    }
}

/// Estado de salud del bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlHealth {
    pub alive: bool,
    pub heartbeat_age_ms: Option<u64>,
    pub fl_version: Option<String>,
}

/// Spawnea el MIDI worker en un thread OS dedicado.
///
/// Recibe requests por `request_rx` y mensajes MIDI entrantes por `midi_conn.incoming_rx`.
/// Mantiene un map `request_id -> oneshot::Sender` para correlación.
fn spawn_midi_worker(
    request_rx: mpsc::UnboundedReceiver<BridgeRequest>,
    midi_conn: MidiConnection,
    heartbeat: HeartbeatTracker,
    cancel: CancellationToken,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        run_midi_worker(request_rx, midi_conn, heartbeat, cancel);
    })
}

fn run_midi_worker(
    mut request_rx: mpsc::UnboundedReceiver<BridgeRequest>,
    mut midi_conn: MidiConnection,
    heartbeat: HeartbeatTracker,
    cancel: CancellationToken,
) {
    // Map: request_id -> oneshot::Sender para entregar el response cuando llegue.
    let pending: std::sync::Mutex<HashMap<String, oneshot::Sender<Result<Value>>>> =
        std::sync::Mutex::new(HashMap::new());

    // Bloqueamos el thread en el recv de MIDI. No podemos await en std::thread.
    // Pero mpsc::UnboundedReceiver se puede usar sync vía `try_recv` + sleep, o
    // vía `blocking_recv` (no existe para tokio). Usaremos un patrón sync/async:
    // el worker hace poll en un loop con `try_recv` y sleep corto.

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // 1. Procesar requests pendientes (no bloqueante)
        while let Ok(req) = request_rx.try_recv() {
            let id = new_request_id();
            let payload = json!({
                "v": crate::MIDI_PROTOCOL_VERSION,
                "cmd": req.command,
                "params": req.params,
            });
            let sysex = encode_message(Direction::Request, &id, &payload);
            if let Err(e) = send_sysex(&midi_conn.out, &sysex) {
                let _ = req.response.send(Err(e));
                continue;
            }
            // Guardamos el oneshot para entregar el response cuando llegue
            let mut p = pending.lock().expect("single-thread worker");
            p.insert(id, req.response);
        }

        // 2. Procesar mensajes MIDI entrantes (no bloqueante — usamos try_recv sync)
        //    El channel incoming_rx es async, pero como estamos en thread OS,
        //    hacemos try_recv. Si está vacío, sleepamos un poco.
        match midi_conn.incoming_rx.try_recv() {
            Ok(msg_bytes) => {
                if let Some(decoded) = decode_message(&msg_bytes) {
                    if decoded.direction == Direction::Heartbeat {
                        // Heartbeat → tracker
                        let h = heartbeat.clone();
                        let payload = decoded.payload.clone();
                        // Spawn tokio task para actualizar el tracker (es async)
                        tokio::spawn(async move {
                            h.record(payload).await;
                        });
                    } else if decoded.direction == Direction::Response {
                        // Response → match por request_id y entregar
                        let id = decoded.request_id.clone();
                        let payload = decoded.payload.clone();
                        let mut p = match pending.lock() {
                            Ok(g) => g,
                            Err(_) => continue,
                        };
                        if let Some(tx) = p.remove(&id) {
                            // FL puede devolver {"ok": false, "error": ...} o {"ok": true, "data": ...}
                            // El formato del FLStudioMCP legacy es: response.ok + response.data
                            let result = if payload.get("ok") == Some(&Value::Bool(true)) {
                                Ok(payload.get("data").cloned().unwrap_or(Value::Null))
                            } else {
                                let msg = payload
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("FL error")
                                    .to_string();
                                let code = payload
                                    .get("code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("fl_error")
                                    .to_string();
                                Err(HereticError::Other(format!("[{code}] {msg}")))
                            };
                            let _ = tx.send(result);
                        }
                    }
                    // Direction::Request desde FL no debería ocurrir (FL no nos envía requests)
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                // No hay mensajes — sleepamos un poco
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // Bridge dropeado — salimos
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_config_defaults() {
        let c = BridgeConfig::default();
        assert_eq!(c.port_to_fl, "FLStudioMCP RX");
        assert_eq!(c.port_from_fl, "FLStudioMCP TX");
        assert_eq!(c.default_timeout, Duration::from_secs(5));
        assert!(c.wait_for_first_heartbeat);
    }

    #[test]
    fn song_position_deserialize() {
        let json = r#"{"position_ms": 1000.0, "position_ticks": 96, "position_beats": 4.0, "bpm": 128.0}"#;
        let pos: SongPosition = serde_json::from_str(json).unwrap();
        assert_eq!(pos.position_ms, 1000.0);
        assert_eq!(pos.bpm, 128.0);
    }
}