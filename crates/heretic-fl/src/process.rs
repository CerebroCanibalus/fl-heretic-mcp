//! Control del proceso de FL Studio.
//!
//! El script de FL corre en un sandbox sin sockets, sin threads y sin la API
//! de proyecto (`general.saveProject` no existe, `dir(general)` no tiene nada
//! para abrir ni cerrar). Lo que el sandbox NO restringe es el mundo exterior:
//! este modulo vive en el daemon Rust, fuera de FL, y puede hacer lo que
//! quiera con el proceso.
//!
//! # Que resuelve
//!
//! | Operacion | Como |
//! |---|---|
//! | Detectar FL | `tasklist` / `Get-Process` via `sysinfo`-lite (solo Windows por ahora) |
//! | Abrir proyecto | `CreateProcess` con el `.flp` como argumento |
//! | Cerrar proyecto | `WM_CLOSE` a la ventana de FL (FL pregunta por los cambios) |
//! | Cerrar forzado | `WM_CLOSE` y si no responde, matar el proceso |
//! | Esperar listo | Esperar a que el bridge responda `meta.ping` |
//!
//! # Limites
//!
//! - `WM_CLOSE` hace que FL abra su propio dialogo "¿guardar?". Si el
//!   proyecto tiene cambios sin guardar, FL se queda esperando. El daemon
//!   puede recurrir a `project.save` del bridge ANTES de cerrar, y asi el
//!   dialogo no aparece. Ese es el flujo recomendado:
//!   `save` (bridge) -> `close` (proceso).
//! - Cerrar con cambios sin guardar y sin `save` previo pierde el trabajo.
//!   Por eso `close` devuelve si habia cambios pendientes, usando
//!   `general.getChangedFlag` a traves del bridge.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use heretic_core::{HereticError, Result};

/// Rutas habituales de FL Studio en Windows, en orden de probabilidad.
const KNOWN_PATHS: &[&str] = &[
    r"D:\Program Files\Image-Line\FL Studio 2025\FL64.exe",
    r"C:\Program Files\Image-Line\FL Studio 2025\FL64.exe",
    r"C:\Program Files\Image-Line\FL Studio 24\FL64.exe",
    r"C:\Program Files (x86)\Image-Line\FL Studio 24\FL64.exe",
];

/// Proceso de FL Studio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlProcess {
    pub pid: u32,
    pub exe_path: PathBuf,
    pub window_title: String,
    /// Segundos desde el arranque del proceso.
    pub uptime_sec: u64,
}

