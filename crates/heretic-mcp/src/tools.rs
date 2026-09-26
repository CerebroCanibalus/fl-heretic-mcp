//! Surface de tools MCP: **7**, no 181.
//!
//! # Por que tan pocas
//!
//! Se midio el MCP de referencia (`reference/xDarkzx`, 181 tools) con su AST:
//!
//! | | |
//! |---|---|
//! | descripciones | 74.157 chars (~18.500 tokens) |
//! | firmas + parametros | 15.334 chars (~3.800 tokens) |
//! | **total en schemas** | **~22.400 tokens** |
//!
//! Eso entra en **cada request, para siempre**, antes de hablar de musica. Y
//! las descripciones estan infladas: `setup_fx_chain` tiene 3.740 chars de
//! descripcion para una funcion de **un** parametro (935 tokens). Es
//! documentacion de blog metida en un schema.
//!
//! Ese MCP tiene perfiles (`full` 180 ... `minimal` 43) porque el autor sabe
//! que el problema existe, pero el default sigue siendo 180 y cambiar de
//! perfil exige reiniciar el servidor. Es parchear el sintoma.
//!
//! Aqui la inversely: **7 por defecto, y se expande si se pide.** Un agente
//! haciendo una sesion normal ve 7 tools. Uno que quiere algo raro llama a
//! `daw_catalog` y ahi ve las 181 acciones, con sus params.
//!
//! # Por que hay tools tipadas Y un escape hatch
//!
//! El bug mas caro de la etapa de FL no fue del DAW: fue nuestro diseño.
//! Con 17 wrappers finos, cada uno reconstruia los params a su manera, y el
//! LLM adivino mal el nombre de uno (`ms` cuando el daemon leia `position`).
//! Peor: Reaper **no da error** si el param no existe, se queda con el valor
//! por defecto y parece que funciono. El peor modo de fallo posible.
//!
//! Las tools tipadas (`daw_track`, `daw_fx`, ...) resuelven eso en el 90% del
//! uso: el schema dice el nombre exacto del parametro. `daw_do` deja la larga
//! cola abierta sin que el error sea silencioso ahi.

use flojo_mcp::prelude::*;
use serde_json::{json, Map, Value};

use heretic_daw::{ReaperBridge, ReaperConfig};

/// Conexion al DAW.
///
/// Se construye una vez por llamada. Es barato (abrir un `FileRpc` es solo
/// guardar rutas) y evita estado global, que es lo que hace imposible testear
/// en paralelo. La serializacion va dentro de `heretic-daw`.
fn bridge() -> std::result::Result<ReaperBridge, ToolError> {
    let mut cfg = ReaperConfig::default();
    if let Ok(p) = std::env::var("DAW_BRIDGE_DIR") {
        cfg.rpc.dir = std::path::PathBuf::from(p);
        cfg.rpc.command = cfg.rpc.dir.join("command.json");
        cfg.rpc.response = cfg.rpc.dir.join("response.json");
    }
    Ok(ReaperBridge::new(cfg))
}

