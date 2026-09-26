//! `daw_debug` — por qué el DAW no contesta, y qué se puede hacer al respecto.
//!
//! # El problema que resuelve
//!
//! Reaper ejecuta los ReaScript en su hilo principal. Un error de Lua abre un
//! **diálogo modal de Windows**; mientras está abierto, Reaper **no** vuelve a
//! leer `command.json`. El efecto observable desde fuera es único:
//!
//! ```text
//! el bridge no respondio en 10s
//! ```
//!
//! Ni una palabra de la causa. Pasó dos veces en una sola sesión, y las dos
//! veces el texto útil (`attempt to call a nil value (field
//! 'GetTrackChannelInfo')`, línea 27) estaba a un click, detrás de un modal,
//! y se tardó tres intentos y 40 líneas de PowerShell en llegar a verlo.
//!
//! Para un agente eso es un lazo ciego: no puede avanzar, no puede diagnosticar
//! y no puede ni distinguir "el bridge está muerto" de "el bridge está
//! esperando que alguien pulse un botón".
//!
//! # Las cuatro cosas que hace
//!
//! 1. **Ver** (`op=modal`): lista los diálogos que están bloqueando Reaper y
//!    **su texto**, con los botones que ofrecen.
//! 2. **Desbloquear** (`op=unblock`): pulsa el botón que corresponde. Con una
//!    límite deliberado: **nunca** pulsa un botón que cambie preferencias del
//!    usuario sin que se pida. Ese caso se reporta para que lo decida la
//!    persona.
//! 3. **Ejecutar sin morir** (`op=eval`): corre un fragmento Lua dentro del
//!    guardián, que lo envuelve en `pcall`. Un error deja de ser un modal y
//!    pasa a ser texto. Y si aun así se cuelga, el timeout **lee el modal** y
//!    lo devuelve en el error, en vez de un "no respondió" seco.
//! 4. **Auditar** (`op=log`, `op=exchange`): qué se mandó, qué volvió y
//!    cuándo, para cuando el síntoma es "algo cambió entre medias".
//!
//! # Por que el guardian y no `script_run_start` a pelo
//!
//! `script_run_start` ejecuta el fichero tal cual. Un `nil` donde tocaba una
//! función produce el modal. `daw_guard.lua` envuelve el payload en `pcall`, así
//! que el dialogo modal deja de ser el fallo y pasa a ser un valor de
//! retorno (`{"ok":false,"error":...}`) que el agente puede leer y corregir.
//!
//! El guardián **no es un sandbox**: el payload es código arbitrario con
//! permisos plenos dentro de Reaper. Eso es deliberado (es la única forma de
//! llegar a las 569 funciones de la API que el bridge no envuelve), y por eso
//! `op=eval` está documentado como lo peligroso que es.

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

/// `…%APPDATA%\REAPER\Scripts`, donde el bridge busca los scripts.
fn scripts_dir() -> std::result::Result<PathBuf, ToolError> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| ToolError::internal("no encuentro %APPDATA%; ¿esto es Windows?"))?;
    Ok(PathBuf::from(appdata).join("REAPER").join("Scripts"))
}

/// Bitácora del MCP, junto a los ficheros del RPC.
fn log_path() -> PathBuf {
    heretic_daw::file_rpc::RpcConfig::default_config().dir.join("daw_debug.log")
}

/// Una línea en la bitácora. Nunca falla la tool por no poder escribir aquí.
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
    // Aproximación suficiente para una bitácora: días -> h:m:s.
    let (h, m, sec) = ((s / 3600) % 24, (s / 60) % 60, s % 60);
    format!("{h:02}:{m:02}:{sec:02}.{ms:03}")
}


// ============================================================================
// Diálogos modales
// ============================================================================

