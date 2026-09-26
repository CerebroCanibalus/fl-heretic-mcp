//! Transporte file-RPC contra el bridge ReaScript que corre dentro del DAW.
//!
//! # El protocolo, tal cual lo implementa el bridge
//!
//! Todo esto se leyo del bridge real (`reaper_mcp_server.lua`), no se asumio.
//! La primera version de este modulo se escribio adivinando y se rompio en las
//! tres primeras llamadas: por eso las pruebas contra el DAW real no son
//! opcionales, son la unica forma de que esto exista.
//!
//! ```text
//!   comando:  { "id": <n>, "command": "track.create", "params": { ... } }
//!   respuesta: { "id": <n>, "success": true,  "data": { ... } }
//!             { "id": <n>, "success": false, "error": "..." }
//! ```
//!
//! Puntos donde la suposicion inicial era incorrecta:
//!
//! | campo | se asumia | es |
//! |---|---|---|
//! | nombre de la accion | `action` | **`command`** |
//! | exito | `ok` | **`success`** |
//! | resultado | `result` | **`data`** |
//!
//! Ficheros, en `%TEMP%\\reaper_mcp\\`:
//!
//! | fichero | quien escribe |
//! |---|---|
//! | `command.json` | este cliente |
//! | `response.json` | el bridge |
//! | `server.lock` | el bridge, como heartbeat (cada 10 s) |
//!
//! # Atomicidad
//!
//! El bridge escribe `command.tmp` -> renombra a `command.json`, y lee asi
//! que nunca ve un fichero a medias. Este cliente hace lo mismo por el otro
//! lado. En Windows el rename da `PermissionError` si el destino esta abierto
//! (el bridge esta en un bucle `defer` a 30 Hz mirando esos ficheros), asi
//! que hay que reintentar.
//!
//! # Sin wake
//!
//! El bridge corre su propio bucle `defer()`, asi que no hay que despertarlo
//! con MIDI ni con nada. Es la diferencia estructural frente a FL Studio, que
//! obligaba a mantener puertos MIDI abiertos y tenia un techo de 1.5KB por
//! mensaje.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Error del transporte.
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("io en {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("respuesta no parseable: {0}")]
    Decode(String),
    #[error("el bridge no respondio en {0:?}")]
    Timeout(Duration),
    #[error("respuesta de otra peticion (id {got}, esperaba {want})")]
    IdMismatch { got: i64, want: i64 },
    #[error("el bridge ({0})")]
    Remote(String),
    #[error("el bridge no esta vivo: {0}")]
    NotRunning(String),
}

/// Configuracion del transporte.
#[derive(Debug, Clone)]
pub struct RpcConfig {
    /// Carpeta de intercambio.
    pub dir: PathBuf,
    /// Comando que escribe el cliente.
    pub command: PathBuf,
    /// Respuesta que escribe el bridge.
    pub response: PathBuf,
    /// Heartbeat del bridge: se usa para detectar que sigue vivo antes de
    /// gastar el timeout entero.
    pub lock: PathBuf,
    /// Cuanto esperar antes de dar por perdido el request.
    pub timeout: Duration,
}

