//! Transporte file-RPC contra el FL Heretic Bridge que corre dentro de FL Studio.
//!
//! # Por qué file-RPC y no nada más
//!
//! El sandbox del sub-intérprete de FL Studio 2025 **no deja** crear sockets
//! (`<slot wrapper '__init__' of '_socket.socket' returned NULL`), ni
//! importar `ctypes`, ni usar `os.rename` / `os.mkdir` / `os.unlink`, ni
//! listar directorios. Lo único que aguanta es `open()` de lectura y escritura
//! en su propio directorio. Medido en `tests/probe_sandbox.py`.
//!
//! # Mailbox rotativo
//!
//! El bridge escribe `hr_req_<n>.json` y lee `hr_resp_<n>.json`, con
//! `n = id % 8`. Un solo par de ficheros Suffice para un cliente, pero con el
//! mailbox:
//!
//! - Una request a medio escribir vive en un slot y no pisa la anterior.
//! - El daemon puede tener varias rounds en vuelo sin perder ninguna.
//! - Un `resp` viejo de otro slot no se confunde con el actual: el id se
//!   comprueba siempre contra el que se pidió.
//!
//! # Protocolo
//!
//! ```text
//! request   {"id": <u64>, "action": "channels.setVolume", "params": {...}}
//! response  {"id": <u64>, "ok": true,  "result": ...}\n
//!           {"id": <u64>, "ok": false, "error": "...", "traceback": "..."}\n
//! ```
//!
//! La respuesta **siempre** termina en `\n`. El script de FL no puede hacer
//! rename (bloqueado), así que escribe el fichero directamente y podría
//! dejarse a medias; el `\n` final es el marcador de "esto ya está completo".
//! El que lee descarta cualquier cosa que no termine en `\n` o no parsee.
//!
//! El `id` es estrictamente creciente (nanosegundos del reloj), así que también
//! lo es entre reinicios del daemon. El script solo procesa un id distinto al
//! último que vio, así que cualquier valor monotónico sirve.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::Instant;

use heretic_core::{HereticError, Result};

/// Intervalo de sondeo del fichero de respuesta.
const POLL_INTERVAL: Duration = Duration::from_millis(8);

/// Timeout por defecto. FL puede dejar de disparar callbacks si hay un diálogo
/// modal abierto, así que damos margen generoso.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Número de slots del mailbox. Debe coincidir con `SLOT_COUNT` del script.
const SLOT_COUNT: usize = 8;

/// Cliente file-RPC.
pub struct FileRpc {
    dir: PathBuf,
    timeout: Duration,
    /// Serializa las peticiones: el bridge solo entiende de una en una por
    /// slot, y dos escrituras concurrentes al mismo slot se pisarían.
    gate: Mutex<()>,
    next_id: AtomicU64,
}

impl std::fmt::Debug for FileRpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileRpc")
            .field("dir", &self.dir)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl FileRpc {
    /// Crea un cliente sobre un directorio de script.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            timeout: DEFAULT_TIMEOUT,
            gate: Mutex::new(()),
            next_id: AtomicU64::new(seed_id()),
        }
    }

    /// Override del timeout por petición.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Directorio del script (diagnóstico).
    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    fn req_path(&self, slot: usize) -> PathBuf {
        self.dir.join(format!("hr_req_{slot}.json"))
    }

    fn resp_path(&self, slot: usize) -> PathBuf {
        self.dir.join(format!("hr_resp_{slot}.json"))
    }

    /// Invoca una action y devuelve su `result`.
    ///
    /// `after_write` se ejecuta justo después de dejar la request en disco y
    /// antes de esperar la respuesta. El daemon le pasa el wake por MIDI, y el
    /// **orden importa**: FL tiene que encontrar la request cuando despierte.
    /// Si se despierta antes de que el fichero exista, no ve nada y no vuelve
    /// a mirar hasta el siguiente evento MIDI.
    pub async fn call_with_wake<F>(
        &self,
        action: &str,
        params: &Value,
        after_write: F,
    ) -> Result<Value>
    where
        F: FnOnce(),
    {
        let _guard = self.gate.lock().await;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let slot = (id % SLOT_COUNT as u64) as usize;

        // Limpiar la respuesta de este slot para no leer la de una petición
        // anterior mientras esperamos. Es lo que hace también el script en
        // OnInit, pero aquí es por petición.
        let resp = self.resp_path(slot);
        let _ = tokio::fs::write(&resp, "").await;

        // 1. Escribir la request de forma atómica (temp + rename) para que FL
        //    nunca lea un JSON truncado. El daemon corre FUERA del sandbox, así
        //    que rename sí está disponible aquí.
        let payload = json!({ "id": id, "action": action, "params": params });
        let bytes = serde_json::to_vec(&payload)
            .map_err(|e| HereticError::Other(format!("serializando petición: {e}")))?;

        let req = self.req_path(slot);
        let tmp = {
            let mut p = req.clone().into_os_string();
            p.push(".tmp");
            PathBuf::from(p)
        };
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| HereticError::Other(format!("escribiendo {}: {e}", tmp.display())))?;
        match tokio::fs::rename(&tmp, &req).await {
            Ok(()) => {}
            Err(e) => {
                // En Windows rename() falla con PermissionError si FL tiene el
                // fichero destino abierto (y lo tiene: lo acaba de leer). Se
                // cae a escritura directa. El script descarta un JSON ilegible
                // y reintenta en el siguiente pump, así que no se pierde nada.
                tracing::debug!(error = %e, slot, "rename atómico falló; escribiendo directo");
                tokio::fs::write(&req, &bytes)
                    .await
                    .map_err(|e| {
                        HereticError::Other(format!("escribiendo {}: {e}", req.display()))
                    })?;
            }
        }

        // 2. Despertar a FL. Siempre DESPUÉS de escribir.
        after_write();

        // 3. Esperar la respuesta con NUESTRO id. El script escribe sin
        //    atomicidad, así que una lectura a medias se descarta y se reintenta.
        let deadline = Instant::now() + self.timeout;
        loop {
            if Instant::now() >= deadline {
                return Err(HereticError::Other(format!(
                    "timeout de {:?} esperando '{action}' (id={id}, slot={slot}).\n\
                     ¿Está FL Studio abierto con el controller script 'FL Heretic Bridge' \
                     seleccionado en Options > MIDI Settings, sin diálogos modales?\n\
                     Si el wake MIDI no llega, FL nunca despacha: mira el campo \
                     'midi_wake_ports' de fl_status. Si es 0, no hay puertos MIDI \
                     OUT abiertos (¿loopMIDI corriendo?).",
                    self.timeout
                )));
            }
            tokio::time::sleep(POLL_INTERVAL).await;

            let Ok(raw) = tokio::fs::read_to_string(&resp).await else {
                continue;
            };
            // Marcador de escritura completa. El script no puede hacer rename,
            // así que sin esto no hay forma de saber si lo leído está entero.
            if !raw.ends_with('\n') {
                continue;
            }
            let body = match raw.split_once('\n') {
                Some((b, _)) => b,
                None => continue,
            };
            let Ok(v) = serde_json::from_str::<Value>(body) else {
                continue; // JSON a medias
            };
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // respuesta de una petición anterior en este slot
            }

            if v.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }

            let mut msg = v
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("error desconocido del bridge")
                .to_string();
            if let Some(tb) = v.get("traceback").and_then(Value::as_str) {
                if !tb.is_empty() {
                    msg.push_str("\n--- traceback FL ---\n");
                    msg.push_str(tb);
                }
            }
            return Err(HereticError::Other(format!("bridge '{action}': {msg}")));
        }
    }

    /// Invoca una action sin wake (para tests, o si el pump viene de otro sitio).
    pub async fn call(&self, action: &str, params: &Value) -> Result<Value> {
        self.call_with_wake(action, params, || {}).await
    }
}

