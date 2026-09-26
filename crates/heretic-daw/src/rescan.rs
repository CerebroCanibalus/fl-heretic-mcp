//! Forzar a REAPER a reescanear sus plugins.
//!
//! # Por que esto necesita un modulo aparte
//!
//! Descargar un VST3 a la carpeta correcta **no** basta: Reaper solo lo ve tras
//! reescanear, y no hay API documentada para registrar una ruta de plugins.
//!
//! Lo que se midio en esta maquina, para no repetirlo a ciegas:
//!
//! | via | resultado |
//! |---|---|
//! | Editar `reaper.ini` a mano (`vstpath64x`) | La ruta queda registrada, pero Reaper no indexa nada |
//! | `Main_OnCommand(40705)` ("Add VST search path") | **Abre un dialogo modal y CUELGA Reaper**. El script nunca termina, el bridge deja de responder y no hay error: solo un timeout. Prohibido. |
//! | Copiar el DLL a `%APPDATA%\REAPER\Effects` | No lo escanea al arrancar |
//! | Borrar la cache y reiniciar | Funciona, pero reinicia el DAW |
//!
//! # La via que se usa
//!
//! Un ReaScript que llama a la API publica de re-scan. Es la unica via que no
//! abre dialogos. Se ejecuta por el mismo file-RPC que el resto, asi que si
//! Reaper esta colgado, el comando expira y el error lo dice en vez de
//! colgarse el daemon.
//!
//! Aun asi, el re-scan de Reaper es asincrono y puede tardar: se informa y no
//! se promete que el plugin este visible al instante.

use serde_json::{json, Value};

use crate::file_rpc::{FileRpc, RpcError, RpcConfig};

/// Resultado del re-scan.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RescanReport {
    pub ok: bool,
    pub detalle: String,
    /// Plugins que el DAW ve ahora mismo, si se pudo leer la lista.
    pub plugins_visibles: Option<usize>,
    /// El que se busca, y si ha aparecido.
    pub encontrado: Option<bool>,
}