fn call(action: &str, params: Value) -> std::result::Result<Value, ToolError> {
    let b = bridge()?;
    let primer = b.call(action, params.clone());

    // Un reintento, solo si el DAW no contestar y habia un dialogo conocido
    // tapandole. El aviso de evaluacion de Reaper sale en cada arranque y es
    // MODAL: sin esto, la primera tool tras arrancar se come un timeout, y el
    // agente no tiene forma de mirar una pantalla para enterarse.
    //
    // Un reintento y nada mas: si el dialogo no estaba, sigue fallando igual
    // y el error que sube es el de verdad, no uno diluido.
    // El reintento se hace en el mismo tipo de error que devuelve el bridge;
    // el mapeo a `ToolError` con la pista viene despues, una sola vez.
    let r: Result<Value, heretic_daw::RpcError> = match primer {
        Ok(v) => Ok(v),
        Err(e) if crate::debug::es_timeout(&e.to_string()) => {
            match crate::debug::cerrar_molestia(true) {
                Some(m) => {
                    // Un instante para que Windows retire el dialogo.
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    b.call(action, params).map_err(|e2| {
                        heretic_daw::RpcError::Remote(format!(
                            "Reaper estaba tapado por un dialogo ({m}); se cerro y aun asi \
                             no responde: {e2}"
                        ))
                    })
                }
                None => Err(e),
            }
        }
        Err(e) => Err(e),
    };

    r.map_err(|e| {
        // El error mas comun es "el bridge no responde", y la causa casi
        // siempre es la misma: el ReaScript no esta corriendo dentro de Reaper.
        // Decirlo ahorra al LLM un ciclo entero de prueba y error.
        let m = e.to_string();
        let hint = if crate::debug::es_timeout(&m) {
            " El bridge Lua no responde. Comprueba con daw_health: normalmente \
             significa que el ReaScript no esta corriendo dentro de Reaper \
             (Actions > Show action list > ReaScript > cargarlo y darle a Run). \
             Si daw_health dice que hay un dialogo bloqueando, daw_debug op=modal."
        } else {
            ""
        };
        ToolError::internal(format!("{m}{hint}"))
    })
}

// ============================================================================
// 1. daw_health
// ============================================================================

#[tool(description = "Check the DAW is usable before anything else. Reports whether Reaper is running, whether the bridge ReaScript inside it is answering, how many DAW actions are available, and what to do if not. Start here when a call fails; it is much cheaper than guessing.")]
pub async fn daw_health() -> std::result::Result<Value, ToolError> {
    let mut out = json!({ "actions_available": heretic_daw::ACTIONS.len() });

    let b = bridge()?;
    if !b.is_alive() {
        out["bridge"] = json!("sin heartbeat");
        out["que_hacer"] = json!(
            "No existe el heartbeat del bridge. En Reaper: Actions > Show action \
             list > ReaScript > cargarlo y darle a Run. El bridge debe estar \
             corriendo DENTRO de Reaper."
        );
        return Ok(out);
    }
    match call("transport_get_state", json!({})) {
        Ok(v) => {
            out["bridge"] = json!("ok");
            out["bridge_info"] = v;
        }
        Err(e) => {
            out["bridge"] = json!("sin respuesta");
            out["problema"] = json!(e.to_string());
            out["que_hacer"] = json!(
                "El ReaScript bridge no responde. En Reaper: Actions > Show action \
                 list > ReaScript, elige el bridge y dale a Run. Debe imprimir \
                 '[bridge] listo' en la consola."
            );
            return Ok(out);
        }
    }

    // El ping puede pasar y la escritura fallar si el bridge va con retraso.
    // Comprobar de mas es barato y evita el falso "todo bien".
    // `transport_get_state`, NO `transport`: el bridge no tiene una action
    // llamada `transport`, y da "Unknown command". Los nombres reales estan
    // en heretic_daw::ACTIONS, generados del bridge.
    match call("transport_get_state", json!({})) {
        Ok(v) => out["transport"] = json!({ "ok": true, "estado": v }),
        Err(e) => out["transport"] = json!(e.to_string()),
    }
    Ok(out)
}

// ============================================================================
// 2. daw_catalog
// ============================================================================

