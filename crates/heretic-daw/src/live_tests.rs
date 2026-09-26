//! Prueba del transporte contra un DAW REAL.
//!
//! Ignorada por defecto: solo corre con `--ignored`, y solo tiene sentido con
//! Reaper abierto y el bridge cargado.
//!
//!     cargo test -p heretic-daw -- --ignored --nocapture
//!
//! # Por que esto es una test ignorada y no parte del suite
//!
//! El 100% de los bugs caros de la etapa de FL Studio pasaron la suite en
//! local y fallaron contra el DAW. La primera version de este transporte se
//! escribio adivinando el protocolo y se rompio en las tres primeras
//! llamadas: el bridge usa `command`/`success`/`data`, no
//! `action`/`ok`/`result`. Ningun test contra mocks lo habria detectado.
//!
//! Estos tests son el recordatorio de que el contrato vive en el bridge, no
//! aqui. Si cambian, hay que volver a leerlo.

use crate::file_rpc::{FileRpc, RpcConfig};
use std::time::Duration;

fn cliente() -> FileRpc {
    let mut cfg = RpcConfig::default_config();
    // Margen generoso: el bridge responde en un tick de defer (~33 ms), pero
    // si Reaper esta cargando puede tardar bastante mas.
    cfg.timeout = Duration::from_secs(15);
    FileRpc::new(cfg)
}

#[test]
#[ignore = "requiere Reaper abierto con el bridge cargado"]
fn el_bridge_esta_vivo() {
    let c = cliente();
    assert!(
        c.is_bridge_alive(),
        "no hay heartbeat en {}. Reaper abierto? __startup.lua carga el bridge?",
        c.config().lock.display()
    );
    let age = c.heartbeat_age().expect("lock existe");
    println!("  heartbeat de hace {:?}", age);
    assert!(
        age < Duration::from_secs(60),
        "el heartbeat tiene {:?}: el bridge parece colgado",
        age
    );
}

#[test]
#[ignore = "requiere Reaper abierto con el bridge cargado"]
fn transport_devuelve_estado_real() {
    let c = cliente();
    let r = c.call("transport_get_state", serde_json::json!({})).expect("transport_get_state deberia responder");
    println!("  transport_get_state -> {}", serde_json::to_string_pretty(&r).unwrap_or_default());
    assert!(
        r.get("bpm").is_some() || r.get("tempo").is_some(),
        "transport_get_state deberia devolver el tempo, dio: {r}"
    );
}

#[test]
#[ignore = "requiere Reaper abierto con el bridge cargado"]
fn comanda_desconocida_da_error_nombrado() {
    // El bridge responde "Unknown command: X". Que el error llegue con ese
    // texto es la unica prueba de que el campo se llama `command` y no
    // `action`: con `action` el nombre seria valido pero la accion no.
    let c = cliente();
    let e = c
        .call("no.existe.esta", serde_json::json!({}))
        .expect_err("debe fallar");
    let m = e.to_string();
    println!("  error -> {m}");
    assert!(m.contains("Unknown command"), "{m}");
}

#[test]
#[ignore = "requiere Reaper abierto con el bridge cargado"]
fn ida_y_vuelta_del_tempo() {
    // La prueba de verdad: escribir, leer de vuelta, y que coincida. Es la
    // que en FL fallaba cuando el proyecto quedaba sucio o habia un modal.
    let c = cliente();
    let antes = c
        .call("transport_get_state", serde_json::json!({}))
        .expect("leer transport")
        .get("bpm")
        .and_then(serde_json::Value::as_f64)
        .expect("transport devuelve bpm");
    println!("  tempo antes: {antes}");

    let destino = if (antes - 111.0).abs() > 1.0 { 111.0 } else { 122.0 };
    c.call("transport_set_bpm", serde_json::json!({ "bpm": destino }))
        .unwrap_or_else(|e| panic!("setTempo({destino}) fallo: {e}"));

    std::thread::sleep(Duration::from_millis(200));
    let despues = c
        .call("transport_get_state", serde_json::json!({}))
        .expect("releer transport")
        .get("bpm")
        .and_then(serde_json::Value::as_f64)
        .expect("transport devuelve bpm");
    println!("  tempo despues: {despues}");

    assert!(
        (despues - destino).abs() < 0.01,
        "escribi {destino} y lei {despues}"
    );

    // Restaurar.
    c.call("transport_set_bpm", serde_json::json!({ "bpm": antes }))
        .expect("restaurar tempo");
}

#[test]
#[ignore = "requiere Reaper abierto con el bridge cargado"]
fn crear_pista_y_listar() {
    let c = cliente();
    let antes = c
        .call("track_get_all", serde_json::json!({}))
        .ok()
        .and_then(|v| v.get("count").and_then(serde_json::Value::as_i64))
        .unwrap_or(-1);
    println!("  pistas antes: {antes}");

    c.call("track_create", serde_json::json!({ "name": "daw-heretic-prueba" }))
        .unwrap_or_else(|e| panic!("track_create fallo: {e}"));
    std::thread::sleep(Duration::from_millis(200));

    let r = c.call("track_get_all", serde_json::json!({})).expect("track_get_all");
    let found = serde_json::to_string(&r).unwrap_or_default();
    println!("  track_get_all -> {} bytes", found.len());
    assert!(
        found.contains("daw-heretic-prueba"),
        "la pista nueva no aparece en la lista"
    );

    // Limpieza: borrar la pista de prueba.
    if let Some(t) = r
        .get("tracks")
        .and_then(|v| v.as_array())
        .and_then(|a| {
            a.iter()
                .find(|t| {
                    t.get("name")
                        .and_then(|n| n.as_str())
                        .is_some_and(|n| n == "daw-heretic-prueba")
                })
                .and_then(|t| t.get("index").and_then(serde_json::Value::as_i64))
        })
    {
        c.call("track_delete_batch", serde_json::json!({ "entries": [{ "track_index": t }] }))
            .unwrap_or_else(|e| panic!("borrar la pista de prueba fallo: {e}"));
        println!("  pista de prueba borrada");
    }
}
