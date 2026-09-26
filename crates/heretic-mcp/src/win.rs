//! FFI mínima a `user32` para **leer y cerrar los diálogos modales de Reaper**.
//!
//! ## Por qué hace falta
//!
//! Un error en un ReaScript abre un diálogo modal de Windows. Mientras está
//! abierto, Reaper no procesa el `command.json`, así que el bridge no contesta
//! y el agente ve exactamente una cosa:
//!
//! ```text
//! el bridge no respondio en 10s
//! ```
//!
//! Ninguna pista de la causa. Pasó de verdad: se tardó 3 intentos y un script
//! de PowerShell de 40 líneas en encontrar que la causa era un `attempt to call
//! a nil value` en la línea 27 de un Lua propio.
//!
//! Con este modulo, `daw_debug` lee el diálogo y lo cuenta. El agente puede
//! diagnosticar solo.
//!
//! ## Por qué `unsafe` aquí y en ningún otro sitio
//!
//! El crate es `#![deny(unsafe_code)]`: seis llamadas FFI a `user32` son el
//! único motivo legítimo que ha aparecido, y aislarlas en un módulo con
//! `#[allow(unsafe_code)]` deja la garantía intacta para el resto. Todo lo
//! demás (JSON, RPC, tools) sigue sin `unsafe` compilado.
//!
//! ## Lo que este módulo NO hace
//!
//! No pulsa botones a lo bruto. `daw_debug` decide cuáles, y por defecto se
//! niega a pulsar los que cambian preferencias del usuario (ver
//! `debug.rs::puede_pulsar`).

#![allow(unsafe_code)]

use std::ffi::c_void;

pub type Hwnd = isize;

#[repr(C)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub type EnumProc = unsafe extern "system" fn(Hwnd, *mut c_void) -> i32;

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(cb: EnumProc, l: *mut c_void) -> i32;
    fn EnumChildWindows(parent: Hwnd, cb: EnumProc, l: *mut c_void) -> i32;
    fn GetClassNameW(h: Hwnd, buf: *mut u16, n: i32) -> i32;
    fn GetWindowTextW(h: Hwnd, buf: *mut u16, n: i32) -> i32;
    fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
    fn SendMessageW(h: Hwnd, msg: u32, w: usize, l: isize) -> isize;
    fn IsWindowVisible(h: Hwnd) -> i32;
}

/// BM_CLICK: lo que se manda a un `Button` para pulsarlo como un click de verdad.
const BM_CLICK: u32 = 0x00F5;
/// WM_CLOSE
const WM_CLOSE: u32 = 0x0010;

/// Una ventana de Windows con lo que se sabe de ella.
#[derive(Debug, Clone, Default)]
pub struct WinInfo {
    pub hwnd: Hwnd,
    pub class: String,
    pub title: String,
    pub pid: u32,
    pub visible: bool,
    /// Controles hijos con texto: `(hwnd, clase, texto)`.
    ///
    /// La clase importa: llamar "boton" a cualquier texto corto fue un error
    /// mio que casi Mikkelsen's el aviso de licencia de Reaper, que tiene
    /// controles que se leen como botones y no lo son.
    pub children: Vec<(Hwnd, String, String)>,
}

impl WinInfo {
    /// Botones (clase `Button`) cuyo texto encaja con alguno de `nombres`.
    ///
    /// Sin distinguir mayúsculas y tolerando el `&` de los mnemonicos: `&No`,
    /// `&Si`. Solo clase `Button`: un `Static` con el texto "OK" no se pulsa.
    pub fn boton(&self, nombres: &[&str]) -> Option<Hwnd> {
        let botones: Vec<&(Hwnd, String, String)> =
            self.children.iter().filter(|(_, c, _)| c == "Button").collect();
        let norm = |s: &str| s.trim_start_matches('&').trim().to_lowercase();
        for objetivo in nombres {
            for (h, _, t) in &botones {
                if norm(t) == norm(objetivo) {
                    return Some(*h);
                }
            }
        }
        // Segunda pasada: coincidencia por contenido, para "End script (x)".
        for objetivo in nombres {
            for (h, _, t) in &botones {
                if norm(t).contains(&objetivo.to_lowercase()) {
                    return Some(*h);
                }
            }
        }
        None
    }

    /// Textos de los botones, para informar de lo que hay.
    pub fn botones(&self) -> Vec<&str> {
        self.children
            .iter()
            .filter(|(_, c, _)| c == "Button")
            .map(|(_, _, t)| t.as_str())
            .collect()
    }

    /// Controles que NO son botones, para no llamarles de cualquier forma.
    pub fn otros_controles(&self) -> Vec<&str> {
        self.children
            .iter()
            .filter(|(_, c, _)| c != "Button")
            .map(|(_, _, t)| t.as_str())
            .filter(|t| !t.trim().is_empty())
            .collect()
    }
}