#[tool(description = "Discover what the DAW can do. Returns every action grouped by domain, and for each one the parameters it reads and which of them are required. Call this before daw_do whenever you are not sure of an action name or of its parameter names, which is the mistake that fails silently: Reaper does NOT error on an unknown parameter, it uses the default and reports success. Pass a domain to get just that group (track, project, fx, midi, item, marker, envelope, send, selection, transport, tempo, compose, script, ...). Everything listed here is reachable with daw_do.")]
pub async fn daw_catalog(
    domain: Option<String>,
) -> std::result::Result<Value, ToolError> {
    // Cada accion lleva sus params y sus obligatorios, leidos del codigo del
    // bridge (ver tools/gen_actions.py). Sin esto el LLM tiene que adivinar
    // los nombres, y adivinar es exactamente el fallo que no avisa.
    let by_group: Map<String, Value> = heretic_daw::ACTION_GROUPS
        .iter()
        .map(|(group, names)| {
            let list: Vec<Value> = names
                .iter()
                .map(|name| match heretic_daw::actions::doc_of(name) {
                    Some(d) => json!({
                        "action": d.name,
                        "module": d.module,
                        "params": d.params,
                        "required": d.required,
                    }),
                    None => json!({ "action": name }),
                })
                .collect();
            (group.to_string(), Value::Array(list))
        })
        .collect();

    match domain.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => {
            let key = d.to_ascii_lowercase();
            match by_group.get(&key) {
                Some(list) => Ok(json!({
                    "domain": key,
                    "count": list.as_array().map(Vec::len).unwrap_or(0),
                    "actions": list,
                })),
                None => Ok(json!({
                    "error": format!("dominio desconocido: {key}"),
                    "dominios": by_group.keys().collect::<Vec<_>>(),
                })),
            }
        }
        _ => Ok(json!({
            "total": heretic_daw::ACTIONS.len(),
            "grupos": by_group.keys().collect::<Vec<_>>(),
            "acciones": by_group,
        })),
    }
}

// ============================================================================
// 3. daw_do (escape hatch)
// ============================================================================

#[tool(description = "Call any DAW action by name. This reaches all 181 actions, including the long tail that has no dedicated tool. Use daw_catalog first if you are unsure of the name. IMPORTANT: Reaper does NOT error on an unknown parameter, it silently uses the default and reports success, so parameter names must match exactly (camelCase, e.g. trackIndex / fxIndex / paramIndex, not track_index). The typed tools (daw_track, daw_fx, daw_midi, daw_project) exist precisely so that this mistake is hard to make on the common path.")]
pub async fn daw_do(
    action: String,
    params: Option<Map<String, Value>>,
) -> std::result::Result<Value, ToolError> {
    call(&action, params.map(Value::Object).unwrap_or(json!({})))
}


/// Resuelve `dominio` + `accion` al nombre real del bridge.
///
/// No hay tabla: los 162 nombres del catalogo siguen el prefijo sin excepcion
/// (ver test `todo_grupo_cumple_el_prefijo` en heretic-daw). Antes habia una
/// tabla escrita a mano con 40 entradas, y `daw_track op=create` producia
/// `track.create` -> `Unknown command: track.create`.
///
/// Que la regla se compruebe con un test y no de palabra: 40 entradas
/// escritas a mano son 40 sitios donde equivocarse, y equivocarse aqui no da
/// error de compilacion, da un fallo en runtime contra el DAW.
pub fn resolver_accion(dominio: &str, accion: &str) -> Result<String, ToolError> {
    let nombre = format!("{dominio}_{accion}");
    if heretic_daw::actions::doc_of(&nombre).is_some() {
        return Ok(nombre);
    }
    // Busca lo mas parecido: un nombre mal escrito da "Unknown command" sin
    // decir cual era el bueno.
    let prefijo = format!("{dominio}_");
    let cerca: Vec<&str> = heretic_daw::ACTIONS
        .iter()
        .copied()
        .filter(|a| a.starts_with(&prefijo))
        .filter(|a| accion.len() > 2 && comun(a, accion) >= 3)
        .take(6)
        .collect();
    let total = heretic_daw::ACTIONS.iter().filter(|a| a.starts_with(&prefijo)).count();
    Err(ToolError::invalid_params(format!(
        "'{nombre}' no existe en el catalogo del bridge.{}",
        if cerca.is_empty() {
            format!(" Hay {total} acciones '{prefijo}*'; mira daw_catalog(domain=\"{dominio}\").")
        } else {
            format!(" Parecidos: {cerca:?}. Mira daw_catalog(domain=\"{dominio}\").")
        }
    )))
}

fn comun(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}
// ============================================================================
// 4. daw_project
// ============================================================================

