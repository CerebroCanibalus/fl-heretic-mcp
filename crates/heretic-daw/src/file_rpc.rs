//! Transporte file-RPC contra el bridge que corre DENTRO del DAW.
//!
//! # El patron
//!
//! El bridge (un ReaScript Lua dentro de REAPER) no puede abrir un listener de
//! red de forma fiable, pero si tiene file I/O libre. Asi que la comunicacion
//! es por ficheros, con un `defer()` en el DAW que los va mirando:
//!
//! ```text
//! MCP (Rust)  --escribe-->  command.json   [dentro del DAW]
//! MCP (Rust)  <--lee--     response.json  [dentro del DAW]
//! ```
//!
//! Esto es lo mismo que hacia el bridge de FL (mailbox de 8 slots con commit
//! marker), con una diferencia importante: **el patron de Fichero unico
//! necesita un wake**. En FL habia que mandar MIDI para que el DAW se
//! despertara. En REAPER no: el ReaScript corre su propio bucle `defer()`,
//! que se ejecuta a ~30 Hz sin que nadie lo despierte.
//!
//! Esa es una mejora real frente a FL y no es un detalle: elimina de raiz la
//! necesidad del wake MIDI y con ella el techo de 1.5KB y toda la clase de
//! fallos "el DAW no responde" queunieramos tanto en FL.
//!
//! # Atomicidad
//!
//! Se escribe primero en `.tmp` y luego se renombra, para que el DAW nunca lea
//! un fichero a medio escribir. En Windows `os.replace` de Python puede dar
//! `PermissionError` si el destino esta abierto, asi que se reintenta.

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
    #[error("respuesta invalida: {0}")]
    Decode(String),
    #[error("el bridge no respondio en {0:?}")]
    Timeout(Duration),
    #[error("respuesta con error: {0}")]
    Remote(String),
}

/// Configuracion del transporte.
#[derive(Debug, Clone)]
pub struct RpcConfig {
    /// Carpeta de intercambio (la usa el bridge como ipc_dir).
    pub dir: PathBuf,
    /// Fichero de comandos que escribe el cliente.
    pub command: PathBuf,
    /// Fichero de respuestas que escribe el bridge.
    pub response: PathBuf,
    /// Cuanto esperar antes de dar por perdido el request.
    pub timeout: Duration,
}

impl RpcConfig {
    /// Config por defecto: `%TEMP%\reaper_mcp`, que es donde el bridge Lua
    /// de xDarkzx pone su `ipc_dir` en Windows.
    pub fn default_config() -> Self {
        let tmp = std::env::var("TEMP")
            .or_else(|_| std::env::var("TMP"))
            .unwrap_or_else(|_| "C:\\Windows\\Temp".into());
        let dir = PathBuf::from(tmp).join("reaper_mcp");
        Self {
            command: dir.join("command.json"),
            response: dir.join("response.json"),
            dir,
            timeout: Duration::from_secs(10),
        }
    }
}

