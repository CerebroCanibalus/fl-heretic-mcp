//! Handlers de ciclo de vida de FL Studio y del proyecto.
//!
//! Estas operaciones NO se pueden hacer desde el bridge, porque el sandbox del
//! script de FL no expone la API de proyecto:
//!
//! - `general.saveProject` no existe (comprobado con `hasattr`).
//! - `dir(general)` no tiene nada para abrir ni cerrar un proyecto.
//! - `midi.FPT_*` tiene `FPT_Save` y `FPT_SaveNew` pero **no** `FPT_Open` ni
//!   `FPT_Close` (79 constantes revisadas).
//!
//! La solucion es que el daemon, que vive FUERA de FL, controle el proceso:
//!
//! | Handler    | Mecanismo                 | Nota
//! |------------|---------------------------|-----
//! | `open`     | `CreateProcess` con el .flp| abre en la instancia existente si ya hay una
//! | `save`     | bridge `project.save`     | `FPT_Save`, el atajo Ctrl+S
//! | `create`   | copiar .flp + CreateProcess | ruta arbitraria sin pelear con la UI
//! | `close`    | `WM_CLOSE` a la ventana   | FL pregunta por los cambios
//! | `status`   | `tasklist` + ping         | proceso y bridge
//!
//! # Flujo recomendado para cambiar de proyecto
//!
//! ```text
//! fl_close  ->  (si hay cambios sin guardar, fl_save antes)
//! fl_open   ->  fl_wait_ready
//! ```
//!
//! Cerrar sin guardar primero pierde el trabajo, y `WM_CLOSE` no lo evita:
//! FL abre su dialogo modal y el daemon se queda esperando. Por eso `close`
//! consulta `getChangedFlag` y avisa ANTES de cerrar.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use heretic_core::{HereticError, Result};
use heretic_fl::FlBridge;

/// Handlers de ciclo de vida.
pub struct Lifecycle {
    bridge: Arc<FlBridge>,
}

impl Lifecycle {
    pub fn new(bridge: Arc<FlBridge>) -> Self {
        Self { bridge }
    }

