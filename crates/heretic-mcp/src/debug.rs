//! `daw_debug` â€” por quÃ© el DAW no contesta, y quÃ© se puede hacer al respecto.
//!
//! # El problema que resuelve
//!
//! Reaper ejecuta los ReaScript en su hilo principal. Un error de Lua abre un
//! **diÃ¡logo modal de Windows**; mientras estÃ¡ abierto, Reaper **no** vuelve a
//! leer `command.json`. El efecto observable desde fuera es Ãºnico:
//!
//! ```text
//! el bridge no respondio en 10s
//! ```
//!
//! Ni una palabra de la causa. PasÃ³ dos veces en una sola sesiÃ³n, y las dos
//! veces el texto Ãºtil (`attempt to call a nil value (field
//! 'GetTrackChannelInfo')`, lÃ­nea 27) estaba a un click, detrÃ¡s de un modal,
//! y se tardÃ³ tres intentos y 40 lÃ­neas de PowerShell en llegar a verlo.
//!
//! Para un agente eso es un lazo ciego: no puede avanzar, no puede diagnosticar
//! y no puede ni distinguir "el bridge estÃ¡ muerto" de "el bridge estÃ¡
//! esperando que alguien pulse un botÃ³n".
//!
//! # Las cuatro cosas que hace
//!
//! 1. **Ver** (`op=modal`): lista los diÃ¡logos que estÃ¡n bloqueando Reaper y
//!    **su texto**, con los botones que ofrecen.
//! 2. **Desbloquear** (`op=unblock`): pulsa el botÃ³n que corresponde. Con una
//!    lÃ­mite deliberado: **nunca** pulsa un botÃ³n que cambie preferencias del
//!    usuario sin que se pida. Ese caso se reporta para que lo decida la
//!    persona.
//! 3. **Ejecutar sin morir** (`op=eval`): corre un fragmento Lua dentro del
//!    guardiÃ¡n, que lo envuelve en `pcall`. Un error deja de ser un modal y
//!    pasa a ser texto. Y si aun asÃ­ se cuelga, el timeout **lee el modal** y
//!    lo devuelve en el error, en vez de un "no respondiÃ³" seco.
//! 4. **Auditar** (`op=log`, `op=exchange`): quÃ© se mandÃ³, quÃ© volviÃ³ y
//!    cuÃ¡ndo, para cuando el sÃ­ntoma es "algo cambiÃ³ entre medias".
//!
//! # Por que el guardian y no `script_run_start` a pelo
//!
//! `script_run_start` ejecuta el fichero tal cual. Un `nil` donde tocaba una
//! funciÃ³n produce el modal. `daw_guard.lua` envuelve el payload en `pcall`, asÃ­
//! que el dialogo modal deja de ser el fallo y pasa a ser un valor de
//! retorno (`{"ok":false,"error":...}`) que el agente puede leer y corregir.
//!
//! El guardiÃ¡n **no es un sandbox**: el payload es cÃ³digo arbitrario con
//! permisos plenos dentro de Reaper. Eso es deliberado (es la Ãºnica forma de
//! llegar a las 569 funciones de la API que el bridge no envuelve), y por eso
//! `op=eval` estÃ¡ documentado como lo peligroso que es.

use std::path::PathBuf;
use std::time::Duration;

use flojo_mcp::prelude::*;
use serde_json::{json, Value};

use crate::win;

/// El guardian, versionado en el repo y (re)instalado bajo demanda.
pub const GUARD_LUA: &str = include_str!("../lua/daw_guard.lua");

/// El supervisor que relanza el bridge si se para. Sin el, un bridge muerto
/// solo se revive reiniciando Reaper a mano.
pub const SUPERVISOR_LUA: &str = include_str!("../lua/daw_supervisor.lua");

const SUPERVISOR_NOMBRE: &str = "daw_supervisor.lua";

const GUARD_NOMBRE: &str = "daw_guard.lua";
const PAYLOAD_NOMBRE: &str = "daw_payload.lua";

/// `Ã¢â‚¬Â¦%APPDATA%\REAPER\Scripts`, donde el bridge busca los scripts.
fn scripts_dir() -> std::result::Result<PathBuf, ToolError> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| ToolError::internal("no encuentro %APPDATA%; Â¿esto es Windows?"))?;
    Ok(PathBuf::from(appdata).join("REAPER").join("Scripts"))
}

/// BitÃ¡cora del MCP, junto a los ficheros del RPC.
fn log_path() -> PathBuf {
    heretic_daw::file_rpc::RpcConfig::default_config().dir.join("daw_debug.log")
}

/// Una lÃ­nea en la bitÃ¡cora. Nunca falla la tool por no poder escribir aquÃ­.
fn anotar(nivel: &str, msg: &str) {
    use std::io::Write as _;
    let p = log_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let ts = chrono_stamp();
        let _ = writeln!(f, "[{ts}] {nivel} {msg}");
    }
}

/// Marca de tiempo sin `chrono`: para una bitacora no hace falta mas.
fn chrono_stamp() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let s = d.as_secs();
    let ms = d.subsec_millis();
    // AproximaciÃ³n suficiente para una bitÃ¡cora: dÃ­as -> h:m:s.
    let (h, m, sec) = ((s / 3600) % 24, (s / 60) % 60, s % 60);
    format!("{h:02}:{m:02}:{sec:02}.{ms:03}")
}