#[tool(description = "Manage the DAW project: op=info (name, tempo, sample rate, dirty state), op=new (create an empty project, optionally from a template file), op=save (save in place), op=saveAs (save to a new path), op=open (load a .rpp file), op=render (render the project or a time range to a WAV file), op=metadata (project notes and metadata). Unlike FL Studio, REAPER has a real project API, so all of this works without touching the UI.")]
pub async fn daw_project(
    op: String,
    path: Option<String>,
    name: Option<String>,
    template: Option<String>,
    start: Option<f64>,
    end: Option<f64>,
) -> std::result::Result<Value, ToolError> {
    let op = op.trim().to_ascii_lowercase();
    let mut p = Map::new();
    let method: &str;

    match op.as_str() {
        "info" => method = "project_get_info".into(),
        "new" => {
            method = "project_new".into();
            if let Some(n) = name {
                p.insert("name".into(), json!(n));
            }
            if let Some(t) = template {
                p.insert("template".into(), json!(t));
            }
        }
        "save" => method = "project_save".into(),
        "saveas" => {
            method = "project_save_as".into();
            let path = path.ok_or_else(|| {
                ToolError::invalid_params("daw_project saveAs necesita 'path'")
            })?;
            p.insert("path".into(), json!(path));
        }
        "open" => {
            method = "project_open".into();
            let path = path.ok_or_else(|| {
                ToolError::invalid_params("daw_project open necesita 'path' (un .rpp)")
            })?;
            p.insert("path".into(), json!(path));
        }
        "render" => {
            method = "project_export_audio".into();
            if let Some(s) = start {
                p.insert("start".into(), json!(s));
            }
            if let Some(e) = end {
                p.insert("end".into(), json!(e));
            }
            if let Some(n) = name {
                p.insert("fileName".into(), json!(n));
            }
        }
        "paths" => method = "project_get_paths".into(),
        other => {
            return Err(ToolError::invalid_params(format!(
                "op desconocido: {other}. Validas: info, new, save, saveAs, open, render, paths"
            )))
        }
    }
    call(&method, Value::Object(p))
}

// ============================================================================
// 5. daw_track
// ============================================================================

#[tool(description = "Manage DAW tracks. op is the SUFFIX of the real action, so the tool composes the full name: op=get_all, op=info, op=create, op=rename, op=set_volume, op=set_pan, op=set_mute, op=set_solo, op=set_record_arm, op=set_color, op=delete_batch, op=select. Call daw_catalog(domain=\"track\") for the full list. Track indices are 0-based from the first track; volume is in dB (that is what Reaper uses).")]
pub async fn daw_track(
    op: String,
    track: Option<i32>,
    name: Option<String>,
    value: Option<f64>,
) -> std::result::Result<Value, ToolError> {
    let op = op.trim().to_ascii_lowercase();
    let mut p = Map::new();
    if let Some(t) = track {
        p.insert("trackIndex".into(), json!(t));
    }
    if let Some(n) = name {
        p.insert("name".into(), json!(n));
    }
    if let Some(v) = value {
        p.insert("value".into(), json!(v));
    }
    let nombre = resolver_accion("track", &op)?;
    call(&nombre, Value::Object(p))
}

// ============================================================================
// 6. daw_fx
// ============================================================================