    /// Dispatcher: matchea el nombre del método al handler.
    pub async fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "open" => self.open(params).await,
            "save" => self.save().await,
            "create_project" => self.create_project(params).await,
            "close" => self.close(params).await,
            "status" => self.status().await,
            "wait_ready" => self.wait_ready(params).await,
            other => Err(HereticError::InvalidRequest(format!(
                "metodo de lifecycle desconocido: {other}"
            ))),
        }
    }

    // ================================================================
    // Handlers
    // ================================================================

    /// Abre un proyecto: lanza FL Studio con el `.flp` como argumento.
    ///
    /// Si FL ya esta corriendo, Windows enruta el `.flp` a la instancia
    /// existente y lo abre ahi, que es el comportamiento normal de FL.
    ///
    /// Parametros: `{ "path": "C:/.../proyecto.flp", "wait": 20 }`.
    /// `wait` son los segundos maximos que se espera a que aparezca el proceso.
    pub async fn open(&self, params: Value) -> Result<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HereticError::InvalidRequest("open: falta 'path'".into()))?;
        let wait = params.get("wait").and_then(|v| v.as_u64()).unwrap_or(20);

        let already = heretic_fl::running_process();
        let p = PathBuf::from(path);
        let proc = heretic_fl::launch(Some(&p), wait)?;

        Ok(json!({
            "opened": path,
            "was_running": already.is_some(),
            "pid": proc.pid,
            "note": if already.is_some() {
                "FL Studio ya estaba corriendo: el proyecto se abrio en esa instancia"
            } else {
                "FL Studio lanzado"
            },
        }))
    }

    /// Lanza FL Studio sin abrir ningun proyecto.
    pub async fn launch(&self, params: Value) -> Result<Value> {
        let wait = params.get("wait").and_then(|v| v.as_u64()).unwrap_or(20);
        let proc = heretic_fl::launch(None, wait)?;
        Ok(json!({"pid": proc.pid, "launched": true}))
    }

    /// Guarda el proyecto via el atajo `FPT_Save` (Ctrl+S).
    ///
    /// AVISO: si el proyecto no tiene ruta, FL abre un dialogo modal de
    /// "Save as" y se queda esperando al usuario. El bridge lo avisa en su
    /// respuesta, pero el dialogo bloquea FL: el pump no vuelve a correr
    /// hasta que se cierre.
    pub async fn save(&self) -> Result<Value> {
        let r = self.bridge.call("project.save", json!({})).await?;
        Ok(json!({
            "saved": true,
            "has_file": r.get("had_file").and_then(Value::as_bool).unwrap_or(true),
            "warning": r.get("warning").and_then(Value::as_str),
        }))
    }

    /// Crea un proyecto NUEVO en la carpeta habitual y lo abre.
    ///
    /// Por que no se usa el dialogo de "Save as" de FL: la FL Python API no
    /// expone la API de proyecto (`dir(general)` no tiene `saveProject` ni
    /// `getProjectFilePath`) y de las 79 constantes `midi.FPT_*` no hay ninguna
    /// que guarde con ruta. `FPT_SaveNew` abre el dialogo la primera vez,
    /// pero sus campos son `TQuickEdit` (controles Delphi internos) y al
    /// confirmar el dialogo se cierra SIN guardar. Intentar inyectar la ruta
    /// por `WM_SETTEXT` no funciona de forma fiable.
    ///
    /// Un `.flp` es un fichero: la via determinista es copiar una plantilla a
    /// la ruta deseada y pedirle a FL que la abra con `CreateProcess`. Cero
    /// interfaz, cero teclas, cero dialogos.
    ///
    /// Parametros:
    /// - `name` (obligatorio): nombre del proyecto, sin extension.
    /// - `dir` (opcional): carpeta destino. Defecto: la de FL.
    /// - `template` (opcional): `.flp` base. Defecto: el mas reciente de la
    ///   carpeta de FL que no sea backup ni autosave.
    /// - `open` (opcional, def. true): abrirlo en FL despues de crearlo.
    pub async fn create_project(&self, params: Value) -> Result<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                HereticError::InvalidRequest("create_project: falta 'name'".into())
            })?
            .to_string();
        let dir = params.get("dir").and_then(|v| v.as_str()).map(PathBuf::from);
        let template = params
            .get("template")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        let open = params.get("open").and_then(Value::as_bool).unwrap_or(true);

        let dir_ref = dir.as_deref();
        let tpl_ref = template.as_deref();
        let path = heretic_fl::create_project_file(&name, dir_ref, tpl_ref)?;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        let mut out = json!({
            "created": path.display().to_string(),
            "file_size": size,
            "opened": false,
        });

        if open {
            match heretic_fl::launch(Some(&path), 25) {
                Ok(proc) => {
                    out["opened"] = json!(true);
                    out["pid"] = json!(proc.pid);
                    // Da tiempo a que FL levante el bridge antes de devolver,
                    // o el primer comando del LLM fallara por timeout.
                    let _ = self.wait_ready(json!({ "timeout": 30 })).await;
                    out["bridge_ready"] = json!(true);
                }
                Err(e) => {
                    out["open_error"] = json!(e.to_string());
                }
            }
        }
        Ok(out)
    }

    /// Cierra FL Studio enviando `WM_CLOSE` a su ventana.
    ///
    /// Parametros: `{ "force": false, "wait": 15, "save_first": true }`.
    ///
    /// - `save_first` (por defecto true): guarda antes via el bridge, para que
    ///   FL no abra el dialogo "¿guardar?". Si no se puede guardar, se
    ///   aborta y no se cierra nada.
    /// - `force`: si FL sigue abierto tras `wait` segundos, lo mata. PIERDE
    ///   cambios sin guardar.
    pub async fn close(&self, params: Value) -> Result<Value> {
        let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
        let wait = params.get("wait").and_then(|v| v.as_u64()).unwrap_or(15);
        let save_first = params
            .get("save_first")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        if heretic_fl::running_process().is_none() {
            return Ok(json!({"closed": true, "was_running": false}));
        }

        // Estado de cambios pendientes ANTES de tocar nada.
        let changed = self
            .bridge
            .call("project.metadata", json!({}))
            .await
            .ok()
            .and_then(|v| v.get("changed").and_then(Value::as_bool));

        if save_first {
            if changed == Some(true) {
                let r = self.bridge.call("project.save", json!({})).await?;
                if r.get("had_file").and_then(Value::as_bool) == Some(false) {
                    return Err(HereticError::Other(format!(
                        "no se puede cerrar de forma segura: el proyecto tiene cambios \
                         sin guardar y NO tiene ruta, asi que el guardado abriria un \
                         dialogo modal y FL se quedaria bloqueado.\n\
                         Guardalo tu a mano (Ctrl+S) y luego cierra, o pasa \
                         save_first=false para cerrarlo igualmente."
                    )));
                }
            }
        }

        let closed = heretic_fl::close(force, wait)?;

        Ok(json!({
            "closed": closed,
            "force": force,
            "had_unsaved_changes": changed.unwrap_or(false),
            "saved_first": save_first && changed == Some(true),
        }))
    }

    /// Estado: proceso de FL + bridge online.
    pub async fn status(&self) -> Result<Value> {
        let st = heretic_fl::status(&self.bridge).await;
        Ok(json!({
            "fl_running": st.running,
            "pid": st.pid,
            "exe": st.exe_path.map(|p| p.display().to_string()),
            "bridge_online": st.bridge_online,
            "bridge_error": st.bridge_error,
            "midi_wake_ports": heretic_fl::midi::open_all(),
        }))
    }

    /// Espera a que el bridge responda. Util tras abrir un proyecto.
    /// Parametros: `{ "timeout": 30 }`.
    pub async fn wait_ready(&self, params: Value) -> Result<Value> {
        let timeout = params.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);
        let t0 = std::time::Instant::now();
        loop {
            if let Ok(info) = self.bridge.ping().await {
                return Ok(json!({
                    "ready": true,
                    "waited_sec": t0.elapsed().as_secs_f64(),
                    "bridge_version": info.bridge_version,
                    "fl_version": info.fl_version,
                }));
            }
            if t0.elapsed().as_secs() >= timeout {
                return Err(HereticError::Other(format!(
                    "el bridge no respondio en {timeout}s tras arrancar FL. \
                     Comprueba que el controller script 'FL Heretic Bridge' esta \
                     seleccionado en Options > MIDI Settings."
                )));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> Lifecycle {
        let dir = std::env::temp_dir().join("heretic-lifecycle-test");
        std::fs::create_dir_all(&dir).ok();
        let b = FlBridge::with_config(heretic_fl::BridgeConfig {
            script_dir: dir,
            timeout: std::time::Duration::from_millis(10),
        })
        .expect("bridge");
        Lifecycle::new(b)
    }

    #[tokio::test]
    async fn metodo_desconocido_da_error_claro() {
        let l = dummy();
        let e = l.dispatch("nope", json!({})).await.unwrap_err();
        assert!(e.to_string().contains("desconocido"), "{e}");
    }

    #[tokio::test]
    async fn open_sin_path_da_error_claro() {
        let l = dummy();
        let e = l.open(json!({})).await.unwrap_err();
        assert!(e.to_string().contains("path"), "{e}");
    }

    #[tokio::test]
    async fn open_con_ruta_inexistente_no_lanza_fl() {
        let l = dummy();
        let e = l
            .open(json!({ "path": "Z:\\no\\existe\\proyecto.flp" }))
            .await
            .unwrap_err();
        assert!(e.to_string().contains("no existe"), "{e}");
    }

    #[tokio::test]
    async fn status_no_entra_en_panic() {
        let l = dummy();
        // Puede fallar el ping (no hay FL), pero debe devolver algo o un error
        // de HereticError, nunca un panic.
        let _ = l.status().await;
    }
}
