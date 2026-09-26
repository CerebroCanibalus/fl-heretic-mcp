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