unsafe fn texto(h: Hwnd, clase_o_texto: bool) -> String {
    // 512 wchar_t de sobra para un titulo o un boton de Windows.
    let mut buf = [0u16; 512];
    let n = unsafe {
        if clase_o_texto {
            GetClassNameW(h, buf.as_mut_ptr(), 512)
        } else {
            GetWindowTextW(h, buf.as_mut_ptr(), 512)
        }
    };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

unsafe extern "system" fn cb_hijos(h: Hwnd, l: *mut c_void) -> i32 {
    let acc = unsafe { &mut *(l as *mut Vec<(Hwnd, String, String)>) };
    let t = unsafe { texto(h, false) };
    if !t.is_empty() {
        let c = unsafe { texto(h, true) };
        acc.push((h, c, t));
    }
    1
}

unsafe extern "system" fn cb_toplevel(h: Hwnd, l: *mut c_void) -> i32 {
    let acc = unsafe { &mut *(l as *mut Vec<WinInfo>) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, &mut pid) };
    acc.push(WinInfo {
        hwnd: h,
        class: unsafe { texto(h, true) },
        title: unsafe { texto(h, false) },
        pid,
        visible: unsafe { IsWindowVisible(h) != 0 },
        children: Vec::new(),
    });
    1
}

/// Todas las ventanas de primer nivel.
///
/// `pid == 0` significa "de cualquier proceso": sin ese caso, `pid_reaper()`
/// se llama a si mismo con 0 y el filtro se queda con nada, que es
/// exactamente el bug que hacia que `daw_debug` no detectase ni el primer
/// dialogo. Lo encontro probarlo contra el DAW, no el test.
pub fn ventanas(pid: u32) -> Vec<WinInfo> {
    let mut acc: Vec<WinInfo> = Vec::new();
    unsafe { EnumWindows(cb_toplevel, &mut acc as *mut _ as *mut c_void) };
    if pid != 0 {
        acc.retain(|w| w.pid == pid);
    }
    acc
}

/// Rellena `children` de cada ventana. Separado de [`ventanas`] porque el
/// recorrido de hijos es el caro y no siempre hace falta.
pub fn con_hijos(mut ws: Vec<WinInfo>) -> Vec<WinInfo> {
    for w in ws.iter_mut() {
        let mut acc: Vec<(Hwnd, String, String)> = Vec::new();
        unsafe { EnumChildWindows(w.hwnd, cb_hijos, &mut acc as *mut _ as *mut c_void) };
        w.children = acc;
    }
    ws
}

/// Busca la ventana principal de Reaper para quedarse con su PID sin depender
/// de `tasklist` ni del crate `sysinfo`.
pub fn pid_reaper() -> Option<u32> {
    let ws = ventanas(0);
    ws.iter()
        .find(|w| w.class == "REAPERwnd")
        .map(|w| w.pid)
        .filter(|p| *p != 0)
}

/// Diálogos modales de Reaper: los que están congelando el DAW.
pub fn dialogos_modales(pid: u32) -> Vec<WinInfo> {
    let ws = ventanas(pid);
    let dlg: Vec<WinInfo> = ws
        .into_iter()
        .filter(|w| w.class == "#32770" && w.visible)
        .collect();
    con_hijos(dlg)
}

/// Pulsa un botón con BM_CLICK.
///
/// Se usa BM_CLICK y no `PostMessage(WM_COMMAND)` porque Reaper procesa el
/// clic en su propio bucle de mensajes: si el diálogo es modal, hace falta el
/// clic sincrónico o se queda abierto.
pub fn pulsar(h: Hwnd) {
    unsafe { SendMessageW(h, BM_CLICK, 0, 0) };
}

pub fn cerrar(h: Hwnd) {
    unsafe { SendMessageW(h, WM_CLOSE, 0, 0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_ventana_que_no_existe_no_devuelve_nada() {
        // hwnd inventado: la API debe tolerarlo y no Romper.
        let antes = dialogos_modales(u32::MAX).len();
        assert_eq!(antes, 0);
    }

    #[test]
    fn el_boton_ignora_los_mnemonicos_ampersand() {
        let w = WinInfo {
            children: vec![
                (1, "Button".into(), "&No".into()),
                (2, "Button".into(), "&Continuar".into()),
                (3, "Button".into(), String::new()),
            ],
            ..Default::default()
        };
        assert_eq!(w.boton(&["continuar"]), Some(2));
        assert_eq!(w.boton(&["no"]), Some(1));
        // "Sí" existe pero nadie la pide, asi que no aparece.
        assert_eq!(w.boton(&["end script"]), None);
    }

    #[test]
    fn un_static_que_dice_ok_no_es_un_boton() {
        // El aviso de licencia de Reaper tiene controles que se leen como
        // botones. Pulsarlos sin querer es tocar la licencia del usuario.
        let w = WinInfo {
            children: vec![
                (1, "Static".into(), "OK".into()),
                (2, "Button".into(), "Close".into()),
            ],
            ..Default::default()
        };
        assert_eq!(w.boton(&["ok"]), None, "un Static no se pulsa");
        assert_eq!(w.boton(&["close"]), Some(2));
        assert_eq!(w.botones(), vec!["Close"]);
        assert_eq!(w.otros_controles(), vec!["OK"]);
    }

    #[test]
    fn el_boton_cae_a_coincidencia_parcial_para_textos_con_sufijo() {
        let w = WinInfo {
            children: vec![(7, "Button".into(), "End script (daw_probe.lua)".into())],
            ..Default::default()
        };
        assert_eq!(w.boton(&["end script"]), Some(7));
    }

    #[test]
    fn encuentra_el_pid_de_reaper_si_esta_corriendo() {
        // No se puede afirmar nada si Reaper no esta abierto; lo que se
        // comprueba es que la llamada no entre en panico ni devuelva 0.
        if let Some(pid) = pid_reaper() {
            assert!(pid > 0);
            // Y si hay PID, tiene que haber alguna ventana suya.
            assert!(!ventanas(pid).is_empty());
        }
    }
}

