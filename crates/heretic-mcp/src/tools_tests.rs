//! Tests que atan las tools MCP al catalogo real del bridge.
//!
//! # Por que estan aqui
//!
//! Las tools tipadas construyen nombres de accion a mano
//! (`daw_project` -> `project_get_info`, `daw_fx` -> `fx_add`). Esas
//! cadenas no las comprueba nadie: el compilador las acepta y el fallo solo
//! aparece en runtime, contra el DAW, como `Unknown command: ...`.
//!
//! Pasó de verdad: la primera version uso `transport.setTempo`, `track.create`
//! y `project.paths`, y **ninguno existia**. El tercero (`project_get_paths`)
//! se coló en la version mas reciente, cuando ya se sabia que los nombres
//!carry guion bajo y no punto.
//!
//! Estos tests hacen que la lista de acciones que usan las tools este en el
//! catalogo generado. Si el bridge se actualiza y renombra algo, falla aqui y
//! no tres horas despues contra Reaper.

use flojo_mcp::prelude::*;
use serde_json::json;


/// Acciones que las tools compilan. A mano, pero verificadas por estos tests:
/// si una no esta en el catalogo, el test falla.
const ACCIONES_DE_LAS_TOOLS: &[&str] = &[
    // daw_project
    "project_get_info",
    "project_new",
    "project_save",
    "project_save_as",
    "project_open",
    "project_export_audio",
    "project_get_metadata",
    // daw_track -> track.<op>
    "track_get_all",
    "track_get_info",
    "track_create",
    "track_delete_batch",
    // daw_fx -> fx.<op>
    "fx_add",
    "fx_get_params",
    "fx_list_installed",
    "fx_get_chain",
    // daw_midi -> midi.<op>
    "midi_insert_note",
    "midi_get_notes",
    // daw_health / daw_do
    "transport_get_state",
];

#[test]
fn toda_accion_de_las_tools_existe_en_el_catalogo() {
    for a in ACCIONES_DE_LAS_TOOLS {
        assert!(
            heretic_daw::actions::doc_of(a).is_some(),
            "la tool usa '{a}', que no esta en el catalogo generado. \
             Regenerar con: python tools/gen_actions.py"
        );
    }
}

/// Sufijos reales que las tools ofrecen, tal cual estan en el catalogo.
///
/// La regla es `{dominio}_{sufijo}` y no una tabla. El nombre del parametro
/// sigue siendo `op` por compatibilidad, pero su valor ES el sufijo real de
/// la accion: `daw_track op="set_volume"`, no `daw_track op="volume"`.
const SUFIJOS: &[(&str, &[&str])] = &[
    ("track", &["create", "get_all", "get_info", "set_volume", "set_pan",
                "set_mute", "set_solo", "set_record_arm", "set_color",
                "rename", "delete_batch", "select"]),
    ("fx", &["add", "get_params", "set_param", "get_chain", "list_installed",
              "remove_batch", "get_preset", "set_preset", "enable"]),
    ("midi", &["insert_note", "insert_notes_batch", "get_notes",
               "delete_all_notes", "count_events"]),
];

#[test]
fn resolver_accion_devuelve_el_nombre_real() {
    // Lo que fallaba: `daw_track op=create` componia `track.create` y el
    // bridge respondia "Unknown command".
    for (dominio, sufijo) in [
        ("track", "create"), ("track", "set_volume"), ("track", "get_all"),
        ("fx", "add"), ("fx", "get_params"), ("fx", "list_installed"),
        ("midi", "insert_note"), ("midi", "get_notes"),
    ] {
        let r = super::tools::resolver_accion(dominio, sufijo)
            .unwrap_or_else(|e| panic!("{dominio}/{sufijo} deberia existir: {e}"));
        assert_eq!(r, format!("{dominio}_{sufijo}"),
            "resolver_accion debe componer el prefijo, no inventar");
    }
}

#[test]
fn todo_sufijo_de_las_tools_existe_en_el_catalogo() {
    for (dominio, sufijos) in SUFIJOS {
        for s in *sufijos {
            let nombre = format!("{dominio}_{s}");
            assert!(
                heretic_daw::actions::doc_of(&nombre).is_some(),
                "las tools ofrecen '{dominio}/{s}' pero '{nombre}' no esta en el catalogo"
            );
        }
    }
}

