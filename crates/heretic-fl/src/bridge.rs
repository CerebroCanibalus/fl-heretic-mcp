//! Cliente TCP simple al VST3 plugin.
//!
//! El plugin VST3 corre dentro de FL Studio y expone un servidor TCP en
//! 127.0.0.1:9790 que actúa como PROXY TCP↔file-RPC al FL Heretic Bridge.
//!
//! Este módulo es el cliente Rust que habla con el VST3 vía TCP JSON-RPC.
//!
//! Protocolo (idéntico al fLMCP Bridge):
//! - Frame: [4 bytes BE u32 length][payload JSON]
//! - Request:  {"id": int, "action": str, "params": {...}}
//! - Response: {"id": int, "ok": bool, "result": ..., "error": str|None}
//!
//! Topología:
//! ```text
//! [daemon Rust] --TCP 127.0.0.1:9790--> [VST3 plugin] --file-RPC--> [FL Heretic Bridge] --FL API--> FL Studio
//! ```

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use heretic_core::{HereticError, Result};

const HEADER_SIZE: usize = 4;
const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Config del bridge (cliente TCP al VST3).
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9790,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Versión de FL reportada por el bridge (via meta.ping).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlVersionInfo {
    pub bridge_version: String,
    pub fl_version: String,
    pub uptime_sec: f64,
    pub script_dir: Option<String>,
}

/// Posición de canción reportada por FL (de transport.status).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongPosition {
    pub position_ticks: i64,
    pub position_bars: f64,
    pub position_seconds: f64,
    pub bpm: f64,
}

/// Estado del bridge (de transport.status).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportStatus {
    pub is_playing: bool,
    pub is_recording: bool,
    pub bpm: f64,
    pub position_ticks: i64,
    pub position_bars: f64,
    pub position_seconds: f64,
}

/// Cliente TCP al VST3 plugin.
///
/// Mantiene UNA conexión persistente con reconexión automática.
#[derive(Clone)]
pub struct FlBridge {
    inner: Arc<FlBridgeInner>,
}

struct FlBridgeInner {
    config: BridgeConfig,
    stream: Mutex<Option<TcpStream>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for FlBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlBridge")
            .field("config", &self.inner.config)
            .finish()
    }
}

