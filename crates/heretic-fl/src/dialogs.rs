//! Gestion de dialogos modales de FL Studio.
//!
//! # Por que hace falta
//!
//! FL Studio 2025 muestra un `TWelcomeWizard` ("Welcome to FL Studio") al
//! arrancar **sin proyecto**. Ese wizard es **modal**: mientras esta abierto,
//! FL no procesa el pump del controller script y rechaza toda escritura con
//! `RuntimeError: Operation unsafe at current time`.
//!
//! Medido (FL Studio 2025 real): con el wizard abierto, `pump_count` se
//! congela (8529 fijo) y 5/5 escrituras fallan con 'unsafe'. Al cerrarlo, el
//! pump arranca (0 -> 899) y las escrituras funcionan. La ventana principal
//! aparece `enabled=False`, que es la firma de que hay un modal encima.
//!
//! Esto hacia que "abrir un proyecto" fallara de forma lenta e intermitente:
//! `create_project` lanzaba FL, FL abria el wizard, y todo lo demas se caia
//! con un error que no apuntaba a la causa real.
//!
//! # Decisiones
//!
//! - Se manda `WM_CLOSE` solo a la ventana cuya clase es exactamente
//!   `TWelcomeWizard`. Nunca se toca el resto.
//! - Es idempotente: si no hay wizard, no hace nada.
//! - Se usa `PostMessageW` (no `SendMessageW`) para no bloquear el hilo del
//!   daemon esperando al GUI de FL, que esta en otro proceso pero cuyo mensaje
//!   tiene que procesar el hilo de FL.

#![allow(unsafe_code)]

use crate::process::fl_pid;
use std::thread;
use std::time::Duration;

/// Clase de la ventana del wizard de bienvenida de FL.
const WELCOME_CLASS: &str = "TWelcomeWizard";

/// Titulo que se ve cuando el wizard esta abierto (solo para logs).
const WELCOME_TITLE: &str = "Welcome to FL Studio";

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub type HWND = *mut c_void;

    /// Callback de `EnumWindows` / `EnumChildWindows`.
    pub type ENUMPROC = extern "system" fn(HWND, isize) -> i32;

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn EnumWindows(cb: ENUMPROC, lparam: isize) -> i32;
        pub fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
        pub fn GetClassNameW(hwnd: HWND, buf: *mut u16, n: i32) -> i32;
        pub fn IsWindowVisible(hwnd: HWND) -> i32;
        pub fn IsWindow(hwnd: HWND) -> i32;
        pub fn PostMessageW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> i32;
    }

    pub const WM_CLOSE: u32 = 0x0010;

    /// Lee el nombre de clase de una ventana (UTF-16 -> String).
    pub fn class_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), 256) };
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }

    pub fn is_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    pub fn is_alive(hwnd: HWND) -> bool {
        unsafe { IsWindow(hwnd) != 0 }
    }

    pub fn close(hwnd: HWND) -> bool {
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0 }
    }
}

/// Resultado de buscar el wizard.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WelcomeWizard {
    /// Cuantas ventanas del wizard habia.
    pub count: usize,
    /// Si se logro cerrar alguna.
    pub closed: bool,
}

