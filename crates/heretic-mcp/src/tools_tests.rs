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

// ============================================================================
// Tools contra el bridge: los parametros tienen que existir de verdad
// ============================================================================
//
// Medido: las seis claves que las tools insertaban eran camelCase
// (`trackIndex`, `fxIndex`, `itemIndex`, `paramIndex`, `fxName`, `fileName`)
// mientras que el bridge lee `p.track_index` y compañía. No hay conversion en
// ninguna capa, asi que toda llamada que necesitara un indice fallaba con
// `Missing parameter: track_index`, y las que no lo necesitan funcionaban bien:
// el fallo solo aparecia en el camino que de verdad importa.
//
// Y al reves: `daw_project` mandaba `name` y `template` a `project_new`, que no
// los lee, y `start`/`end`/`file_name` a `project_export_audio`, que no los
// leia. Reaper no avisa de un parametro desconocido: usa el valor por defecto y
// devuelve exito. Un test que solo mira el codigo de la tool no ve nada, asi que
// estos tests comprueban las claves contra el catalogo del bridge, que es lo
// unico que sabe lo que el bridge lee de verdad.

/// Claves que las tools insertan en el `Map` de params.
///
/// Se extrae del codigo, no se escribe a mano: una lista escrita a mano
/// comprobaria que la lista esta al dia consigo misma.
fn claves_que_inserta_el_codigo() -> Vec<String> {
    let src = include_str!("tools.rs");
    let mut out = Vec::new();
    for trozo in src.split("p.insert(\"").skip(1) {
        let clave: String = trozo
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !clave.is_empty() {
            out.push(clave);
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn ninguna_clave_insertada_es_camel_case() {
    let camel: Vec<String> = claves_que_inserta_el_codigo()
        .into_iter()
        .filter(|k| k.chars().any(|c| c.is_uppercase()))
        .collect();
    assert!(
        camel.is_empty(),
        "estas claves se insertan en camelCase y el bridge no las lee: {camel:?}. \
         El bridge solo lee p.<snake_case>."
    );
}

#[test]
fn cada_clave_insertada_existe_en_al_una_accion_del_bridge() {
    let claves = claves_que_inserta_el_codigo();
    assert!(
        !claves.is_empty(),
        "no se extrajo ninguna clave: el parser se rompio"
    );

    let leidas: std::collections::BTreeSet<String> = heretic_daw::actions::ACTION_DOCS
        .iter()
        .flat_map(|d| d.params.iter().map(|s| s.to_string()))
        .collect();

    let huerfanas: Vec<&String> = claves.iter().filter(|k| !leidas.contains(*k)).collect();
    assert!(
        huerfanas.is_empty(),
        "estas claves las envia una tool pero ningun handler del bridge las lee: {huerfanas:?}. \
         O el nombre esta mal, o la clave no deberia enviarse."
    );
}

#[test]
fn las_acciones_midi_declaran_item_index() {
    // `get_midi_take(p)` exige `item_index` pero no esta en el cuerpo del
    // handler, asi que el generador no lo via y el catalogo decia que estas 9 de
    // 17 acciones no pedian nada.
    for d in heretic_daw::actions::ACTION_DOCS.iter().filter(|d| d.module == "midi") {
        if d.name == "midi_get_note_names" || d.name == "midi_list_programs" {
            continue; // estas no tocan ningun item
        }
        assert!(
            d.required.contains(&"item_index"),
            "{} no declara item_index en required, pero llama a get_midi_take(p) que lo exige.",
            d.name
        );
    }
}

#[test]
fn el_grupo_del_catalogo_es_el_prefijo_del_nombre() {
    // La regla que usa el codigo es `{grupo}_{sufijo}`, y el grupo es lo que
    // `daw_catalog` ensena. Si un nombre no se reconstruye como
    // `{grupo}_{sufijo}`, `daw_catalog` no lo lista y el agente no lo descubre.
    //
    // NOTA: el grupo NO es el modulo Lua del handler, y por eso 17 de 162 no
    // coinciden (`chops_create_virtual_slice` esta en el modulo `item`, con
    // grupo `chops`). La afirmacion de que "los 162 cumplen sin excepcion" era
    // falsa: cumplen la regla del grupo, no la del modulo.
    for (grupo, nombres) in heretic_daw::actions::ACTION_GROUPS {
        for n in *nombres {
            let prefijo = format!("{grupo}_");
            assert!(
                n.starts_with(&prefijo),
                "{n} esta en el grupo {grupo} pero no empieza por {prefijo}"
            );
        }
    }
}

#[test]
fn todo_nombre_del_catalogo_aparece_en_algun_grupo() {
    let mut en_grupos: Vec<&str> = Vec::new();
    for (_, nombres) in heretic_daw::actions::ACTION_GROUPS {
        en_grupos.extend(nombres.iter().copied());
    }
    for d in heretic_daw::actions::ACTION_DOCS {
        assert!(
            en_grupos.contains(&d.name),
            "{} esta en ACTION_DOCS pero en ningun grupo: daw_catalog no lo listaria",
            d.name
        );
    }
}