impl FlBridge {
    /// Crea un bridge (no conecta inmediatamente — lazy en primer `call`).
    pub fn connect(config: BridgeConfig) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            inner: Arc::new(FlBridgeInner {
                config,
                stream: Mutex::new(None),
                next_id: std::sync::atomic::AtomicU64::new(1),
            }),
        }))
    }

    /// Espera a que el VST3 plugin esté listo (verifica con meta.ping).
    pub async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.ping().await {
                Ok(_) => return Ok(()),
                Err(_) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(HereticError::Other(format!(
                            "VST3 plugin no responde en {}s. ¿FL Studio está corriendo con el plugin cargado?",
                            timeout.as_secs()
                        )));
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    async fn ensure_connected(&self) -> Result<()> {
        let mut g = self.inner.stream.lock().await;
        if g.is_none() {
            tracing::debug!("[vst3-bridge] conectando a {}:{}", self.inner.config.host, self.inner.config.port);
            let stream = tokio::time::timeout(
                Duration::from_secs(5),
                TcpStream::connect((self.inner.config.host.as_str(), self.inner.config.port)),
            )
            .await
            .map_err(|_| HereticError::Other("timeout conectando a VST3 plugin".into()))?
            .map_err(|e| HereticError::Other(format!("VST3 connect falló: {e}")))?;
            stream.set_nodelay(true).ok();
            *g = Some(stream);
            tracing::debug!("[vst3-bridge] conectado a VST3 plugin");
        }
        Ok(())
    }

    fn next_id(&self) -> i64 {
        self.inner
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed) as i64
    }

    /// Llamada raw al bridge (cualquier action).
    pub async fn call(&self, action: &str, params: Value) -> Result<Value> {
        // Retry una vez si la conexión se cayó
        for attempt in 0..2 {
            if let Err(e) = self.ensure_connected().await {
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                return Err(e);
            }
            let id = self.next_id();
            let req = json!({"id": id, "action": action, "params": params});
            let body = serde_json::to_vec(&req)?;

            let mut g = self.inner.stream.lock().await;
            let stream = match g.as_mut() {
                Some(s) => s,
                None => {
                    drop(g);
                    if attempt == 0 {
                        continue;
                    }
                    return Err(HereticError::Other("VST3 stream not initialized".into()));
                }
            };

            // 1. Enviar frame
            let len = body.len() as u32;
            if let Err(e) = tokio::time::timeout(
                self.inner.config.timeout,
                async {
                    stream.write_all(&len.to_be_bytes()).await?;
                    stream.write_all(&body).await?;
                    stream.flush().await?;
                    Ok::<(), std::io::Error>(())
                },
            )
            .await
            {
                *g = None; // reset connection
                drop(g);
                if attempt == 0 {
                    continue;
                }
                return Err(HereticError::Other(format!("VST3 write timeout: {e}")));
            }

            // 2. Leer response
            let mut header = [0u8; HEADER_SIZE];
            match tokio::time::timeout(self.inner.config.timeout, stream.read_exact(&mut header)).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    *g = None;
                    drop(g);
                    if attempt == 0 {
                        continue;
                    }
                    return Err(HereticError::Other(format!("VST3 read header: {e}")));
                }
                Err(_) => {
                    *g = None;
                    drop(g);
                    if attempt == 0 {
                        continue;
                    }
                    return Err(HereticError::Other(format!("VST3 read timeout (action: {action})")));
                }
            }
            let resp_len = u32::from_be_bytes(header) as usize;
            if resp_len > MAX_FRAME {
                *g = None;
                drop(g);
                return Err(HereticError::Other(format!("VST3 response demasiado grande: {resp_len}")));
            }
            let mut body = vec![0u8; resp_len];
            match tokio::time::timeout(self.inner.config.timeout, stream.read_exact(&mut body)).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    *g = None;
                    drop(g);
                    if attempt == 0 {
                        continue;
                    }
                    return Err(HereticError::Other(format!("VST3 read body: {e}")));
                }
                Err(_) => {
                    *g = None;
                    drop(g);
                    if attempt == 0 {
                        continue;
                    }
                    return Err(HereticError::Other(format!("VST3 read body timeout (action: {action})")));
                }
            }
            drop(g);

            // 3. Parsear
            let resp: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(e) => {
                    return Err(HereticError::Other(format!(
                        "VST3 response parse error: {e}, body={}",
                        String::from_utf8_lossy(&body[..body.len().min(200)])
                    )));
                }
            };

            if resp.get("ok") == Some(&Value::Bool(true)) {
                return Ok(resp.get("result").cloned().unwrap_or(Value::Null));
            } else {
                let err = resp
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                return Err(HereticError::Other(format!(
                    "VST3 action '{action}' error: {err}"
                )));
            }
        }
        Err(HereticError::Other("VST3 call failed after retries".into()))
    }

    // ============================================================
    // Tools transport (mapean a actions fLMCP Bridge via VST3 proxy)
    // ============================================================

    /// `meta.ping` — health check del bridge.
    pub async fn ping(&self) -> Result<FlVersionInfo> {
        let data = self.call("meta.ping", json!({})).await?;
        Ok(FlVersionInfo {
            bridge_version: data
                .get("bridge_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            fl_version: data
                .get("fl_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            uptime_sec: data
                .get("uptime_sec")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            script_dir: data.get("script_dir").and_then(|v| v.as_str()).map(String::from),
        })
    }

    /// `transport.status` — devuelve estado completo (playing, recording, tempo, position).
    pub async fn transport_status(&self) -> Result<TransportStatus> {
        let data = self.call("transport.status", json!({})).await?;
        Ok(TransportStatus {
            is_playing: data.get("is_playing").and_then(|v| v.as_bool()).unwrap_or(false),
            is_recording: data.get("is_recording").and_then(|v| v.as_bool()).unwrap_or(false),
            bpm: data.get("bpm").and_then(|v| v.as_f64()).unwrap_or(120.0),
            position_ticks: data.get("position_ticks").and_then(|v| v.as_i64()).unwrap_or(0),
            position_bars: data.get("position_bars").and_then(|v| v.as_f64()).unwrap_or(0.0),
            position_seconds: data.get("position_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0),
        })
    }

    /// `transport.start`.
    pub async fn play(&self) -> Result<()> {
        let _ = self.call("transport.start", json!({})).await?;
        Ok(())
    }

    /// `transport.stop`.
    pub async fn stop(&self) -> Result<()> {
        let _ = self.call("transport.stop", json!({})).await?;
        Ok(())
    }

    /// `transport.set_tempo`.
    pub async fn set_tempo(&self, bpm: f64) -> Result<f64> {
        if !(10.0..=999.0).contains(&bpm) {
            return Err(HereticError::Other(format!("bpm fuera de rango 10-999: {bpm}")));
        }
        let data = self.call("transport.set_tempo", json!({ "bpm": bpm })).await?;
        data.get("bpm")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| HereticError::Other("set_tempo response sin bpm".into()))
    }

    /// `transport.set_position` — units: "bars" (default), "ms", "seconds", "ticks", "steps".
    pub async fn set_position(&self, position: f64, unit: &str) -> Result<TransportStatus> {
        let _ = self
            .call(
                "transport.set_position",
                json!({ "position": position, "unit": unit }),
            )
            .await?;
        self.transport_status().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c = BridgeConfig::default();
        assert_eq!(c.host, "127.0.0.1");
        assert_eq!(c.port, 9790);
    }
}