/// Vuelca los diálogos que bloquean Reaper a algo legible.
fn modal_dump() -> Option<Vec<Value>> {
    let pid = win::pid_reaper()?;
    let dlg = win::dialogos_modales(pid);
    if dlg.is_empty() {
        return None;
    }
    Some(
        dlg.iter()
            .map(|w| {
                json!({
                    "titulo": w.title,
                    "texto": w.children.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(),
                    "botones": w.children.iter()
                        .filter(|(_, t)| t.len() < 40 && !t.contains('\n'))
                        .map(|(_, t)| t.as_str())
                        .collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// Texto de una línea para meter en un error de la tool.
fn modal_resumen() -> Option<String> {
    let ds = modal_dump()?;
    Some(
        ds.iter()
            .map(|d| {
                let texto = d["texto"].as_array().map(|a| {
                    a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")
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

/// Botones que este modulo **no** pulsa solo.
///
/// Un "Sí"/"Yes" en un diálogo de Reaper suele ser "cambiar esta preferencia",
/// y eso es una decisión del usuario, no del agente. Se reporta; no se pulsa.
const NUNCA_SOLO: &[&str] = &["si", "s&iacute;", "yes", "aceptar", "accept", "ok", "enable", "activar"];

fn puede_pulsar(texto: &str) -> bool {
    let t = texto.trim_start_matches('&').to_lowercase();
    !NUNCA_SOLO.iter().any(|n| t.starts_with(n) || n.starts_with(&t))
}

/// Cierra los diálogos que bloquean Reaper.
///
/// `forzar` permite pulsar también botones que cambian preferencias; sin él
/// `daw_debug` se niega y devuelve el diálogo para que decida la persona.
fn desbloquear(forzar: bool) -> std::result::Result<Value, ToolError> {
    let Some(pid) = win::pid_reaper() else {
        return Err(ToolError::internal(
            "Reaper no esta corriendo: no hay ventanas suyas que inspeccionar.",
        ));
    };
    let dlg = win::dialogos_modales(pid);
    if dlg.is_empty() {
        return Ok(json!({ "accion": "nada", "motivo": "no hay ningun dialogo bloqueando" }));
    }
    let mut pulsados = Vec::new();
    let mut esperando = Vec::new();
    for w in &dlg {
        // Orden de preferencia: lo que cierra sin cambiar nada, luego lo que
        // aborta, y "Continue" antes que "End script" porque "End script" mata
        // el ReaScript que fallo.
        let candidatos = ["Continue", "Continuar", "No", "OK", "Cerrar", "Close", "End script"];
        let elegido = candidatos
            .iter()
            .find_map(|c| w.boton(&[c]))
            .map(|h| (h, candidatos.iter().find(|c| w.boton(&[c]) == Some(h)).copied().unwrap_or("?")));
        match elegido {
            Some((h, nombre)) if forzar || puede_pulsar(nombre) => {
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
// Evaluar Lua con el guardián
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
async fn evaluar(code: &str) -> std::result::Result<Value, ToolError> {
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
            if let Some(modal) = modal_resumen() {
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
/// supervisor. Del `__startup.lua` existente **solo se接管 si es el nuestro o
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

// ============================================================================
// La tool
// ============================================================================

/// Diagnostico del DAW y de la tool: qué está bloqueado, por qué, y qué se
/// puede hacer. Ademas ejecuta Lua de forma segura.
///
/// op:
/// - `status` (por defecto): todo junto. Empieza por aquí.
/// - `modal`: los diálogos que están congelando Reaper, con su texto.
/// - `unblock`: cerrarlos. `force=true` pulsa también botones que cambian
///   preferencias; sin eso se niega y los reporta.
/// - `eval`: ejecuta un fragmento Lua con `pcall` y devuelve su valor como
///   JSON. Es la vía a las 569 funciones de la API que el bridge no envuelve,
///   y también la forma de que un error llegue como texto en vez de congelar
///   el DAW. **No es un sandbox**: el código se ejecuta con permisos plenos
///   dentro de Reaper.
/// - `log`: las ultimas lineas de la bitácora de la tool.
/// - `exchange`: que hay ahora mismo en `command.json` / `response.json`, la
///   edad del heartbeat, y si hay un modal.
/// - `install`: (re)instala el guardián y dice si cambió.
#[tool(description = "Diagnose why the DAW stopped answering, and safely run Lua inside it. Start with op=status when anything feels stuck. op=modal lists the dialog windows that are freezing Reaper and shows their text; op=unblock closes them (it refuses to press buttons that change your preferences unless force=true); op=eval runs a Lua snippet wrapped in pcall so an error comes back as text instead of a modal that freezes the DAW, and returns its value as JSON - that is also how to reach the 569 Reaper API functions the bridge does not wrap; op=log tails this tool's own log; op=exchange shows the current command.json/response.json and the heartbeat age.")]
pub async fn daw_debug(
    op: String,
    code: Option<String>,
    force: Option<bool>,
    limit: Option<usize>,
) -> std::result::Result<Value, ToolError> {
    let n = limit.unwrap_or(40).clamp(1, 500);
    let forzar = force.unwrap_or(false);

    match op.as_str() {
        "status" | "estado" => {
            let bridge = heretic_daw::ReaperBridge::with_defaults();
            let modales = modal_dump();
            let vivo = bridge.is_alive();
            let detalle = if vivo {
                match bridge.call("transport_get_state", json!({})) {
                    Ok(v) => json!({ "ok": true, "estado": v }),
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                }
            } else {
                json!({ "ok": false, "error": "sin heartbeat: el ReaScript bridge no esta corriendo" })
            };
            let cfg = heretic_daw::file_rpc::RpcConfig::default_config();
            let out = json!({
                "reaper_corriendo": win::pid_reaper().is_some(),
                "bridge_heartbeat": heretic_daw::FileRpc::new(cfg).heartbeat_age().map(|d| d.as_millis() as u64),
                "bridge": detalle,
                "dialogos_bloqueando": modales,
                "diagnostico": diagnostico(vivo, modales.is_some()),
                "si_hay_dialogo": {
                    "leer": "daw_debug op=modal",
                    "cerrar": "daw_debug op=unblock",
                },
            });
            anotar("info", "op=status");
            Ok(out)
        }

        "modal" | "modales" => match modal_dump() {
            Some(ds) => {
                anotar("warn", &format!("op=modal: {} dialogo(s)", ds.len()));
                Ok(json!({
                    "bloqueando": ds.len(),
                    "dialogos": ds,
                    "cierre": "daw_debug op=unblock",
                }))
            }
            None => Ok(json!({
                "bloqueando": 0,
                "dialogos": [],
                "nota": "Reaper no tiene ningun dialogo modal abierto. Si el bridge no \
                         contesta, el problema es otro: mira op=exchange (heartbeat) y \
                         la consola de Reaper.",
            })),
        },

        "unblock" | "desbloquear" => {
            let r = desbloquear(forzar).map_err(|e| {
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
            match evaluar(&code).await {
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
                "dialogos_bloqueando": modal_dump(),
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
fn diagnostico(vivo: bool, hay_modal: bool) -> String {
    match (hay_modal, vivo) {
        (true, _) => "Reaper esta CONGELADO por un dialogo modal. El bridge no contesta \
                      por eso, no porque este muerto. daw_debug op=unblock lo cierra."
            .into(),
        (false, true) => "El bridge responde. Si algo falla, el problema no es la \
                          comunicacion.".into(),
        (false, false) => "No hay dialogos, pero tampoco heartbeat. El ReaScript bridge no \
                           esta corriendo dentro de Reaper: Actions > Show action list > \
                           ReaScript, elige reaper_mcp_server.lua y dale a Run."
            .into(),
    }
}

/// Resumen de una línea de un valor JSON cualquiera, para la bitácora.
fn primer_plano(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 200 {
        format!("{}…", s.chars().take(200).collect::<String>())
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

    #[test]
    fn el_diagnostico_distingue_modal_de_bridge_muerto() {
        // `diagnostico(vivo, hay_modal)`. Los dos fallos producen el MISMO
        // sintoma ("el bridge no respondio en 10 s"); confundirlos cuesta un
        // ciclo entero de prueba y error.
        let con_modal = diagnostico(true, true);
        assert!(con_modal.contains("CONGELADO"), "{con_modal}");
        assert!(con_modal.contains("unblock"), "debe decir como arreglarlo: {con_modal}");

        assert!(diagnostico(true, false).contains("responde"));
        assert!(diagnostico(false, false).contains("heartbeat"));
    }

    #[test]
    fn un_modal_manda_sobre_el_heartbeat_ausente() {
        // Matiz que se aprende sufriendolo: cuando Reaper esta congelado por un
        // dialogo, TAMPOCO escribe el heartbeat. Asi que "no hay heartbeat" no
        // descarta "hay un modal", y al reves. Por eso `status` mira las dos
        // cosas y no se fia del heartbeat para culpar al bridge.
        let congelado = diagnostico(false, true);
        assert!(
            congelado.contains("CONGELADO"),
            "con un modal abierto el veredicto debe ser el modal, aunque no \
             haya heartbeat. Got: {congelado}"
        );
    }

    #[test]
    fn la_bitacora_no_revienta_si_el_disco_da_problemas() {
        // anotar() nunca debe hacer fallar la tool.
        anotar("test", "escribiendo en la bitacora");
        let p = log_path();
        assert!(p.ends_with("daw_debug.log"), "{p:?}");
    }
}