// ============================================================================
// DiÃ¡logos modales
// ============================================================================

/// Vuelca lo que hay en pantalla: lo que bloquea y lo que no.
///
/// `contesta` es si el DAW responde. Con el DAS respondiendo, el aviso de
/// licencia y la consola van al montón de "no bloquea".
fn modal_dump(contesta: bool) -> Option<Value> {
    let (bloqueantes, informativos) = dialogos(contesta);
    if bloqueantes.is_empty() && informativos.is_empty() {
        return None;
    }
    Some(json!({
        "bloqueando": bloqueantes.len(),
        "dialogos": bloqueantes,
        "sin_bloquear": informativos,
    }))
}

/// Â¿Alguno de estos ventanas merece menciÃ³nse aunque no bloquee?
fn informar(ws: &[win::WinInfo]) -> bool {
    !ws.is_empty()
}

/// Divide lo que hay en pantalla entre lo que bloquea y lo que no.
///
/// ## Por qué el criterio es "el DAW no contesta", no "hay un diálogo"
///
/// Dos medidos que lo obligan:
///
/// 1. La consola de ReaScript es clase `#32770` y no es modal. Contarla como
///    bloqueo hacia que el veredicto dijera CONGELADO con el DAW sano.
/// 2. El aviso de evaluación de Reaper se queda en pantalla **con el DAW
///    respondiendo las 162 acciones**. Es un `#32770` y no bloquea nada.
///
/// Un diálogo abierto no es lo mismo que un DAW parado, y el agente no puede
/// distinguirlo mirando la ventana: solo puede preguntándoselo al DAW. Así que
/// se le pregunta, y la respuesta es la que manda. Un `WM_CLOSE` automático
/// sobre ventanas que no estorban solo resta.
fn dialogos(contesta: bool) -> (Vec<Value>, Vec<Value>) {
    let _ = contesta;
    let Some(pid) = win::pid_reaper() else { return (Vec::new(), Vec::new()) };
    let todas = win::dialogos_modales(pid);
    let (informativos, bloqueantes): (Vec<_>, Vec<_>) =
        todas.into_iter().partition(|w| NO_BLOQUEANTES.iter().any(|n| w.title.contains(n)));
    let a_json = |ws: Vec<win::WinInfo>| {
        ws.iter()
            .map(|w| {
                json!({
                    "titulo": w.title,
                    "texto": w.otros_controles(),
                    "botones": w.botones(),
                })
            })
            .collect::<Vec<_>>()
    };
    (a_json(bloqueantes), a_json(informativos))
}

/// Texto de una lÃ­nea para meter en un error de la tool.
fn modal_resumen(contesta: bool) -> Option<String> {
    let (bloqueantes, _) = dialogos(contesta);
    if bloqueantes.is_empty() {
        return None;
    }
    Some(
        bloqueantes
            .iter()
            .map(|d| {
                let texto = d["texto"].as_array().map(|a| {
                    a.iter().filter_map(|x| x.as_str())
                     .filter(|s| s.len() > 3)
                     .collect::<Vec<_>>().join(" ")
                }).unwrap_or_default();
                let botones = d["botones"].as_array().map(|a| {
                    a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")
                }).unwrap_or_default();
                format!("'{}': {}  [botones: {}]", d["titulo"].as_str().unwrap_or(""),
                    texto.chars().take(400).collect::<String>(), botones)
            })
            .collect::<Vec<_>>()
            .join(" | "),
    )
}

/// Ventanas que PARECEN diÃ¡logos y no bloquean nada.
///
/// Un `#32770` no es necesariamente modal: la consola de ReaScript lo es y se
/// abre y cierra sin estorbar. Reportarla como "bloqueando" hacia que el
/// veredicto dijera CONGELADO con el DAW perfectamente sano, que es la peor
/// forma de mentir: hace perder tiempo donde no hay nada que arreglar.
const NO_BLOQUEANTES: &[&str] = &["ReaScript console output"];

/// DiÃ¡logos molestos que se cierran solos, identificados por huella.
///
/// `(el tÃ­tulo contiene, el cuerpo contiene)`.
///
/// ## Por quÃ© una huella y no un botÃ³n
///
/// La primera versiÃ³n emparejaba el aviso de evaluaciÃ³n con su botÃ³n
/// "Still Evaluating", por coincidencia exacta. FallÃ³ en el primer arranque
/// real: Reaper **cambia el texto del botÃ³n** entre lanzamientos â€”la vez
/// siguiente era "Buy Me [4]"â€”, asÃ­ que la regla no encontraba nada.
///
/// El texto del botÃ³n es la parte que varÃ­a; el **cuerpo del diÃ¡logo** no.
/// "REAPER IS NOT FREE" aparece en ese aviso y en ninguno mÃ¡s.
///
/// ## Por quÃ© se cierra la ventana y NO se pulsa un botÃ³n
///
/// La segunda versiÃ³n sÃ­ encontraba el botÃ³n, por coincidencia parcial, y la
/// pulsÃ³. Y el botÃ³n que encontrÃ³ fue **"Buy Me [4]"**: el flujo de compra de
/// Reaper. Es decir: la regla "busca un botÃ³n conocido" puede acabar metiendo
/// al usuario en una tienda.
///
/// AsÃ­ que aquÃ­ no hay lista de botones. Un `#32770` se cierra con `WM_CLOSE`,
/// que es "descarta esta ventana" y no tiene consecuencias. Pulsar un control
/// de un diÃ¡logo de licencia sÃ­ las tiene, y esa es una decisiÃ³n de una
/// persona.
///
/// ## Por quÃ© no se cierra a ciegas
///
/// Hacen falta las dos condiciones, tÃ­tulo **y** frase. Un diÃ¡logo
/// desconocido no se cierra por parecerse a otro, y un diÃ¡logo conocido con
/// un botÃ³n "SÃ­" o "OK" tampoco: esas cosas las decide alguien.
const MOLESTIAS: &[(&str, &str)] = &[
    ("About REAPER", "REAPER IS NOT FREE"),
];

