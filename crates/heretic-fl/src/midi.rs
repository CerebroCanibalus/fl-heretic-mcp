//! Wake por MIDI.
//!
//! # Por que hace falta
//!
//! FL Studio 2025 **no llama nunca a `OnIdle`** en los controller scripts
//! (medido: `idle_ticks = 0` tras cargar el script). El unico pump del bridge
//! corre en `OnMidiIn` / `OnMidiMsg`, o sea que FL solo procesa una peticion
//! cuando **le llega un evento MIDI**. Sin MIDI no hay pump, y la peticion se
//! queda en disco hasta que llegue algo.
//!
//! Medido en FL Studio 2025 (MIDI scripting v38):
//!
//! ```text
//! puerto OUT abierto y cerrado en cada envio  -> pump_count se queda en 0
//! puerto OUT abierto y persistente           -> pump_count 1 -> 7601
//! ```
//!
//! O sea que el handle del puerto MIDI tiene que vivir **toda la vida del
//! proceso**, no abrirse por peticion. Este modulo lo mantiene.
//!
//! # Que envia
//!
//! Un Note On por los **16 canales** a cada puerto OUT. FL filtra el MIDI
//! entrante por el canal asignado al controller en MIDI Settings, y ese canal
//! no es publica ni estable, asi que se mandan los 16: son 16 bytes y
//! garantiza el despertar sin tener que adivinar el canal.
//!
//! Usa la API `winmm` nativa (la misma que ve el resto del sistema) en vez de
//! un crate de MIDI: aqui no hace falta nada mas que `midiOutOpen` +
//! `midiOutShortMsg`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use std::sync::{Mutex, OnceLock};

    pub struct Handles(Vec<usize>);

    pub fn open_all() -> usize {
        let slot = HANDLES.get_or_init(|| Mutex::new(Handles(Vec::new())));
        let mut guard = match slot.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Si ya hay handles, no se reabre: midiOutOpen sobre un puerto ya
        // abierto por este proceso falla con MMSYSERR_ALLOCATED, y eso hacia
        // que una segunda llamada devolviera 0 en vez del numero de puertos.
        if !guard.0.is_empty() {
            return guard.0.len();
        }

        let mut out = Vec::new();
        unsafe {
            let n = midiOutGetNumDevs();
            for dev in 0..n {
                let mut handle: usize = 0;
                let rc = midiOutOpen(
                    &mut handle as *mut usize as *mut _,
                    dev,
                    0,
                    0,
                    0, // CALLBACK_NULL
                );
                if rc == 0 {
                    out.push(handle);
                }
            }
        }
        let n = out.len();
        guard.0 = out;
        n
    }

    /// Envia un Note On a todos los puertos abiertos.
    pub fn wake() {
        let guard = HANDLES.get_or_init(|| Mutex::new(Handles(Vec::new())));
        let list = match guard.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        unsafe {
            for &h in &list.0 {
                // Un Note On por cada canal. FL filtra el MIDI entrante por el
                // canal que tiene asignado el controller en MIDI Settings y ese
                // canal no es publico, asi que se mandan los 16.
                for ch in 0..16u32 {
                    let msg = 0x90u32 | (ch & 0x0F) | (0x00 << 8) | (0x40 << 16);
                    let _ = midiOutShortMsg(h as *mut _, msg);
                }
                // Respiro ANTES de los note-off. Sin este descanso el driver
                // descarta el burst entero y FL no ve nada: medido, con los 32
                // mensajes seguidos el pump se queda en 0, y con 20ms de
                // respiro el pump avanza con normalidad.
                std::thread::sleep(std::time::Duration::from_millis(20));
                for ch in 0..16u32 {
                    let msg = 0x80u32 | (ch & 0x0F) | (0x00 << 8) | (0x40 << 16);
                    let _ = midiOutShortMsg(h as *mut _, msg);
                }
            }
        }
    }

    pub fn close_all() {
        let guard = HANDLES.get_or_init(|| Mutex::new(Handles(Vec::new())));
        if let Ok(mut g) = guard.lock() {
            unsafe {
                for &h in &g.0 {
                    let _ = midiOutClose(h as *mut _);
                }
            }
            g.0.clear();
        }
    }

    pub fn port_count() -> usize {
        HANDLES
            .get()
            .and_then(|h| h.lock().ok())
            .map(|g| g.0.len())
            .unwrap_or(0)
    }

    static HANDLES: OnceLock<Mutex<Handles>> = OnceLock::new();

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn midiOutGetNumDevs() -> u32;
        fn midiOutOpen(
            handle: *mut usize,
            device: u32,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> u32;
        fn midiOutClose(handle: *mut usize) -> u32;
        fn midiOutShortMsg(handle: *mut usize, message: u32) -> u32;
    }
}

/// Abre (o reabre) los puertos MIDI OUT y los mantiene abiertos.
///
/// Idempotente: la primera llamada abre, las siguientes no hacen nada.
/// Devuelve cuantos puertos hay abiertos.
pub fn open_all() -> usize {
    #[cfg(windows)]
    {
        let n = win::open_all();
        OPEN.store(true, Ordering::Relaxed);
        n
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Envia el wake: un Note On por los 16 canales a cada puerto abierto.
///
/// Barato (32 mensajes) y no hace falta abrir/cerrar nada, que es justo lo que
/// hay que evitar.
pub fn wake() {
    #[cfg(windows)]
    {
        if win::port_count() == 0 {
            // Por si el daemon se constructyo sin llamar a open_all.
            open_all();
        }
        win::wake();
    }
    #[cfg(not(windows))]
    {}
}

/// Wake + espera breve. Se usa cuando la peticion es urgente y no queremos
/// que FL la procese a mitad de escritura.
pub fn wake_and_settle() {
    wake();
    std::thread::sleep(Duration::from_millis(5));
}

/// Cierra los puertos. Solo para tests y para un cierre ordenado.
pub fn close_all() {
    #[cfg(windows)]
    {
        win::close_all();
        OPEN.store(false, Ordering::Relaxed);
    }
}

static OPEN: AtomicBool = AtomicBool::new(false);

/// ¿Estan los puertos abiertos?
pub fn is_open() -> bool {
    #[cfg(windows)]
    {
        OPEN.load(Ordering::Relaxed) && win::port_count() > 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Los puertos MIDI OUT son un recurso global del sistema: si dos tests
    /// los abren a la vez, el segundo falla con MMSYSERR_ALLOCATED. Este mutex
    /// serializa los tests que tocan el estado real.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn abrir_es_idempotente() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        close_all();
        let a = open_all();
        let b = open_all();
        assert_eq!(a, b, "la segunda llamada no debe reabrir ni cambiar el count");
    }

    #[test]
    fn abrir_tras_cerrar_vuelve_a_devolver_puertos() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        close_all();
        let a = open_all();
        assert!(is_open(), "tras open_all deberia estar abierto");
        close_all();
        assert!(!is_open(), "tras close_all no deberia estar abierto");
        let b = open_all();
        assert_eq!(a, b, "reabrir debe devolver los mismos puertos");
    }

    #[test]
    fn wake_no_panea_sin_puertos() {
        // Debe ser seguro incluso si nunca se abrio nada.
        wake();
    }

    #[test]
    fn is_open_refleja_el_estado() {
        open_all();
        // En Windows con loopMIDI deberia quedar abierto; en otros sistemas
        // siempre false. Solo comprobamos que no entre en panic.
        let _ = is_open();
    }
}