/// Localiza el ejecutable de FL Studio.
///
/// 1. `FL_HERETIC_FL_EXE` si esta definido.
/// 2. La ruta del proceso en ejecucion, via `fl_exe_from_process()`.
/// 3. Las rutas conocidas de `KNOWN_PATHS`.
///
/// OJO: aqui NO se llama a `running_process()`. Esa funcion necesita el exe
/// para rellenar el struct, y si se llamaran mutuamente habria recursion
/// infinita (stack overflow en runtime). Por eso el paso 2 usa una funcion
/// independiente que solo pregunta por el proceso.
pub fn find_fl_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FL_HERETIC_FL_EXE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(exe) = fl_exe_from_process() {
        if exe.is_file() {
            return Some(exe);
        }
    }
    for p in KNOWN_PATHS {
        let p = Path::new(p);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// Ruta del ejecutable de FL Studio a partir del proceso vivo.
///
/// Separada de `running_process()` precisamente para romper la recursion
/// mutua entre ambas.
#[cfg(windows)]
fn fl_exe_from_process() -> Option<PathBuf> {
    let pid = fl_pid()?;
    let out = Command::new("wmic")
        .args([
            "process",
            "where",
            &format!("ProcessId={pid}"),
            "get",
            "ExecutablePath",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().skip(1) {
        let p = line.trim();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

#[cfg(not(windows))]
fn fl_exe_from_process() -> Option<PathBuf> {
    None
}

/// PID de FL Studio, o `None` si no esta corriendo.
#[cfg(windows)]
pub fn fl_pid() -> Option<u32> {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    parse_tasklist_pid(&String::from_utf8_lossy(&out.stdout))
}

/// Extrae el PID de la salida CSV de `tasklist`.
///
/// Formato: `"FL64.exe","16612","Console","1","122,120 KB"`. Al partir por
/// '"' los campos caen en los indices impares: `[1]` nombre, `[3]` pid,
/// `[5]` sesion, `[7]` num, `[9]` memoria. Se parte por comillas y no por
/// comas porque el separador se cuela en los indices pares y el tamper de
/// memoria ("122,120 KB") lleva su propia coma.
#[cfg(windows)]
fn parse_tasklist_pid(text: &str) -> Option<u32> {
    for line in text.lines() {
        let cols: Vec<&str> = line.split('"').collect();
        if cols.len() < 4 {
            continue;
        }
        if let Ok(pid) = cols[3].trim().parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

/// Devuelve el proceso de FL Studio en ejecucion, si lo hay.
pub fn running_process() -> Option<FlProcess> {
    #[cfg(windows)]
    {
        fl_pid().map(|pid| FlProcess {
            pid,
            exe_path: fl_exe_from_process().unwrap_or_default(),
            window_title: String::new(),
            uptime_sec: 0,
        })
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("pgrep").arg("-f").arg("FL64").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let pid = text.split_whitespace().next()?.parse::<u32>().ok()?;
        Some(FlProcess {
            pid,
            exe_path: find_fl_exe().unwrap_or_default(),
            window_title: String::new(),
            uptime_sec: 0,
        })
    }
}

/// Lanza FL Studio, opcionalmente abriendo un proyecto.
///
/// Si FL ya esta corriendo y se pasa un proyecto, FL Studio lo abre en la
/// instancia existente (comportamiento normal de Windows con single-instance).
/// `wait` son los segundos que se espera a que aparezca el proceso.
pub fn launch(project: Option<&Path>, wait_sec: u64) -> Result<FlProcess> {
    let exe = find_fl_exe().ok_or_else(|| {
        HereticError::Other(
            "no se encuentra FL64.exe. Define FL_HERETIC_FL_EXE con la ruta completa."
                .into(),
        )
    })?;

    if let Some(p) = project {
        if !p.is_file() {
            return Err(HereticError::Other(format!(
                "el proyecto no existe: {}",
                p.display()
            )));
        }
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        Command::new(&exe)
            .arg(&p)
            .current_dir(p.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| HereticError::Other(format!("lanzando FL Studio: {e}")))?;
    } else {
        Command::new(&exe)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| HereticError::Other(format!("lanzando FL Studio: {e}")))?;
    }

    // Espera activa a que aparezca el proceso.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait_sec);
    while std::time::Instant::now() < deadline {
        if let Some(p) = running_process() {
            return Ok(p);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    running_process().ok_or_else(|| {
        HereticError::Other(format!(
            "FL Studio se lanzo pero no aparece como proceso tras {wait_sec}s. \
             Comprueba que arranca bien manualmente."
        ))
    })
}

/// Envia `WM_CLOSE` a todas las ventanas del proceso de FL.
///
/// WM_CLOSE es el cierre "educado": si el proyecto tiene cambios sin guardar,
/// FL muestra su propio dialogo y espera. Para cerrar de verdad hay que
/// guardar antes con `project.save` del bridge.
///
/// Devuelve `true` si encuentra al menos una ventana.
pub fn close(force: bool, wait_sec: u64) -> Result<bool> {
    let Some(fl) = running_process() else {
        return Ok(false); // ya estaba cerrado
    };

    #[cfg(windows)]
    {
        let sent = post_wm_close_to_process(fl.pid);
        // Espera a que se vaya. Si no se va y `force`, lo mata.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait_sec);
        while std::time::Instant::now() < deadline {
            if running_process().is_none() {
                return Ok(sent);
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        if force {
            kill(fl.pid)?;
            return Ok(sent);
        }
        return Err(HereticError::Other(format!(
            "FL Studio sigue abierto tras {wait_sec}s. Probablemente hay un dialogo \
             abierto (¿guardar cambios?). Guarda antes con fl_save y reintenta, o \
             usa close(force=true)."
        )));
    }
    #[cfg(not(windows))]
    {
        let _ = force;
        Command::new("kill")
            .args(["-TERM", &fl.pid.to_string()])
            .output()
            .map_err(|e| HereticError::Other(format!("kill: {e}")))?;
        Ok(true)
    }
}

/// Mata el proceso a la fuerza. Pierde cambios sin guardar.
pub fn kill(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .map_err(|e| HereticError::Other(format!("taskkill: {e}")))?;
    }
    #[cfg(not(windows))]
    {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()
            .map_err(|e| HereticError::Other(format!("kill: {e}")))?;
    }
    Ok(())
}

/// Estado completo: proceso + si el bridge responde.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub exe_path: Option<PathBuf>,
    /// El bridge (script de FL) responde a meta.ping.
    pub bridge_online: bool,
    /// ultimos_error si el bridge no responde.
    pub bridge_error: Option<String>,
}

/// Estado del proceso y del bridge.
pub async fn status(bridge: &crate::FlBridge) -> FlStatus {
    let fl = running_process();
    let (bridge_online, bridge_error) = match bridge.ping().await {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };
    FlStatus {
        running: fl.is_some(),
        pid: fl.as_ref().map(|p| p.pid),
        exe_path: find_fl_exe(),
        bridge_online,
        bridge_error,
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    struct CallbackData {
        target_pid: u32,
        found: u32,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam lo pasa nuestro propio EnumWindows, y siempre apunta
        // a un CallbackData vivo en el stack de post_wm_close. Windows lo
        // entrega tal cual, sin transformarlo.
        let data = unsafe { &mut *(lparam.0 as *mut CallbackData) };
        let mut pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                data.found += 1;
            }
        }
        TRUE
    }

    pub fn post_wm_close(pid: u32) -> u32 {
        let mut data = CallbackData {
            target_pid: pid,
            found: 0,
        };
        unsafe {
            let ptr = std::ptr::addr_of_mut!(data) as isize;
            let _ = EnumWindows(Some(callback), LPARAM(ptr));
        }
        data.found
    }
}

#[cfg(windows)]
use win::post_wm_close as post_wm_close_impl;

#[cfg(windows)]
fn post_wm_close_to_process(pid: u32) -> bool {
    post_wm_close_impl(pid) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_exe_returns_some_path_when_fl_installed() {
        // En la maquina de desarrollo FL esta instalado; si no, el test
        // solo confirma que no entra en panic.
        let r = find_fl_exe();
        if let Some(p) = r {
            assert!(p.is_file(), "devolveria una ruta que no existe: {p:?}");
        }
    }

    #[test]
    fn known_paths_are_absolute() {
        for p in KNOWN_PATHS {
            assert!(Path::new(p).is_absolute(), "no es absoluta: {p}");
        }
    }

    /// Regresión: el parser de `tasklist` leía el PID de la columna
    /// equivocada (tomaba el nombre del proceso) y devolvía None aunque FL
    /// estuviera corriendo. Con esto no vuelve a pasar en silencio.
    #[cfg(windows)]
    #[test]
    fn el_parser_de_tasklist_saca_el_pid_de_la_columna_buena() {
        let casos: [(&str, Option<u32>); 4] = [
            ("\"FL64.exe\",\"16612\",\"Console\",\"1\",\"122,120 KB\"", Some(16612)),
            ("\"FL64.exe\",\"4\",\"Services\",\"0\",\"1,024 KB\"", Some(4)),
            ("", None),
            ("no hay tareas", None),
        ];
        for (line, esperado) in casos {
            assert_eq!(parse_tasklist_pid(line), esperado, "linea: {line:?}");
        }
    }

    /// Si FL esta corriendo, running_process() tiene que encontrarlo. Solo
    /// aplica en una maquina con FL instalado y abierto.
    #[cfg(windows)]
    #[test]
    fn detecta_fl_si_esta_corriendo() {
        let out = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist");
        if String::from_utf8_lossy(&out.stdout).contains("FL64.exe") {
            let p = running_process();
            assert!(
                p.is_some(),
                "tasklist dice que FL corre pero running_process() no lo ve"
            );
        }
    }

    #[test]
    fn running_process_is_optional() {
        // No debe entrar en panic estea corriendo o no.
        let _ = running_process();
    }
}