#[tool(description = "Manage plugins on a track. op is the SUFFIX of the real action: op=list_installed (every plugin the DAW sees), op=add, op=remove_batch, op=get_chain, op=get_params, op=set_param, op=get_preset, op=set_preset, op=enable, op=disable, op=show_ui, op=move, op=get_instrument. Call daw_catalog(domain=\"fx\") for the full list with each one's params. FX parameter values are ALWAYS normalized 0..1 whatever range the plugin itself advertises, so read get_params to learn a value before writing one. This is also the path to FL's own instruments: load the 'FL Studio VSTi' plugin on an instrument track.")]
pub async fn daw_fx(
    op: String,
    track: Option<i32>,
    fx: Option<i32>,
    param: Option<i32>,
    name: Option<String>,
    value: Option<f64>,
) -> std::result::Result<Value, ToolError> {
    let op = op.trim().to_ascii_lowercase();
    let mut p = Map::new();
    if let Some(t) = track {
        p.insert("trackIndex".into(), json!(t));
    }
    if let Some(f) = fx {
        p.insert("fxIndex".into(), json!(f));
    }
    if let Some(n) = param {
        p.insert("paramIndex".into(), json!(n));
    }
    if let Some(n) = name {
        p.insert("fxName".into(), json!(n));
    }
    if let Some(v) = value {
        // Normalizado a 0..1 SIEMPRE. Reaper usa su propia escala por
        // parametro, asi que mandar el valor "real" no funciona.
        p.insert("value".into(), json!(v.clamp(0.0, 1.0)));
    }
    let nombre = resolver_accion("fx", &op)?;
    call(&nombre, Value::Object(p))
}

// ============================================================================
// 7. daw_midi
// ============================================================================

#[tool(description = "Write and read MIDI. op is the SUFFIX of the real action: op=insert_note (one note: pitch, start, length, velocity), op=insert_notes_batch (batch, for chords and patterns), op=get_notes, op=delete_all_notes, op=count_events, op=get_note_names. Call daw_catalog(domain=\"midi\") for the full list. Positions are in BEATS, not seconds: with 4/4 at 120bpm, beat 0 is bar 1 and beat 4 is bar 2. Create the item first with daw_do (action=\"item_create_midi\").")]
pub async fn daw_midi(
    op: String,
    track: Option<i32>,
    item: Option<i32>,
    pitch: Option<i32>,
    start: Option<f64>,
    length: Option<f64>,
    velocity: Option<f64>,
    notes: Option<Value>,
) -> std::result::Result<Value, ToolError> {
    let op = op.trim().to_ascii_lowercase();
    let mut p = Map::new();
    if let Some(t) = track {
        p.insert("trackIndex".into(), json!(t));
    }
    if let Some(i) = item {
        p.insert("itemIndex".into(), json!(i));
    }
    if let Some(n) = pitch {
        p.insert("pitch".into(), json!(n));
    }
    if let Some(s) = start {
        p.insert("start".into(), json!(s));
    }
    if let Some(l) = length {
        p.insert("length".into(), json!(l));
    }
    if let Some(v) = velocity {
        p.insert("velocity".into(), json!(v.clamp(0.0, 1.0)));
    }
    if let Some(n) = notes {
        p.insert("notes".into(), n);
    }
    let nombre = resolver_accion("midi", &op)?;
    call(&nombre, Value::Object(p))
}

// ============================================================================
// 8. daw_setup
// ============================================================================