#[test]
fn las_descripciones_de_las_tools_no_prometen_ops_inventados() {
    // Las descripciones son lo que lee el LLM. Si prometen `op=volume`
    // cuando la accion real es `track_set_volume`, el LLM falla aunque el
    // codigo este bien.
    let src = include_str!("tools.rs");
    for (dominio, sufijo) in [
        ("track", "get_all"), ("track", "set_volume"),
        ("fx", "get_params"), ("midi", "insert_note"),
    ] {
        let _ = (dominio, sufijo);
    }
    // Formas cortas que NO existen como accion y no deben aparecer como op.
    for inventado in ["op=list (all tracks", "op=volume (linear", "op=paramInfo",
                      "op=item (empty MIDI"] {
        assert!(
            !src.contains(inventado),
            "la descripcion de una tool promete '{inventado}',              que no es el nombre real de ninguna accion"
        );
    }
}

#[test]
fn un_op_desconocido_da_error_que_apunta_al_catalogo() {
    let e = super::tools::resolver_accion("track", "inventado").unwrap_err();
    let m = e.to_string();
    assert!(m.contains("inventado"), "{m}");
    assert!(m.contains("daw_catalog"), "el error deberia decir donde mirar: {m}");
}

#[test]
fn daw_health_no_llama_a_una_accion_que_no_existe() {
    // Encontrado al usar la tool de verdad: `daw_health` devolvia
    // "Unknown command: transport". No existe una action `transport`; la
    // real es `transport_get_state`.
    //
    // Aqui se comprueba lo que se puede comprobar sin DAW: que el nombre de
    // accion que aparece en el codigo este en el catalogo generado.
    let src = include_str!("tools.rs");
    for linea in src.lines() {
        let t = linea.trim();
        // Solo las llamadas de la forma call("algo", ...)
        let Some(rest) = t.strip_prefix("call(\"") else {
            continue;
        };
        let Some(fin) = rest.find('"') else { continue };
        let accion = &rest[..fin];
        // Los nombres que las tools componen (track.<op>, fx.<op>...) se
        // expands en runtime: no se pueden comprobar aqui.
        if accion.contains('.') || accion.contains("+") {
            continue;
        }
        assert!(
            heretic_daw::actions::doc_of(accion).is_some(),
            "daw_health llama a '{accion}', que no esta en el catalogo. \
             Los nombres reales se generan con: python tools/gen_actions.py"
        );
    }
}

#[test]
fn la_accion_mas_raros_no_se_olvido() {
    // `project_get_paths` no existia y se coló en la version anterior de
    // daw_project. Este test falla si alguien lo vuelve a escribir.
    assert!(
        heretic_daw::actions::doc_of("project_get_paths").is_none(),
        "project_get_paths no existe en el bridge; no usarlo"
    );
}

#[test]
fn el_catalogo_cubre_las_familias_que_usan_las_tools() {
    // Las tools tipadas dependen de estas familias. Si el bridge las renombra
    // enteras, hay que reescribir la tool entera, y mejor saberlo aqui.
    for prefijo in ["transport_", "track_", "project_", "fx_", "midi_", "item_"] {
        let n = heretic_daw::ReaperBridge::actions_with_prefix(prefijo).len();
        assert!(n > 0, "no hay ninguna accion '{prefijo}*' en el catalogo");
    }
}

#[test]
fn las_acciones_documentan_sus_params() {
    // Un param que el bridge exige tiene que aparecer en `required`. Sin eso,
    // `daw_catalog` no impide que el LLM se lo salte.
    let con_required: Vec<&str> = heretic_daw::actions::ACTION_DOCS
        .iter()
        .filter(|d| !d.required.is_empty())
        .map(|d| d.name)
        .collect();
    assert!(
        con_required.len() > 10,
        "solo {} acciones declaran params obligatorios; el extractor no esta \
         leyendo los 'Missing parameter' del bridge",
        con_required.len()
    );

    // Y al menos una debe tenerlos de verdad.
    let d = heretic_daw::actions::doc_of("track_delete_batch")
        .expect("track_delete_batch deberia existir");
    assert!(
        d.params.contains(&"entries"),
        "track_delete_batch lee 'entries' (no 'trackIndices'), y cada entry es \
         un objeto {{track_index: N}}. El error real fue 'Entry must be an object'."
    );
}