/// Id inicial monotónico: nanosegundos del reloj del sistema, para que también
/// sea creciente entre reinicios del daemon. Nunca 0: el script trata `id == 0`
/// como "sin id".
fn seed_id() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1) as u64;
    nanos.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_nonzero_and_monotonic() {
        let a = seed_id();
        let b = seed_id();
        assert!(a > 0 && b > 0);
        assert!(b >= a, "los ids deben ser no decrecientes");
    }

    #[test]
    fn los_slots_cubren_el_rango_del_id() {
        // El id es monotónico y el slot es id % 8: dos peticiones seguidas
        // caen en slots distintos, que es justo lo que evita el pisado.
        let id1 = 1790371383561325900u64;
        let id2 = id1 + 1;
        assert_ne!(id1 % 8, id2 % 8);
    }

    #[test]
    fn los_nombres_de_fichero_son_los_del_script() {
        let dir = PathBuf::from("/tmp/x");
        let rpc = FileRpc::new(dir);
        assert_eq!(
            rpc.req_path(3),
            PathBuf::from("/tmp/x").join("hr_req_3.json")
        );
        assert_eq!(
            rpc.resp_path(3),
            PathBuf::from("/tmp/x").join("hr_resp_3.json")
        );
    }

    #[tokio::test]
    async fn timeout_cuando_no_hay_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let rpc = FileRpc::new(dir.path()).with_timeout(Duration::from_millis(120));
        let err = rpc.call("meta.ping", &json!({})).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("timeout"), "mensaje inesperado: {msg}");
    }

    #[tokio::test]
    async fn escribe_la_peticion_en_el_slot_correcto() {
        let dir = tempfile::tempdir().unwrap();
        let rpc = FileRpc::new(dir.path()).with_timeout(Duration::from_millis(150));
        let _ = rpc.call("meta.ping", &json!({})).await;
        // Tiene que haber escrito en exactamente un hr_req_*.json, no en un
        // rpc_request.json (el nombre del diseño viejo).
        let mut found = 0;
        for slot in 0..SLOT_COUNT {
            let p = dir.path().join(format!("hr_req_{slot}.json"));
            if p.exists() && std::fs::read_to_string(&p).unwrap().len() > 0 {
                found += 1;
                let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
                assert_eq!(v["action"], "meta.ping");
                assert!(v["id"].as_u64().unwrap() > 0);
            }
        }
        assert_eq!(found, 1, "debería escribir en exactamente un slot");
        assert!(
            !dir.path().join("rpc_request.json").exists(),
            "no debe usar el nombre del diseño antiguo"
        );
        assert!(!dir.path().join("hr_req_0.json.tmp").exists());
    }

    #[tokio::test]
    async fn el_hook_se_dispara_despues_de_escribir() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        let dir = tempfile::tempdir().unwrap();
        let rpc = FileRpc::new(dir.path()).with_timeout(Duration::from_millis(150));
        let flag = Arc::new(AtomicBool::new(false));
        let f2 = flag.clone();
        let _ = rpc
            .call_with_wake("meta.ping", &json!({}), move || {
                // Cuando el hook corre, la request ya tiene que estar escrita.
                let mut written = false;
                for slot in 0..SLOT_COUNT {
                    let p = dir.path().join(format!("hr_req_{slot}.json"));
                    if p.exists() && std::fs::read_to_string(&p).unwrap_or_default().len() > 0 {
                        written = true;
                    }
                }
                assert!(written, "el wake se disparó antes de escribir la request");
                f2.store(true, Ordering::SeqCst);
            })
            .await;
        assert!(flag.load(Ordering::SeqCst), "el hook no se llegó a disparar");
    }
}