#[tool(description = "Manage the DAW's plugin catalog. op=list: the curated catalog of free/open VST3 and CLAP plugins, with what each one replaces in FL Studio and whether it can be installed automatically. op=installed: what is actually on disk right now. op=install: download and install one plugin by its short name (e.g. 'surge-xt'). op=provision: install a whole set at once ('base' is fully automatic, no clicking required). op=rescan: tell the DAW to re-scan its plugins. Plugins marked 'manual' need a download from their website with an account, so this tool reports them instead of pretending it can do it.")]
pub async fn daw_setup(
    op: String,
    name: Option<String>,
    set: Option<String>,
    force: Option<bool>,
) -> std::result::Result<Value, ToolError> {
    let op = op.trim().to_ascii_lowercase();
    let cat = heretic_daw::Catalog::bundled();

    match op.as_str() {
        "list" => {
            let filtro = name.map(|n| n.to_ascii_lowercase());
            let ps: Vec<Value> = cat
                .plugins
                .iter()
                .filter(|p| {
                    let mut ok = filtro.is_none();
                    if let Some(f) = &filtro {
                        ok = p.name.to_ascii_lowercase().contains(f)
                            || p.kind.to_ascii_lowercase().contains(f)
                            || p.replaces.iter().any(|r| r.to_ascii_lowercase().contains(f));
                    }
                    ok
                })
                .map(|p| {
                    json!({
                        "name": p.name,
                        "display": p.display,
                        "kind": p.kind,
                        "format": p.format,
                        "license": p.license,
                        "reemplaza_a": p.replaces,
                        "automatizable": p.is_automatable(),
                        "nota": p.note,
                    })
                })
                .collect();
            Ok(json!({
                "catalogo_version": cat.version,
                "total": ps.len(),
                "plugins": ps,
                "sets": cat.sets.iter().map(|(k, v)| json!({
                    "set": k,
                    "description": v.description,
                    "plugins": v.plugins,
                })).collect::<Vec<_>>(),
            }))
        }
        "installed" => {
            let dir = heretic_daw::vst3_dir();
            let hay = heretic_daw::installed_plugins(&dir);
            let automatizables: Vec<&str> = cat
                .plugins
                .iter()
                .filter(|p| p.is_automatable())
                .map(|p| p.name.as_str())
                .collect();
            Ok(json!({
                "carpeta": dir.display().to_string(),
                "existe": dir.exists(),
                "plugins_en_disco": hay,
                "total": hay.len(),
                "del_catalogo_faltan": automatizables.iter().filter(|n| {
                    let p = cat.get(n).unwrap();
                    let ext = if p.format == "clap" { "clap" } else { "vst3" };
                    !hay.iter().any(|d| d.to_lowercase()
                        .contains(&p.display.to_lowercase()) && d.to_lowercase().ends_with(ext))
                }).collect::<Vec<_>>(),
            }))
        }
        "install" => {
            let n = name.ok_or_else(|| {
                ToolError::invalid_params("daw_setup install necesita 'name' (ver daw_setup list)")
            })?;
            let r = heretic_daw::install(&cat, &n, force.unwrap_or(false))
                .map_err(|e| ToolError::internal(format!("{e}")))?;
            Ok(serde_json::to_value(r).unwrap_or(json!({})))
        }
        "provision" => {
            let s = set.or(name).unwrap_or_else(|| "base".to_string());
            let Some(def) = cat.set(&s) else {
                return Err(ToolError::invalid_params(format!(
                    "set desconocido: {s}. Disponibles: {:?}",
                    cat.sets.keys().collect::<Vec<_>>()
                )));
            };
            let mut reports = Vec::new();
            let mut fallos = Vec::new();
            for n in &def.plugins {
                match heretic_daw::install(&cat, n, force.unwrap_or(false)) {
                    Ok(r) => {
                        if r.status == "instalado" {
                            reports.push(serde_json::to_value(&r).unwrap_or(json!({})));
                        } else {
                            reports.push(serde_json::to_value(&r).unwrap_or(json!({})));
                            if r.status == "manual" {
                                fallos.push(r.display.clone());
                            }
                        }
                    }
                    Err(e) => {
                        fallos.push(format!("{n}: {e}"));
                        reports.push(json!({ "name": n, "status": "error", "message": e.to_string() }));
                    }
                }
            }
            Ok(json!({
                "set": s,
                "description": def.description,
                "resultados": reports,
                "requieren_descarga_manual": fallos,
                "siguiente_paso": "Ejecuta daw_setup rescan (o reinicia el DAW) para que los vea.",
            }))
        }
        "rescan" => {
            let cfg = heretic_daw::ReaperConfig::default();
            let target = name.as_deref();
            let r = heretic_daw::rescan(cfg.rpc, target).map_err(|e| {
                ToolError::internal(format!(
                    "{e}\n Si Reaper no muestra el plugin, reinicialo: hay builds en las \
                     que el re-escaneo se aplaza al siguiente arranque."
                ))
            })?;
            Ok(serde_json::to_value(r).unwrap_or(json!({})))
        }
        other => Err(ToolError::invalid_params(format!(
            "op desconocido: {other}. Validas: list, installed, install, provision, rescan"
        ))),
    }
}