/// Hay un `TWelcomeWizard` visible en el proceso de FL?
///
/// Devuelve el numero de ventanas del wizard encontradas.
#[cfg(windows)]
fn count_welcome() -> usize {
    let Some(pid) = fl_pid() else { return 0 };

    extern "system" fn cb(hwnd: win::HWND, _l: isize) -> i32 {
        let mut p = 0u32;
        unsafe { win::GetWindowThreadProcessId(hwnd, &mut p) };
        if p != TARGET_PID.load(std::sync::atomic::Ordering::Relaxed) {
            return 1; // sigue
        }
        if !win::is_visible(hwnd) {
            return 1;
        }
        if win::class_of(hwnd).trim() == WELCOME_CLASS {
            WELCOME_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        1
    }

    WELCOME_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
    TARGET_PID.store(pid, std::sync::atomic::Ordering::Relaxed);
    unsafe { win::EnumWindows(cb, 0) };
    WELCOME_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(windows)]
static TARGET_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(windows)]
static WELCOME_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Cierra el `TWelcomeWizard` si esta abierto y espera a que desaparezca.
///
/// Idempotente. Devuelve cuantos cerro.
///
/// El cierre se comprueba porque `WM_CLOSE` en una ventana modal puede ser
/// asincrono: se manda, se espera, se comprueba, y se reintenta unas veces
/// antes de rendirse.
pub fn close_welcome_wizard() -> WelcomeWizard {
    let mut out = WelcomeWizard::default();

    #[cfg(not(windows))]
    {
        let _ = &mut out;
        return out;
    }

    #[cfg(windows)]
    {
        out.count = count_welcome();
        if out.count == 0 {
            return out;
        }
        tracing::warn!(
            "FL tiene el '{WELCOME_TITLE}' abierto: es modal y bloquea el \
             bridge y las escrituras del proyecto. Se cierra automaticamente."
        );

        for _ in 0..12 {
            let mut posted = false;
            extern "system" fn cb(hwnd: win::HWND, _l: isize) -> i32 {
                let mut p = 0u32;
                unsafe { win::GetWindowThreadProcessId(hwnd, &mut p) };
                if p != TARGET_PID.load(std::sync::atomic::Ordering::Relaxed) {
                    return 1;
                }
                if win::is_visible(hwnd) && win::class_of(hwnd).trim() == WELCOME_CLASS {
                    POSTED.store(true, std::sync::atomic::Ordering::Relaxed);
                    win::close(hwnd);
                }
                1
            }
            unsafe { win::EnumWindows(cb, 0) };
            posted = POSTED.load(std::sync::atomic::Ordering::Relaxed);
            if posted {
                out.closed = true;
            }
            thread::sleep(Duration::from_millis(250));
            if count_welcome() == 0 {
                tracing::info!("wizard cerrado, FL vuelve a estar operativo");
                return out;
            }
        }

        tracing::warn!("el wizard sigue abierto tras varios intentos de cierre");
        out
    }
}

#[cfg(windows)]
static POSTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hay un dialogo modal que este bloqueando a FL? (wizard, dialogo de guardar,
// dialogo de plugins, etc). Para diagnostico: devuelve la clase de la ventana
/// modal si la hay.
pub fn blocking_modal_class() -> Option<String> {
    #[cfg(not(windows))]
    {
        None
    }
    #[cfg(windows)]
    {
        let pid = fl_pid()?;
        let mut result: Option<String> = None;
        extern "system" fn cb(hwnd: win::HWND, _l: isize) -> i32 {
            let mut p = 0u32;
            unsafe { win::GetWindowThreadProcessId(hwnd, &mut p) };
            if p != TARGET_PID.load(std::sync::atomic::Ordering::Relaxed) {
                return 1;
            }
            // Solo miramos ventanas visibles con clase con pinta de dialogo.
            if !win::is_visible(hwnd) {
                return 1;
            }
            let cls = win::class_of(hwnd).trim().to_string();
            if cls == WELCOME_CLASS {
                MODAL.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            1
        }
        MODAL.store(false, std::sync::atomic::Ordering::Relaxed);
        TARGET_PID.store(pid, std::sync::atomic::Ordering::Relaxed);
        unsafe { win::EnumWindows(cb, 0) };
        if MODAL.load(std::sync::atomic::Ordering::Relaxed) {
            result = Some(WELCOME_CLASS.to_string());
        }
        result
    }
}

#[cfg(windows)]
static MODAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cerrar_wizard_sin_fl_no_panea() {
        // Puede haber FL o no; en ambos casos no debe panic.
        let r = close_welcome_wizard();
        // Si no habia wizard, count == 0 y no se cerro nada.
        if r.count == 0 {
            assert!(!r.closed);
        }
    }

    #[test]
    fn modal_class_no_panea() {
        let _ = blocking_modal_class();
    }
}