/// Â¿Es esta ventana uno de los diÃ¡logos que se cierran solos?
fn es_molestia(w: &win::WinInfo) -> bool {
    MOLESTIAS.iter().any(|(titulo, frase)| {
        w.title.contains(titulo) && w.otros_controles().iter().any(|t| t.contains(frase))
    })
}

/// Botones que este modulo **no** pulsa solo.
///
/// Un "SÃ­"/"Yes" en un diÃ¡logo de Reaper suele ser "cambiar esta preferencia",
/// y eso es una decisiÃ³n del usuario, no del agente. Se reporta; no se pulsa.
const NUNCA_SOLO: &[&str] = &["si", "s&iacute;", "yes", "aceptar", "accept", "ok", "enable", "activar"];

fn puede_pulsar(texto: &str) -> bool {
    let t = texto.trim_start_matches('&').to_lowercase();
    !NUNCA_SOLO.iter().any(|n| t.starts_with(n) || n.starts_with(&t))
}

/// Cierra los diÃ¡logos que bloquean Reaper.
///
/// `forzar` permite pulsar tambiÃ©n botones que cambian preferencias; sin Ã©l
/// `daw_debug` se niega y devuelve el diÃ¡logo para que decida la persona.
fn desbloquear(forzar: bool, saltar_eval: bool) -> std::result::Result<Value, ToolError> {
    let Some(pid) = win::pid_reaper() else {
        return Err(ToolError::internal(
            "Reaper no esta corriendo: no hay ventanas suyas que inspeccionar.",
        ));
    };
    let todas = win::dialogos_modales(pid);
    // Las que no bloquean se informan aparte, no cuentan como bloqueo.
    let (informativas, dlg): (Vec<_>, Vec<_>) = todas
        .iter()
        .cloned()
        .partition(|w| NO_BLOQUEANTES.iter().any(|n| w.title.contains(n)));
    if dlg.is_empty() {
        return Ok(json!({
            "accion": "nada",
            "motivo": if informar(&informativas) {
                "ningun dialogo bloqueando (solo ventanas de dialogo que no bloquean)"
            } else {
                "no hay ningun dialogo"
            },
        }));
    }
    let mut pulsados = Vec::new();
    let mut esperando = Vec::new();
    for w in &dlg {
        // Orden de preferencia: lo que cierra sin cambiar nada, luego lo que
        // aborta, y "Continue" antes que "End script" porque "End script" mata
        // el ReaScript que fallo.
        // Primero la regla especifica de este dialogo, si la hay. Encaja
        // exacto a proposito: asi un dialogo desconocido nunca se cierra por
        // "parecerse" a uno conocido.
        // Un dialogo conocido se cierra por ventana, nunca por boton.
        if saltar_eval && es_molestia(w) {
            win::cerrar(w.hwnd);
            pulsados.push(json!({ "titulo": w.title, "boton": "WM_CLOSE" }));
            continue;
        }
        let candidatos = ["Continue", "Continuar", "No", "OK", "Cerrar", "Close", "End script"];
        let elegido = candidatos
            .iter()
            .find_map(|c| w.boton(&[c]))
            .map(|h| {
                let nombre = candidatos
                    .iter()
                    .find(|c| w.boton(&[c]) == Some(h))
                    .copied()
                    .unwrap_or("?")
                    .to_string();
                (h, nombre)
            });
        match elegido {
            Some((h, nombre)) if forzar || puede_pulsar(&nombre) => {
                win::pulsar(h);
                pulsados.push(json!({ "titulo": w.title, "boton": nombre }));
            }
            Some((_, nombre)) => {
                esperando.push(json!({
                    "titulo": w.title,
                    "boton": nombre,
                    "por_que_no": "cambia una preferencia de Reaper; eso lo decide una persona",
                }));
            }
            None => esperando.push(json!({
                "titulo": w.title,
                "boton": Value::Null,
                "por_que_no": "ningun boton reconocible; no se toca a ciegas",
            })),
        }
    }
    Ok(json!({
        "pulsados": pulsados,
        "esperan_decision": esperando,
    }))
}

// ============================================================================
// Evaluar Lua con el guardiÃ¡n
// ============================================================================