/// Peticion que se escribe en `command.json`.
///
/// El `id` es un contador monotonico, no un timestamp: el bridge compara
/// contra el ultimo id visto, asi que tiene que ir siempre en Increase para
/// que no se pierda ninguno.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: u64,
    pub action: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Respuesta que se lee de `response.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
}

/// Cliente file-RPC.
pub struct FileRpc {
    cfg: RpcConfig,
    next_id: std::sync::atomic::AtomicU64,
}

impl FileRpc {
    pub fn new(cfg: RpcConfig) -> Self {
        // El id arranca alto para no colisionar con una sesion anterior del
        // bridge, que recuerda el ultimo id que vio.
        let next_id = std::sync::atomic::AtomicU64::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(1_000),
        );
        Self { cfg, next_id }
    }

    pub fn config(&self) -> &RpcConfig {
        &self.cfg
    }

    /// Escribe una peticion y espera su respuesta.
    pub fn call(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let cmd = Command { id, action: action.to_string(), params };

        // Respuesta viejo: si no se limpia, un fallo de escritura nos haria
        // leer la respuesta de la peticion anterior y creernos que funciono.
        let _ = std::fs::remove_file(&self.cfg.response);

        self.write_atomic(&self.cfg.command, &serde_json::to_vec(&cmd).unwrap_or_default())?;

        let t0 = Instant::now();
        let poll = Duration::from_millis(5);
        loop {
            if t0.elapsed() > self.cfg.timeout {
                return Err(RpcError::Timeout(self.cfg.timeout));
            }
            if let Ok(bytes) = std::fs::read(&self.cfg.response) {
                if let Ok(resp) = serde_json::from_slice::<Response>(&bytes) {
                    // Ignorar respuestas de ids viejos: el bridge puede estar
                    // terminando una peticion anterior.
                    if resp.id == id {
                        let _ = std::fs::remove_file(&self.cfg.response);
                        return if resp.ok {
                            Ok(resp.result)
                        } else {
                            Err(RpcError::Remote(
                                resp.error.unwrap_or_else(|| "error sin mensaje".into()),
                            ))
                        };
                    }
                }
            }
            std::thread::sleep(poll);
        }
    }

    /// Escribe atomico: `.tmp` + rename, con reintentos.
    ///
    /// En Windows el rename falla con `PermissionError` si otro proceso tiene
    /// el destino abierto, y el bridge lee en bucle, asi que hay que reintentar.
    fn write_atomic(&self, target: &Path, bytes: &[u8]) -> Result<(), RpcError> {
        let tmp = target.with_extension("json.tmp");
        let io = |p: &Path, source: std::io::Error| RpcError::Io {
            path: p.to_path_buf(),
            source,
        };
        // La carpeta tiene que existir. El bridge la crea cuando arranca, pero
        // el cliente no puede asumirlo: si no existe, write() falla con
        // "ruta no encontrada" y el error dice "io" en vez de "el bridge no
        // esta", que es lo que el LLM necesita para saber que hacer.
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
        let mut last = None;
        for _ in 0..20 {
            match std::fs::rename(&tmp, target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        Err(io(target, last.unwrap_or_else(|| {
            std::io::Error::other("rename fallo sin error")
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_por_defecto_apunta_a_temp() {
        let c = RpcConfig::default_config();
        assert!(c.dir.to_string_lossy().contains("reaper_mcp"), "{c:?}");
        assert!(c.command.ends_with("command.json"));
        assert!(c.response.ends_with("response.json"));
    }

    #[test]
    fn id_monotonico() {
        // Dos fetch_add seguidos: cada uno devuelve el valor ANTERIOR, asi
        // que el segundo tiene que ver uno mas grande. Con load() en medio no
        // habria carrera, pero el test seria mas fragil.
        let r = FileRpc::new(RpcConfig::default_config());
        let a = r.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let b = r.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(b, a + 1, "los ids tienen que ir en aumento: {a} -> {b}");
    }

    #[test]
    fn command_usa_camelCase_para_params() {
        // El bridge Lua lee `params` con claves en camelCase. Si el cliente
        // manda snake_case, Reaper se queda con los valores por defecto en
        // silencio, que es el peor fallo posible: parece que funciono.
        let c = Command {
            id: 1,
            action: "track.create".into(),
            params: serde_json::json!({"trackIndex": 0}),
        };
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains("trackIndex"), "{j}");
        assert!(!j.contains("track_index"), "{j}");
    }

    #[test]
    fn timeout_no_cuelga() {
        // Sin bridge corriendo, call() debe fallar rapido, no colgarse.
        let mut cfg = RpcConfig::default_config();
        cfg.dir = std::env::temp_dir().join("heretic-daw-test-sin-bridge");
        cfg.command = cfg.dir.join("command.json");
        cfg.response = cfg.dir.join("response.json");
        cfg.timeout = Duration::from_millis(120);
        let r = FileRpc::new(cfg);
        let e = r.call("ping", serde_json::json!({})).unwrap_err();
        assert!(matches!(e, RpcError::Timeout(_)), "{e}");
    }
}