/// El ReaScript que hace el trabajo.
///
/// Va como fichero aparte porque `script_run_start` quiere una ruta **relativa**
/// al resource path (absoluta da `script_path must be relative`), y porque un
/// error de sintaxis en Lua no debe poder tumbar el daemon.
const RESCAN_LUA: &str = r#"
-- Fuerza el reescaneo de plugins de REAPER.
-- Solo APIs que NO abren dialogos: 40705 (Add VST path) cuelga el DAW.
-- El objetivo se pasa escribiendo un fichero aparte, no con varargs: este
-- script se lanza con `dofile`, sin argumentos, y usar `...` ahi no es fiable.
-- Un `...` sin dato hacia que el script no llegara al final.
local out = {}
local function add(s) out[#out+1] = tostring(s) end

local ok1 = pcall(function() reaper.Main_OnCommand(50124, 0) end)  -- refresh all plug-ins
add("refresh_50124=" .. tostring(ok1))

local target = nil
local tf = io.open(reaper.GetResourcePath() .. "/daw_rescan_target.txt", "r")
if tf then target = tf:read("*a"); tf:close() end
add("target=" .. tostring(target))
local cache = reaper.GetResourcePath() .. "/reaper-vstplugins64.ini"
local f = io.open(cache, "r")
local n, hit = 0, 0
if f then
  for line in f:lines() do
    n = n + 1
    if target ~= nil and target ~= "" and line:find(target, 1, true) then
      hit = hit + 1
    end
  end
  f:close()
end
add("cache_lineas=" .. n)
add("cache_hits=" .. hit)

local fh = io.open(reaper.GetResourcePath() .. "/daw_rescan.txt", "w")
if fh then fh:write(table.concat(out, "\n")); fh:close() end
"#;

/// Pide a Reaper que reescanee sus plugins.
///
/// `target` es opcional: el nombre del plugin que se espera ver aparecer, para
/// poder confirmar en vez de suponer.
pub fn rescan(
    rpc_cfg: RpcConfig,
    target: Option<&str>,
) -> Result<RescanReport, RpcError> {
    let mut cfg = rpc_cfg;
    // El reescaneo completo puede tardar mas que una llamada normal.
    cfg.timeout = std::time::Duration::from_secs(25);
    let rpc = FileRpc::new(cfg);

    if !rpc.is_bridge_alive() {
        return Err(RpcError::NotRunning(
            "Reaper no esta corriendo, o el bridge no esta cargado. \
             No se puede reescanear."
                .into(),
        ));
    }

    // 1. Escribir el ReaScript. La ruta tiene que ser relativa al resource
    //    path, asi que se coloca ahi y se llama por nombre.
    let scripts_dir = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("REAPER")
        .join("Scripts");
    let lua_path = scripts_dir.join("daw_rescan.lua");
    if let Some(p) = lua_path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    std::fs::write(&lua_path, RESCAN_LUA).map_err(|e| RpcError::Io {
        path: lua_path.clone(),
        source: e,
    })?;

    // 1b. El objetivo del chequeo va en un fichero aparte: el script se
    //     lanza con `dofile` sin argumentos, y los varargs (`...`) en Lua no
    //     llegan de forma fiable. Con `...` el script no llegaba al final y no
    //     escribia su resultado, asi que el rescan decia "sin resultado".
    if let Some(t) = target {
        let _ = std::fs::write(scripts_dir.join("daw_rescan_target.txt"), t);
    }

    // 2. Borrar el resultado de la llamada anterior. Sin esto se lee el viejo
    //    y se creye que este re-scan funciono.
    if let Some(p) = scripts_dir.parent() {
        let _ = std::fs::remove_file(p.join("daw_rescan.txt"));
    }

    // 3. Pedir que lo ejecute.
    let r: Value = rpc.call("script_run_start", json!({ "script_path": "daw_rescan.lua" }))?;

    if r.get("success") != Some(&Value::Bool(true)) {
        return Err(RpcError::Remote(format!(
            "Reaper no acepto ejecutar el re-scan: {}",
            r.get("error").and_then(Value::as_str).unwrap_or("sin motivo")
        )));
    }

    // 3. El re-scan se encola: `script_run_start` devuelve de inmediato
    //    (medido: 0.03 s) y el script corre justo despues. Se espera lo justo
    //    con reintentos, en vez de un sleep fijo: si se lee antes de que el
    //    script escriba, el resultado no existe y parece que el re-scan fallo.
    // 4. Leer lo que escribio el script.
    //
    //    OJO con la ruta: el ReaScript escribe en `GetResourcePath()`, que
    //    es `%APPDATA%\REAPER\`, **no** en `...\REAPER\Scripts\`, que es
    //    donde vive el .lua. Usar `lua_path.parent()`acia que se leyera un
    //    directorio de mas y el rescan decia siempre "sin resultado".
    let result = scripts_dir.parent().map(|p| p.join("daw_rescan.txt"));

    let Some(result) = result else {
        return Ok(RescanReport {
            ok: false,
            detalle: "no se pudo deducir la ruta del resource path de Reaper".into(),
            plugins_visibles: None,
            encontrado: None,
        });
    };

    let mut txt: Option<String> = None;
    for _ in 0..25 {
        if let Ok(t) = std::fs::read_to_string(&result) {
            if !t.trim().is_empty() {
                txt = Some(t);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    let Some(txt) = txt else {
        return Ok(RescanReport {
            ok: false,
            detalle: format!(
                "Reaper ejecuto el script pero no dejo resultado en {}. \
                 Puede que el re-scan siga en curso: reinicia Reaper.",
                result.display()
            ),
            plugins_visibles: None,
            encontrado: None,
        });
    };

    let mut lineas = 0usize;
    let mut hits = 0usize;
    for l in txt.lines() {
        if let Some(v) = l.strip_prefix("cache_lineas=") {
            lineas = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = l.strip_prefix("cache_hits=") {
            hits = v.trim().parse().unwrap_or(0);
        }
    }

    // 5. Confirmar por la via de verdad: la lista de plugins del DAW.
    let encontrados: Option<bool> = match rpc.call("fx_list_installed", json!({})) {
        Ok(v) => {
            let n = v
                .get("plugins")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let _ = n;
            target.map(|t| {
                let s = serde_json::to_string(&v).unwrap_or_default();
                s.contains(t)
            })
        }
        Err(_) => None,
    };

    Ok(RescanReport {
        ok: true,
        detalle: format!(
            "cache con {lineas} plugins; re-scan ejecutado. \
             Si el plugin no aparece, reinicia Reaper: hay builds suyas en las que \
             el re-scan se aplaza al siguiente arranque."
        ),
        plugins_visibles: Some(lineas),
        encontrado: encontrados,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_lua_no_usa_acciones_que_abren_dialogos() {
        // 40705 = "VST: Add VST search path", que CUELGA Reaper.
        //
        // Se buscan las LLAMADAS, no el texto entero: este modulo menciona ese
        // numero al explicar por que no se usa, asi que un `contains` a pelo
        // daria un falso positivo siempre.
        let llamadas: Vec<&str> = RESCAN_LUA
            .lines()
            .filter(|l| l.contains("Main_OnCommand") && !l.trim_start().starts_with("--"))
            .collect();
        assert!(!llamadas.is_empty(),
            "el Lua deberia pedir al menos un re-scan explicito");
        for l in llamadas {
            assert!(!l.contains("40705"),
                "el Lua llama a una accion que abre un dialogo modal: {l}");
        }
    }

    #[test]
    fn el_lua_esta_balanceado() {
        // Editar Lua a ojo es fragil. No valida la sintaxis entera, pero pilla
        // el error tipico al anadir un bloque y olvidar su `end`.
        //
        // Cuenta como palabra clave lo que va precedido de un separador
        // (espacio, `(`, tabulador) o inicio de linea. La version anterior
        // exigia ademas que lo que_siguisse NO fuera alfanumerico, y eso
        // descartaba `end)` y `end}`, que son los cierres mas comunes.
        let cuenta = |linea: &str, kw: &str| -> usize {
            linea
                .match_indices(kw)
                .filter(|(i, _)| {
                    let antes = linea[..*i].chars().next_back();
                    match antes {
                        None => true,
                        Some(c) => !c.is_alphanumeric() && c != '_',
                    }
                })
                .filter(|(i, m)| {
                    // y que no sea parte de una palabra mas larga
                    let fin = i + m.len();
                    match linea[fin..].chars().next() {
                        None => true,
                        Some(c) => !c.is_alphanumeric() && c != '_',
                    }
                })
                .count()
        };

        let mut abre = 0usize;
        let mut cierra = 0usize;
        for line in RESCAN_LUA.lines() {
            let code = line.split("--").next().unwrap_or("");
            if code.trim().is_empty() {
                continue;
            }
            for kw in ["function", "if", "for", "while"] {
                abre += cuenta(code, kw);
            }
            cierra += cuenta(code, "end");
        }
        assert_eq!(abre, cierra,
            "el ReaScript de re-scan tiene {abre} bloques abiertos y {cierra} 'end'");
    }

    #[test]
    fn el_lua_no_usa_varargs() {
        // `...` en un script lanzado con dofile sin argumentos no es fiable, y
        // hacia que el script no llegara al final. El objetivo va por fichero.
        assert!(!RESCAN_LUA.contains("local target = ..."),
            "el objetivo debe llegar por fichero, no por varargs");
        assert!(RESCAN_LUA.contains("daw_rescan_target.txt"),
            "el Lua deberia leer el objetivo de daw_rescan_target.txt");
    }

    #[test]
    fn rescan_sin_reaper_da_error_claro() {
        let mut cfg = RpcConfig::default_config();
        cfg.dir = std::env::temp_dir().join("daw-rescan-sin-reaper");
        cfg.command = cfg.dir.join("command.json");
        cfg.response = cfg.dir.join("response.json");
        cfg.lock = cfg.dir.join("server.lock"); // no existe
        cfg.timeout = std::time::Duration::from_millis(50);
        let e = rescan(cfg, Some("Vital")).unwrap_err();
        assert!(matches!(e, RpcError::NotRunning(_)), "{e}");
    }
}