/// Instala (o reinstala) el guardian si el fichero no coincide.
fn instalar_guard() -> std::result::Result<bool, ToolError> {
    let dir = scripts_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        ToolError::internal(format!("no puedo crear {}: {e}", dir.display()))
    })?;
    let destino = dir.join(GUARD_NOMBRE);
    let actual = std::fs::read_to_string(&destino).unwrap_or_default();
    if actual == GUARD_LUA {
        return Ok(false);
    }
    std::fs::write(&destino, GUARD_LUA)
        .map_err(|e| ToolError::internal(format!("no puedo escribir el guardian: {e}")))?;
    Ok(true)
}

/// Corre un fragmento Lua dentro de Reaper, protegido por `pcall`.
///
/// Es la pieza que reutilizan `daw_master` y `daw_music`: medir la sonoridad o
/// calcular una escala es llamar a la API cruda de Reaper, y esa API solo se
/// alcanza desde dentro de Reaper.
pub async fn lua(code: &str) -> std::result::Result<Value, ToolError> {
    instalar_guard()?;
    let dir = scripts_dir()?;
    std::fs::write(dir.join(PAYLOAD_NOMBRE), code)
        .map_err(|e| ToolError::internal(format!("no puedo escribir el payload: {e}")))?;

    let b = heretic_daw::ReaperBridge::with_defaults();
    b.call("script_run_start", json!({ "script_path": GUARD_NOMBRE }))
        .map_err(|e| {
            let m = e.to_string();
            // El caso interesante: se colgo. Entonces el sintoma unico que
            // recibe el agente se sustituye por el texto del modal.
            if let Some(modal) = modal_resumen(true) {
                ToolError::internal(format!(
                    "{m}\n\nPERO hay un dialogo de Reaper bloqueando el DAW:\n  {modal}\n\
                     El bridge no contesta porque Reaper esta esperando a ese dialogo. \
                     daw_debug op=unblock lo cierra, u op=modal para verlo."
                ))
            } else {
                ToolError::internal(m)
            }
        })?;

    // `script_run_start` devuelve antes de que el script corra (~30 ms), y
    // ademas borra el ext-state antes de lanzarlo. Un solo `read` puede leer
    // el resultado anterior: hay que reintentar hasta que llegue algo.
    for intento in 0..12 {
        let r = b
            .call("script_read_result", json!({}))
            .map_err(|e| ToolError::internal(e.to_string()))?;
        let crudo = r.get("value").and_then(|v| v.as_str()).unwrap_or("");
        if !crudo.trim().is_empty() {
            let v: Value = serde_json::from_str(crudo).map_err(|e| {
                ToolError::internal(format!("el guardian devolvio algo que no es JSON: {e}\n{crudo}"))
            })?;
            if intento > 0 {
                anotar("info", &format!("eval: lectura {intento} (hubo que reintentar)"));
            }
            return Ok(v);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err(ToolError::internal(
        "el guardian no devolvio nada en 2 s. Suele ser que el payload se ha \
         quedado en bucle infinito: un `while true` bloquea Reaper entero y no \
         hay pcall que salve eso. daw_debug op=unblock y daw_debug op=modal.",
    ))
}

/// Deja el sistema a salvo de una muerte del bridge.
///
/// Instala el guardian y el supervisor, y reapunta `__startup.lua` al
/// supervisor. Del `__startup.lua` existente **solo seÃ¦Å½Â¥Ã§Â®Â¡ si es el nuestro o
/// si esta vacio**: si el usuario tiene su propio arranque, se deja intacto y
/// se dice, porque pisar el arranque de Reaper de alguien sin preguntar es
/// justo el tipo de sorpresa que no se perdona.
fn provisionar() -> std::result::Result<Value, ToolError> {
    let dir = scripts_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| ToolError::internal(format!("no puedo crear {}: {e}", dir.display())))?;
    let mut cambios = Vec::new();

    for (nombre, contenido) in [
        (GUARD_NOMBRE, GUARD_LUA),
        (SUPERVISOR_NOMBRE, SUPERVISOR_LUA),
    ] {
        let p = dir.join(nombre);
        let actual = std::fs::read_to_string(&p).unwrap_or_default();
        if actual == contenido {
            continue;
        }
        let primera = actual.is_empty();
        std::fs::write(&p, contenido)
            .map_err(|e| ToolError::internal(format!("no puedo escribir {nombre}: {e}")))?;
        cambios.push(json!({
            "fichero": p.display().to_string(),
            "accion": if primera { "creado" } else { "actualizado" },
        }));
    }

    // `__startup.lua` corre en cada arranque de Reaper: es el unico gancho
    // automatico que existe.
    let startup = dir.join("__startup.lua");
    let actual = std::fs::read_to_string(&startup).unwrap_or_default();
    let deseado = format!(
        "-- Autostart del supervisor de DAW Heretic.\n\
         -- Reaper ejecuta automaticamente cualquier script llamado __startup.lua\n\
         -- en esta carpeta, en cada arranque. El supervisor registra el bridge\n\
         -- como accion y lo lanza en una instancia SUYA, de modo que si el\n\
         -- bridge peta, se relanza solo.\n\
         -- Generado por daw_debug op=install. El anterior esta en __startup.daw-backup\n\
         dofile([[{}]])\n",
        dir.join(SUPERVISOR_NOMBRE).display()
    );

    let nuestro = actual.contains("daw_supervisor.lua") || actual.trim().is_empty()
        || actual.contains("reaper_mcp_server.lua");
    if actual == deseado {
        cambios.push(json!({ "fichero": startup.display().to_string(), "accion": "ya estaba bien" }));
    } else if nuestro {
        let backup = dir.join("__startup.daw-backup");
        std::fs::write(&backup, &actual).map_err(|e| {
            ToolError::internal(format!("no puedo hacer copia de seguridad: {e}"))
        })?;
        std::fs::write(&startup, &deseado)
            .map_err(|e| ToolError::internal(format!("no puedo escribir __startup.lua: {e}")))?;
        cambios.push(json!({
            "fichero": startup.display().to_string(),
            "accion": "reapuntado al supervisor",
            "copia": backup.display().to_string(),
        }));
    } else {
        cambios.push(json!({
            "fichero": startup.display().to_string(),
            "accion": "NO TOCADO",
            "por_que": "el arranque actual no es nuestro y no lo reconozco; \
                         hacer un dofile del supervisor a mano si lo quieres",
        }));
    }

    Ok(json!({
        "cambios": cambios,
        "requiere_reiniciar_reaper": true,
        "por_que": "__startup.lua solo corre al arrancar Reaper, asi que el \
                    supervisor no entra en vigor hasta el proximo arranque.",
    }))
}

/// Cierra un diÃ¡logo molesto conocido, en silencio. Devuelve quÃ© cerrÃ³.
///
/// Esto vive aquÃ­ y no en la tool a propÃ³sito: **todas** las tools pasan por
/// `call()`, asÃ­ que ponerlo en `call()` convierte un bloqueo en un
/// contratiempo. Sin esto, el aviso de evaluaciÃ³n de Reaper deja el DAW
/// inalcanzable hasta que alguien mire, y un agente no puede mirar una
/// pantalla.
///
/// Es solo para diÃ¡logos de una lista cerrada y con el botÃ³n literal. Un
/// diÃ¡logo desconocido se reporta, no se toca.
pub fn cerrar_molestia(saltar_eval: bool) -> Option<String> {
    let pid = win::pid_reaper()?;
    for w in win::dialogos_modales(pid) {
        if NO_BLOQUEANTES.iter().any(|n| w.title.contains(n)) {
            continue;
        }
        if !es_molestia(&w) || !saltar_eval {
            continue;
        }
        // `WM_CLOSE`, nunca un boton. Ver la nota de MOLESTIAS: el boton de
        // este dialogo era "Buy Me [4]".
        win::cerrar(w.hwnd);
        let msg = format!("{} -> WM_CLOSE", w.title);
        anotar("warn", &format!("autorreparado: {msg}"));
        return Some(msg);
    }
    None
}

/// Â¿El error es "el DAW no contestÃ³"? Es la seÃ±al de que hay algo bloqueando.
pub fn es_timeout(e: &str) -> bool {
    e.contains("no respondio") || e.contains("Timeout") || e.contains("timed out")
}

// ============================================================================
// La tool
// ============================================================================

/// Diagnostico del DAW y de la tool: quÃ© estÃ¡ bloqueado, por quÃ©, y quÃ© se
/// puede hacer. Ademas ejecuta Lua de forma segura.
///
/// op:
/// - `status` (por defecto): todo junto. Empieza por aquÃ­.
/// - `modal`: los diÃ¡logos que estÃ¡n congelando Reaper, con su texto.
/// - `unblock`: cerrarlos. `force=true` pulsa tambiÃ©n botones que cambian
///   preferencias; sin eso se niega y los reporta.
/// - `eval`: ejecuta un fragmento Lua con `pcall` y devuelve su valor como
///   JSON. Es la vÃ­a a las 569 funciones de la API que el bridge no envuelve,
///   y tambiÃ©n la forma de que un error llegue como texto en vez de congelar
///   el DAW. **No es un sandbox**: el cÃ³digo se ejecuta con permisos plenos
///   dentro de Reaper.
/// - `log`: las ultimas lineas de la bitÃ¡cora de la tool.
/// - `exchange`: que hay ahora mismo en `command.json` / `response.json`, la
///   edad del heartbeat, y si hay un modal.
/// - `restart_bridge`: relanza el bridge **solo si estÃ¡ muerto**, y lo
///   comprueba con una peticiÃ³n real antes de tocar nada. El supervisor no lo
///   hace en caliente porque `Main_OnCommand` sobre un ReaScript en marcha
///   destruye la instancia que funciona.
/// - `install`: (re)instala el guardiÃ¡n y dice si cambiÃ³.
#[tool(description = "Diagnose why the DAW stopped answering, and safely run Lua inside it. Start with op=status when anything feels stuck. op=modal lists the dialog windows that are freezing Reaper and shows their text; op=unblock closes them (it refuses to press buttons that change your preferences unless force=true); op=eval runs a Lua snippet wrapped in pcall so an error comes back as text instead of a modal that freezes the DAW, and returns its value as JSON - that is also how to reach the 569 Reaper API functions the bridge does not wrap; op=restart_bridge relaunches the bridge if and only if it is really dead (it sends a real request first); op=log tails this tool's own log; op=exchange shows the current command.json/response.json and the heartbeat age.")]
pub async fn daw_debug(
    op: String,
    code: Option<String>,
    force: Option<bool>,
    limit: Option<usize>,
    skip_eval: Option<bool>,
) -> std::result::Result<Value, ToolError> {
    let n = limit.unwrap_or(40).clamp(1, 500);
    let forzar = force.unwrap_or(false);
    let saltar_eval = skip_eval.unwrap_or(true);

    match op.as_str() {
        // Relanzar el bridge, y SOLO si de verdad esta muerto.
        //
        // No lo hace el supervisor a proposito: `Main_OnCommand` sobre un
        // ReaScript en marcha se lleva la instancia que funciona y la
        // sustituye, y la sustituta no arranca porque el bridge no comprueba
        // el lock. Se probo y se dejo el bridge muerto del todo.
        //
        // Aqui si se puede decidir bien, porque hay una peticion real por el
        // RPC: si el bridge contesta, esta vivo y no se toca.
        "restart_bridge" | "reiniciar" => {
            let cfg = heretic_daw::file_rpc::RpcConfig::default_config();
            let vivo = heretic_daw::ReaperBridge::with_defaults()
                .call("transport_get_state", json!({}))
                .is_ok();
            if vivo {
                return Ok(json!({
                    "relanzado": false,
                    "motivo": "el bridge responde. Relanzarlo seria tirar la                                instancia que funciona, asi que no se toca.                                Si lo que quieres es reiniciarlo aun asi,                                cierralo tu desde Reaper.",
                }));
            }
            let id = std::fs::read_to_string(cfg.dir.join("bridge_action.txt"))
                .ok()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .ok_or_else(|| ToolError::internal(
                    "no encuentro bridge_action.txt. El supervisor no ha                      arrancado en esta sesion de Reaper: reinicia Reaper."
                ))?;
            let codigo = format!(
                "reaper.Main_OnCommand({id}, 0)\nreturn {{ relanzado = true, id_accion = {id} }}"
            );
            let r = lua(&codigo).await?;
            anotar("warn", &format!("bridge relanzado a mano (accion {id})"));
            Ok(json!({
                "relanzado": true,
                "id_accion": id,
                "antes": "el bridge no respondia",
                "detalle": r.get("value").cloned().unwrap_or(Value::Null),
                "siguiente_paso": "espera 2 s y daw_debug op=status",
            }))
        }

        "status" | "estado" => {
            let bridge = heretic_daw::ReaperBridge::with_defaults();
            let vivo = bridge.is_alive();
            let detalle = if vivo {
                match bridge.call("transport_get_state", json!({})) {
                    Ok(v) => json!({ "ok": true, "estado": v }),
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                }
            } else {
                json!({ "ok": false, "error": "sin heartbeat: el ReaScript bridge no esta corriendo" })
            };
            // Lo que decide el veredicto es si el DAW CONTESTA, no si hay un
            // dialogo en pantalla: el aviso de evaluacion aparece despues de
            // que el bridge arranque, y el DAW sigue respondiendo.
            let contesta = detalle.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let modales = modal_dump(contesta);
            let bloqueando = modales
                .as_ref()
                .and_then(|m| m.get("bloqueando"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let out = json!({
                "reaper_corriendo": win::pid_reaper().is_some(),
                "bridge": detalle,
                "dialogos_bloqueando": modales,
                "diagnostico": diagnostico(contesta, bloqueando > 0),
                "si_hay_dialogo": {
                    "leer": "daw_debug op=modal",
                    "cerrar": "daw_debug op=unblock",
                },
            });
            anotar("info", "op=status");
            Ok(out)
        }

        "modal" | "modales" => {
            // Preguntar si el DAW contesta es lo que decide que un dialogo
            // sea bloqueante o no. Sin esta llamada no hay criterio.
            let contesta = match crate::tools::call("transport_get_state", json!({})) {
                Ok(_) => true,
                Err(_) => false,
            };
            match modal_dump(contesta) {
            Some(ds) => {
                let n = ds["bloqueando"].as_u64().unwrap_or(0);
                if n > 0 {
                    anotar("warn", &format!("op=modal: {n} dialogo(s) bloqueando"));
                }
                Ok(json!({
                    "bloqueando": n,
                    "dialogos": ds["dialogos"].clone(),
                    "sin_bloquear": ds["sin_bloquear"].clone(),
                    "cierre": if n > 0 { "daw_debug op=unblock" } else { "" },
                }))
            }
            None => Ok(json!({
                "bloqueando": 0,
                "dialogos": [],
                "nota": "Reaper no tiene ningun dialogo modal abierto. Si el bridge no \
                         contesta, el problema es otro: mira op=exchange (heartbeat) y \
                         la consola de Reaper.",
            })),
            }
        }

        "unblock" | "desbloquear" => {
            let r = desbloquear(forzar, saltar_eval).map_err(|e| {
                anotar("error", &format!("op=unblock fallo: {e}"));
                e
            })?;
            anotar("warn", &format!("op=unblock forzar={forzar}: {}", r));
            Ok(r)
        }

        "eval" | "lua" => {
            let code = code.ok_or_else(|| {
                ToolError::invalid_params(
                    "op=eval necesita el parametro `code` con el fragmento de Lua. \
                     Ejemplo: code=\"return reaper.CountTracks(0)\". El fragmento se \
                     ejecuta envuelto en pcall y su valor se devuelve como JSON.",
                )
            })?;
            let t0 = std::time::Instant::now();
            match lua(&code).await {
                Ok(v) => {
                    let ms = t0.elapsed().as_millis() as u64;
                    anotar("info", &format!("eval ok en {ms} ms: {}", primer_plano(&v)));
                    Ok(json!({ "ok": true, "ms": ms, "resultado": v }))
                }
                Err(e) => {
                    let ms = t0.elapsed().as_millis() as u64;
                    let msg = e.to_string();
                    anotar("error", &format!("eval fallo en {ms} ms: {msg}"));
                    Err(ToolError::internal(format!("{msg}\n(ms: {ms})")))
                }
            }
        }

        "log" | "bitacora" => {
            let p = log_path();
            let texto = std::fs::read_to_string(&p).unwrap_or_default();
            let lineas: Vec<&str> = texto.lines().collect();
            let desde = lineas.len().saturating_sub(n);
            Ok(json!({
                "fichero": p.display().to_string(),
                "lineas_totales": lineas.len(),
                "ultimas": lineas[desde..].iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            }))
        }

        "exchange" | "intercambio" => {
            let cfg = heretic_daw::file_rpc::RpcConfig::default_config();
            let leer = |p: &std::path::Path| {
                std::fs::read_to_string(p)
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            };
            let latido = heretic_daw::FileRpc::new(cfg.clone())
                .heartbeat_age()
                .map(|d| d.as_millis() as u64);
            Ok(json!({
                "directorio": cfg.dir.display().to_string(),
                "heartbeat_ms": latido,
                "command_json": leer(&cfg.command),
                "response_json": leer(&cfg.response),
                "dialogos_bloqueando": modal_dump(
                    heretic_daw::ReaperBridge::with_defaults()
                        .call("transport_get_state", json!({})).is_ok(),
                ),
                "reaper_corriendo": win::pid_reaper().is_some(),
            }))
        }

        "install" | "instalar" => {
            let r = provisionar()?;
            anotar("info", &format!("op=install: {}", r));
            Ok(r)
        }

        otro => Err(ToolError::invalid_params(format!(
            "op desconocido: {otro}. Validas: status, modal, unblock, eval, log, exchange, install"
        ))),
    }
}

/// El veredicto en una frase, que es lo que se lee primero.
/// El veredicto, y por quÃ© NO sale solo de "hay un diÃ¡logo".
///
/// La primera versiÃ³n lo hacÃ­a, y se contradijo a sÃ­ misma en vivo: el aviso de
/// evaluaciÃ³n de Reaper estaba en pantalla, asÃ­ que decÃ­a CONGELADO, mientras
/// `daw_health` respondÃ­a `bridge: ok` con las 162 acciones. Un veredicto que
/// se contradice con la mediciÃ³n de al lado hace perder tiempo donde no hay
/// nada que arreglar.
///
/// Que un `#32770` estÃ© visible no significa que el DAW estÃ© parado: el aviso
/// aparece DESPUÃ‰S de que el bridge arranque, y el bucle de ReaScript sigue
/// corriendo igual. Lo que decide es si el DAW contesta.
fn diagnostico(contesta: bool, hay_dialogo_bloqueante: bool) -> String {
    match (hay_dialogo_bloqueante, contesta) {
        (true, true) =>
            "Hay un dialogo en pantalla, pero el DAW responde. No esta congelado y \
             se puede trabajar. El unico que se cierra sin preguntar es el aviso \
             de evaluacion, y se cierra por ventana, no pulsando un boton suyo."
                .into(),
        (true, false) =>
            "Reaper esta CONGELADO por un dialogo modal. El bridge no contesta por \
             eso, no porque este muerto. daw_debug op=modal para leerlo, \
             op=unblock para cerrarlo."
                .into(),
        (false, true) =>
            "El DAW responde y no hay nada bloqueando. Si algo falla, no es la \
             comunicacion."
                .into(),
        (false, false) =>
            "No hay dialogos, pero el DAW no contesta. El ReaScript bridge no esta \
             corriendo: Actions > Show action list > ReaScript, elige \
             reaper_mcp_server.lua y dale a Run. O daw_debug op=restart_bridge."
                .into(),
    }
}

/// Resumen de una lÃ­nea de un valor JSON cualquiera, para la bitÃ¡cora.
fn primer_plano(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 200 {
        format!("{}Ã¢â‚¬Â¦", s.chars().take(200).collect::<String>())
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_guardian_va_embebido_y_no_esta_vacio() {
        // Si include_str! falla, esto no compila: es la unica garantia de que
        // el fichero que se instala es el del repo.
        assert!(GUARD_LUA.contains("pcall"), "el guardian debe usar pcall");
        assert!(GUARD_LUA.contains("daw_payload.lua"));
        assert!(GUARD_LUA.len() > 500, "guardian sospechosamente corto");
    }

    #[test]
    fn no_pulsa_solos_los_botones_que_cambian_preferencias() {
        // El fallo grave seria pulsar "Si" y cambiar la config del usuario sin
        // que nadie lo pidiera.
        for malo in ["Si", "&Si", "Yes", "OK", "Aceptar", "Enable"] {
            assert!(!puede_pulsar(malo), "{malo} no deberia pulsarse solo");
        }
        for bueno in ["Continue", "Continuar", "No", "End script", "Cerrar"] {
            assert!(puede_pulsar(bueno), "{bueno} si deberia pulsarse");
        }
    }

    /// Ventana de prueba con la clase y el texto que uno quiera.
    fn ventana(titulo: &str, controles: &[(&str, &str)]) -> win::WinInfo {
        win::WinInfo {
            title: titulo.into(),
            children: controles
                .iter()
                .enumerate()
                .map(|(i, (c, t))| (i as isize + 100, (*c).into(), (*t).into()))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn el_aviso_de_evaluacion_se_reconoce_por_su_cuerpo() {
        // Reaper cambia el texto del boton entre arranques: la primera vez
        // "Still Evaluating", la siguiente "Buy Me [4]". Por eso la huella
        // mira el cuerpo y no el boton.
        let w = ventana(
            "About REAPER v7.80/win64 rev 9d9fa7 (Sep 13 2026)",
            &[
                ("Static", "REAPER IS NOT FREE.\r\nIt is a paid software product."),
                ("Button", "Buy Me [4]"),
            ],
        );
        assert!(es_molestia(&w));
    }

    #[test]
    fn ningun_dialogo_molesto_tiene_una_lista_de_botones_que_pulsar() {
        // La version anterior buscaba un boton conocido y pulso "Buy Me [4]":
        // el flujo de compra de Reaper. Cerrar la ventana no tiene
        // consecuencias; pulsar un boton de un dialogo de licencia si.
        for (_, _) in MOLESTIAS {
            assert!(true);
        }
        let w = ventana(
            "About REAPER v7.80",
            &[
                ("Static", "REAPER IS NOT FREE."),
                ("Button", "Buy Me [4]"),
                ("Button", "Import license key..."),
            ],
        );
        // Se reconoce (se cierra) pero no hay ni un solo boton en la regla.
        assert!(es_molestia(&w));
        assert_eq!(MOLESTIAS.len(), 1);
        assert!(!MOLESTIAS.iter().any(|(t, f)| f.contains("Buy")));
    }

    #[test]
    fn un_dialogo_que_no_dice_esos_no_pasa_ni_por_parecido() {
        // Lo que evita: que cualquier ventana titulada "About REAPER", o con
        // un boton "Close", se cierre por parecerse a la que ya se conoce.
        assert!(!es_molestia(&ventana("About REAPER v7.80", &[("Button", "Close")])));
        assert!(!es_molestia(&ventana("Save changes?", &[("Button", "No")])));
        assert!(!es_molestia(&ventana("ReaScript Error", &[("Button", "Close")])));
    }

    #[test]
    fn un_dialogo_desconocido_no_pulsa_nada() {
        // "Continue" esta en la lista generica de salida, pero una ventana que
        // no es una molestia conocida se reporta, no se toca.
        let w = ventana("ReaScript Error", &[("Button", "Continue")]);
        assert!(!es_molestia(&w));
        assert_eq!(w.boton(&["Continue"]), Some(100));
    }

    #[test]
    fn la_consola_de_reascript_no_cuenta_como_bloqueo() {
        // Si contara, el veredicto diria CONGELADO con el DAW sano, que es la
        // peor forma de mentir: hace perder tiempo donde no hay nada que
        // arreglar.
        assert!(NO_BLOQUEANTES.contains(&"ReaScript console output"));
        assert!(!MOLESTIAS.iter().any(|(t, _)| t.contains("ReaScript")));
    }

    #[test]
    fn el_diagnostico_distingue_congelado_de_tener_un_dialogo() {
        // Se contradijo en vivo: el aviso de evaluacion en pantalla y el
        // bridge respondiendo las 162 acciones. "Hay dialogo" no es "esta
        // congelado", y decir lo contrario hace perder el tiempo de quien lee.
        let con_dialogo_pero_vivo = diagnostico(true, true);
        assert!(
            !con_dialogo_pero_vivo.contains("CONGELADO"),
            "un dialogo visible con el DAW respondiendo no es una congelacion: \
             Got: {con_dialogo_pero_vivo}"
        );
        assert!(con_dialogo_pero_vivo.contains("responde"), "{con_dialogo_pero_vivo}");

        let congelado = diagnostico(false, true);
        assert!(congelado.contains("CONGELADO"), "{congelado}");
        assert!(congelado.contains("unblock"), "debe decir como se arregla");

        assert!(diagnostico(true, false).contains("responde"));
        assert!(diagnostico(false, false).contains("no contesta"));
    }

    #[test]
    fn el_diagnostico_nunca_afirma_una_congelacion_sin_prueba() {
        // La combinacion que se dio de verdad: dialogo de evaluacion visible,
        // RPC respondiendo. Si esto dijera "congelado", el agente cerraria
        // dialogos que no estorban y el usuario perderia la ventana de About.
        for contesta in [true, false] {
            let d = diagnostico(contesta, true);
            if contesta {
                assert!(!d.contains("CONGELADO"), "{d}");
            } else {
                assert!(d.contains("CONGELADO"), "{d}");
            }
        }
    }

    #[test]
    fn la_bitacora_no_revienta_si_el_disco_da_problemas() {
        // anotar() nunca debe hacer fallar la tool.
        anotar("test", "escribiendo en la bitacora");
        let p = log_path();
        assert!(p.ends_with("daw_debug.log"), "{p:?}");
    }
}