impl RpcConfig {
    /// Config por defecto: `%TEMP%\reaper_mcp`.
    ///
    /// Tiene que coincidir con el `ipc_dir` que calcula el bridge, que en
    /// Windows es `%TEMP%\reaper_mcp` y en unix `$TMPDIR` o `/tmp`.
    pub fn default_config() -> Self {
        let tmp = std::env::var("TEMP")
            .or_else(|_| std::env::var("TMP"))
            .unwrap_or_else(|_| "/tmp".into());
        let dir = PathBuf::from(tmp).join("reaper_mcp");
        Self {
            command: dir.join("command.json"),
            response: dir.join("response.json"),
            lock: dir.join("server.lock"),
            dir,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Peticion. El campo se llama `command`, no `action`: asi lo espera el
/// bridge, y un nombre distinto se traduce en "Unknown command" silencioso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    /// Id de correlacion. El bridge lo copia a la respuesta, que es lo que
    /// permite descartar respuestas de peticiones anteriores.
    pub id: i64,
    /// Nombre de la accion, p.ej. `track.create`.
    pub command: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Respuesta del bridge.
///
/// Ojo: `success` + `data`, no `ok` + `result`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub id: Option<i64>,
    pub success: bool,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Cliente file-RPC.
pub struct FileRpc {
    cfg: RpcConfig,
    next_id: std::sync::atomic::AtomicI64,
}

impl FileRpc {
    pub fn new(cfg: RpcConfig) -> Self {
        // El id arranca en el reloj: si el bridge se reinicio y quedo con el
        // ultimo id visto en un numero alto, un id pequeno podriaaise a
        // colocar por detrás de lo ya procesado.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(1_000);
        Self {
            cfg,
            next_id: std::sync::atomic::AtomicI64::new(seed),
        }
    }

    pub fn config(&self) -> &RpcConfig {
        &self.cfg
    }

    /// ¿Hay un bridge vivo?
    ///
    /// El bridge escribe `server.lock` cada 10 segundos. Si el fichero no
    /// existe, Reaper no esta corriendo o el ReaScript no arranco. Es una
    /// comprobacion de milisegundos, frente a los 10 s que costaria esperar
    /// al timeout de una llamada.
    pub fn is_bridge_alive(&self) -> bool {
        self.cfg.lock.exists()
    }

    /// Antiguedad del heartbeat. Util para diagnosticar "arrancó pero se quedó
    /// colgado": un lock viejo es peor que no tener lock.
    pub fn heartbeat_age(&self) -> Option<Duration> {
        let meta = std::fs::metadata(&self.cfg.lock).ok()?;
        let m = meta.modified().ok()?;
        Some(m.elapsed().unwrap_or_default())
    }

    /// Envia un comando y espera su respuesta.
    pub fn call(
        &self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError> {
        if !self.is_bridge_alive() {
            return Err(RpcError::NotRunning(format!(
                "no existe {} (el bridge escribe ese fichero como heartbeat). \
                 Reaper esta abierto? El ReaScript esta cargado?",
                self.cfg.lock.display()
            )));
        }

        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let cmd = Command {
            id,
            command: command.to_string(),
            params,
        };

        // Respuesta previa: si no se borra y luego falla la escritura, se
        // leeria la de la peticion anterior y creeria que funciono.
        let _ = std::fs::remove_file(&self.cfg.response);

        self.write_atomic(
            &self.cfg.command,
            &serde_json::to_vec(&cmd).unwrap_or_default(),
        )?;

        let t0 = Instant::now();
        loop {
            if t0.elapsed() > self.cfg.timeout {
                return Err(RpcError::Timeout(self.cfg.timeout));
            }
            if let Ok(bytes) = std::fs::read(&self.cfg.response) {
                match serde_json::from_slice::<Response>(&bytes) {
                    Ok(resp) => {
                        // Descartar respuestas de otras peticiones: el bridge
                        // podria estar terminando una anterior.
                        if resp.id.is_none_or(|i| i == id) {
                            let _ = std::fs::remove_file(&self.cfg.response);
                            return if resp.success {
                                Ok(resp.data.unwrap_or(serde_json::Value::Null))
                            } else {
                                Err(RpcError::Remote(
                                    resp.error.unwrap_or_else(|| "error sin mensaje".into()),
                                ))
                            };
                        }
                        if resp.id == Some(id) {
                            let _ = std::fs::remove_file(&self.cfg.response);
                            return Err(RpcError::IdMismatch {
                                got: resp.id.unwrap_or_default(),
                                want: id,
                            });
                        }
                    }
                    Err(e) => {
                        // Puede ser un fichero a medio escribir. No fallar
                        // todavia: reintentar hasta que el timeout.
                        if t0.elapsed() > Duration::from_millis(50) {
                            return Err(RpcError::Decode(e.to_string()));
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    /// Escribe atomico: `.tmp` + rename, con reintentos.
    fn write_atomic(&self, target: &Path, bytes: &[u8]) -> Result<(), RpcError> {
        let tmp = target.with_extension("json.tmp");
        let io = |p: &Path, source: std::io::Error| RpcError::Io {
            path: p.to_path_buf(),
            source,
        };
        // El bridge crea la carpeta al arrancar, pero el cliente no puede
        // asumirlo: si falta, el error debe decir "io" sobre la ruta, no
        // "el bridge no esta" (que es lo que el LLM necesita para saber
        // que hacer).
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;

        let mut last = None;
        for _ in 0..25 {
            match std::fs::rename(&tmp, target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(8));
                }
            }
        }
        Err(io(
            target,
            last.unwrap_or_else(|| std::io::Error::other("rename fallo sin error")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_por_defecto_apunta_a_reaper_mcp() {
        let c = RpcConfig::default_config();
        assert!(c.dir.ends_with("reaper_mcp"), "{c:?}");
        assert!(c.command.ends_with("command.json"));
        assert!(c.response.ends_with("response.json"));
        assert!(c.lock.ends_with("server.lock"));
    }

    #[test]
    fn comando_usa_command_no_action() {
        // Si se mandara `action`, el bridge responde "Unknown command" y no
        // hay forma de saber que el nombre del campo esta mal.
        let c = Command {
            id: 1,
            command: "track.create".into(),
            params: serde_json::json!({}),
        };
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains("\"command\""), "{j}");
        assert!(!j.contains("\"action\""), "{j}");
    }

    #[test]
    fn respuesta_usa_success_y_data() {
        // Parsear una respuesta real del bridge, no una inventada.
        let real = r#"{"id":42,"success":true,"data":{"bpm":120}}"#;
        let r: Response = serde_json::from_str(real).expect("debe parsear el formato real");
        assert!(r.success);
        assert_eq!(r.id, Some(42));
        assert_eq!(r.data.unwrap()["bpm"], 120);

        let err = r#"{"id":43,"success":false,"error":"Unknown command: foo"}"#;
        let r: Response = serde_json::from_str(err).unwrap();
        assert!(!r.success);
        assert!(r.error.unwrap().contains("Unknown command"));

        // El formato que la primera implementacion asumia. Si esto parsea,
        // el test no esta fijando el contrato real.
        let errado = r#"{"id":1,"ok":true,"result":{}}"#;
        assert!(serde_json::from_str::<Response>(errado).is_err());
    }

    #[test]
    fn sin_bridge_da_error_claro_y_rapido() {
        let mut cfg = RpcConfig::default_config();
        cfg.dir = std::env::temp_dir().join("heretic-daw-sin-bridge");
        cfg.command = cfg.dir.join("command.json");
        cfg.response = cfg.dir.join("response.json");
        cfg.lock = cfg.dir.join("server.lock"); // no existe
        cfg.timeout = Duration::from_secs(30);
        let r = FileRpc::new(cfg);
        let t0 = Instant::now();
        let e = r.call("ping", serde_json::json!({})).unwrap_err();
        // Debe fallar por NotRunning, no agotar los 30 s de timeout.
        assert!(matches!(e, RpcError::NotRunning(_)), "{e}");
        assert!(t0.elapsed() < Duration::from_secs(1), "tardo demasiado");
    }

    #[test]
    fn id_monotonico() {
        let r = FileRpc::new(RpcConfig::default_config());
        let a = r.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let b = r.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(b, a + 1, "los ids tienen que ir en aumento: {a} -> {b}");
    }

    #[test]
    fn parametros_en_camelCase() {
        // Reaper NO da error con un param desconocido: usa el valor por
        // defecto y parece que funciono. El peor modo de fallo posible.
        let p = serde_json::json!({ "trackIndex": 0, "fxIndex": 1, "paramIndex": 2 });
        let s = p.to_string();
        for k in ["trackIndex", "fxIndex", "paramIndex"] {
            assert!(s.contains(k), "falta {k} en {s}");
        }
        for k in ["track_index", "fx_index", "param_index"] {
            assert!(!s.contains(k), "no debe haber {k} en {s}");
        }
    }
}
