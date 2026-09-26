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
    b.call(action, params).map_err(|e| {
        // El error mas comun es "el bridge no responde", y la causa quase
        // siempre es la misma: el ReaScript no esta corriendo dentro de Reaper.
        // Decirlo ahorra al LLM un ciclo entero de prueba y error.
        let m = e.to_string();
        let hint = if m.contains("no respondio") || m.contains("Timeout") {
            " El bridge Lua no responde. Comprueba con daw_health: normalmente \
             significa que el ReaScript no esta corriendo dentro de Reaper \
             (Actions > Show action list > ReaScript > cargarlo y darle a Run)."
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

    match call("ping", json!({})) {
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
    match call("transport", json!({})) {
        Ok(v) => out["transport"] = json!({ "ok": true, "estado": v }),
        Err(e) => out["transport"] = json!(e.to_string()),
    }
    Ok(out)
}

// ============================================================================
// 2. daw_catalog
// ============================================================================

#[tool(description = "List the DAW actions available, grouped by domain, with the parameters each one takes. Call this before daw_do when you are not sure of an exact action name or its params. Passing a domain returns only that group (project, track, fx, midi, item, marker, envelope, send, compose, selection, transport, tempo, script). This is how you discover the long tail: daw_do can call anything that shows up here.")]
pub async fn daw_catalog(
    domain: Option<String>,
) -> std::result::Result<Value, ToolError> {
    let all = heretic_daw::ACTIONS;
    let grouped: Map<String, Value> = all
        .iter()
        .fold(Map::new(), |mut acc, a| {
            let d = a.split('.').next().unwrap_or("otros");
            let entry = acc.entry(d.to_string()).or_insert_with(|| json!([]));
            if let Some(arr) = entry.as_array_mut() {
                arr.push(json!(a));
            }
            acc
        });

    match domain.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => {
            let key = d.to_ascii_lowercase();
            match grouped.get(&key) {
                Some(list) => Ok(json!({ "domain": key, "actions": list })),
                None => Ok(json!({
                    "error": format!("dominio desconocido: {key}"),
                    "dominios_disponibles": grouped.keys().collect::<Vec<_>>(),
                })),
            }
        }
        _ => Ok(json!({
            "total": all.len(),
            "por_dominio": grouped.keys().collect::<Vec<_>>(),
            "acciones": grouped,
            "nota": "Para la lista completa con descripciones, mira el catalogo \
                     del bridge Lua o llama a daw_do con la accion directa.",
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

// ============================================================================
// 4. daw_project
// ============================================================================

#[tool(description = "Manage the DAW project: op=info (name, tempo, sample rate, dirty state), op=new (create an empty project, optionally from a template file), op=save (save in place), op=saveAs (save to a new path), op=open (load a .rpp file), op=render (render the project or a time range to a WAV file), op=paths (project and resource directories). Unlike FL Studio, REAPER has a real project API, so all of this works without touching the UI.")]
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
    let mut method = "project".to_string();

    match op.as_str() {
        "info" => method = "project.info".into(),
        "new" => {
            method = "project.new".into();
            if let Some(n) = name {
                p.insert("name".into(), json!(n));
            }
            if let Some(t) = template {
                p.insert("template".into(), json!(t));
            }
        }
        "save" => method = "project.save".into(),
        "saveas" => {
            method = "project.saveAs".into();
            let path = path.ok_or_else(|| {
                ToolError::invalid_params("daw_project saveAs necesita 'path'")
            })?;
            p.insert("path".into(), json!(path));
        }
        "open" => {
            method = "project.open".into();
            let path = path.ok_or_else(|| {
                ToolError::invalid_params("daw_project open necesita 'path' (un .rpp)")
            })?;
            p.insert("path".into(), json!(path));
        }
        "render" => {
            method = "project.render".into();
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
        "paths" => method = "project.paths".into(),
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

#[tool(description = "Manage DAW tracks. op=list (all tracks with volume, pan, mute, solo, FX count), op=info (one track in detail), op=create (new track, optional name), op=delete, op=rename, op=volume (linear 0..1 or dB depending on the DAW, read with op=info to see the scale), op=pan, op=mute, op=solo, op=arm, op=color. Track indices are 0-based and count from the first track.")]
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
    call(&format!("track.{op}"), Value::Object(p))
}

// ============================================================================
// 6. daw_fx
// ============================================================================

#[tool(description = "Manage plugins on a track. op=search (find installed plugins by name, including the FL Studio VSTi if you added it), op=add (insert a plugin by name, returns its index), op=remove, op=list (FX chain of a track), op=paramInfo (names and ranges of a plugin's parameters, with values normalized 0..1), op=getParam / op=setParam (value is ALWAYS normalized 0..1 regardless of the plugin's own range), op=preset (list or load presets), op=toggle (bypass). This is the path to FLEX: load 'FL Studio VSTi (Multi)' on an instrument track and Reaper can play FL's own instruments.")]
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
    call(&format!("fx.{op}"), Value::Object(p))
}

// ============================================================================
// 7. daw_midi
// ============================================================================

#[tool(description = "Write and read MIDI. op=createItem (empty MIDI item on a track; position in beats, optional), op=addNote (single note: pitch, start, length, velocity), op=addNotes (batch, for chords and patterns), op=getNotes (read back the notes in an item), op=clear, op=read (parse notes from any item). Positions are in BEATS, not seconds: with a 4/4 bar at 120bpm, beat 0 is bar 1 and beat 4 is bar 2.")]
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
    call(&format!("midi.{op}"), Value::Object(p))
}
